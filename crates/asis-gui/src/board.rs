//! 真ん中。ボード(issue ごとの段にカード)とリスト

use super::*;
use iced::widget::{column, row};

/// ボードのカードの高さ。全部 同じ大きさにする(PR がまだ無い issue の 1 枚も同じ)。試作と同じ 136
pub const CARD_H: f32 = 136.0;

pub fn card_box<'a>(body: Element<'a, Msg>, line: Color, on: bool, msg: Msg) -> Element<'a, Msg> {
    mouse_area(
        container(body)
            .padding([11, 12])
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
                cap(format!("{} #{}", short(&v.repo), p.number)).wrapping(text::Wrapping::None)
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
        Space::new().height(Length::Fill),
        chip(p),
        row![
            container(cap(p.author.clone()).wrapping(text::Wrapping::None))
                .width(Length::Fill)
                .height(16)
                .clip(true),
            if mine {
                text("You").size(13).color(app.accent)
            } else if p.turn.is_empty() {
                text("—").size(13).color(DIM)
            } else {
                text(p.turn.join("・")).size(13).color(DIM)
            },
        ]
        .spacing(8),
    ]
    .spacing(7);
    card_box(body.into(), line, on, Msg::Select(key, Some(p.number)))
}

/// PR がまだ無い issue の、点線の代わりの 1 枚
pub fn todo_card<'a>(app: &App, v: &'a RepoView, r: &'a Row) -> Element<'a, Msg> {
    let key = key_of(v, r);
    let mine = r.is_mine(&v.viewer);
    let local = r.local.first().map(|l| l.branch.clone());
    let body = column![
        cap(match (&local, r.kind) {
            (Some(b), _) => format!("Local: {b}"),
            (None, _) if r.stage == Stage::Doing => "Branch pushed · no PR yet".to_string(),
            _ => "No PR yet".to_string(),
        }),
        Space::new().height(Length::Fill),
        row![
            Space::new().width(Length::Fill),
            turn_of(r, &v.viewer, app.accent)
        ],
    ];
    card_box(
        body.into(),
        if mine { app.accent } else { FRAME },
        false,
        Msg::Select(key, None),
    )
}

/// 真ん中:ボードかリスト。
/// ボードは、issue ごとに段を分け、その issue の PR を状態の列に 1 枚ずつ置く。
/// issue につながっていない PR は、一番下の段。列の幅は 190〜236 に収める。
pub fn middle<'a>(app: &'a App, width: f32) -> Element<'a, Msg> {
    let all = shown(app);
    let tasks = lanes(&all);
    let free = loose(&all);
    if all.is_empty() {
        let every = items(app);
        let others = every
            .iter()
            .filter(|(v, r)| r.shows(Filter::Others, &v.viewer))
            .count();
        return panels::empty(app, every.len(), others);
    }
    if app.look == Look::List {
        let mut body = column![].spacing(8).padding([14, 22]).width(Length::Fill);
        for (i, name) in LIST_ORDER.map(|i| (i, COLUMNS[i])) {
            let here: Vec<&Item<'_>> = tasks
                .iter()
                .filter(|(_, r)| column_of(r.stage) == i)
                .collect();
            if here.is_empty() {
                continue;
            }
            body = body.push(
                row![text(name).size(13).color(TEXT), cap(here.len().to_string())]
                    .spacing(9)
                    .padding([6, 0]),
            );
            for (v, r) in here {
                body = body.push(list_row(app, v, r));
            }
        }
        if !free.is_empty() {
            body = body.push(
                row![
                    text("Unlinked PRs").size(13).color(TEXT),
                    cap(free.len().to_string())
                ]
                .spacing(9)
                .padding([10, 0]),
            );
            for (v, r) in &free {
                body = body.push(list_row(app, v, r));
            }
        }
        return scrollable(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let col_w = ((width - 44.0 - 42.0) / 4.0).clamp(190.0, 236.0);
    let cells = |mut pick: Box<dyn FnMut(usize) -> Element<'a, Msg> + 'a>| -> Element<'a, Msg> {
        let mut r = row![].spacing(14);
        for i in 0..4 {
            r = r.push(container(pick(i)).width(col_w));
        }
        r.into()
    };
    // 列の見出し。数字は、その列にあるカードの枚数(先に数えておく)
    let totals: [usize; 4] = std::array::from_fn(|i| {
        let in_tasks: usize = tasks
            .iter()
            .map(|(_, r)| {
                if r.prs.iter().all(|p| column_of_pr(p.mark).is_none()) {
                    usize::from(column_of(r.stage) == i)
                } else {
                    r.prs
                        .iter()
                        .filter(|p| column_of_pr(p.mark) == Some(i))
                        .count()
                }
            })
            .sum();
        in_tasks + free.iter().filter(|(_, r)| column_of(r.stage) == i).count()
    });
    let head = cells(Box::new(move |i| {
        column![
            row![
                text(COLUMNS[i]).size(13).color(TEXT),
                cap(totals[i].to_string())
            ]
            .spacing(8),
            rule(),
        ]
        .spacing(8)
        .into()
    }));
    let mut body = column![].spacing(0).width(Length::Fill);
    for (v, r) in &tasks {
        let (v, r) = (*v, *r);
        let key = key_of(v, r);
        let on = app.selected.as_ref() == Some(&key) && app.pr.is_none();
        // 段の頭 = issue 1 件
        let lane_head = button(
            row![
                cap(label_of(r)),
                container(
                    text(r.title.clone())
                        .size(15)
                        .color(TEXT)
                        .wrapping(text::Wrapping::None)
                )
                .width(Length::Fill)
                .height(20)
                .clip(true),
                cap(short(&v.repo).to_string()),
                cap(r.why.clone()),
                turn_of(r, &v.viewer, app.accent),
                age_of(&r.age, r.stale),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([9, 22])
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
            ..button::Style::default()
        })
        .on_press(Msg::Select(key, None));
        let has_cards = r.prs.iter().any(|p| column_of_pr(p.mark).is_some());
        let lane = cells(Box::new(move |i| {
            let mut c = column![].spacing(9);
            for p in r.prs.iter().filter(|p| column_of_pr(p.mark) == Some(i)) {
                c = c.push(pr_card(app, v, r, p));
            }
            if !has_cards && column_of(r.stage) == i {
                c = c.push(todo_card(app, v, r));
            }
            c.into()
        }));
        body = body
            .push(lane_head)
            .push(container(lane).padding([2, 22]))
            .push(Space::new().height(10))
            .push(rule());
    }
    if !free.is_empty() {
        body = body.push(
            container(
                row![
                    text("Unlinked PRs").size(13).color(TEXT),
                    cap(free.len().to_string())
                ]
                .spacing(9),
            )
            .padding([12, 22]),
        );
        let free2 = free.clone();
        let lane = cells(Box::new(move |i| {
            let mut c = column![].spacing(9);
            for (v, r) in free2.iter().filter(|(_, r)| column_of(r.stage) == i) {
                if let Some(p) = r.prs.first() {
                    c = c.push(pr_card(app, v, r, p));
                }
            }
            c.into()
        }));
        body = body
            .push(container(lane).padding([2, 22]))
            .push(Space::new().height(14));
    }
    column![
        container(head).padding([14, 22]),
        scrollable(body).width(Length::Fill).height(Length::Fill),
    ]
    .into()
}

/// リストの 1 行(issue 1 件か、つながっていない PR 1 本)
pub fn list_row<'a>(app: &App, v: &'a RepoView, r: &'a Row) -> Element<'a, Msg> {
    let key = key_of(v, r);
    let on = app.selected.as_ref() == Some(&key);
    let mine = r.is_mine(&v.viewer);
    let pr = (r.kind == Kind::Pr).then_some(r.number);
    let mut chips = row![].spacing(5);
    for p in r.prs.iter().take(4) {
        chips = chips.push(chip(p));
    }
    let body = row![
        container(cap(label_of(r))).width(64),
        column![
            text(r.title.clone())
                .size(15)
                .color(TEXT)
                .wrapping(text::Wrapping::None),
            row![cap(short(&v.repo).to_string()), chips, cap(now_of(r))]
                .spacing(8)
                .align_y(Alignment::Center),
        ]
        .spacing(5)
        .width(Length::Fill)
        .clip(true),
        container(turn_of(r, &v.viewer, app.accent)).width(110),
        container(age_of(&r.age, r.stale)).width(48),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
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
        [9, 14],
        Msg::Select(key, pr),
    )
}
