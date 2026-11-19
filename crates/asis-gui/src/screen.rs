//! 窓の中身の組み立て。左端に部屋の列 → 左の欄(部屋の中の道か、この PC の設定の道)→ 真ん中(ページ)

use super::*;
use iced::widget::{column, row};

/// いま出すページ。読み直しで無くなったもの(外した repository・外れた人)を指していたら、1 つ上に戻す
pub fn page_of(app: &App) -> Page {
    let room = panels::open_room(app);
    let read = |repo: &String| views(app).iter().any(|v| v.repo == *repo);
    let listed = |repo: &String| room.is_some_and(|r| r.repos.contains(repo));
    match &app.page {
        // 読めていない repository は、ボードが無いので設定を出す
        Page::Repo(repo) if !read(repo) && listed(repo) => Page::RepoSettings(repo.clone()),
        Page::Repo(repo) | Page::RepoSettings(repo) if !read(repo) && !listed(repo) => Page::Repos,
        Page::Members | Page::Member(_) | Page::RoomSettings if room.is_none() => Page::Repos,
        Page::Member(id) if !room.is_some_and(|r| r.members.iter().any(|m| m.github_id == *id)) => {
            Page::Members
        }
        page => page.clone(),
    }
}

/// 窓の中身。上の帯を自分で描く出し方なら、帯を足す
pub fn view(app: &App) -> Element<'_, Msg> {
    match PLATFORM.get() {
        Some(p) if p.own_bar => chrome::frame("AsIs", screen(app), Msg::Win),
        _ => screen(app),
    }
}

/// 左端の 1 枚。数字は、その部屋で自分の番の件数
fn rail_tile<'a>(
    label: String,
    on: bool,
    badge: usize,
    accent: Color,
    msg: Msg,
) -> Element<'a, Msg> {
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
                color: if on { accent } else { FRAME },
                width: 1.0,
                radius: 5.0.into(),
            },
            ..button::Style::default()
        })
        .on_press(msg)
        .into()
}

/// 左の欄の 1 行。居る場所は明るく
fn nav<'a>(label: &str, count: Option<usize>, on: bool, msg: Msg) -> Element<'a, Msg> {
    button(
        row![
            text(label.to_string())
                .size(14)
                .color(if on { TEXT } else { DIM }),
            Space::new().width(Length::Fill),
            cap(count.map(|n| n.to_string()).unwrap_or_default()),
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
}

/// ボードの上の帯。repository の切り替え(絞り込みは置かない)・見せ方・その repository の設定・読み直し・New issue
fn board_bar<'a>(app: &'a App, repo: &str) -> Element<'a, Msg> {
    let mut bar = row![].spacing(6).align_y(Alignment::Center);
    for v in views(app) {
        let (_, mine) = counts(v);
        let name = short(&v.repo);
        bar = bar.push(seg(
            if mine > 0 {
                format!("{name} {mine}")
            } else {
                name.to_string()
            },
            v.repo == repo,
            Msg::Go(Page::Repo(v.repo.clone())),
        ));
    }
    let status = match (app.loading, app.synced) {
        (true, _) => "Loading…".to_string(),
        (false, Some(t)) => format!("Synced {}s ago", t.elapsed().as_secs()),
        (false, None) => String::new(),
    };
    let notes = views(app)
        .iter()
        .find(|v| v.repo == repo)
        .map_or(0, |v| v.notes.len());
    bar.push(Space::new().width(Length::Fill))
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
        .push(seg(
            if notes > 0 {
                format!("Repository settings · {notes} note")
            } else {
                "Repository settings".to_string()
            },
            false,
            Msg::Go(Page::RepoSettings(repo.to_string())),
        ))
        .push(seg("Refresh".into(), false, Msg::Refresh))
        .push(primary("New issue", app.accent, Some(Msg::WriteOpen)))
        .into()
}

pub fn screen(app: &App) -> Element<'_, Msg> {
    let page = page_of(app);
    let settings = match app.area {
        Area::Settings(tab) => Some(tab),
        Area::Room => None,
    };

    // 左端:部屋の列。部屋をまたいだ「全部」は置かない
    let mut rail = column![].spacing(10).align_x(Alignment::Center);
    for (i, s) in app.sections.iter().enumerate() {
        let mine = s
            .repos
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .map(|v| counts(v).1)
            .sum();
        let name: String = short(&s.project).chars().take(4).collect();
        rail = rail.push(rail_tile(
            name,
            settings.is_none() && app.room == Some(i),
            mine,
            app.accent,
            Msg::Room(Some(i)),
        ));
    }
    rail = rail
        .push(rail_tile(
            "+".into(),
            app.pop == Some(Pop::NewRoom),
            0,
            app.accent,
            Msg::Pop(Some(Pop::NewRoom)),
        ))
        .push(Space::new().height(Length::Fill))
        .push(rail_tile(
            "Settings".into(),
            settings.is_some(),
            0,
            app.accent,
            Msg::Area(Area::Settings(SetTab::Look)),
        ));

    // 左の欄:部屋の中の道(Repositories / Members / Room settings)か、この PC の設定の道
    let title = |s: String| container(text(s).size(17).color(TEXT)).padding([18, 18]);
    let side: Element<'_, Msg> = match settings {
        Some(tab) => {
            let go =
                |label: &str, x: SetTab| nav(label, None, tab == x, Msg::Area(Area::Settings(x)));
            column![
                title("Settings".into()),
                go("Appearance", SetTab::Look),
                go("Account", SetTab::Account),
                go("Claude", SetTab::Claude),
                go("Local work", SetTab::Local),
                go("Database", SetTab::Db),
                go("About", SetTab::About),
            ]
            .into()
        }
        None => {
            let mut side = column![title(
                section(app)
                    .map(|s| s.project.clone())
                    .unwrap_or_else(|| "AsIs".into())
            )];
            if let Some(s) = section(app) {
                let room = panels::open_room(app);
                side = side.push(nav(
                    "Repositories",
                    Some(room.map_or(s.repos.len(), |r| r.repos.len())),
                    matches!(page, Page::Repos | Page::Repo(_) | Page::RepoSettings(_)),
                    Msg::Go(Page::Repos),
                ));
                // Members と Room settings は、DB の部屋のときだけ
                if let Some(room) = room {
                    side = side
                        .push(nav(
                            "Members",
                            Some(room.members.len()),
                            matches!(page, Page::Members | Page::Member(_)),
                            Msg::Go(Page::Members),
                        ))
                        .push(nav(
                            "Room settings",
                            None,
                            page == Page::RoomSettings,
                            Msg::Go(Page::RoomSettings),
                        ));
                }
            }
            side.into()
        }
    };

    // 読めなかった理由は、どのページでも上に出す(黙って欠けさせない)
    let mut errors = column![].spacing(2);
    if let Some(e) = &app.error {
        errors = errors.push(text(e.clone()).size(13).color(BAD));
    }
    if let Rooms::Failed(e) = &app.rooms {
        errors = errors.push(text(format!("Rooms: {e}")).size(13).color(BAD));
    }
    // 新しい版に入れ替えたこと・入れ替えられなかったこと
    match update_outcome() {
        Some(asis::update::Outcome::Ready(version)) => {
            errors = errors.push(
                text(if app.english {
                    format!(
                        "Version {version} is installed. It starts the next time you open AsIs."
                    )
                } else {
                    format!("版 {version} を入れました。次に AsIs を開くと、その版で動きます。")
                })
                .size(13)
                .color(DIM),
            );
        }
        Some(asis::update::Outcome::Blocked(e)) => {
            errors = errors.push(
                text(if app.english {
                    format!("This version can no longer be used, and the update failed: {e}")
                } else {
                    format!("この版はもう使えませんが、新しい版に入れ替えられませんでした: {e}")
                })
                .size(13)
                .color(BAD),
            );
        }
        Some(asis::update::Outcome::Nothing) | None => {}
    }
    // 上に出す行が 1 つでもあるか(読めなかった理由か、版の入れ替えの知らせ)
    let noted = app.error.is_some()
        || matches!(app.rooms, Rooms::Failed(_))
        || matches!(
            update_outcome(),
            Some(asis::update::Outcome::Ready(_) | asis::update::Outcome::Blocked(_))
        );

    let foot = row![
        cap("↑↓ Move"),
        cap("Tab Next repository"),
        cap("V Board / List"),
        cap("Enter Open"),
        cap("←→ Prev / Next PR"),
        cap("C New issue"),
        cap("R Refresh"),
        cap("Esc Close"),
    ]
    .spacing(18)
    .padding([9, 22]);

    // 真ん中:この PC の設定 → ボードを出せないときの代わりの画面 → 部屋のページ、の順に決める
    let body: Element<'_, Msg> = if let Some(tab) = settings {
        settings_page(app, tab)
    } else if let Some(scene) = panels::scene(app) {
        scene
    } else if let Some(repo) = board_repo(app) {
        // ボードは repository 1 つにつき 1 つ
        column![
            container(board_bar(app, &repo)).padding([14, 22]),
            rule(),
            responsive(move |size| middle(app, size.width)),
            rule(),
            foot,
        ]
        .into()
    } else {
        responsive(move |size| room_page(app, page.clone(), size.width)).into()
    };
    let main: Element<'_, Msg> = if noted {
        column![container(errors).padding([8, 22]), rule(), body].into()
    } else {
        body
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
    // 1 件を作る画面(New room)は、前に出す
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
    // 選んだ 1 件の中身(issue・PR)と、New issue も、前に大きく出す
    let detail = if app.write.is_some() {
        write_form(app)
    } else {
        detail_of(app)
    };
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
