//! 部屋の中のページ。ページ 1 つに役目 1 つ。同じものを 2 か所に出さない。
//!
//! - Repositories: repository のカード。押すと、その repository のボード(ボードは `board.rs`)
//! - Repository settings: ボードの奥。読んだときの注記・誰の分を出すか・外す
//! - Members: 人のカード。押すと、その人の Roles
//! - Room settings: 部屋そのもの(名前・数えない check・出る・消す)
//!
//! この PC の設定(Settings)のページの中身は `panels.rs`。
use super::*;
use iced::widget::{column, row};

/// ページの中身の幅の上限
const PAGE_W: f32 = 1040.0;
/// カードの大きさ。一覧のカードは、全部 同じ大きさ
const TILE_W: f32 = 240.0;
const TILE_H: f32 = 104.0;
const TILE_GAP: f32 = 12.0;

/// ページの外枠。縦に長ければ、中だけが動く
fn frame<'a>(body: Element<'a, Msg>) -> Element<'a, Msg> {
    scrollable(
        container(container(body).max_width(PAGE_W))
            .padding([22, 26])
            .width(Length::Fill),
    )
    .height(Length::Fill)
    .into()
}

fn h1<'a>(s: impl Into<String>) -> iced::widget::Text<'a> {
    text(s.into()).size(20).color(TEXT)
}

/// どこから来たか。押すと、その階へ戻る。最後の 1 つが今の場所
fn crumb<'a>(parts: Vec<(String, Option<Msg>)>) -> Element<'a, Msg> {
    let mut r = row![].spacing(8).align_y(Alignment::Center);
    let n = parts.len();
    for (i, (label, go)) in parts.into_iter().enumerate() {
        let last = i + 1 == n;
        let word = text(label).size(13).color(if last { TEXT } else { DIM });
        r = r.push(match go {
            Some(msg) => Element::from(
                button(word)
                    .padding(0)
                    .style(|_, _| button::Style::default())
                    .on_press(msg),
            ),
            None => word.into(),
        });
        if !last {
            r = r.push(cap("›"));
        }
    }
    r.into()
}

/// 気づいてほしいことの、小さな赤い札
fn mark<'a>(label: impl Into<String>) -> Element<'a, Msg> {
    container(text(label.into()).size(12).color(BAD))
        .padding([1, 7])
        .style(boxed(Color::from_rgb(0.35, 0.18, 0.16), Color::TRANSPARENT))
        .into()
}

fn only_admin<'a>(app: &App) -> Element<'a, Msg> {
    text(app.t("Admin だけが変えられます", "Only an Admin can change this"))
        .size(12)
        .color(BAD)
        .into()
}

/// 押せるカード
fn tile<'a>(body: Element<'a, Msg>, line: Color, msg: Msg) -> Element<'a, Msg> {
    mouse_area(still(body, line)).on_press(msg).into()
}

/// 押せないカード(足す・招く)
fn still<'a>(body: Element<'a, Msg>, line: Color) -> Element<'a, Msg> {
    container(body)
        .padding([13, 16])
        .width(Length::Fill)
        .height(TILE_H)
        .clip(true)
        .style(boxed(line, Color::TRANSPARENT))
        .into()
}

/// その幅に、カードを横に何枚 並べるか
fn per_line(width: f32) -> usize {
    (((width + TILE_GAP) / (TILE_W + TILE_GAP)).floor() as usize).max(1)
}

/// カードを、幅に入るだけ横に並べる。足りない所は空けて、カードの幅をそろえる
fn grid<'a>(cards: Vec<Element<'a, Msg>>, width: f32) -> Element<'a, Msg> {
    let n = per_line(width);
    let mut out = column![].spacing(TILE_GAP);
    let mut line = row![].spacing(TILE_GAP);
    let mut in_line = 0;
    for card in cards {
        line = line.push(container(card).width(Length::FillPortion(1)));
        in_line += 1;
        if in_line == n {
            out = out.push(std::mem::replace(&mut line, row![].spacing(TILE_GAP)));
            in_line = 0;
        }
    }
    if in_line > 0 {
        for _ in in_line..n {
            line = line.push(Space::new().width(Length::FillPortion(1)));
        }
        out = out.push(line);
    }
    out.into()
}

/// 部屋のページ。`width` は、真ん中の幅
pub fn room_page(app: &App, page: Page, width: f32) -> Element<'_, Msg> {
    let inner = (width - 52.0).min(PAGE_W);
    frame(match page {
        Page::Repos | Page::Repo(_) => repos(app, inner),
        Page::RepoSettings(repo) => repo_settings(app, repo),
        Page::Members => members(app, inner),
        Page::Member(id) => member(app, id),
        Page::RoomSettings => room_settings(app),
    })
}

/// この PC の設定のページ
pub fn settings_page(app: &App, tab: SetTab) -> Element<'_, Msg> {
    frame(match tab {
        SetTab::Look => look(app),
        SetTab::Account => account(app),
        SetTab::Claude => claude(app),
        SetTab::Local => local_work(app),
        SetTab::Db => database(app),
        SetTab::About => about(app),
    })
}

/// その repository で、開いているものの数と、自分の番の数
pub fn counts(view: &RepoView) -> (usize, usize) {
    let rows = rows_of(view);
    let mine = rows.iter().filter(|(v, r)| r.is_mine(&v.viewer)).count();
    (rows.len(), mine)
}

/// 部屋に入っているのに、読めていない repository
fn unread(app: &App) -> Vec<String> {
    let read = views(app);
    panels::open_room(app)
        .map(|room| {
            room.repos
                .iter()
                .filter(|name| !read.iter().any(|v| v.repo.eq_ignore_ascii_case(name)))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// 読めなかった理由
fn failures(app: &App) -> Vec<String> {
    section(app)
        .map(|s| {
            s.repos
                .iter()
                .filter_map(|r| r.as_ref().err().cloned())
                .collect()
        })
        .unwrap_or_default()
}

fn repos(app: &App, width: f32) -> Element<'_, Msg> {
    let room = panels::open_room(app);
    let admin = room.is_some_and(|r| is_admin(app, r));
    let mut cards: Vec<Element<'_, Msg>> = Vec::new();
    for v in views(app) {
        let (open, mine) = counts(v);
        let owner = v.repo.split('/').next().unwrap_or_default();
        let mut foot = row![
            text(format!("Your turn {mine}"))
                .size(13)
                .color(if mine > 0 { app.accent } else { DIM }),
            cap(format!("{open} open")),
            Space::new().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if !v.notes.is_empty() {
            foot = foot.push(mark("note"));
        }
        let body = column![
            cap(format!("{owner} /")),
            container(
                text(short(&v.repo).to_string())
                    .size(15)
                    .color(TEXT)
                    .wrapping(text::Wrapping::None)
            )
            .height(22)
            .clip(true),
            Space::new().height(Length::Fill),
            foot.push(cap("›")),
        ]
        .spacing(3);
        cards.push(tile(
            body.into(),
            if mine > 0 { app.accent } else { FRAME },
            Msg::Go(Page::Repo(v.repo.clone())),
        ));
    }
    // 読めていないものも、カードで出す(黙って消さない)。押すと、設定が開く
    for name in unread(app) {
        let owner = name.split('/').next().unwrap_or_default().to_string();
        let body = column![
            cap(format!("{owner} /")),
            text(short(&name).to_string()).size(15).color(DIM),
            Space::new().height(Length::Fill),
            row![
                if app.loading && app.synced.is_none() {
                    Element::from(cap("Loading…"))
                } else {
                    mark("Not read")
                },
                Space::new().width(Length::Fill),
                cap("›"),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(3);
        cards.push(tile(body.into(), FRAME, Msg::Go(Page::RepoSettings(name))));
    }
    if let Some(room) = room.filter(|_| admin) {
        let repo = app.form.repo.trim().to_string();
        let add = (!app.busy && !repo.is_empty())
            .then(|| Msg::RoomOp(Op::AddRepo(room.id.clone(), repo)));
        cards.push(still(
            column![
                cap("Add repository"),
                Space::new().height(Length::Fill),
                row![
                    input("owner/repo", &app.form.repo, Field::Repo, add.clone())
                        .width(Length::Fill),
                    primary("Add", app.accent, add),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .into(),
            LINE2,
        ));
    }
    let mut c = column![
        h1("Repositories"),
        cap(app.t(
            "この Room で見る repositories。押すと、その repository のボードを開きます。",
            "Repositories this Room watches. Open one to see its board."
        )),
    ]
    .spacing(14);
    if room.is_some() && !admin {
        c = c.push(only_admin(app));
    }
    for e in failures(app) {
        c = c.push(text(e).size(13).color(BAD));
    }
    if cards.is_empty() {
        c = c.push(cap(if app.loading {
            "Loading from GitHub…"
        } else {
            "No repositories yet"
        }));
    } else {
        c = c.push(grid(cards, width));
    }
    c.push(notice_line(app)).into()
}

fn repo_settings(app: &App, repo: String) -> Element<'_, Msg> {
    let room = panels::open_room(app);
    let can = room.is_some_and(|r| is_admin(app, r)) && !app.busy;
    let view = views(app).into_iter().find(|v| v.repo == repo);
    let name = short(&repo).to_string();
    // 読んだときに気づいたこと
    let mut notes: Vec<Element<'_, Msg>> = Vec::new();
    match view {
        Some(v) => {
            for n in &v.notes {
                notes.push(text(n.clone()).size(14).color(TEXT).into());
            }
            if !v.only.is_empty() {
                notes.push(
                    text(format!("Showing only: {}", v.only.join(", ")))
                        .size(14)
                        .color(TEXT)
                        .into(),
                );
            }
            if notes.is_empty() {
                notes.push(
                    cap(app.t(
                        "注記はありません。全部 読めています。",
                        "No notes. Everything was read.",
                    ))
                    .into(),
                );
            }
        }
        None => {
            notes.push(
                text(app.t(
                    "この repository は、まだ読めていません。",
                    "This repository has not been read.",
                ))
                .size(14)
                .color(BAD)
                .into(),
            );
            for e in failures(app) {
                notes.push(text(e).size(13).color(BAD).into());
            }
        }
    }
    let mut rows: Vec<Element<'_, Msg>> = Vec::new();
    if let Some(room) = room {
        let every = room.everyone.contains(&repo);
        let set = |on: bool| {
            (can && every != on)
                .then(|| Msg::RoomOp(Op::Everyone(room.id.clone(), repo.clone(), on)))
        };
        rows.push(
            row![
                label(
                    "Whose work is shown",
                    app.t(
                        "既定は Members の分だけ。Everyone にすると、Members でない人の PR も出します。",
                        "Members only by default. Everyone also shows PRs by people outside the Room."
                    )
                ),
                tog("Members only", !every, app.accent, set(false)),
                tog("Everyone", every, app.accent, set(true)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    rows.push(
        row![
            label("GitHub", ""),
            seg(
                format!("Open {name} on GitHub"),
                false,
                Msg::Open(format!("https://github.com/{repo}"))
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    );
    if let Some(room) = room {
        rows.push(
            row![
                label(
                    "Remove from this Room",
                    app.t(
                        "この Room で見なくなるだけです。GitHub の中身は何も変わりません",
                        "It only stops being watched here. Nothing on GitHub changes"
                    )
                ),
                danger(
                    "Remove",
                    can.then(|| Msg::RoomOp(Op::DropRepo(room.id.clone(), repo.clone())))
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    let mut c = column![
        crumb(vec![
            ("Repositories".into(), Some(Msg::Go(Page::Repos))),
            (name, view.map(|v| Msg::Go(Page::Repo(v.repo.clone())))),
            ("Settings".into(), None),
        ]),
        h1("Repository settings"),
    ]
    .spacing(14);
    if room.is_some_and(|r| !is_admin(app, r)) {
        c = c.push(only_admin(app));
    }
    c.push(sect(
        column![
            h4("Reading"),
            cap(app.t(
                "GitHub から読んだときに気づいたこと。黙って欠けさせないために出します。",
                "What AsIs noticed while reading GitHub. Shown so nothing goes missing silently."
            )),
            lines(notes),
        ]
        .spacing(8)
        .into(),
    ))
    .push(sect(lines(rows)))
    .push(notice_line(app))
    .into()
}

/// その人の番になっているものの数
fn waiting_on(app: &App, login: &str) -> usize {
    room_items(app)
        .iter()
        .filter(|(_, r)| r.turn.iter().any(|t| t.eq_ignore_ascii_case(login)))
        .count()
}

fn avatar<'a>(login: &str) -> Element<'a, Msg> {
    container(
        text(
            login
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string(),
        )
        .size(12)
        .color(TEXT),
    )
    .center_x(24)
    .center_y(24)
    .style(boxed(LINE2, Color::TRANSPARENT))
    .into()
}

fn members(app: &App, width: f32) -> Element<'_, Msg> {
    let Some(room) = panels::open_room(app) else {
        return cap("Loading…").into();
    };
    let admin = is_admin(app, room);
    let mut cards: Vec<Element<'_, Msg>> = Vec::new();
    for m in &room.members {
        let mine = is_me(app, &m.login);
        let n = waiting_on(app, &m.login);
        // 役は字で並べる(札にすると、4 つ持つ人でカードからはみ出す)
        let held: Vec<&str> = Role::ALL
            .into_iter()
            .filter(|r| r.of(m))
            .map(|r| r.label())
            .collect();
        let roles = if held.is_empty() {
            "Member".to_string()
        } else {
            held.join(" · ")
        };
        let body = column![
            row![
                avatar(&m.login),
                container(
                    text(if mine {
                        format!("{} (you)", m.login)
                    } else {
                        m.login.clone()
                    })
                    .size(15)
                    .color(TEXT)
                    .wrapping(text::Wrapping::None)
                )
                .width(Length::Fill)
                .height(22)
                .clip(true),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
            Space::new().height(Length::Fill),
            row![
                text(format!("{n} waiting"))
                    .size(13)
                    .color(if mine && n > 0 { app.accent } else { DIM }),
                container(cap(roles).wrapping(text::Wrapping::None))
                    .width(Length::Fill)
                    .height(16)
                    .clip(true),
                cap("›"),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ];
        cards.push(tile(
            body.into(),
            if mine && n > 0 { app.accent } else { FRAME },
            Msg::Go(Page::Member(m.github_id)),
        ));
    }
    if admin {
        let who = app.form.invite.trim().to_string();
        let send =
            (!app.busy && !who.is_empty()).then(|| Msg::RoomOp(Op::Invite(room.id.clone(), who)));
        cards.push(still(
            column![
                cap("Invite"),
                Space::new().height(Length::Fill),
                row![
                    input(
                        "GitHub username",
                        &app.form.invite,
                        Field::Invite,
                        send.clone()
                    )
                    .width(Length::Fill),
                    primary("Invite", app.accent, send),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .into(),
            LINE2,
        ));
    }
    let mut c = column![
        h1("Members"),
        cap(app.t(
            "この Room に入っている人。押すと、その人の Roles を開きます。",
            "People in this Room. Open one for their Roles."
        )),
        cap(app.t(
            "Review の役の人が居なければ、作者以外の Members の番になります。Merge・Deploy の役の人が居なければ、作者の番になります。",
            "With no reviewer, the turn goes to Members other than the author. With no one for Merge or Deploy, it goes to the author."
        )),
    ]
    .spacing(14);
    if !admin {
        c = c.push(only_admin(app));
    }
    c.push(grid(cards, width))
        .push(cap(app.t(
            "Invite された人は、次に開いたとき、この Room が出ます。Invite しても、repository を見る権限は渡りません。",
            "Invited people see this Room the next time they open AsIs. An invite never grants access to a repository."
        )))
        .push(notice_line(app))
        .into()
}

fn member(app: &App, id: i64) -> Element<'_, Msg> {
    let Some((room, m)) = panels::open_room(app)
        .and_then(|room| Some((room, room.members.iter().find(|m| m.github_id == id)?)))
    else {
        return cap("Loading…").into();
    };
    let admin = is_admin(app, room);
    let can = admin && !app.busy;
    let mine = is_me(app, &m.login);
    let rows: Vec<Element<'_, Msg>> = Role::ALL
        .into_iter()
        .map(|role| {
            let on = role.of(m);
            let note = match role {
                Role::Review => app.t(
                    "Review required の PR が、この人の番になります",
                    "PRs that need review wait on this member",
                ),
                Role::Merge => app.t(
                    "Approved の PR が、この人の番になります",
                    "Approved PRs wait on this member",
                ),
                Role::Deploy => app.t(
                    "Merged で、まだ本番に出ていないものが、この人の番になります",
                    "Merged work that is not deployed yet waits on this member",
                ),
                Role::Admin => app.t(
                    "この Room の repositories・Members・設定を変えられます",
                    "Can change repositories, Members and settings of this Room",
                ),
            };
            row![
                label(role.label(), note),
                tog(
                    if on { "On" } else { "Off" },
                    on,
                    app.accent,
                    can.then(|| Msg::RoomOp(Op::Role(room.id.clone(), m.github_id, role, !on))),
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        })
        .collect();
    let mut c = column![
        crumb(vec![
            ("Members".into(), Some(Msg::Go(Page::Members))),
            (m.login.clone(), None),
        ]),
        row![
            avatar(&m.login),
            h1(if mine {
                format!("{} (you)", m.login)
            } else {
                m.login.clone()
            }),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(14);
    if !admin {
        c = c.push(only_admin(app));
    }
    c = c.push(sect(
        column![
            h4("Roles"),
            cap(app.t(
                "Roles は「誰の番か」を出すためのものです。何かを禁止はしません。",
                "Roles decide whose turn is shown. They do not restrict anything."
            )),
            lines(rows),
        ]
        .spacing(8)
        .into(),
    ));
    if !mine {
        c = c.push(sect(
            row![
                label(
                    "Remove from this Room",
                    app.t(
                        "この人の画面から、この Room が消えます",
                        "The Room disappears from this member's view"
                    )
                ),
                danger(
                    "Remove",
                    can.then(|| Msg::RoomOp(Op::Kick(room.id.clone(), m.github_id)))
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        ));
    }
    c.push(notice_line(app)).into()
}

fn room_settings(app: &App) -> Element<'_, Msg> {
    let Some(room) = panels::open_room(app) else {
        return cap("Loading…").into();
    };
    let admin = is_admin(app, room);
    let can = admin && !app.busy;
    let free = !app.busy;
    let mut checks: Vec<Element<'_, Msg>> = room
        .ignore_checks
        .iter()
        .map(|c| {
            let rest: Vec<String> = room
                .ignore_checks
                .iter()
                .filter(|x| *x != c)
                .cloned()
                .collect();
            row![
                text(c.clone()).size(14).color(TEXT).width(Length::Fill),
                tog(
                    "Remove",
                    false,
                    app.accent,
                    can.then(|| Msg::RoomOp(Op::Checks(room.id.clone(), rest))),
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        })
        .collect();
    if checks.is_empty() {
        checks = vec![cap("None").into()];
    }
    let check = app.form.check.trim().to_string();
    let add_check = (can && !check.is_empty() && !room.ignore_checks.contains(&check)).then(|| {
        let mut list = room.ignore_checks.clone();
        list.push(check);
        Msg::RoomOp(Op::Checks(room.id.clone(), list))
    });
    let mut c = column![h1("Room settings")].spacing(14);
    if !admin {
        c = c.push(only_admin(app));
    }
    c.push(sect(
        column![
            h4("General"),
            row![
                text("Name").size(14).color(TEXT).width(Length::Fill),
                text(room.name.clone()).size(14).color(DIM),
            ],
        ]
        .spacing(10)
        .into(),
    ))
    .push(sect(
        column![
            h4("Ignored checks"),
            cap(app.t(
                "落ちていても Merge を止めない check の名前(preview deploy など)。",
                "Checks that do not block Merge even when failing (e.g. preview deploys)."
            )),
            lines(checks),
            row![
                input(
                    "Check name",
                    &app.form.check,
                    Field::Check,
                    add_check.clone()
                )
                .width(320),
                seg_opt("Add", add_check),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(10)
        .into(),
    ))
    .push(sect(
        column![
            h4("Danger zone"),
            lines(vec![
                row![
                    label(
                        "Leave room",
                        app.t(
                            "自分の画面から、この Room が消えます",
                            "The Room disappears from your view only"
                        )
                    ),
                    seg_opt(
                        "Leave",
                        free.then(|| Msg::RoomOp(Op::Leave(room.id.clone())))
                    ),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
                row![
                    label(
                        "Delete room",
                        app.t(
                            "全員の画面から消えます。GitHub の中身は何も変わりません",
                            "Removes it for everyone. Nothing on GitHub changes"
                        )
                    ),
                    if app.confirm_delete {
                        danger(
                            "Confirm delete",
                            free.then(|| Msg::RoomOp(Op::Delete(room.id.clone()))),
                        )
                    } else {
                        danger("Delete", Some(Msg::AskDelete))
                    },
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
            ]),
        ]
        .spacing(10)
        .into(),
    ))
    .push(notice_line(app))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_fill_the_width_and_never_drop_below_one() {
        assert_eq!(per_line(100.0), 1);
        assert_eq!(per_line(TILE_W), 1);
        assert_eq!(per_line(TILE_W * 2.0 + TILE_GAP), 2);
        assert_eq!(per_line(PAGE_W), 4);
    }
}
