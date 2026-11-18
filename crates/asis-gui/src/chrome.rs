//! 窓そのもの:どの出し方で開くか・どの大きさで描くか・上の帯。
//!
//! 画面の中身(`lib.rs`)は、ここが決めたものを知らずに描く。OS ごとの違いは、ここに集める。
//!
//! - Windows・macOS・ふつうの Linux: 帯は OS が描く。大きさも OS が伝えてくる
//! - WSL: Windows 版が入っていれば、窓はそちらに任せる(`hand_off_to_windows`)。
//!   画面は画面のある OS で動かすのが、いちばん速くて素直(仕事は WSL の中のまま。`asis::host`)
//! - WSL で、Windows 版が無いとき: Wayland で開いて、帯を自分で描く。大きさは Windows に聞く
//!   - WSL の X11 の窓は、OS が描く帯が白い。帯を消して自分で動かすと、動かすたびに画面の側と行き来して重く、最大化も効かない
//!   - Wayland なら、動かす・大きさを変える・最大化は、頼むだけで画面の側(compositor)がやる
//!   - WSL の窓には、Windows の表示の拡大(150% など)が伝わらないので、Windows の設定を読む
use super::{boxed, fill, BG, DIM, LINE, LINE2, PANEL};
use iced::widget::{button, column, container, mouse_area, row, stack, text, Space};
use iced::window::{self, Direction};
use iced::{mouse, Alignment, Element, Length, Task};

/// 窓の出し方。起動のときに 1 回 決める
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Platform {
    /// 上の帯を自分で描く
    pub own_bar: bool,
    /// 描く大きさの倍率。OS が伝えてくる倍率に、さらに掛ける
    pub scale: f32,
}

impl Platform {
    /// 環境から決める。WSL で X11 に戻すなら `ASIS_BACKEND=x11`(帯は OS が描く)。
    /// `ASIS_SCALE=1.25` で倍率を決め打ちできる
    pub fn detect() -> Platform {
        let wsl = std::env::var_os("WSL_DISTRO_NAME").is_some();
        let wayland = wsl
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("ASIS_BACKEND").as_deref() != Ok("x11");
        if wsl && !wayland {
            // winit は、これがあると Wayland を選ぶ
            std::env::remove_var("WAYLAND_DISPLAY");
        }
        let scale = std::env::var("ASIS_SCALE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|s| (0.5..=4.0).contains(s))
            .or_else(|| wsl.then(windows_scale).flatten())
            .unwrap_or(1.0);
        Platform {
            own_bar: wayland,
            scale,
        }
    }
}

/// WSL から開かれたら、Windows 版の窓に任せて、閉じるまで待つ。任せたら、その終わり方を返す。
/// 任せないのは:WSL でない・Windows 版が入っていない・`ASIS_BACKEND` で WSL の窓を選んでいる
pub fn hand_off_to_windows() -> Option<std::process::ExitStatus> {
    let distro = std::env::var("WSL_DISTRO_NAME").ok()?;
    if std::env::var_os("ASIS_BACKEND").is_some() {
        return None;
    }
    let exe = windows_exe()?;
    // 試すときの指定(窓の題・大きさ)は、Windows の側へも渡す
    let pass: Vec<&str> = ["ASIS_TITLE", "ASIS_WINDOW", "ASIS_SCALE", "ASIS_POP"]
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some())
        .collect();
    let wslenv = [std::env::var("WSLENV").unwrap_or_default(), pass.join(":")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(":");
    std::process::Command::new(exe)
        .args(["--wsl", &distro])
        .args(std::env::args_os().skip(1))
        .env("WSLENV", wslenv)
        .status()
        .ok()
}

/// この窓と同じ名前の、Windows 版の置き場(`install.ps1` が入れる所)。無ければ None。
/// `ASIS_WINDOWS_EXE` で、別の置き場を指せる
fn windows_exe() -> Option<std::path::PathBuf> {
    if let Some(exe) = std::env::var_os("ASIS_WINDOWS_EXE") {
        return Some(exe.into()).filter(|p: &std::path::PathBuf| p.is_file());
    }
    let name = std::env::current_exe().ok()?.file_stem()?.to_owned();
    let out = windows_tool("cmd.exe")
        .args(["/c", "echo %LOCALAPPDATA%"])
        .current_dir("/mnt/c")
        .output()
        .ok()?;
    let local = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let out = asis::host::local("wslpath")
        .args(["-u", &local])
        .output()
        .ok()?;
    let mut exe = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    exe.extend(["Programs", "asis"]);
    exe.push(name);
    exe.set_extension("exe");
    exe.is_file().then_some(exe)
}

/// Windows の道具。PATH に Windows の場所が無い開き方(メニューからなど)でも呼べるように、置き場でも探す
fn windows_tool(name: &str) -> std::process::Command {
    let fixed = std::path::Path::new("/mnt/c/Windows/System32").join(name);
    if fixed.is_file() {
        asis::host::local(&fixed.to_string_lossy())
    } else {
        asis::host::local(name)
    }
}

/// Windows の表示の拡大(1.0 = 100%)。WSL の中から、Windows の設定を聞く
fn windows_scale() -> Option<f32> {
    let out = windows_tool("reg.exe")
        .args([
            "query",
            r"HKCU\Control Panel\Desktop\WindowMetrics",
            "/v",
            "AppliedDPI",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    scale_of(&String::from_utf8_lossy(&out.stdout))
}

/// `reg query` の返事(`AppliedDPI    REG_DWORD    0x90`)から、拡大を読む。96 が 100%
fn scale_of(reply: &str) -> Option<f32> {
    let hex = reply
        .split_whitespace()
        .find_map(|w| w.strip_prefix("0x"))?;
    let dpi = u32::from_str_radix(hex, 16).ok()?;
    (96..=480).contains(&dpi).then(|| dpi as f32 / 96.0)
}

/// 自分で描いた帯からの、窓の操作
#[derive(Debug, Clone, Copy)]
pub enum Win {
    Drag,
    Minimize,
    Maximize,
    Close,
    Resize(Direction),
}

/// 自分で描く帯が覚えておくこと
#[derive(Debug, Default)]
pub struct Bar {
    /// 最大化を頼んである。
    /// 画面の側に「今 最大化しているか」を聞くと、WSL では最大化した直後に古い答えが返り、
    /// 戻すための 1 回目が効かない。頼んだことを自分で覚えておく
    maximized: bool,
}

impl Bar {
    /// 窓の操作を、画面の側に頼む
    pub fn perform<M: Send + 'static>(&mut self, op: Win) -> Task<M> {
        if let Win::Maximize = op {
            self.maximized = !self.maximized;
        }
        let maximized = self.maximized;
        window::latest().and_then(move |id| match op {
            Win::Drag => window::drag(id),
            Win::Minimize => window::minimize(id, true),
            Win::Maximize => window::maximize(id, maximized),
            Win::Close => window::close(id),
            Win::Resize(direction) => window::drag_resize(id, direction),
        })
    }
}

/// 帯の高さ
const BAR: f32 = 32.0;
/// 端をつかめる幅
const GRIP: f32 = 5.0;
/// 角をつかめる長さ
const CORNER: f32 = 14.0;

/// 中身に、上の帯と、端をつかんで大きさを変える所を足す
pub fn frame<'a, M: Clone + 'a>(
    title: &'a str,
    content: Element<'a, M>,
    on: impl Fn(Win) -> M + Copy + 'a,
) -> Element<'a, M> {
    let key = |label: &'static str, op: Win| {
        button(text(label).size(13).color(DIM))
            .padding([6, 14])
            .style(|_, status| button::Style {
                background: matches!(status, button::Status::Hovered).then(|| PANEL.into()),
                text_color: DIM,
                ..button::Style::default()
            })
            .on_press(on(op))
    };
    // 帯をつかむと動く。2 回 押すと最大化(もう一度で戻る)
    let handle = mouse_area(
        container(text(title).size(13).color(DIM))
            .padding([0, 14])
            .center_y(Length::Fill)
            .width(Length::Fill),
    )
    .on_press(on(Win::Drag))
    .on_double_click(on(Win::Maximize));
    let bar = container(
        row![
            handle,
            key("—", Win::Minimize),
            key("□", Win::Maximize),
            key("✕", Win::Close),
        ]
        .align_y(Alignment::Center),
    )
    .height(BAR)
    .style(fill(BG));
    let body = column![
        bar,
        container(Space::new().width(Length::Fill).height(1)).style(fill(LINE)),
        content,
    ];
    stack![
        container(body).style(boxed(LINE2, BG)).padding(1),
        grips(on),
    ]
    .into()
}

/// 窓の端と角。つかむと、その向きに大きさが変わる。上の端は帯なので、角だけ
fn grips<'a, M: Clone + 'a>(on: impl Fn(Win) -> M + Copy + 'a) -> Element<'a, M> {
    use mouse::Interaction::{
        ResizingDiagonallyDown as Down, ResizingDiagonallyUp as Up, ResizingHorizontally as Wide,
        ResizingVertically as Tall,
    };
    let grip = |w: Length, h: Length, direction: Direction, cursor: mouse::Interaction| {
        mouse_area(Space::new().width(w).height(h))
            .interaction(cursor)
            .on_press(on(Win::Resize(direction)))
    };
    let (edge, corner, rest) = (Length::Fixed(GRIP), Length::Fixed(CORNER), Length::Fill);
    column![
        row![
            grip(corner, edge, Direction::NorthWest, Down),
            Space::new().width(rest),
            grip(corner, edge, Direction::NorthEast, Up),
        ],
        row![
            grip(edge, rest, Direction::West, Wide),
            Space::new().width(rest),
            grip(edge, rest, Direction::East, Wide),
        ]
        .height(rest),
        row![
            grip(corner, edge, Direction::SouthWest, Up),
            grip(rest, edge, Direction::South, Tall),
            grip(corner, edge, Direction::SouthEast, Down),
        ],
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_scale_is_read_from_the_registry_reply() {
        let reply = "\r\nHKEY_CURRENT_USER\\Control Panel\\Desktop\\WindowMetrics\r\n    AppliedDPI    REG_DWORD    0x90\r\n";
        assert_eq!(scale_of(reply), Some(1.5));
        assert_eq!(scale_of("    AppliedDPI    REG_DWORD    0x60"), Some(1.0));
        // 読めない返事・ありえない値は使わない
        assert_eq!(scale_of("ERROR"), None);
        assert_eq!(scale_of("AppliedDPI REG_DWORD 0x1"), None);
    }
}
