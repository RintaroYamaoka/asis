use crate::core::{Color, Rectangle, Size};
use crate::graphics::compositor::{self, Information};
use crate::graphics::damage;
use crate::graphics::error::{self, Error};
use crate::graphics::{self, Shell, Viewport};
use crate::{Layer, Renderer, Settings};

use std::collections::VecDeque;
use std::num::NonZeroU32;

pub struct Compositor {
    context: softbuffer::Context<Box<dyn compositor::Display>>,
    settings: Settings,
}

pub struct Surface {
    window: softbuffer::Surface<
        Box<dyn compositor::Display>,
        Box<dyn compositor::Window>,
    >,
    clip_mask: crate::engine::ClipMask,
    /// The picture of the window, kept in our own memory.
    ///
    /// Everything is drawn here first and then copied to the buffer of the
    /// window. The buffer of the window may be shared with the display
    /// server; blending in it directly reads that memory back for every
    /// pixel, and what it holds depends on how old the buffer is.
    frame: tiny_skia::Pixmap,
    /// What `frame` shows.
    layers: Option<Vec<Layer>>,
    /// The regions redrawn in the last presents, newest first. A buffer of
    /// the window that is `age` presents old misses the first `age` of them.
    damage_history: VecDeque<Vec<Rectangle>>,
    background_color: Color,
}

impl crate::graphics::Compositor for Compositor {
    type Renderer = Renderer;
    type Surface = Surface;

    async fn with_backend(
        settings: graphics::Settings,
        display: impl compositor::Display,
        _compatible_window: impl compositor::Window,
        _shell: Shell,
        backend: Option<&str>,
    ) -> Result<Self, Error> {
        match backend {
            None | Some("tiny-skia") | Some("tiny_skia") => {
                Ok(new(settings.into(), display))
            }
            Some(backend) => Err(Error::GraphicsAdapterNotFound {
                backend: "tiny-skia",
                reason: error::Reason::DidNotMatch {
                    preferred_backend: backend.to_owned(),
                },
            }),
        }
    }

    fn create_renderer(&self) -> Self::Renderer {
        Renderer::new(
            self.settings.default_font,
            self.settings.default_text_size,
        )
    }

    fn create_surface<W: compositor::Window + Clone>(
        &mut self,
        window: W,
        width: u32,
        height: u32,
    ) -> Self::Surface {
        let window = softbuffer::Surface::new(
            &self.context,
            Box::new(window.clone()) as _,
        )
        .expect("Create softbuffer surface for window");

        let mut surface = Surface {
            window,
            clip_mask: crate::engine::ClipMask::new(1, 1)
                .expect("Create clip mask"),
            frame: tiny_skia::Pixmap::new(1, 1).expect("Create frame"),
            layers: None,
            damage_history: VecDeque::new(),
            background_color: Color::BLACK,
        };

        if width > 0 && height > 0 {
            self.configure_surface(&mut surface, width, height);
        }

        surface
    }

    fn configure_surface(
        &mut self,
        surface: &mut Self::Surface,
        width: u32,
        height: u32,
    ) {
        surface
            .window
            .resize(
                NonZeroU32::new(width).expect("Non-zero width"),
                NonZeroU32::new(height).expect("Non-zero height"),
            )
            .expect("Resize surface");

        surface.clip_mask = crate::engine::ClipMask::new(width, height)
            .expect("Create clip mask");
        surface.frame =
            tiny_skia::Pixmap::new(width, height).expect("Create frame");
        surface.layers = None;
        surface.damage_history.clear();
    }

    fn information(&self) -> Information {
        Information {
            adapter: String::from("CPU"),
            backend: String::from("tiny-skia"),
        }
    }

    fn present(
        &mut self,
        renderer: &mut Self::Renderer,
        surface: &mut Self::Surface,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), compositor::SurfaceError> {
        present(
            renderer,
            surface,
            viewport,
            background_color,
            on_pre_present,
        )
    }

    fn screenshot(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Vec<u8> {
        screenshot(renderer, viewport, background_color)
    }
}

pub fn new(
    settings: Settings,
    display: impl compositor::Display,
) -> Compositor {
    #[allow(unsafe_code)]
    let context = softbuffer::Context::new(Box::new(display) as _)
        .expect("Create softbuffer context");

    Compositor { context, settings }
}

pub fn present(
    renderer: &mut Renderer,
    surface: &mut Surface,
    viewport: &Viewport,
    background_color: Color,
    on_pre_present: impl FnOnce(),
) -> Result<(), compositor::SurfaceError> {
    let window = Rectangle::with_size(viewport.logical_size());

    // What changed since the last time we drew.
    let damage = match &surface.layers {
        Some(last_layers) if surface.background_color == background_color => {
            damage::group(
                damage::diff(
                    last_layers,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    Layer::damage,
                ),
                window,
            )
        }
        _ => vec![window],
    };

    if !damage.is_empty() {
        surface.layers = Some(renderer.layers().to_vec());
        surface.background_color = background_color;

        renderer.draw(
            &mut surface.frame.as_mut(),
            &mut surface.clip_mask,
            viewport,
            &damage,
            background_color,
        );
    }

    let mut buffer = surface
        .window
        .buffer_mut()
        .map_err(|_| compositor::SurfaceError::Lost)?;

    surface.damage_history.push_front(damage);
    surface.damage_history.truncate(MAX_BUFFER_AGE);

    // The buffer misses what was drawn while it was away. Without a known
    // age, or with one older than we remember, it gets the whole frame.
    let age = buffer.age() as usize;
    let whole = [window];
    let missed: Vec<Rectangle> =
        if age == 0 || age > surface.damage_history.len() {
            whole.to_vec()
        } else {
            surface
                .damage_history
                .iter()
                .take(age)
                .flatten()
                .copied()
                .collect()
        };

    let copied = copy_regions(
        &surface.frame,
        bytemuck::cast_slice_mut(&mut buffer),
        &missed,
        viewport.scale_factor(),
    );

    on_pre_present();

    if copied.is_empty() {
        buffer.present()
    } else {
        buffer.present_with_damage(&copied)
    }
    .map_err(|_| compositor::SurfaceError::Lost)
}

/// How many presents of damage are remembered. Display servers hand out two
/// or three buffers in turn.
const MAX_BUFFER_AGE: usize = 4;

/// Copies the given regions of `frame` to `buffer`, which has the same size
/// and pixel layout, and returns the pixels that were copied.
fn copy_regions(
    frame: &tiny_skia::Pixmap,
    buffer: &mut [u8],
    regions: &[Rectangle],
    scale_factor: f32,
) -> Vec<softbuffer::Rect> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let source = frame.data();

    if buffer.len() != source.len() {
        return Vec::new();
    }

    regions
        .iter()
        .filter_map(|region| {
            let region = *region * scale_factor;
            let left = (region.x.floor().max(0.0) as usize).min(width);
            let top = (region.y.floor().max(0.0) as usize).min(height);
            let right =
                ((region.x + region.width).ceil().max(0.0) as usize).min(width);
            let bottom = ((region.y + region.height).ceil().max(0.0) as usize)
                .min(height);

            for y in top..bottom {
                let row = y * width * 4;

                buffer[row + left * 4..row + right * 4]
                    .copy_from_slice(&source[row + left * 4..row + right * 4]);
            }

            Some(softbuffer::Rect {
                x: left as u32,
                y: top as u32,
                width: NonZeroU32::new((right - left) as u32)?,
                height: NonZeroU32::new((bottom - top) as u32)?,
            })
        })
        .collect()
}

pub fn screenshot(
    renderer: &mut Renderer,
    viewport: &Viewport,
    background_color: Color,
) -> Vec<u8> {
    let size = viewport.physical_size();

    let mut offscreen_buffer: Vec<u32> =
        vec![0; size.width as usize * size.height as usize];

    let mut clip_mask = crate::engine::ClipMask::new(size.width, size.height)
        .expect("Create clip mask");

    renderer.draw(
        &mut tiny_skia::PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut offscreen_buffer),
            size.width,
            size.height,
        )
        .expect("Create offscreen pixel map"),
        &mut clip_mask,
        viewport,
        &[Rectangle::with_size(Size::new(
            size.width as f32,
            size.height as f32,
        ))],
        background_color,
    );

    offscreen_buffer.iter().fold(
        Vec::with_capacity(offscreen_buffer.len() * 4),
        |mut acc, pixel| {
            const A_MASK: u32 = 0xFF_00_00_00;
            const R_MASK: u32 = 0x00_FF_00_00;
            const G_MASK: u32 = 0x00_00_FF_00;
            const B_MASK: u32 = 0x00_00_00_FF;

            let a = ((A_MASK & pixel) >> 24) as u8;
            let r = ((R_MASK & pixel) >> 16) as u8;
            let g = ((G_MASK & pixel) >> 8) as u8;
            let b = (B_MASK & pixel) as u8;

            acc.extend([r, g, b, a]);
            acc
        },
    )
}
