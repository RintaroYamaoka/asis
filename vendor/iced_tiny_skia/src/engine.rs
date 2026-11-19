use crate::Primitive;
use crate::core::renderer::Quad;
use crate::core::{
    Background, Color, Gradient, Rectangle, Size, Transformation, Vector,
};
use crate::graphics::{Image, Text};
use crate::text;

#[derive(Debug)]
pub struct Engine {
    text_pipeline: text::Pipeline,

    #[cfg(feature = "image")]
    pub(crate) raster_pipeline: crate::raster::Pipeline,
    #[cfg(feature = "svg")]
    pub(crate) vector_pipeline: crate::vector::Pipeline,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            text_pipeline: text::Pipeline::new(),
            #[cfg(feature = "image")]
            raster_pipeline: crate::raster::Pipeline::new(),
            #[cfg(feature = "svg")]
            vector_pipeline: crate::vector::Pipeline::new(),
        }
    }

    pub fn draw_quad(
        &mut self,
        quad: &Quad,
        background: &Background,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut ClipMask,
        clip_bounds: Rectangle,
    ) {
        let physical_bounds = quad.bounds * transformation;

        if !clip_bounds.intersects(&physical_bounds) {
            return;
        }

        // Where the visible part of the quad is only straight edges, it is
        // filled as rectangles clipped by geometry. Clipping with the mask
        // costs the whole area of the quad, however little of it is inside
        // the clip bounds.
        if let Background::Color(color) = background {
            if has_only_straight_edges(
                quad,
                physical_bounds,
                transformation.scale_factor(),
                clip_bounds,
            ) {
                fill_straight_edges(
                    quad,
                    *color,
                    physical_bounds,
                    transformation.scale_factor(),
                    pixels,
                    clip_bounds,
                );

                return;
            }
        }

        let clip_mask = (!physical_bounds.is_within(&clip_bounds))
            .then_some(clip_mask.mask());

        let transform = into_transform(transformation);

        // Make sure the border radius is not larger than the bounds
        let border_width = quad
            .border
            .width
            .min(quad.bounds.width / 2.0)
            .min(quad.bounds.height / 2.0);

        let mut fill_border_radius = <[f32; 4]>::from(quad.border.radius);

        for radius in &mut fill_border_radius {
            *radius = (*radius)
                .min(quad.bounds.width / 2.0)
                .min(quad.bounds.height / 2.0);
        }

        let path = rounded_rectangle(quad.bounds, fill_border_radius);

        let shadow = quad.shadow;

        if shadow.color.a > 0.0 {
            let shadow_bounds = Rectangle {
                x: quad.bounds.x + shadow.offset.x - shadow.blur_radius,
                y: quad.bounds.y + shadow.offset.y - shadow.blur_radius,
                width: quad.bounds.width + shadow.blur_radius * 2.0,
                height: quad.bounds.height + shadow.blur_radius * 2.0,
            } * transformation;

            let radii = fill_border_radius
                .into_iter()
                .map(|radius| radius * transformation.scale_factor())
                .collect::<Vec<_>>();
            let (x, y, width, height) = (
                shadow_bounds.x as u32,
                shadow_bounds.y as u32,
                shadow_bounds.width as u32,
                shadow_bounds.height as u32,
            );
            let half_width = physical_bounds.width / 2.0;
            let half_height = physical_bounds.height / 2.0;

            let colors = (y..y + height)
                .flat_map(|y| (x..x + width).map(move |x| (x as f32, y as f32)))
                .filter_map(|(x, y)| {
                    tiny_skia::Size::from_wh(half_width, half_height).map(
                        |size| {
                            let shadow_distance = rounded_box_sdf(
                                Vector::new(
                                    x - physical_bounds.position().x
                                        - (shadow.offset.x
                                            * transformation.scale_factor())
                                        - half_width,
                                    y - physical_bounds.position().y
                                        - (shadow.offset.y
                                            * transformation.scale_factor())
                                        - half_height,
                                ),
                                size,
                                &radii,
                            )
                            .max(0.0);
                            let shadow_alpha = 1.0
                                - smoothstep(
                                    -shadow.blur_radius
                                        * transformation.scale_factor(),
                                    shadow.blur_radius
                                        * transformation.scale_factor(),
                                    shadow_distance,
                                );

                            let mut color = into_color(shadow.color);
                            color.apply_opacity(shadow_alpha);

                            color.to_color_u8().premultiply()
                        },
                    )
                })
                .collect();

            if let Some(pixmap) = tiny_skia::IntSize::from_wh(width, height)
                .and_then(|size| {
                    tiny_skia::Pixmap::from_vec(
                        bytemuck::cast_vec(colors),
                        size,
                    )
                })
            {
                pixels.draw_pixmap(
                    x as i32,
                    y as i32,
                    pixmap.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::default(),
                    None,
                );
            }
        }

        pixels.fill_path(
            &path,
            &tiny_skia::Paint {
                shader: match background {
                    Background::Color(color) => {
                        tiny_skia::Shader::SolidColor(into_color(*color))
                    }
                    Background::Gradient(Gradient::Linear(linear)) => {
                        let (start, end) =
                            linear.angle.to_distance(&quad.bounds);

                        let stops: Vec<tiny_skia::GradientStop> = linear
                            .stops
                            .into_iter()
                            .flatten()
                            .map(|stop| {
                                tiny_skia::GradientStop::new(
                                    stop.offset,
                                    tiny_skia::Color::from_rgba(
                                        stop.color.b,
                                        stop.color.g,
                                        stop.color.r,
                                        stop.color.a,
                                    )
                                    .expect("Create color"),
                                )
                            })
                            .collect();

                        tiny_skia::LinearGradient::new(
                            tiny_skia::Point {
                                x: start.x,
                                y: start.y,
                            },
                            tiny_skia::Point { x: end.x, y: end.y },
                            if stops.is_empty() {
                                vec![tiny_skia::GradientStop::new(
                                    0.0,
                                    tiny_skia::Color::BLACK,
                                )]
                            } else {
                                stops
                            },
                            tiny_skia::SpreadMode::Pad,
                            tiny_skia::Transform::identity(),
                        )
                        .expect("Create linear gradient")
                    }
                },
                anti_alias: true,
                ..tiny_skia::Paint::default()
            },
            tiny_skia::FillRule::EvenOdd,
            transform,
            clip_mask,
        );

        if border_width > 0.0 {
            // Border path is offset by half the border width
            let border_bounds = Rectangle {
                x: quad.bounds.x + border_width / 2.0,
                y: quad.bounds.y + border_width / 2.0,
                width: quad.bounds.width - border_width,
                height: quad.bounds.height - border_width,
            };

            // Make sure the border radius is correct
            let mut border_radius = <[f32; 4]>::from(quad.border.radius);
            let mut is_simple_border = true;

            for radius in &mut border_radius {
                *radius = if *radius == 0.0 {
                    // Path should handle this fine
                    0.0
                } else if *radius > border_width / 2.0 {
                    *radius - border_width / 2.0
                } else {
                    is_simple_border = false;
                    0.0
                }
                .min(border_bounds.width / 2.0)
                .min(border_bounds.height / 2.0);
            }

            // Stroking a path works well in this case
            if is_simple_border {
                let border_path =
                    rounded_rectangle(border_bounds, border_radius);

                pixels.stroke_path(
                    &border_path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(into_color(
                            quad.border.color,
                        )),
                        anti_alias: true,
                        ..tiny_skia::Paint::default()
                    },
                    &tiny_skia::Stroke {
                        width: border_width,
                        ..tiny_skia::Stroke::default()
                    },
                    transform,
                    clip_mask,
                );
            } else {
                // Draw corners that have too small border radii as having no border radius,
                // but mask them with the rounded rectangle with the correct border radius.
                let mut temp_pixmap = tiny_skia::Pixmap::new(
                    quad.bounds.width as u32,
                    quad.bounds.height as u32,
                )
                .unwrap();

                let mut quad_mask = tiny_skia::Mask::new(
                    quad.bounds.width as u32,
                    quad.bounds.height as u32,
                )
                .unwrap();

                let zero_bounds = Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: quad.bounds.width,
                    height: quad.bounds.height,
                };
                let path = rounded_rectangle(zero_bounds, fill_border_radius);

                quad_mask.fill_path(
                    &path,
                    tiny_skia::FillRule::EvenOdd,
                    true,
                    transform,
                );
                let path_bounds = Rectangle {
                    x: border_width / 2.0,
                    y: border_width / 2.0,
                    width: quad.bounds.width - border_width,
                    height: quad.bounds.height - border_width,
                };

                let border_radius_path =
                    rounded_rectangle(path_bounds, border_radius);

                temp_pixmap.stroke_path(
                    &border_radius_path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(into_color(
                            quad.border.color,
                        )),
                        anti_alias: true,
                        ..tiny_skia::Paint::default()
                    },
                    &tiny_skia::Stroke {
                        width: border_width,
                        ..tiny_skia::Stroke::default()
                    },
                    transform,
                    Some(&quad_mask),
                );

                pixels.draw_pixmap(
                    quad.bounds.x as i32,
                    quad.bounds.y as i32,
                    temp_pixmap.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    transform,
                    clip_mask,
                );
            }
        }
    }

    pub fn draw_text(
        &mut self,
        text: &Text,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut ClipMask,
        clip_bounds: Rectangle,
    ) {
        match text {
            Text::Paragraph {
                paragraph,
                position,
                color,
                clip_bounds: local_clip_bounds,
                transformation: local_transformation,
            } => {
                let transformation = transformation * *local_transformation;
                let Some(clip_bounds) = clip_bounds
                    .intersection(&(*local_clip_bounds * transformation))
                else {
                    return;
                };

                let physical_bounds =
                    Rectangle::new(*position, paragraph.min_bounds)
                        * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = match physical_bounds.is_within(&clip_bounds) {
                    true => None,
                    false => {
                        adjust_clip_mask(clip_mask, clip_bounds);
                        Some(clip_mask.mask())
                    }
                };

                self.text_pipeline.draw_paragraph(
                    paragraph,
                    *position,
                    *color,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
            Text::Editor {
                editor,
                position,
                color,
                clip_bounds: local_clip_bounds,
                transformation: local_transformation,
            } => {
                let transformation = transformation * *local_transformation;
                let Some(clip_bounds) = clip_bounds
                    .intersection(&(*local_clip_bounds * transformation))
                else {
                    return;
                };

                let physical_bounds =
                    Rectangle::new(*position, editor.bounds) * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = match physical_bounds.is_within(&clip_bounds) {
                    true => None,
                    false => {
                        adjust_clip_mask(clip_mask, clip_bounds);
                        Some(clip_mask.mask())
                    }
                };

                self.text_pipeline.draw_editor(
                    editor,
                    *position,
                    *color,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
            Text::Cached {
                content,
                bounds,
                color,
                size,
                line_height,
                font,
                align_x,
                align_y,
                shaping,
                clip_bounds: local_clip_bounds,
            } => {
                let physical_bounds = *local_clip_bounds * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = match physical_bounds.is_within(&clip_bounds) {
                    true => None,
                    false => {
                        adjust_clip_mask(clip_mask, clip_bounds);
                        Some(clip_mask.mask())
                    }
                };

                self.text_pipeline.draw_cached(
                    content,
                    *bounds,
                    *color,
                    *size,
                    *line_height,
                    *font,
                    *align_x,
                    *align_y,
                    *shaping,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
            Text::Raw {
                raw,
                transformation: local_transformation,
            } => {
                let Some(buffer) = raw.buffer.upgrade() else {
                    return;
                };

                let transformation = transformation * *local_transformation;
                let (width, height) = buffer.size();

                let physical_bounds = Rectangle::new(
                    raw.position,
                    Size::new(
                        width.unwrap_or(clip_bounds.width),
                        height.unwrap_or(clip_bounds.height),
                    ),
                ) * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&clip_bounds))
                    .then_some(clip_mask.mask());

                self.text_pipeline.draw_raw(
                    &buffer,
                    raw.position,
                    raw.color,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
        }
    }

    pub fn draw_primitive(
        &mut self,
        primitive: &Primitive,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut ClipMask,
        clip_bounds: Rectangle,
    ) {
        match primitive {
            Primitive::Fill { path, paint, rule } => {
                let physical_bounds = {
                    let bounds = path.bounds();

                    Rectangle {
                        x: bounds.x(),
                        y: bounds.y(),
                        width: bounds.width(),
                        height: bounds.height(),
                    } * transformation
                };

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&clip_bounds))
                    .then_some(clip_mask.mask());

                pixels.fill_path(
                    path,
                    paint,
                    *rule,
                    into_transform(transformation),
                    clip_mask,
                );
            }
            Primitive::Stroke {
                path,
                paint,
                stroke,
            } => {
                let physical_bounds = {
                    let bounds = path.bounds();

                    Rectangle {
                        x: bounds.x() - stroke.width / 2.0,
                        y: bounds.y() - stroke.width / 2.0,
                        width: bounds.width() + stroke.width,
                        height: bounds.height() + stroke.width,
                    } * transformation
                };

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&clip_bounds))
                    .then_some(clip_mask.mask());

                pixels.stroke_path(
                    path,
                    paint,
                    stroke,
                    into_transform(transformation),
                    clip_mask,
                );
            }
        }
    }

    pub fn draw_image(
        &mut self,
        image: &Image,
        _transformation: Transformation,
        _pixels: &mut tiny_skia::PixmapMut<'_>,
        _clip_mask: &mut ClipMask,
        _clip_bounds: Rectangle,
    ) {
        match image {
            #[cfg(feature = "image")]
            Image::Raster { image, bounds, .. } => {
                let physical_bounds = *bounds * _transformation;

                if !_clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&_clip_bounds))
                    .then_some(_clip_mask.mask());

                let center = physical_bounds.center();
                let radians = f32::from(image.rotation);

                let transform = into_transform(_transformation).post_rotate_at(
                    radians.to_degrees(),
                    center.x,
                    center.y,
                );

                self.raster_pipeline.draw(
                    &image.handle,
                    image.filter_method,
                    *bounds,
                    image.opacity,
                    _pixels,
                    transform,
                    clip_mask,
                );
            }
            #[cfg(feature = "svg")]
            Image::Vector { svg, bounds, .. } => {
                let physical_bounds = *bounds * _transformation;

                if !_clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&_clip_bounds))
                    .then_some(_clip_mask.mask());

                let center = physical_bounds.center();
                let radians = f32::from(svg.rotation);

                let transform = into_transform(_transformation).post_rotate_at(
                    radians.to_degrees(),
                    center.x,
                    center.y,
                );

                self.vector_pipeline.draw(
                    &svg.handle,
                    svg.color,
                    *bounds,
                    svg.opacity,
                    _pixels,
                    transform,
                    clip_mask,
                );
            }
            #[cfg(not(feature = "image"))]
            Image::Raster { .. } => {
                log::warn!(
                    "Unsupported primitive in `iced_tiny_skia`: {image:?}",
                );
            }
            #[cfg(not(feature = "svg"))]
            Image::Vector { .. } => {
                log::warn!(
                    "Unsupported primitive in `iced_tiny_skia`: {image:?}",
                );
            }
        }
    }

    pub fn trim(&mut self) {
        self.text_pipeline.trim_cache();

        #[cfg(feature = "image")]
        self.raster_pipeline.trim_cache();

        #[cfg(feature = "svg")]
        self.vector_pipeline.trim_cache();
    }
}

pub fn into_color(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(color.b, color.g, color.r, color.a)
        .expect("Convert color from iced to tiny_skia")
}

fn into_transform(transformation: Transformation) -> tiny_skia::Transform {
    let translation = transformation.translation();

    tiny_skia::Transform {
        sx: transformation.scale_factor(),
        kx: 0.0,
        ky: 0.0,
        sy: transformation.scale_factor(),
        tx: translation.x,
        ty: translation.y,
    }
}

fn rounded_rectangle(
    bounds: Rectangle,
    border_radius: [f32; 4],
) -> tiny_skia::Path {
    let [top_left, top_right, bottom_right, bottom_left] = border_radius;

    if top_left == 0.0
        && top_right == 0.0
        && bottom_right == 0.0
        && bottom_left == 0.0
    {
        return tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
            )
            .expect("Build quad rectangle"),
        );
    }

    if top_left == top_right
        && top_left == bottom_right
        && top_left == bottom_left
        && top_left == bounds.width / 2.0
        && top_left == bounds.height / 2.0
    {
        return tiny_skia::PathBuilder::from_circle(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
            top_left,
        )
        .expect("Build circle path");
    }

    let mut builder = tiny_skia::PathBuilder::new();

    builder.move_to(bounds.x + top_left, bounds.y);
    builder.line_to(bounds.x + bounds.width - top_right, bounds.y);

    if top_right > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bounds.width - top_right,
            bounds.y,
            bounds.x + bounds.width,
            bounds.y + top_right,
            top_right,
        );
    }

    maybe_line_to(
        &mut builder,
        bounds.x + bounds.width,
        bounds.y + bounds.height - bottom_right,
    );

    if bottom_right > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bounds.width,
            bounds.y + bounds.height - bottom_right,
            bounds.x + bounds.width - bottom_right,
            bounds.y + bounds.height,
            bottom_right,
        );
    }

    maybe_line_to(
        &mut builder,
        bounds.x + bottom_left,
        bounds.y + bounds.height,
    );

    if bottom_left > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bottom_left,
            bounds.y + bounds.height,
            bounds.x,
            bounds.y + bounds.height - bottom_left,
            bottom_left,
        );
    }

    maybe_line_to(&mut builder, bounds.x, bounds.y + top_left);

    if top_left > 0.0 {
        arc_to(
            &mut builder,
            bounds.x,
            bounds.y + top_left,
            bounds.x + top_left,
            bounds.y,
            top_left,
        );
    }

    builder.finish().expect("Build rounded rectangle path")
}

fn maybe_line_to(path: &mut tiny_skia::PathBuilder, x: f32, y: f32) {
    if path.last_point() != Some(tiny_skia::Point { x, y }) {
        path.line_to(x, y);
    }
}

fn arc_to(
    path: &mut tiny_skia::PathBuilder,
    x_from: f32,
    y_from: f32,
    x_to: f32,
    y_to: f32,
    radius: f32,
) {
    let svg_arc = kurbo::SvgArc {
        from: kurbo::Point::new(f64::from(x_from), f64::from(y_from)),
        to: kurbo::Point::new(f64::from(x_to), f64::from(y_to)),
        radii: kurbo::Vec2::new(f64::from(radius), f64::from(radius)),
        x_rotation: 0.0,
        large_arc: false,
        sweep: true,
    };

    match kurbo::Arc::from_svg_arc(&svg_arc) {
        Some(arc) => {
            arc.to_cubic_beziers(0.1, |p1, p2, p| {
                path.cubic_to(
                    p1.x as f32,
                    p1.y as f32,
                    p2.x as f32,
                    p2.y as f32,
                    p.x as f32,
                    p.y as f32,
                );
            });
        }
        None => {
            path.line_to(x_to, y_to);
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let x = ((x - a) / (b - a)).clamp(0.0, 1.0);

    x * x * (3.0 - 2.0 * x)
}

fn rounded_box_sdf(
    to_center: Vector,
    size: tiny_skia::Size,
    radii: &[f32],
) -> f32 {
    let radius = match (to_center.x > 0.0, to_center.y > 0.0) {
        (true, true) => radii[2],
        (true, false) => radii[1],
        (false, true) => radii[3],
        (false, false) => radii[0],
    };

    let x = (to_center.x.abs() - size.width() + radius).max(0.0);
    let y = (to_center.y.abs() - size.height() + radius).max(0.0);

    (x.powf(2.0) + y.powf(2.0)).sqrt() - radius
}

pub fn adjust_clip_mask(clip_mask: &mut ClipMask, bounds: Rectangle) {
    clip_mask.set(bounds);
}

/// A clip mask that always holds exactly one rectangle.
///
/// It remembers that rectangle, so moving the clip costs the area of the two
/// rectangles involved instead of the area of the whole surface.
#[derive(Debug)]
pub struct ClipMask {
    mask: tiny_skia::Mask,
    /// The pixels currently set: `(left, top, right, bottom)`.
    filled: Option<(usize, usize, usize, usize)>,
}

impl ClipMask {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        Some(Self {
            mask: tiny_skia::Mask::new(width, height)?,
            filled: None,
        })
    }

    pub fn mask(&self) -> &tiny_skia::Mask {
        &self.mask
    }

    /// Makes the mask cover `bounds` and nothing else.
    pub fn set(&mut self, bounds: Rectangle) {
        let width = self.mask.width() as usize;
        let height = self.mask.height() as usize;
        let next = (
            (pixel_edge(bounds.x).max(0.0) as usize).min(width),
            (pixel_edge(bounds.y).max(0.0) as usize).min(height),
            (pixel_edge(bounds.x + bounds.width).max(0.0) as usize).min(width),
            (pixel_edge(bounds.y + bounds.height).max(0.0) as usize)
                .min(height),
        );

        if self.filled == Some(next) {
            return;
        }

        if let Some(last) = self.filled.take() {
            self.paint(last, 0);
        }

        self.paint(next, 255);
        self.filled = Some(next);
    }

    fn paint(
        &mut self,
        (left, top, right, bottom): (usize, usize, usize, usize),
        coverage: u8,
    ) {
        if left >= right {
            return;
        }

        let width = self.mask.width() as usize;
        let data = self.mask.data_mut();

        for y in top..bottom {
            data[y * width + left..y * width + right].fill(coverage);
        }
    }
}

/// The pixel boundary nearest to `coordinate`.
///
/// Everything that decides which pixels a region covers (clearing it,
/// masking it, clipping a quad to it) goes through here, so that the three
/// always agree. Two neighboring regions then share no pixel and leave none
/// between them.
fn pixel_edge(coordinate: f32) -> f32 {
    (coordinate + 0.5).floor()
}

/// The pixels a region covers.
pub fn clip_pixels(clip_bounds: Rectangle) -> Rectangle {
    let left = pixel_edge(clip_bounds.x);
    let top = pixel_edge(clip_bounds.y);

    Rectangle {
        x: left,
        y: top,
        width: pixel_edge(clip_bounds.x + clip_bounds.width) - left,
        height: pixel_edge(clip_bounds.y + clip_bounds.height) - top,
    }
}

/// Whether the part of the quad inside `clip_bounds` has no rounded corner
/// and no shadow in it.
///
/// That is the case for a quad with square corners, and for a quad with
/// rounded corners when none of them is inside the clip.
fn has_only_straight_edges(
    quad: &Quad,
    physical_bounds: Rectangle,
    scale_factor: f32,
    clip_bounds: Rectangle,
) -> bool {
    if quad.shadow.color.a != 0.0 {
        return false;
    }

    let Some(visible) = physical_bounds.intersection(&clip_pixels(clip_bounds))
    else {
        return true;
    };

    let Rectangle {
        x,
        y,
        width,
        height,
    } = physical_bounds;
    let [top_left, top_right, bottom_right, bottom_left] =
        <[f32; 4]>::from(quad.border.radius).map(|radius| {
            (radius * scale_factor).min(width / 2.0).min(height / 2.0)
        });

    [
        (x, y, top_left),
        (x + width - top_right, y, top_right),
        (
            x + width - bottom_right,
            y + height - bottom_right,
            bottom_right,
        ),
        (x, y + height - bottom_left, bottom_left),
    ]
    .into_iter()
    .all(|(x, y, radius)| {
        radius == 0.0
            || !visible.intersects(&Rectangle {
                x,
                y,
                width: radius,
                height: radius,
            })
    })
}

/// Fills a quad and its border as rectangles clipped to `clip_bounds`.
///
/// Only right where [`has_only_straight_edges`] holds.
fn fill_straight_edges(
    quad: &Quad,
    color: Color,
    physical_bounds: Rectangle,
    scale_factor: f32,
    pixels: &mut tiny_skia::PixmapMut<'_>,
    clip_bounds: Rectangle,
) {
    let clip_bounds = clip_pixels(clip_bounds);

    let mut fill = |bounds: Rectangle, color: Color| {
        if color.a == 0.0 {
            return;
        }

        // An empty rectangle paints nothing, as an empty path does.
        let Some(visible) = bounds
            .intersection(&clip_bounds)
            .filter(|visible| visible.width > 0.0 && visible.height > 0.0)
        else {
            return;
        };

        let paint = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(into_color(color)),
            anti_alias: true,
            ..tiny_skia::Paint::default()
        };

        for (x, width) in pixel_spans(visible.x, visible.width) {
            for (y, height) in pixel_spans(visible.y, visible.height) {
                if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, width, height) {
                    pixels.fill_rect(
                        rect,
                        &paint,
                        tiny_skia::Transform::identity(),
                        None,
                    );
                }
            }
        }
    };

    fill(physical_bounds, color);

    let border = (quad.border.width * scale_factor)
        .min(physical_bounds.width / 2.0)
        .min(physical_bounds.height / 2.0);

    if border > 0.0 {
        let Rectangle {
            x,
            y,
            width,
            height,
        } = physical_bounds;
        let side = height - border * 2.0;

        for edge in [
            Rectangle {
                x,
                y,
                width,
                height: border,
            },
            Rectangle {
                x,
                y: y + height - border,
                width,
                height: border,
            },
            Rectangle {
                x,
                y: y + border,
                width: border,
                height: side,
            },
            Rectangle {
                x: x + width - border,
                y: y + border,
                width: border,
                height: side,
            },
        ] {
            fill(edge, quad.border.color);
        }
    }
}

/// Splits a span that covers parts of exactly two neighbouring pixels, and no
/// whole pixel between them, at the pixel boundary.
///
/// tiny-skia's anti-aliased rectangle fill hits a `debug_assert!(false)` for
/// such a span (it finds no whole pixel to fill between the two partial ones),
/// which aborts debug builds. A clipped sliver of a bordered quad is enough to
/// produce one. Each half lies inside a single pixel, which tiny-skia handles.
fn pixel_spans(start: f32, length: f32) -> Vec<(f32, f32)> {
    let end = start + length;
    let boundary = start.floor() + 1.0;

    if start.fract() != 0.0 && end > boundary && end < boundary + 1.0 {
        vec![(start, boundary - start), (boundary, end - boundary)]
    } else {
        vec![(start, length)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Border, Shadow};

    #[test]
    fn a_span_over_two_partial_pixels_is_split_at_the_boundary() {
        // 10.5..11.5: half of pixel 10 and half of pixel 11, no whole pixel
        assert_eq!(pixel_spans(10.5, 1.0), vec![(10.5, 0.5), (11.0, 0.5)]);
        // whole pixels in between, aligned starts, and single-pixel slivers stay whole
        assert_eq!(pixel_spans(10.5, 3.0), vec![(10.5, 3.0)]);
        assert_eq!(pixel_spans(10.0, 1.5), vec![(10.0, 1.5)]);
        assert_eq!(pixel_spans(10.25, 0.5), vec![(10.25, 0.5)]);
        assert_eq!(pixel_spans(10.5, 0.5), vec![(10.5, 0.5)]);
    }

    #[test]
    fn filling_a_sliver_over_two_partial_pixels_does_not_abort() {
        let mut pixmap = tiny_skia::Pixmap::new(32, 32).expect("pixmap");
        let quad = Quad {
            bounds: Rectangle {
                x: 10.5,
                y: 10.5,
                width: 1.0,
                height: 1.0,
            },
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        };
        fill_straight_edges(
            &quad,
            Color::WHITE,
            quad.bounds,
            1.0,
            &mut pixmap.as_mut(),
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 32.0,
                height: 32.0,
            },
        );
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }


    fn covered(mask: &ClipMask) -> usize {
        mask.mask().data().iter().filter(|c| **c == 255).count()
    }

    #[test]
    fn clip_mask_holds_one_rectangle_at_a_time() {
        let mut mask = ClipMask::new(20, 10).unwrap();

        mask.set(Rectangle::new([2.0, 1.0].into(), Size::new(5.0, 4.0)));
        assert_eq!(covered(&mask), 20);

        // Moving it leaves nothing of the old rectangle behind
        mask.set(Rectangle::new([10.0, 6.0].into(), Size::new(3.0, 2.0)));
        assert_eq!(covered(&mask), 6);
        assert_eq!(mask.mask().data()[6 * 20 + 10], 255);
        assert_eq!(mask.mask().data()[20 + 2], 0);

        // A rectangle reaching outside covers only what is inside
        mask.set(Rectangle::new([-5.0, -5.0].into(), Size::new(100.0, 7.0)));
        assert_eq!(covered(&mask), 20 * 2);
    }

    #[test]
    fn neighboring_regions_share_no_pixel_and_leave_none() {
        // The seams of regions that meet on half pixels, as they do at a
        // scale factor of 1.5
        for seam in [3.0, 3.25, 3.5, 3.75, 4.49, 4.5, 4.51] {
            let left = clip_pixels(Rectangle::new(
                [0.0, 0.0].into(),
                Size::new(seam, 4.0),
            ));
            let right = clip_pixels(Rectangle::new(
                [seam, 0.0].into(),
                Size::new(10.0 - seam, 4.0),
            ));

            assert_eq!(left.x + left.width, right.x, "seam at {seam}");
            assert_eq!(right.x + right.width, 10.0, "seam at {seam}");
        }
    }

    fn quad(radius: f32) -> Quad {
        Quad {
            bounds: Rectangle::new([10.0, 10.0].into(), Size::new(100.0, 60.0)),
            border: Border {
                radius: radius.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }

    #[test]
    fn straight_edges_are_found_where_no_corner_is_visible() {
        let straight = |quad: &Quad, clip: Rectangle| {
            has_only_straight_edges(quad, quad.bounds, 1.0, clip)
        };
        let clip = |x, y, w, h| Rectangle::new([x, y].into(), Size::new(w, h));

        // Square corners: always
        assert!(straight(&quad(0.0), clip(0.0, 0.0, 200.0, 200.0)));
        // Rounded corners, all of it visible: never
        assert!(!straight(&quad(5.0), clip(0.0, 0.0, 200.0, 200.0)));
        // Rounded corners, only the middle visible
        assert!(straight(&quad(5.0), clip(40.0, 0.0, 20.0, 200.0)));
        assert!(straight(&quad(5.0), clip(0.0, 30.0, 200.0, 10.0)));
        // One corner inside the clip
        assert!(!straight(&quad(5.0), clip(100.0, 60.0, 50.0, 50.0)));
        // A shadow is never straight edges
        let mut shadowed = quad(0.0);
        shadowed.shadow.color = Color::BLACK;
        assert!(!straight(&shadowed, clip(0.0, 0.0, 200.0, 200.0)));
    }

    #[test]
    fn a_clipped_quad_fills_only_the_clip_and_keeps_its_border() {
        let mut pixmap = tiny_skia::Pixmap::new(40, 20).unwrap();
        let mut quad = quad(0.0);
        quad.bounds = Rectangle::new([0.0, 0.0].into(), Size::new(40.0, 20.0));
        quad.border.width = 1.0;
        quad.border.color = Color::WHITE;

        // Only columns 10..20 are inside the clip
        fill_straight_edges(
            &quad,
            Color::from_rgb8(0, 0, 255),
            quad.bounds,
            1.0,
            &mut pixmap.as_mut(),
            Rectangle::new([10.0, 0.0].into(), Size::new(10.0, 20.0)),
        );

        let pixel = |x: u32, y: u32| pixmap.pixel(x, y).unwrap();

        assert_eq!(pixel(9, 10).alpha(), 0);
        assert_eq!(pixel(20, 10).alpha(), 0);
        // Colors are stored with red and blue swapped, as the window wants
        assert_eq!(pixel(15, 10).red(), 255);
        assert_eq!(pixel(15, 10).green(), 0);
        // The top and bottom borders run through the clip
        assert_eq!(pixel(15, 0).green(), 255);
        assert_eq!(pixel(15, 19).green(), 255);
    }
}
