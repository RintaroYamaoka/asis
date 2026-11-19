//! 見た目の部品。色は 1 色だけ(設定で選ぶ)。面を塗らず、線で区切る

use super::*;

pub const BG: Color = Color::from_rgb(0.051, 0.055, 0.051);
pub const PANEL: Color = Color::from_rgb(0.086, 0.094, 0.086);
pub const LINE: Color = Color::from_rgb(0.122, 0.129, 0.122);
pub const LINE2: Color = Color::from_rgb(0.2, 0.212, 0.2);
pub const BRIGHT: Color = Color::from_rgb(0.42, 0.44, 0.415);
pub const TEXT: Color = Color::from_rgb(0.925, 0.933, 0.914);
pub const DIM: Color = Color::from_rgb(0.553, 0.576, 0.549);
pub const BAD: Color = Color::from_rgb(0.91, 0.51, 0.435);

pub fn boxed(line: Color, fill: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(fill.into()),
        border: Border {
            color: line,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        ..container::Style::default()
    }
}

pub fn rule<'a>() -> Element<'a, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .style(fill(LINE))
        .into()
}

pub fn vrule<'a>() -> Element<'a, Msg> {
    container(Space::new().width(1).height(Length::Fill))
        .style(fill(LINE))
        .into()
}

pub fn cap<'a>(s: impl Into<String>) -> iced::widget::Text<'a> {
    text(s.into()).size(12).color(DIM)
}

/// 番が付いてからの時間。3 日 動きが無いものは、色を変えて印にする
pub fn age_of<'a>(age: &str, stale: bool) -> iced::widget::Text<'a> {
    cap(age.to_string()).color(if stale { BAD } else { DIM })
}

/// 線の枠の、押せる字。選ばれているものは明るく
pub fn seg<'a>(label: String, on: bool, msg: Msg) -> Element<'a, Msg> {
    button(text(label).size(14).color(if on { TEXT } else { DIM }))
        .padding([6, 12])
        .style(move |_, status| button::Style {
            background: Some(
                if on {
                    Color::from_rgb(0.11, 0.118, 0.11)
                } else if matches!(status, button::Status::Hovered) {
                    PANEL
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: TEXT,
            border: Border {
                color: LINE2,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..button::Style::default()
        })
        .on_press(msg)
        .into()
}

pub fn turn_of<'a>(row: &Row, viewer: &str, accent: Color) -> iced::widget::Text<'a> {
    if row.is_mine(viewer) {
        text("You").size(13).color(accent)
    } else if row.turn.is_empty() {
        text("—").size(13).color(DIM)
    } else {
        text(row.turn.join("・")).size(13).color(DIM)
    }
}

pub fn chip<'a>(p: &asis::view::PrLine) -> Element<'a, Msg> {
    let bad = p.mark == "!";
    container(text(word_of(p)).size(12).color(if bad { BAD } else { DIM }))
        .padding([1, 7])
        .style(boxed(
            if bad {
                Color::from_rgb(0.35, 0.18, 0.16)
            } else {
                LINE2
            },
            Color::TRANSPARENT,
        ))
        .into()
}

pub const FRAME: Color = Color::from_rgb(0.165, 0.173, 0.165);

/// 押せる枠。自分の番は枠を色に、選んでいるものは明るい灰色の枠に
pub fn pressable<'a>(
    body: Element<'a, Msg>,
    line: Color,
    on: bool,
    pad: [u16; 2],
    msg: Msg,
) -> Element<'a, Msg> {
    mouse_area(
        container(body)
            .padding(pad)
            .width(Length::Fill)
            .style(boxed(line, if on { PANEL } else { Color::TRANSPARENT })),
    )
    .on_press(msg)
    .into()
}

/// 押してほしいボタン。色の線の枠
pub fn primary<'a>(label: &str, accent: Color, msg: Option<Msg>) -> Element<'a, Msg> {
    let b = button(text(label.to_string()).size(14).color(accent))
        .padding([6, 12])
        .style(move |_, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered) {
                    PANEL
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: accent,
            border: Border {
                color: accent,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..button::Style::default()
        });
    match msg {
        Some(m) => b.on_press(m).into(),
        None => b.into(),
    }
}

pub fn field<'a>(hint: &str, value: &str, on: fn(String) -> Msg, send: Msg) -> Element<'a, Msg> {
    text_input(hint, value)
        .on_input(on)
        .on_submit(send)
        .size(14)
        .padding(9)
        .style(|_, status| text_input::Style {
            background: Color::TRANSPARENT.into(),
            border: Border {
                color: if matches!(status, text_input::Status::Focused { .. }) {
                    BRIGHT
                } else {
                    LINE2
                },
                width: 1.0,
                radius: 5.0.into(),
            },
            icon: DIM,
            placeholder: DIM,
            value: TEXT,
            selection: LINE2,
        })
        .into()
}
