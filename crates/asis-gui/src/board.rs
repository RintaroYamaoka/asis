//! 真ん中。ボード(issue ごとの段にカード)とリスト

use super::*;
use iced::widget::{column, row};

/// ボードのカードの高さ。全部 同じ大きさにする(PR がまだ無い issue の 1 枚も同じ)
pub const CARD_H: f32 = 100.0;
/// カードの幅。列が広いときは、この幅のカードを横に並べる(カードの形を、窓の幅で変えない)
const CARD_MIN: f32 = 190.0;
const CARD_MAX: f32 = 260.0;
/// カードとカードの間
const CARD_GAP: f32 = 9.0;

/// 列の幅に、カードを横に何枚 並べるかと、1 枚の幅
fn fit(col_w: f32) -> (usize, f32) {
    let n = (((col_w + CARD_GAP) / (CARD_MIN + CARD_GAP)).floor() as usize).max(1);
    let w = ((col_w - CARD_GAP * (n as f32 - 1.0)) / n as f32).min(CARD_MAX);
    (n, w)
}

/// 1 つの列の中に、カードを並べる。列が広ければ横にも並べる
fn cards<'a>(list: Vec<Element<'a, Msg>>, col_w: f32) -> Element<'a, Msg> {
    let (n, w) = fit(col_w);
    let mut out = column![].spacing(CARD_GAP);
    let mut line = row![].spacing(CARD_GAP);
    let mut in_line = 0;
    for card in list {
        line = line.push(container(card).width(w));
        in_line += 1;
        if in_line == n {
            out = out.push(std::mem::replace(&mut line, row![].spacing(CARD_GAP)));
            in_line = 0;
        }
    }
    if in_line > 0 {
        out = out.push(line);
    }
    out.into()
}

pub fn card_box<'a>(body: Element<'a, Msg>, line: Color, on: bool, msg: Msg) -> Element<'a, Msg> {
    mouse_area(
        container(body)
            .padding([9, 12])
            .width(Length::Fill)
            .height(CARD_H)
            .clip(true)
            .style(boxed(line, if on { PANEL } else { Color::TRANSPARENT })),
    )
    .on_press(msg)
    .into()
}

/// カード = PR 1 本。細く、縦に
pub fn pr_card<'a>(
    app: &App,
    v: &'a RepoView,
    r: &'a Row,
    p: &'a asis::view::PrLine,
) -> Element<'a, Msg> {
    let key = key_of(v, r);
    let on = app.selected.as_ref() == Some(&key) && app.pr == Some(p.number);
    let mine = p.turn.contains(&v.viewer);
    let line = if mine {
        app.accent
    } else if on {
        BRIGHT
    } else {
        FRAME
    };
    let body = column![
        row![
            container(
                cap(format!("{} #{} · {}", short(&v.repo), p.number, p.author))
                    .wrapping(text::Wrapping::None)
            )
            .width(Length::Fill)
            .height(16)
            .clip(true),
            age_of(&p.age, p.stale),
        ]
        .spacing(8),
        // 題は 2 行ぶんの高さに固定する。カードの形をそろえるため(続きは中身を開いて読む)
        container(
            text(p.title.clone())
                .size(14)
                .color(TEXT)
                .wrapping(text::Wrapping::WordOrGlyph)
        )
        .height(37)
        .clip(true),
        row![
            container(chip(p)).width(Length::Fill).clip(true),
            if mine {
                text("You").size(13).color(app.accent)
            } else if p.turn.is_empty() {
                text("—").size(13).color(DIM)
            } else {
                // 番の人が多いときは、1 人目と残りの人数(全員は、開いた中身に出る)
                text(match p.turn.as_slice() {
                    [one] => one.clone(),
                    [first, rest @ ..] => format!("{first} +{}", rest.len()),
                    [] => String::new(),
                })
                .size(13)
                .color(DIM)
                .wrapping(text::Wrapping::None)
            },
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(5);
    card_box(body.into(), line, on, Msg::Select(key, Some(p.number)))
}

/// Board の左の一覧の幅
const LEFT_W: f32 = 290.0;
/// 段の頭の、開け閉めの印の幅
const CHEV: f32 = 26.0;

/// 段の開け閉めの印。開いていれば「-」、たたんであれば「+」
fn chevron<'a>(open: bool, msg: Msg) -> Element<'a, Msg> {
    button(text(if open { "-" } else { "+" }).size(14).color(DIM))
        .padding([6, 8])
        .width(CHEV)
        .style(|_, status| button::Style {
            background: None,
            text_color: if matches!(status, button::Status::Hovered) {
                TEXT
            } else {
                DIM
            },
            ..button::Style::default()
        })
        .on_press(msg)
        .into()
}

/// 状態の列を 3 つ並べる(In progress / In review / Merged)
fn cells<'a>(col_w: f32, mut pick: impl FnMut(usize) -> Element<'a, Msg>) -> Element<'a, Msg> {
    let mut r = row![].spacing(14);
    for i in 0..PR_COLUMNS.len() {
        r = r.push(container(pick(i)).width(col_w));
    }
    r.into()
}

/// PR がまだ無い行に添える一言
fn no_pr_note(r: &Row) -> String {
    match (r.local.first(), r.kind) {
        (_, Kind::Local) => r.why.clone(),
        (Some(l), _) => format!("Local: {}", l.branch),
        (None, _) if r.stage == Stage::Doing => "Branch pushed · no PR yet".to_string(),
        _ => "No PR yet".to_string(),
    }
}

/// 1 件を 1 行で。issue・文書のタスク・つながっていない PR・手元の作業、どれも同じ形。
/// たたんであるときは、PR の状態を札で並べる(開いているときは、下のカードが言う)
fn line<'a>(
    app: &App,
    v: &'a RepoView,
    r: &'a Row,
    open: bool,
    has_cards: bool,
    pr: Option<u64>,
) -> Element<'a, Msg> {
    let key = key_of(v, r);
    let on = app.selected.as_ref() == Some(&key) && (app.pr.is_none() || app.pr == pr);
    let mine = !open && r.is_mine(&v.viewer);
    let accent = app.accent;
    let mut body = row![
        container(cap(label_of(r))).width(58),
        container(
            text(r.title.clone())
                .size(15)
                .color(TEXT)
                .wrapping(text::Wrapping::None)
        )
        .width(Length::Fill)
        .height(20)
        .clip(true),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if !open {
        if has_cards {
            let mut chips = row![].spacing(5);
            for p in r.prs.iter().take(4) {
                chips = chips.push(chip(p));
            }
            body = body.push(chips);
        } else {
            body = body.push(cap(no_pr_note(r)));
        }
    }
    body = body
        .push(
            container(turn_of(r, &v.viewer, accent))
                .width(70)
                .align_x(Alignment::End),
        )
        .push(
            container(age_of(&r.age, r.stale))
                .width(40)
                .align_x(Alignment::End),
        );
    button(body)
        .padding([8, 10])
        .width(Length::Fill)
        .style(move |_, status| button::Style {
            background: Some(
                if on || matches!(status, button::Status::Hovered) {
                    PANEL
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: TEXT,
            border: Border {
                color: if mine { accent } else { Color::TRANSPARENT },
                width: 1.0,
                radius: 5.0.into(),
            },
            ..button::Style::default()
        })
        .on_press(Msg::Select(key, pr))
        .into()
}

/// 段 = 1 件の行と、開いていれば、その PR のカードを状態の列に並べたもの
fn lane<'a>(app: &'a App, v: &'a RepoView, r: &'a Row, col_w: f32) -> Element<'a, Msg> {
    let key = key_of(v, r);
    let has_cards = r.prs.iter().any(|p| pr_column(p.mark).is_some());
    let open = opened(app, &key, has_cards);
    let head = row![
        if has_cards {
            chevron(open, Msg::Fold(key))
        } else {
            Space::new().width(CHEV).into()
        },
        line(app, v, r, open, has_cards, None),
    ]
    .align_y(Alignment::Center);
    let mut c = column![head];
    if open {
        let lane = cells(col_w, |i| {
            cards(
                r.prs
                    .iter()
                    .filter(|p| pr_column(p.mark) == Some(i))
                    .map(|p| pr_card(app, v, r, p))
                    .collect(),
                col_w,
            )
        });
        c = c.push(container(lane).padding(iced::Padding {
            top: 4.0,
            right: 0.0,
            bottom: 10.0,
            left: CHEV + 10.0,
        }));
    }
    c.push(rule()).into()
}

/// 束の見出し(名前と件数)
fn heading<'a>(name: &str, n: usize) -> Element<'a, Msg> {
    row![
        text(name.to_string()).size(13).color(TEXT),
        cap(n.to_string())
    ]
    .spacing(9)
    .align_y(Alignment::Center)
    .into()
}

/// issue につながっていない PR の束。開いていればカードを列に、たたんであれば 1 本 1 行
fn loose_lanes<'a>(app: &'a App, free: &[Item<'a>], col_w: f32) -> Element<'a, Msg> {
    let open = app.fold_loose.unwrap_or(app.look == Look::Board);
    let mut c = column![row![
        chevron(open, Msg::FoldLoose),
        heading(asis::view::LOOSE, free.len())
    ]
    .padding([8, 0])
    .align_y(Alignment::Center)];
    if open {
        let lane = cells(col_w, |i| {
            cards(
                free.iter()
                    .filter(|(_, r)| column_of(r.stage).checked_sub(1) == Some(i))
                    .filter_map(|(v, r)| r.prs.first().map(|p| pr_card(app, v, r, p)))
                    .collect(),
                col_w,
            )
        });
        c = c.push(container(lane).padding(iced::Padding {
            top: 2.0,
            right: 0.0,
            bottom: 12.0,
            left: CHEV + 10.0,
        }));
    } else {
        for (v, r) in free {
            c = c.push(
                row![
                    Space::new().width(CHEV),
                    line(app, v, r, false, true, Some(r.number))
                ]
                .align_y(Alignment::Center),
            );
        }
    }
    c.into()
}

/// 状態の列の見出し。数字は、その列にあるカードの枚数
fn column_heads<'a>(tasks: &[Item<'a>], free: &[Item<'a>], col_w: f32) -> Element<'a, Msg> {
    let totals: [usize; 3] = std::array::from_fn(|i| {
        let in_tasks: usize = tasks
            .iter()
            .map(|(_, r)| {
                r.prs
                    .iter()
                    .filter(|p| pr_column(p.mark) == Some(i))
                    .count()
            })
            .sum();
        in_tasks
            + free
                .iter()
                .filter(|(_, r)| column_of(r.stage).checked_sub(1) == Some(i))
                .count()
    });
    cells(col_w, move |i| {
        column![heading(PR_COLUMNS[i], totals[i]), rule()]
            .spacing(8)
            .into()
    })
}

/// Board の左の一覧の 1 件(issue か、文書のタスク)
fn issue_row<'a>(app: &App, v: &'a RepoView, r: &'a Row, closed: bool) -> Element<'a, Msg> {
    let key = key_of(v, r);
    let on = app.selected.as_ref() == Some(&key) && app.pr.is_none();
    let mine = !closed && r.is_mine(&v.viewer);
    let under: Element<'a, Msg> = if closed {
        cap(match r.prs.len() {
            0 => asis::view::CLOSED.to_string(),
            n => format!("{} · {n} PR", asis::view::CLOSED),
        })
        .into()
    } else {
        row![
            cap(state_word(r.stage)),
            cap("·"),
            turn_of(r, &v.viewer, app.accent).size(12)
        ]
        .spacing(5)
        .into()
    };
    let body = column![
        row![
            cap(label_of(r)).size(13),
            container(
                text(r.title.clone())
                    .size(14)
                    .color(TEXT)
                    .wrapping(text::Wrapping::None)
            )
            .width(Length::Fill)
            .height(19)
            .clip(true),
        ]
        .spacing(7)
        .align_y(Alignment::Center),
        row![
            container(under).width(Length::Fill).clip(true),
            age_of(&r.age, r.stale)
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(4);
    pressable(
        body.into(),
        if mine {
            app.accent
        } else if on {
            BRIGHT
        } else {
            FRAME
        },
        on,
        [8, 10],
        Msg::Select(key, None),
    )
}

/// Board の左:issue の一覧。開いているものを全部(PR がまだ無いものも、進んでいるものも)。
/// 終わったものは Closed に移る。見出しは、文書のタスクが 1 つでもあれば Plans
fn issue_list<'a>(app: &'a App, tasks: &[Item<'a>], closed: &[Item<'a>]) -> Element<'a, Msg> {
    let open: Vec<&Item<'a>> = tasks
        .iter()
        .filter(|(_, r)| r.kind != Kind::Local)
        .collect();
    let name = list_name(
        open.iter()
            .map(|(_, r)| r.kind)
            .chain(closed.iter().map(|(_, r)| r.kind)),
    );
    let small = |label: String, on: bool, msg: Msg| -> Element<'a, Msg> {
        button(text(label).size(12).color(if on { TEXT } else { DIM }))
            .padding([3, 9])
            .style(move |_, _| button::Style {
                background: Some(
                    if on {
                        Color::from_rgb(0.11, 0.118, 0.11)
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
    };
    let head = row![
        text(name).size(13).color(TEXT),
        Space::new().width(Length::Fill),
        small(
            format!("Open {}", open.len()),
            !app.closed_tab,
            Msg::IssuesTab(false)
        ),
        small(
            format!("Closed {}", closed.len()),
            app.closed_tab,
            Msg::IssuesTab(true)
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    let mut list = column![].spacing(6);
    if app.closed_tab {
        if closed.is_empty() {
            list = list.push(cap("Nothing closed yet"));
        }
        for (v, r) in closed {
            list = list.push(issue_row(app, v, r, true));
        }
    } else {
        if open.is_empty() {
            list = list.push(cap("Nothing open"));
        }
        for (v, r) in open {
            list = list.push(issue_row(app, v, r, false));
        }
    }
    column![
        container(head).padding(iced::Padding {
            top: 12.0,
            right: 10.0,
            bottom: 9.0,
            left: 14.0
        }),
        scrollable(container(list).padding(iced::Padding {
            top: 0.0,
            right: 10.0,
            bottom: 16.0,
            left: 10.0
        }))
        .height(Length::Fill),
    ]
    .width(LEFT_W)
    .into()
}

/// 真ん中:Board か List。中身は同じで、issue 1 件 = 1 行、PR が動いていればカード。
/// - Board:左に issue の一覧(Open / Closed)、右に Pull requests(状態の列。PR のある issue の段を開いた形)
/// - List:状態の束に 1 件 1 行(全部たたんだ形)。行の頭の「+」で、その場に段が開く
///
/// 列は窓の幅いっぱいに 3 つ。カードの幅は 190〜260 のままで、列が広ければ横に並べる。
pub fn middle<'a>(app: &'a App, width: f32) -> Element<'a, Msg> {
    let all = shown(app);
    let done = closed_items(app);
    if all.is_empty() && done.is_empty() {
        return panels::empty(app);
    }
    let tasks = lanes(&all);
    let free = loose(&all);
    let board = app.look == Look::Board;
    let right_w = if board { width - LEFT_W - 1.0 } else { width };
    let col_w = ((right_w - 6.0 - 22.0 - CHEV - 10.0 - 28.0) / 3.0)
        .max(CARD_MIN)
        .floor();
    let any_open = tasks.iter().any(|(v, r)| {
        opened(
            app,
            &key_of(v, r),
            r.prs.iter().any(|p| pr_column(p.mark).is_some()),
        )
    }) || (!free.is_empty() && app.fold_loose.unwrap_or(board));

    let mut body = column![].width(Length::Fill);
    if board {
        // 左の一覧にしか出さないもの = まだ何も動いていない issue(PR もブランチも無い)
        for (v, r) in tasks.iter().filter(|(_, r)| column_of(r.stage) != 0) {
            body = body.push(lane(app, v, r, col_w));
        }
    } else {
        for i in LIST_ORDER {
            let here: Vec<&Item<'a>> = tasks
                .iter()
                .filter(|(_, r)| column_of(r.stage) == i)
                .collect();
            if here.is_empty() {
                continue;
            }
            body = body.push(container(heading(state_name(i), here.len())).padding(
                iced::Padding {
                    top: 14.0,
                    right: 0.0,
                    bottom: 6.0,
                    left: CHEV + 10.0,
                },
            ));
            for (v, r) in here {
                body = body.push(lane(app, v, r, col_w));
            }
        }
    }
    if !free.is_empty() {
        body = body.push(loose_lanes(app, &free, col_w));
    }
    let pad = iced::Padding {
        top: 0.0,
        right: 22.0,
        bottom: 16.0,
        left: 6.0,
    };
    let mut right = column![];
    if board {
        let cards: usize = tasks
            .iter()
            .chain(free.iter())
            .map(|(_, r)| r.prs.iter().filter(|p| pr_column(p.mark).is_some()).count())
            .sum();
        right = right.push(
            container(heading("Pull requests", cards)).padding(iced::Padding {
                top: 14.0,
                right: 22.0,
                bottom: 0.0,
                left: 6.0 + CHEV + 10.0,
            }),
        );
    }
    if any_open {
        right = right.push(
            container(column_heads(&tasks, &free, col_w)).padding(iced::Padding {
                top: 12.0,
                right: 22.0,
                bottom: 4.0,
                left: 6.0 + CHEV + 10.0,
            }),
        );
    }
    right = right.push(
        scrollable(container(body).padding(pad))
            .width(Length::Fill)
            .height(Length::Fill),
    );
    if board {
        row![issue_list(app, &tasks, &done), vrule(), right].into()
    } else {
        right.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_column_puts_cards_side_by_side_instead_of_stretching_them() {
        // ふつうの窓:1 枚ずつ。幅は列いっぱい
        assert_eq!(fit(236.0), (1, 236.0));
        // 1 枚しか入らない広さでは、カードを伸ばさない
        assert_eq!(fit(380.0), (1, CARD_MAX));
        // 2 枚 入る広さになったら、横に並べる
        assert_eq!(fit(389.0), (2, 190.0));
        assert_eq!(fit(600.0), (3, 194.0));
        // 狭すぎても 1 枚は出す
        assert_eq!(fit(120.0).0, 1);
    }
}
