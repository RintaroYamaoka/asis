//! 窓の中身の組み立て。左端の部屋の列 → repo とメンバー → 真ん中

use super::*;
use iced::widget::{column, row};

/// 開いている部屋の repo
pub fn repos_of(app: &App) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, s) in app
        .sections
        .iter()
        .enumerate()
        .filter(|(i, _)| app.room.is_none_or(|r| r == *i))
    {
        for v in s.repos.iter().filter_map(|r| r.as_ref().ok()) {
            if !out.contains(&v.repo) {
                out.push(v.repo.clone());
            }
        }
    }
    out
}

/// 窓の中身。上の帯を自分で描く出し方なら、帯を足す
pub fn view(app: &App) -> Element<'_, Msg> {
    match PLATFORM.get() {
        Some(p) if p.own_bar => chrome::frame("AsIs", screen(app), Msg::Win),
        _ => screen(app),
    }
}

pub fn screen(app: &App) -> Element<'_, Msg> {
    let all = items(app);
    let count = |f: Filter| all.iter().filter(|(v, r)| r.shows(f, &v.viewer)).count();

    // 左端:部屋の列。数字は、その部屋で自分の番の件数
    let tile = |label: String, on: bool, badge: usize, msg: Msg| -> Element<'_, Msg> {
        let accent = app.accent;
        let size = if label.chars().count() > 4 { 10 } else { 12 };
        let inner = column![text(label).size(size).color(if on { accent } else { TEXT })]
            .push(if badge > 0 {
                Element::from(text(badge.to_string()).size(11).color(accent))
            } else {
                Space::new().into()
            })
            .align_x(Alignment::Center);
        button(container(inner).center_x(44).center_y(40))
            .padding(0)
            .style(move |_, status| button::Style {
                background: Some(
                    if matches!(status, button::Status::Hovered) {
                        PANEL
                    } else {
                        Color::TRANSPARENT
                    }
                    .into(),
                ),
                text_color: TEXT,
                border: Border {
                    color: if on {
                        accent
                    } else {
                        Color::from_rgb(0.165, 0.173, 0.165)
                    },
                    width: 1.0,
                    radius: 5.0.into(),
                },
                ..button::Style::default()
            })
            .on_press(msg)
            .into()
    };
    // 部屋をまたいだ「全部」は置かない(1 つの部屋と同じ形で混ぜて出しても、どの部屋の話か分からない)
    let mut rail = column![].spacing(10).align_x(Alignment::Center);
    for (i, s) in app.sections.iter().enumerate() {
        let mine = s
            .repos
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .flat_map(|v| {
                v.groups
                    .iter()
                    .flat_map(move |g| g.rows.iter().map(move |r| (v, r)))
            })
            .filter(|(v, r)| r.is_mine(&v.viewer))
            .count();
        let name: String = short(&s.project).chars().take(4).collect();
        rail = rail.push(tile(name, app.room == Some(i), mine, Msg::Room(Some(i))));
    }
    rail = rail
        .push(tile(
            "+".into(),
            app.pop == Some(Pop::NewRoom),
            0,
            Msg::Pop(Some(Pop::NewRoom)),
        ))
        .push(Space::new().height(Length::Fill));
    rail = rail.push(tile(
        "Settings".into(),
        matches!(app.pop, Some(Pop::Settings(_))),
        0,
        Msg::Pop(Some(Pop::Settings(SetTab::Look))),
    ));

    // 2 列目:その部屋の repo とメンバー
    let section = app.room.and_then(|i| app.sections.get(i));
    let nav = |label: String, n: usize, on: bool, msg: Msg| -> Element<'_, Msg> {
        button(
            row![
                text(label).size(14).color(if on { TEXT } else { DIM }),
                Space::new().width(Length::Fill),
                cap(n.to_string()),
            ]
            .align_y(Alignment::Center),
        )
        .padding([8, 18])
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
        .on_press(msg)
        .into()
    };
    let room_items: Vec<Item<'_>> = app
        .sections
        .iter()
        .enumerate()
        .filter(|(i, _)| app.room.is_none_or(|r| r == *i))
        .flat_map(|(_, s)| s.repos.iter().filter_map(|r| r.as_ref().ok()))
        .flat_map(|v| {
            v.groups
                .iter()
                .flat_map(move |g| g.rows.iter().map(move |r| (v, r)))
        })
        .collect();
    let mut side = column![
        container(
            text(section.map(|s| s.project.clone()).unwrap_or("AsIs".into()))
                .size(17)
                .color(TEXT)
        )
        .padding([18, 18]),
        nav(
            "All".into(),
            room_items.len(),
            app.repo.is_none(),
            Msg::Repo(None)
        ),
    ];
    let mut repos: Vec<&str> = Vec::new();
    for s in app
        .sections
        .iter()
        .enumerate()
        .filter(|(i, _)| app.room.is_none_or(|r| r == *i))
        .map(|(_, s)| s)
    {
        for v in s.repos.iter().filter_map(|r| r.as_ref().ok()) {
            if !repos.contains(&v.repo.as_str()) {
                repos.push(&v.repo);
            }
        }
    }
    for repo in repos {
        let n = room_items.iter().filter(|(v, _)| v.repo == repo).count();
        side = side.push(nav(
            short(repo).to_string(),
            n,
            app.repo.as_deref() == Some(repo),
            Msg::Repo(Some(repo.to_string())),
        ));
    }
    if let Some(s) = section.filter(|s| !s.people.is_empty()) {
        side = side.push(container(cap("Members")).padding([14, 18]));
        for (name, roles) in &s.people {
            let holds = room_items
                .iter()
                .filter(|(_, r)| r.turn.contains(name))
                .count();
            let me = room_items.first().is_some_and(|(v, _)| v.viewer == *name);
            side = side.push(
                container(
                    row![
                        column![
                            text(name.clone()).size(14).color(TEXT),
                            cap(if roles.is_empty() {
                                "Member".to_string()
                            } else {
                                roles.clone()
                            }),
                        ],
                        Space::new().width(Length::Fill),
                        text(if holds > 0 {
                            holds.to_string()
                        } else {
                            String::new()
                        })
                        .size(14)
                        .color(if me { app.accent } else { DIM }),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([5, 18]),
            );
        }
    }

    // 部屋(DB の部屋)を開いているときだけ、下に Room settings
    let side: Element<'_, Msg> = if section.is_some_and(|s| s.room.is_some()) {
        column![
            scrollable(side).height(Length::Fill),
            rule(),
            button(
                row![
                    text("Room settings").size(14).color(DIM),
                    Space::new().width(Length::Fill),
                    cap("›"),
                ]
                .align_y(Alignment::Center),
            )
            .padding([12, 18])
            .width(Length::Fill)
            .style(|_, status| button::Style {
                background: Some(
                    if matches!(status, button::Status::Hovered) {
                        PANEL
                    } else {
                        Color::TRANSPARENT
                    }
                    .into(),
                ),
                text_color: TEXT,
                ..button::Style::default()
            })
            .on_press(Msg::Pop(Some(Pop::Room(RoomTab::Members)))),
        ]
        .into()
    } else {
        scrollable(side).height(Length::Fill).into()
    };

    // 上の段:絞り込み、見せ方、読み直し
    let status = match (app.loading, app.synced) {
        (true, _) => "Loading…".to_string(),
        (false, Some(t)) => format!("Synced {}s ago", t.elapsed().as_secs()),
        (false, None) => String::new(),
    };
    let mut bar = row![].spacing(6).align_y(Alignment::Center);
    for f in [Filter::All, Filter::Mine, Filter::Others] {
        bar = bar.push(seg(
            format!("{} {}", f.label(), count(f)),
            f == app.filter,
            Msg::Filter(f),
        ));
    }
    bar = bar
        .push(Space::new().width(Length::Fill))
        .push(cap(status))
        .push(seg(
            "Board".into(),
            app.look == Look::Board,
            Msg::Look(Look::Board),
        ))
        .push(seg(
            "List".into(),
            app.look == Look::List,
            Msg::Look(Look::List),
        ))
        .push(seg("Refresh".into(), false, Msg::Refresh))
        .push(primary("New issue", app.accent, Some(Msg::WriteOpen)));

    // 注記と、読めなかった理由。黙って欠けさせない
    let mut notes = column![].spacing(2);
    if let Some(e) = &app.error {
        notes = notes.push(text(e.clone()).size(13).color(BAD));
    }
    if let Rooms::Failed(e) = &app.rooms {
        notes = notes.push(text(format!("Rooms: {e}")).size(13).color(BAD));
    }
    for s in app
        .sections
        .iter()
        .enumerate()
        .filter(|(i, _)| app.room.is_none_or(|r| r == *i))
        .map(|(_, s)| s)
    {
        for r in &s.repos {
            match r {
                Err(e) => notes = notes.push(text(e.clone()).size(13).color(BAD)),
                Ok(v) => {
                    for n in &v.notes {
                        notes = notes.push(cap(format!("{}: {n}", short(&v.repo))));
                    }
                }
            }
        }
    }

    // 真ん中:ボードかリスト。issue につながっていない PR は別の段
    // 右:選んだ 1 件
    let detail = if app.write.is_some() {
        write_form(app)
    } else {
        detail_of(app)
    };

    let foot = row![
        cap("↑↓ Move"),
        cap("Tab Filter"),
        cap("V Board / List"),
        cap("Enter Open"),
        cap("←→ Prev / Next PR"),
        cap("C New issue"),
        cap("R Refresh"),
        cap("Esc Close"),
    ]
    .spacing(18)
    .padding([9, 22]);

    // ボードを出せないとき(GitHub につながっていない・はじめて・読み込み中)は、代わりの画面だけを出す
    let main: Element<'_, Msg> = match panels::scene(app) {
        Some(scene) => scene,
        None => column![
            container(bar).padding([14, 22]),
            rule(),
            container(notes).padding([6, 22]),
            responsive(move |size| middle(app, size.width)),
            rule(),
            foot,
        ]
        .into(),
    };
    let frame = row![
        container(rail).padding([14, 9]).height(Length::Fill),
        vrule(),
        container(side).width(210),
        vrule(),
        container(main).width(Length::Fill),
    ]
    .height(Length::Fill);
    let base: Element<'_, Msg> = container(frame).style(fill(BG)).into();
    // 設定の画面も、前に出す
    if let Some((body, width)) = panels::pop(app) {
        return stack![
            base,
            opaque(
                mouse_area(
                    container(opaque(
                        container(body)
                            .width(width)
                            .max_height(600)
                            .style(boxed(LINE2, BG))
                    ))
                    .center(Length::Fill)
                    .padding(40)
                    .style(fill(Color::from_rgba(0.0, 0.0, 0.0, 0.62)))
                )
                .on_press(Msg::Pop(None))
            ),
        ]
        .into();
    }
    // 選んだ 1 件の中身は、前に大きく出す。ボードの幅を取らないので、カードの形が変わらない
    match detail.filter(|_| app.open || app.write.is_some()) {
        Some(detail) => stack![
            base,
            opaque(
                mouse_area(
                    container(opaque(
                        container(detail)
                            .max_width(820)
                            .max_height(680)
                            .style(boxed(LINE2, BG))
                    ))
                    .center(Length::Fill)
                    .padding(40)
                    .style(fill(Color::from_rgba(0.0, 0.0, 0.0, 0.62)))
                )
                .on_press(Msg::Close)
            ),
        ]
        .into(),
        None => base,
    }
}
