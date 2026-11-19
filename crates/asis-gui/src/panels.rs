//! ボードの代わりに出す画面(はじめの画面・困ったときの見せ方)と、この PC の設定の中身、New room、
//! それに、ページを組む小さな部品。部屋の中のページは `pages.rs`。
//! 形は試作(asis-prototype.html)のとおり。言葉は英語、説明の文だけ設定の言葉で出す。
use super::*;
use iced::widget::{column, row};

/// 前に出す画面。1 件を作るものだけ(設定はページにした)
#[derive(Clone, Copy, PartialEq)]
pub enum Pop {
    NewRoom,
}

/// この PC の設定のページ
#[derive(Clone, Copy, PartialEq)]
pub enum SetTab {
    Look,
    Account,
    Claude,
    Local,
    Db,
    About,
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

pub fn h4<'a>(s: &str) -> iced::widget::Text<'a> {
    text(s.to_string()).size(16).color(TEXT)
}

/// 打つ欄。`send` が無ければ、Enter では何もしない
pub fn input<'a>(
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
pub fn tog<'a>(label: &str, on: bool, accent: Color, msg: Option<Msg>) -> Element<'a, Msg> {
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
pub fn danger<'a>(label: &str, msg: Option<Msg>) -> Element<'a, Msg> {
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
pub fn label<'a>(name: &str, note: &str) -> Element<'a, Msg> {
    column![
        text(name.to_string()).size(14).color(TEXT),
        cap(note.to_string())
    ]
    .spacing(2)
    .width(Length::Fill)
    .into()
}

/// 線で区切った行の束
pub fn lines<'a>(rows: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
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

pub fn choice<'a>(body: Element<'a, Msg>) -> Element<'a, Msg> {
    container(body)
        .padding([18, 20])
        .width(Length::Fill)
        .style(boxed(FRAME, Color::TRANSPARENT))
        .into()
}

pub fn notice_line<'a>(app: &App) -> Element<'a, Msg> {
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

pub fn is_me(app: &App, login: &str) -> bool {
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

pub fn is_admin(app: &App, room: &Room) -> bool {
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

pub fn page<'a>(body: Element<'a, Msg>) -> Element<'a, Msg> {
    scrollable(
        container(container(body).max_width(720))
            .center_x(Length::Fill)
            .padding([60, 22]),
    )
    .height(Length::Fill)
    .into()
}

pub fn code<'a>(s: &str) -> Element<'a, Msg> {
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
                "AsIs は、GitHub の公式コマンド gh のログインを借りて読みます。この PC では、まだ使えません。",
                "AsIs reads GitHub through the official gh command. It is not available on this machine yet."
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
                Some(Msg::Area(Area::Settings(SetTab::Db)))
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
            text("AsIs").size(26).color(TEXT),
            text(app.t(
                "開発を、あるがままに見える化します。何も入力しなくても、GitHub で起きていることから「いまどこまで進んでいて、次は誰の番か」を出します。",
                "See development as it is. With no input at all, AsIs reads what happened on GitHub and shows how far things are and whose turn is next."
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
            choice(
                column![
                    text(app.t("3. Claude をつなぐ", "3. Connect Claude")).size(15).color(TEXT),
                    cap(app.t(
                        "手順書を入れると、Claude が issue から作業を始め、PR を issue につなぎます。あとから Settings › Claude でも入れられます。",
                        "With the skill installed, Claude starts work from an issue and links its PRs. You can also do this later under Settings › Claude."
                    )),
                    row![if app.skill {
                        Element::from(text("Installed").size(13).color(app.accent))
                    } else {
                        seg("Install skill".into(), false, Msg::SkillInstall)
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

pub fn sign_in_button(app: &App) -> Element<'_, Msg> {
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

/// ボードに出すものが無いとき
pub fn empty(app: &App) -> Element<'_, Msg> {
    let (head, note) = if app.loading && app.synced.is_none() {
        ("Loading from GitHub…", "")
    } else {
        (
            "Nothing open",
            app.t(
                "開いている issue と pull request はありません。",
                "No open issues or pull requests.",
            ),
        )
    };
    let mut c = column![text(head).size(15).color(TEXT)]
        .spacing(10)
        .align_x(Alignment::Center);
    if !note.is_empty() {
        c = c.push(cap(note));
    }
    container(c).center(Length::Fill).into()
}

// ---- 前に出す画面(1 件を作るもの)----

pub fn pop(app: &App) -> Option<(Element<'_, Msg>, f32)> {
    Some(match app.pop? {
        Pop::NewRoom => (new_room(app), 600.0),
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
            row![seg("Open Database settings".into(), false, Msg::Area(Area::Settings(SetTab::Db)))],
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

/// 押せないこともある、線の枠のボタン
pub fn seg_opt<'a>(label: &str, msg: Option<Msg>) -> Element<'a, Msg> {
    match msg {
        Some(m) => seg(label.into(), false, m),
        None => container(text(label.to_string()).size(14).color(LINE2))
            .padding([6, 12])
            .style(boxed(LINE, Color::TRANSPARENT))
            .into(),
    }
}

pub fn look(app: &App) -> Element<'_, Msg> {
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
            "AsIs uses a single color: for your turn and for where you are."
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

pub fn account(app: &App) -> Element<'_, Msg> {
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
                Msg::Area(Area::Settings(SetTab::Db)),
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

pub fn database(app: &App) -> Element<'_, Msg> {
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
        cap(app.t(
            "DB の用意のしかたは、README の「部屋」にあります(SQL を流す・GitHub のログインを有効にする)。",
            "Setup steps are in the README under Rooms (run the SQL, enable GitHub sign-in)."
        )),
        notice_line(app),
    ]
    .spacing(14)
    .into()
}

pub fn claude(app: &App) -> Element<'_, Msg> {
    let place = asis::skill::dir()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "~/.claude/skills/asis".into());
    column![
        h4("Claude"),
        cap(app.t(
            "issue は、そのまま Claude への指示になります。手順書を入れると、Claude が「#12 を始めて」で issue から作業を始め、PR を issue につなぎます。",
            "An issue is the instruction for Claude. With the skill installed, Claude starts from an issue on “Start #12” and links its PRs to that issue."
        )),
        sect(
            row![
                label("Skill for Claude Code", &place),
                if app.skill {
                    Element::from(text("Installed").size(14).color(app.accent))
                } else {
                    row![
                        text("Not installed").size(14).color(DIM),
                        primary("Install", app.accent, Some(Msg::SkillInstall)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into()
                },
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        ),
        sect(
            column![
                h4(app.t("Claude が守ること", "What Claude follows")),
                lines(vec![
                    label(
                        app.t("ブランチ名を issue の番号で始める", "Start the branch name with the issue number"),
                        "12-short-name"
                    ),
                    label(
                        app.t("PR の本文に issue を書く", "Write the issue in the PR body"),
                        "Closes #12 / Refs #12"
                    ),
                ]),
                cap(app.t(
                    "repository に別の決まり(CLAUDE.md など)があれば、そちらが優先されます。",
                    "If a repository has its own rules (CLAUDE.md etc.), those win."
                )),
            ]
            .spacing(10)
            .into()
        ),
        notice_line(app),
    ]
    .spacing(14)
    .into()
}

pub fn local_work(app: &App) -> Element<'_, Msg> {
    let mut roots: Vec<Element<'_, Msg>> = app
        .roots
        .iter()
        .map(|root| {
            row![
                text(root.clone()).size(14).color(TEXT).width(Length::Fill),
                tog(
                    "Remove",
                    false,
                    app.accent,
                    Some(Msg::DropRoot(root.clone()))
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        })
        .collect();
    if roots.is_empty() {
        roots = vec![cap("None").into()];
    }
    let ready = !app.form.root.trim().is_empty();
    let place = match asis::host::wsl() {
        Some(distro) => format!("WSL · {distro}"),
        None => "This machine".to_string(),
    };
    column![
        h4("Local work"),
        cap(app.t(
            "この PC の worktree とターミナルを読み、PR になる前の作業をカードに重ねます。ほかの人の PC の作業は見えません。",
            "Reads worktrees and terminals on this machine and shows work before it becomes a PR. Work on other machines is not visible."
        )),
        sect(
            column![
                h4("Folders"),
                cap(app.t(
                    "clone を探すフォルダ。直下にある clone と、その worktree を読みます。",
                    "Folders searched for clones. Clones directly inside, and their worktrees, are read."
                )),
                lines(roots),
                row![
                    input("~/src", &app.form.root, Field::Root, ready.then_some(Msg::AddRoot)).width(320),
                    seg_opt("Add", ready.then_some(Msg::AddRoot)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(10)
            .into()
        ),
        sect(
            row![
                label("Workspace", app.t("gh・git を動かす場所", "Where gh and git run")),
                text(place).size(14).color(DIM),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        ),
        notice_line(app),
    ]
    .spacing(14)
    .into()
}

pub fn about(_: &App) -> Element<'_, Msg> {
    column![
        h4("About"),
        sect(
            row![
                label("AsIs", "development, as is · MIT"),
                text(version()).size(14).color(DIM),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        ),
    ]
    .spacing(14)
    .into()
}

/// 線の枠で囲んだ、ひとまとまり
pub fn sect<'a>(body: Element<'a, Msg>) -> Element<'a, Msg> {
    container(body)
        .padding([16, 18])
        .width(Length::Fill)
        .style(boxed(FRAME, Color::TRANSPARENT))
        .into()
}
