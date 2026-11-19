//! ボード以外の画面:はじめの画面・困ったときの見せ方・Settings・Room settings・New room。
//! 形は試作(asis-prototype.html)のとおり。言葉は英語、説明の文だけ設定の言葉で出す。
use super::*;
use iced::widget::{column, row};

/// 前に出す設定の画面
#[derive(Clone, Copy, PartialEq)]
pub enum Pop {
    Settings(SetTab),
    Room(RoomTab),
    NewRoom,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SetTab {
    Look,
    Account,
    Db,
}

#[derive(Clone, Copy, PartialEq)]
pub enum RoomTab {
    Members,
    Repos,
    Room,
}

/// 選べる 1 色
const COLORS: [(&str, &str); 6] = [
    ("#f2b63c", "Amber"),
    ("#58e0bd", "Teal"),
    ("#ff7352", "Vermilion"),
    ("#8fb3ff", "Blue"),
    ("#d8e84a", "Lime"),
    ("#e6e8ec", "White"),
];

fn h4<'a>(s: &str) -> iced::widget::Text<'a> {
    text(s.to_string()).size(16).color(TEXT)
}

/// 説明の文。設定の言葉で出す(字を組み立てるもの)
fn say(app: &App, ja: String, en: String) -> String {
    if app.english {
        en
    } else {
        ja
    }
}

/// 打つ欄。`send` が無ければ、Enter では何もしない
fn input<'a>(
    hint: &str,
    value: &str,
    f: Field,
    send: Option<Msg>,
) -> text_input::TextInput<'a, Msg> {
    let mut t = text_input(hint, value)
        .on_input(move |s| Msg::Input(f, s))
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
        });
    if let Some(m) = send {
        t = t.on_submit(m);
    }
    t
}

/// 小さな切り替え。付いているものは色の枠
fn tog<'a>(label: &str, on: bool, accent: Color, msg: Option<Msg>) -> Element<'a, Msg> {
    let color = if on { accent } else { DIM };
    let b = button(text(label.to_string()).size(12).color(color))
        .padding([2, 9])
        .style(move |_, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered) {
                    PANEL
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: color,
            border: Border {
                color: if on { accent } else { LINE2 },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..button::Style::default()
        });
    match msg {
        Some(m) => b.on_press(m).into(),
        None => b.into(),
    }
}

/// 取り返しのつかない操作のボタン
fn danger<'a>(label: &str, msg: Option<Msg>) -> Element<'a, Msg> {
    let b = button(text(label.to_string()).size(13).color(BAD))
        .padding([4, 11])
        .style(|_, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered) {
                    PANEL
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: BAD,
            border: Border {
                color: Color::from_rgb(0.35, 0.18, 0.16),
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

/// 名前と説明の 2 段の見出し
fn label<'a>(name: &str, note: &str) -> Element<'a, Msg> {
    column![
        text(name.to_string()).size(14).color(TEXT),
        cap(note.to_string())
    ]
    .spacing(2)
    .width(Length::Fill)
    .into()
}

/// 線で区切った行の束
fn lines<'a>(rows: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let mut c = column![].spacing(0);
    let n = rows.len();
    for (i, r) in rows.into_iter().enumerate() {
        c = c.push(container(r).padding([9, 0]));
        if i + 1 < n {
            c = c.push(rule());
        }
    }
    c.into()
}

fn choice<'a>(body: Element<'a, Msg>) -> Element<'a, Msg> {
    container(body)
        .padding([18, 20])
        .width(Length::Fill)
        .style(boxed(FRAME, Color::TRANSPARENT))
        .into()
}

fn notice_line<'a>(app: &App) -> Element<'a, Msg> {
    notice_of(app).unwrap_or_else(|| Space::new().into())
}

/// 自分の GitHub のアカウント名
pub fn me(app: &App) -> Option<String> {
    if let Some(Gh::Ready(login)) = &app.gh {
        return Some(login.clone());
    }
    app.sections
        .iter()
        .flat_map(|s| s.repos.iter().filter_map(|r| r.as_ref().ok()))
        .map(|v| v.viewer.clone())
        .find(|v| !v.is_empty())
}

fn is_me(app: &App, login: &str) -> bool {
    me(app).is_some_and(|m| m.eq_ignore_ascii_case(login))
}

/// 開いている部屋(DB の部屋のとき)
pub fn open_room(app: &App) -> Option<&Room> {
    if app.want_room.is_some() {
        return None;
    }
    let id = app.room.and_then(|i| app.sections.get(i))?.room.as_ref()?;
    match &app.rooms {
        Rooms::Read(list) => list.iter().find(|r| &r.id == id),
        _ => None,
    }
}

fn is_admin(app: &App, room: &Room) -> bool {
    room.members
        .iter()
        .any(|m| m.is_admin && is_me(app, &m.login))
}

/// 送る前に分かる間違いは、送らずに止める
pub fn refuse(app: &App, op: &Op) -> Option<String> {
    let room = |id: &str| match &app.rooms {
        Rooms::Read(list) => list.iter().find(|r| r.id == id),
        _ => None,
    };
    let (ja, en) = match op {
        Op::Role(id, gid, Role::Admin, false)
            if room(id).is_some_and(|r| {
                r.members.iter().filter(|m| m.is_admin).count() == 1
                    && r.members.iter().any(|m| m.is_admin && m.github_id == *gid)
            }) =>
        {
            (
                "Admin が 1 人もいなくなります",
                "A Room needs at least one Admin",
            )
        }
        Op::Invite(id, login)
            if room(id).is_some_and(|r| {
                r.members
                    .iter()
                    .any(|m| m.login.eq_ignore_ascii_case(login.trim()))
            }) =>
        {
            ("もう Members に入っています", "Already a member")
        }
        Op::AddRepo(id, repo)
            if room(id)
                .is_some_and(|r| r.repos.iter().any(|x| x.eq_ignore_ascii_case(repo.trim()))) =>
        {
            ("もう入っています", "Already added")
        }
        Op::AddRepo(_, repo) if !repo.trim().contains('/') => {
            ("owner/repo の形で入れてください", "Use the form owner/repo")
        }
        _ => return None,
    };
    Some(app.t(ja, en).to_string())
}

// ---- 真ん中に出す、ボードの代わりの画面 ----

/// ボードを出せないときに、代わりに出す画面。出せるなら None
pub fn scene(app: &App) -> Option<Element<'_, Msg>> {
    if matches!(app.gh, Some(Gh::Missing | Gh::SignedOut)) {
        return Some(no_github(app));
    }
    if app.sections.is_empty() {
        if app.synced.is_none() {
            return Some(
                container(text("Loading from GitHub…").size(15).color(DIM))
                    .center(Length::Fill)
                    .into(),
            );
        }
        return Some(welcome(app));
    }
    None
}

fn page<'a>(body: Element<'a, Msg>) -> Element<'a, Msg> {
    scrollable(
        container(container(body).max_width(720))
            .center_x(Length::Fill)
            .padding([60, 22]),
    )
    .height(Length::Fill)
    .into()
}

fn code<'a>(s: &str) -> Element<'a, Msg> {
    container(
        text(s.to_string())
            .size(14)
            .color(TEXT)
            .font(Font::MONOSPACE),
    )
    .padding([8, 11])
    .style(boxed(LINE2, Color::TRANSPARENT))
    .into()
}

/// gh が無い・ログインしていない
fn no_github(app: &App) -> Element<'_, Msg> {
    let installed = matches!(app.gh, Some(Gh::SignedOut));
    page(
        column![
            text("Not connected to GitHub").size(26).color(TEXT),
            text(app.t(
                "asis は、GitHub の公式コマンド gh のログインを借りて読みます。この PC では、まだ使えません。",
                "asis reads GitHub through the official gh command. It is not available on this machine yet."
            ))
            .size(14)
            .color(DIM),
            choice(
                column![
                    text("1. Install gh").size(15).color(TEXT),
                    if installed {
                        Element::from(text(app.t("入っています", "Installed")).size(13).color(app.accent))
                    } else {
                        column![
                            cap(app.t("https://cli.github.com/ から入れる", "Get it from https://cli.github.com/")),
                            row![seg(
                                "Open cli.github.com".into(),
                                false,
                                Msg::Open("https://cli.github.com/".into())
                            )],
                        ]
                        .spacing(10)
                        .into()
                    },
                ]
                .spacing(10)
                .into()
            ),
            choice(
                column![
                    text("2. Sign in").size(15).color(TEXT),
                    cap(app.t("ターミナルで次を打つ", "Run this in a terminal")),
                    row![code("gh auth login")],
                ]
                .spacing(10)
                .into()
            ),
            row![primary("Check again", app.accent, app.gh.is_some().then_some(Msg::CheckGh))],
        ]
        .spacing(18)
        .into(),
    )
}

/// はじめの画面。Sign in → New room の 1 本道
fn welcome(app: &App) -> Element<'_, Msg> {
    let step1: Element<'_, Msg> = match &app.rooms {
        Rooms::NoDb => column![
            cap(app.t(
                "Room を置く DB が、まだ設定にありません。Settings の Database で入れます。",
                "No Database is set yet. Enter it under Settings → Database."
            )),
            row![primary(
                "Open Database settings",
                app.accent,
                Some(Msg::Pop(Some(Pop::Settings(SetTab::Db))))
            )],
        ]
        .spacing(10)
        .into(),
        Rooms::Read(_) => text(match me(app) {
            Some(m) => format!("Signed in as {m}"),
            None => "Signed in".to_string(),
        })
        .size(14)
        .color(app.accent)
        .into(),
        Rooms::SignedOut | Rooms::Failed(_) => {
            let mut c = column![cap(app.t(
                "GitHub のアカウントで入ります。本人確認だけで、repository を読む許可は求めません。",
                "Sign in with your GitHub account. Identity only — no permission to read repositories is requested."
            ))]
            .spacing(10);
            if let Rooms::Failed(e) = &app.rooms {
                c = c.push(text(e.clone()).size(13).color(BAD));
            }
            c.push(sign_in_button(app)).into()
        }
    };
    let signed = matches!(app.rooms, Rooms::Read(_));
    page(
        column![
            text("asis").size(26).color(TEXT),
            text(app.t(
                "開発を、あるがままに見える化します。何も入力しなくても、GitHub で起きていることから「いまどこまで進んでいて、次は誰の番か」を出します。",
                "See development as it is. With no input at all, asis reads what happened on GitHub and shows how far things are and whose turn is next."
            ))
            .size(14)
            .color(DIM),
            choice(column![text("1. Sign in").size(15).color(TEXT), step1].spacing(10).into()),
            choice(
                column![
                    text(app.t("2. Room を作る", "2. Create a Room")).size(15).color(TEXT),
                    cap(app.t(
                        "Room は、repository と Members をまとめたものです。1 人で使うときも、自分の Room を 1 つ作ります。招かれている Room があれば、Sign in すると出ます。",
                        "A Room groups repositories and Members. Even when you work alone, create one Room for yourself. Rooms you were invited to appear after you sign in."
                    )),
                    row![if signed {
                        primary("New room", app.accent, Some(Msg::Pop(Some(Pop::NewRoom))))
                    } else {
                        seg("New room".into(), false, Msg::Pop(Some(Pop::NewRoom)))
                    }],
                ]
                .spacing(10)
                .into()
            ),
            notice_line(app),
        ]
        .spacing(18)
        .into(),
    )
}

fn sign_in_button(app: &App) -> Element<'_, Msg> {
    if app.signing_in {
        column![
            cap(app.t(
                "ブラウザで GitHub のログインを待っています…(5 分まで)",
                "Waiting for GitHub sign-in in your browser… (up to 5 minutes)",
            )),
            cap(app.t(
                "開いたブラウザが GitHub に入っていないときは、リンクをコピーして、入っているブラウザに貼ります。",
                "If the browser that opened is not signed in to GitHub, copy the link and paste it into the one that is.",
            )),
            row![seg("Copy sign-in link".into(), false, Msg::CopySignIn)],
        ]
        .spacing(8)
        .into()
    } else {
        row![primary(
            "Sign in with GitHub",
            app.accent,
            Some(Msg::SignIn)
        )]
        .into()
    }
}

/// 部屋は開いているが、出すものが無いとき
pub fn empty<'a>(app: &'a App, total: usize, waiting_on_others: usize) -> Element<'a, Msg> {
    let section = app.room.and_then(|i| app.sections.get(i));
    let (head, note, go): (String, String, Option<(&str, Msg)>) = if total > 0
        && app.filter == Filter::Mine
    {
        (
            "Nothing is waiting on you".into(),
            say(
                app,
                format!("{waiting_on_others} 件が、ほかの人の番です。All に切り替えると見えます。"),
                format!(
                    "{waiting_on_others} items are waiting on others. Switch to All to see them."
                ),
            ),
            Some(("Show All", Msg::Filter(Filter::All))),
        )
    } else if total > 0 {
        (
            "Nothing is waiting on others".into(),
            String::new(),
            Some(("Show All", Msg::Filter(Filter::All))),
        )
    } else if app.loading && app.synced.is_none() {
        ("Loading from GitHub…".into(), String::new(), None)
    } else if section.is_some_and(|s| s.repos.is_empty() && s.room.is_some()) {
        (
            "No repositories yet".into(),
            app.t(
                "Room settings で repository を足します。",
                "Add repositories in Room settings.",
            )
            .into(),
            Some(("Room settings", Msg::Pop(Some(Pop::Room(RoomTab::Repos))))),
        )
    } else {
        (
            "Nothing open".into(),
            app.t(
                "開いている issue と pull request はありません。",
                "No open issues or pull requests.",
            )
            .into(),
            None,
        )
    };
    let mut c = column![text(head).size(15).color(TEXT)]
        .spacing(10)
        .align_x(Alignment::Center);
    if !note.is_empty() {
        c = c.push(cap(note));
    }
    if let Some((label, msg)) = go {
        c = c.push(seg(label.into(), false, msg));
    }
    container(c).center(Length::Fill).into()
}

// ---- 前に出す設定の画面 ----

fn tab<'a>(label: &str, on: bool, msg: Msg) -> Element<'a, Msg> {
    button(
        text(label.to_string())
            .size(14)
            .color(if on { TEXT } else { DIM }),
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

/// 左に見出しの列、右に中身
fn tabbed<'a>(
    title: String,
    tabs: Vec<Element<'a, Msg>>,
    body: Element<'a, Msg>,
) -> Element<'a, Msg> {
    let mut nav = column![container(text(title).size(15).color(TEXT)).padding([0, 18])]
        .spacing(4)
        .padding([18, 0])
        .width(190);
    for t in tabs {
        nav = nav.push(t);
    }
    row![
        nav,
        vrule(),
        scrollable(container(body).padding([22, 24]).width(Length::Fill)).height(Length::Fill),
    ]
    .into()
}

/// 前に出す設定の画面の中身
pub fn pop(app: &App) -> Option<(Element<'_, Msg>, f32)> {
    Some(match app.pop? {
        Pop::NewRoom => (new_room(app), 600.0),
        Pop::Settings(t) => {
            let body = match t {
                SetTab::Look => look(app),
                SetTab::Account => account(app),
                SetTab::Db => database(app),
            };
            let go = |l: &str, x: SetTab| tab(l, t == x, Msg::Pop(Some(Pop::Settings(x))));
            (
                tabbed(
                    "Settings".into(),
                    vec![
                        go("Appearance", SetTab::Look),
                        go("Account", SetTab::Account),
                        go("Database", SetTab::Db),
                    ],
                    body,
                ),
                820.0,
            )
        }
        Pop::Room(t) => {
            let Some(room) = open_room(app) else {
                return Some((
                    container(cap("Loading…")).center(Length::Fill).into(),
                    820.0,
                ));
            };
            let admin = is_admin(app, room);
            let body = match t {
                RoomTab::Members => members(app, room, admin),
                RoomTab::Repos => repositories(app, room, admin),
                RoomTab::Room => room_page(app, room),
            };
            let mut top = column![].spacing(14);
            if !admin {
                top = top.push(
                    text(app.t("Admin だけが変えられます", "Only an Admin can change this"))
                        .size(12)
                        .color(BAD),
                );
            }
            let go = |l: &str, x: RoomTab| tab(l, t == x, Msg::Pop(Some(Pop::Room(x))));
            (
                tabbed(
                    room.name.clone(),
                    vec![
                        go("Members", RoomTab::Members),
                        go("Repositories", RoomTab::Repos),
                        go("Room", RoomTab::Room),
                    ],
                    top.push(body).into(),
                ),
                820.0,
            )
        }
    })
}

fn new_room(app: &App) -> Element<'_, Msg> {
    let name = app.form.room_name.trim().to_string();
    let send = (!name.is_empty() && !app.busy).then_some(Msg::RoomOp(Op::Create(name)));
    let form: Element<'_, Msg> = match &app.rooms {
        Rooms::Read(_) => column![
            row![
                input("Room name — e.g. shop", &app.form.room_name, Field::RoomName, send.clone())
                    .width(Length::Fill),
                primary("Create", app.accent, send),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            cap(app.t(
                "作ったあと、repositories と Members を足します。Room に入っても、GitHub で見られない repository は見えません。",
                "Add repositories and Members next. Joining a Room never grants access to a repository you cannot see on GitHub."
            )),
        ]
        .spacing(14)
        .into(),
        Rooms::NoDb => column![
            cap(app.t(
                "Room を置く DB が、まだ設定にありません。",
                "No Database is set yet."
            )),
            row![seg("Open Database settings".into(), false, Msg::Pop(Some(Pop::Settings(SetTab::Db))))],
        ]
        .spacing(10)
        .into(),
        _ => column![
            cap(app.t("Room を作るには、先に Sign in します。", "Sign in first to create a Room.")),
            sign_in_button(app),
        ]
        .spacing(10)
        .into(),
    };
    column![
        h4("New room"),
        cap(app.t(
            "Room は、repositories・Members・Roles をまとめたものです。作った人が Admin になります。",
            "A Room groups repositories, Members and Roles. Whoever creates it becomes Admin."
        )),
        form,
        notice_line(app),
        row![Space::new().width(Length::Fill), seg("Cancel".into(), false, Msg::Pop(None))],
    ]
    .spacing(14)
    .padding(24)
    .into()
}

fn members<'a>(app: &'a App, room: &'a Room, admin: bool) -> Element<'a, Msg> {
    let can = admin && !app.busy;
    let mut rows = Vec::new();
    for m in &room.members {
        let mine = is_me(app, &m.login);
        let mut r = row![
            container(
                text(
                    m.login
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string()
                )
                .size(12)
                .color(TEXT)
            )
            .center_x(26)
            .center_y(26)
            .style(boxed(LINE2, Color::TRANSPARENT)),
            text(if mine {
                format!("{} (you)", m.login)
            } else {
                m.login.clone()
            })
            .size(14)
            .color(TEXT)
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        for role in Role::ALL {
            let on = role.of(m);
            r = r.push(tog(
                role.label(),
                on,
                app.accent,
                can.then(|| Msg::RoomOp(Op::Role(room.id.clone(), m.github_id, role, !on))),
            ));
        }
        r = r.push(if mine {
            Element::from(Space::new().width(62))
        } else {
            container(tog(
                "Remove",
                false,
                app.accent,
                can.then(|| Msg::RoomOp(Op::Kick(room.id.clone(), m.github_id))),
            ))
            .width(62)
            .align_right(62)
            .into()
        });
        rows.push(r.into());
    }
    let who = app.form.invite.trim().to_string();
    let send = (can && !who.is_empty()).then(|| Msg::RoomOp(Op::Invite(room.id.clone(), who)));
    column![
        h4("Members"),
        cap(app.t(
            "Roles は「誰の番か」を出すためのものです。何かを禁止はしません。",
            "Roles decide whose turn is shown. They do not restrict anything."
        )),
        lines(rows),
        row![
            input("GitHub username", &app.form.invite, Field::Invite, send.clone()).width(Length::Fill),
            primary("Invite", app.accent, send),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        cap(app.t(
            "Invite された人は、次に開いたとき、この Room が出ます。Invite しても、repository を見る権限は渡りません。",
            "Invited people see this Room the next time they open asis. An invite never grants access to a repository."
        )),
        notice_line(app),
    ]
    .spacing(14)
    .into()
}

fn repositories<'a>(app: &'a App, room: &'a Room, admin: bool) -> Element<'a, Msg> {
    let can = admin && !app.busy;
    let none = || -> Vec<Element<'a, Msg>> { vec![cap("None").into()] };
    let mut repos: Vec<Element<'a, Msg>> = room
        .repos
        .iter()
        .map(|repo| {
            let every = room.everyone.contains(repo);
            row![
                text(repo.clone()).size(14).color(TEXT).width(Length::Fill),
                tog(
                    if every { "Everyone" } else { "Members only" },
                    every,
                    app.accent,
                    can.then(|| Msg::RoomOp(Op::Everyone(room.id.clone(), repo.clone(), !every))),
                ),
                tog(
                    "Remove",
                    false,
                    app.accent,
                    can.then(|| Msg::RoomOp(Op::DropRepo(room.id.clone(), repo.clone()))),
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        })
        .collect();
    if repos.is_empty() {
        repos = none();
    }
    let mut checks: Vec<Element<'a, Msg>> = room
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
        checks = none();
    }
    let repo = app.form.repo.trim().to_string();
    let add_repo =
        (can && !repo.is_empty()).then(|| Msg::RoomOp(Op::AddRepo(room.id.clone(), repo)));
    let check = app.form.check.trim().to_string();
    let add_check = (can && !check.is_empty() && !room.ignore_checks.contains(&check)).then(|| {
        let mut list = room.ignore_checks.clone();
        list.push(check);
        Msg::RoomOp(Op::Checks(room.id.clone(), list))
    });
    column![
        h4("Repositories"),
        cap(app.t(
            "この Room で見る repositories。既定では Members の分だけを出します。",
            "Repositories this Room watches. By default only work by Members is shown."
        )),
        lines(repos),
        row![
            input("owner/repo", &app.form.repo, Field::Repo, add_repo.clone()).width(Length::Fill),
            primary("Add", app.accent, add_repo),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        notice_line(app),
        Space::new().height(6),
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
            .width(Length::Fill),
            seg_opt("Add", add_check),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(14)
    .into()
}

/// 押せないこともある、線の枠のボタン
fn seg_opt<'a>(label: &str, msg: Option<Msg>) -> Element<'a, Msg> {
    match msg {
        Some(m) => seg(label.into(), false, m),
        None => container(text(label.to_string()).size(14).color(LINE2))
            .padding([6, 12])
            .style(boxed(LINE, Color::TRANSPARENT))
            .into(),
    }
}

fn room_page<'a>(app: &'a App, room: &'a Room) -> Element<'a, Msg> {
    let free = !app.busy;
    column![
        h4("Room"),
        lines(vec![
            row![
                text("Name").size(14).color(TEXT).width(Length::Fill),
                text(room.name.clone()).size(14).color(DIM),
            ]
            .into(),
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
        notice_line(app),
    ]
    .spacing(14)
    .into()
}

fn look(app: &App) -> Element<'_, Msg> {
    let mut swatches = row![].spacing(12).align_y(Alignment::Center);
    for (hex, name) in COLORS {
        let color = color_of(hex).unwrap_or(DIM);
        let on = app.accent == color;
        swatches = swatches.push(
            button(Space::new().width(30).height(30))
                .padding(0)
                .style(move |_, _| button::Style {
                    background: Some(color.into()),
                    border: Border {
                        color: if on { TEXT } else { BG },
                        width: 2.0,
                        radius: 6.0.into(),
                    },
                    ..button::Style::default()
                })
                .on_press(Msg::Accent(hex.to_string())),
        );
        let _ = name;
    }
    let apply = Msg::Accent(app.form.color.clone());
    column![
        h4("Color"),
        cap(app.t(
            "画面で使う色は 1 色だけです。自分の番と、いま居る場所に使います。",
            "asis uses a single color: for your turn and for where you are."
        )),
        swatches,
        row![
            input("#rrggbb", &app.form.color, Field::Color, Some(apply.clone())).width(180),
            seg("Apply".into(), false, apply),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        Space::new().height(6),
        h4("Language"),
        cap(app.t(
            "説明の文に使う言葉。Room・Members・Merge のような大事な単語は、どちらでも英語のままです。",
            "Language for explanations. Key terms such as Room, Members and Merge stay in English either way."
        )),
        row![
            tog("日本語", !app.english, app.accent, Some(Msg::Lang(false))),
            tog("English", app.english, app.accent, Some(Msg::Lang(true))),
        ]
        .spacing(8),
        notice_line(app),
    ]
    .spacing(14)
    .into()
}

fn account(app: &App) -> Element<'_, Msg> {
    let github = match &app.gh {
        Some(Gh::Ready(login)) => login.clone(),
        None => "Checking…".into(),
        _ => "Not connected".into(),
    };
    let (state, action): (&str, Element<'_, Msg>) = match &app.rooms {
        Rooms::Read(_) => ("Signed in", seg("Sign out".into(), false, Msg::SignOut)),
        Rooms::NoDb => (
            "No Database",
            seg(
                "Database".into(),
                false,
                Msg::Pop(Some(Pop::Settings(SetTab::Db))),
            ),
        ),
        Rooms::SignedOut | Rooms::Failed(_) => ("Not signed in", sign_in_button(app)),
    };
    let mut rows = column![
        h4("Account"),
        lines(vec![
            row![
                label(
                    "GitHub",
                    app.t(
                        "gh のログインを借りています。repositories はこの権限で読みます",
                        "Uses your gh login. Repositories are read with this permission"
                    )
                ),
                text(github).size(14).color(DIM),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
            row![
                label(
                    "Rooms",
                    app.t(
                        "本人確認だけ。repositories を読む許可は渡していません",
                        "Identity only. No permission to read repositories is shared"
                    )
                ),
                text(state).size(14).color(DIM),
                action,
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        ]),
    ]
    .spacing(14);
    if let Rooms::Failed(e) = &app.rooms {
        rows = rows.push(text(e.clone()).size(13).color(BAD));
    }
    rows.push(notice_line(app)).into()
}

fn database(app: &App) -> Element<'_, Msg> {
    let ready = !app.form.db_url.trim().is_empty() && !app.form.db_key.trim().is_empty();
    column![
        h4("Database"),
        cap(app.t(
            "Room(repositories・Members・Roles)を置く DB。issue・PR・コードは入りません。",
            "Where Rooms (repositories, Members, Roles) are stored. Issues, PRs and code never go there."
        )),
        lines(vec![row![
            text("Connection").size(14).color(TEXT).width(Length::Fill),
            text(app.db_url.clone().unwrap_or_else(|| "Not set".into())).size(14).color(DIM),
        ]
        .into()]),
        cap(app.t(
            "自分の DB を使うときは、場所と公開用の鍵を入れます。つなぎ先を替えると、Sign in をやり直します。",
            "To use your own database, enter its URL and publishable key. Changing it signs you out."
        )),
        row![
            input("https://xxxx.supabase.co", &app.form.db_url, Field::DbUrl, None).width(Length::Fill),
            input("Publishable key", &app.form.db_key, Field::DbKey, ready.then_some(Msg::SaveDb))
                .secure(true)
                .width(Length::Fill),
            seg_opt("Change", ready.then_some(Msg::SaveDb)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        notice_line(app),
        Space::new().height(6),
        h4("About"),
        cap(format!("asis {} · MIT · development, as is", env!("CARGO_PKG_VERSION"))),
    ]
    .spacing(14)
    .into()
}
