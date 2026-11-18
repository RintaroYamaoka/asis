//! asis の窓の画面。読み込みと並べ方は asis の部品を使い、ここは描くだけ。
//!
//! 状態(App)→ 操作(Msg)→ 描く(view)の 3 つに分ける。TUI も同じ形にそろえる。
//! 骨組み:左端に部屋の列 → repo とメンバー → 真ん中(ボードかリスト)→ 右に詳細。
#![cfg_attr(windows, windows_subsystem = "windows")]

use asis::view::{Filter, Kind, RepoView, Row, Stage};
use asis::{Loader, Section};
use iced::keyboard::{self, key::Named, Key as KeyCode};
use iced::widget::{
    button, column, container, mouse_area, opaque, responsive, row, scrollable, stack, text,
    text_input, Space,
};
use iced::{Alignment, Border, Color, Element, Font, Length, Subscription, Task, Theme};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 開いたままのとき、何秒ごとに読み直すか
const EVERY: Duration = Duration::from_secs(30);

/// 行を指す鍵(repo・種類・番号・題)。読み直しても同じ行を選んだままにする
type Key = (String, Kind, u64, String);

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Board,
    List,
}

#[derive(Default)]
struct Write {
    repo: Option<String>,
    title: String,
    body: String,
}

struct App {
    loader: Arc<Mutex<Option<Loader>>>,
    sections: Arc<Vec<Section>>,
    /// 開いている部屋。None は全部
    room: Option<usize>,
    repo: Option<String>,
    filter: Filter,
    look: Look,
    selected: Option<Key>,
    /// 選んだ行の中の、PR 1 本。None なら issue そのもの
    pr: Option<u64>,
    /// 選んだ 1 件の中身を、前に大きく出しているか
    open: bool,
    /// issue を書いている途中の中身
    write: Option<Write>,
    /// 直しを頼む一言を書いている PR
    fixing: Option<u64>,
    comment: String,
    /// 取り込みを 1 回 押した PR(もう 1 回で進む)
    confirm: Option<u64>,
    /// 操作の結果。(文, うまくいったか)
    notice: Option<(String, bool)>,
    /// 操作を GitHub に送っている最中
    busy: bool,
    loading: bool,
    synced: Option<Instant>,
    error: Option<String>,
    accent: Color,
    /// 説明の文を英語で出すか(大事な単語は、どちらでも英語)
    english: bool,
}

impl App {
    /// 説明の文を、設定の言葉で返す
    fn t(&self, ja: &'static str, en: &'static str) -> &'static str {
        if self.english {
            en
        } else {
            ja
        }
    }
}

#[derive(Clone)]
enum Msg {
    Loaded(Result<Arc<Vec<Section>>, String>),
    Room(Option<usize>),
    Repo(Option<String>),
    Filter(Filter),
    Look(Look),
    Select(Key, Option<u64>),
    Close,
    Act(asis::act::Action),
    Acted(Result<String, String>),
    AskFix(u64),
    Comment(String),
    AskMerge(u64),
    WriteOpen,
    WriteRepo(String),
    WriteTitle(String),
    WriteBody(String),
    Open(String),
    Refresh,
    Again,
    Key(keyboard::Event),
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Msg")
    }
}

fn load(loader: Arc<Mutex<Option<Loader>>>, force: bool) -> Task<Msg> {
    Task::perform(
        async move {
            let mut guard = loader
                .lock()
                .map_err(|_| "読み込みが途中で止まりました".to_string())?;
            if guard.is_none() {
                // `asis-gui owner/repo` と打てば、その repo だけを既定の決まりで開く
                let repos: Vec<String> = std::env::args().skip(1).collect();
                let source = if repos.is_empty() {
                    asis::default_source(&[])?
                } else {
                    asis::Source {
                        watch: repos,
                        only: Default::default(),
                        roots: vec![],
                        local: vec![],
                    }
                };
                *guard = Some(Loader::new(source));
            }
            Ok(Arc::new(guard.as_mut().expect("入れたばかり").load(force)))
        },
        Msg::Loaded,
    )
}

fn boot() -> (App, Task<Msg>) {
    let loader = Arc::new(Mutex::new(None));
    // 1 色は、手元の設定で選べる。書いていなければ既定の色
    let accent = asis::config::load()
        .ok()
        .and_then(|l| l.accent)
        .and_then(|c| asis::config::parse_color(&c))
        .map(|[r, g, b]| Color::from_rgb8(r, g, b))
        .unwrap_or(Color::from_rgb8(0xf2, 0xb6, 0x3c));
    let english = asis::config::load().ok().and_then(|l| l.lang).as_deref() == Some("en");
    let app = App {
        loader: loader.clone(),
        sections: Arc::new(Vec::new()),
        room: None,
        repo: None,
        filter: Filter::All,
        look: Look::Board,
        selected: None,
        pr: None,
        open: false,
        write: None,
        fixing: None,
        comment: String::new(),
        confirm: None,
        notice: None,
        busy: false,
        loading: true,
        synced: None,
        error: None,
        accent,
        english,
    };
    (app, load(loader, true))
}

fn update(app: &mut App, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Loaded(result) => {
            app.loading = false;
            match result {
                Ok(sections) => {
                    // 最初に開いたとき、部屋が 1 つならそれを開く
                    if app.synced.is_none() && sections.len() == 1 {
                        app.room = Some(0);
                    }
                    app.sections = sections;
                    // 作るとき用:最初の 1 件の中身を出した状態で開く(画面を撮って確かめるため)
                    #[cfg(debug_assertions)]
                    if app.synced.is_none() && std::env::var_os("ASIS_OPEN_FIRST").is_some() {
                        step(app, 1);
                        app.open = app.selected.is_some();
                    }
                    app.synced = Some(Instant::now());
                    app.error = None;
                    if app.room.is_some_and(|i| i >= app.sections.len()) {
                        app.room = None;
                    }
                }
                Err(e) => app.error = Some(e),
            }
            return Task::perform(async { std::thread::sleep(EVERY) }, |_| Msg::Again);
        }
        Msg::Again | Msg::Refresh if app.loading => {}
        Msg::Again => {
            app.loading = true;
            return load(app.loader.clone(), false);
        }
        Msg::Refresh => {
            app.loading = true;
            return load(app.loader.clone(), true);
        }
        Msg::Room(room) => {
            app.room = room;
            app.repo = None;
            app.selected = None;
            app.open = false;
        }
        Msg::Repo(repo) => app.repo = repo,
        Msg::Filter(f) => app.filter = f,
        Msg::Look(look) => app.look = look,
        Msg::Select(key, pr) => {
            app.selected = Some(key);
            app.pr = pr;
            app.open = true;
            forget(app);
        }
        Msg::Close => {
            app.open = false;
            app.write = None;
            forget(app);
        }
        Msg::Act(action) => {
            if app.busy {
                return Task::none();
            }
            app.busy = true;
            app.notice = None;
            return Task::perform(async move { asis::act::run(&action) }, Msg::Acted);
        }
        Msg::Acted(result) => {
            app.busy = false;
            app.fixing = None;
            app.confirm = None;
            match result {
                Ok(done) => {
                    app.notice = Some((done, true));
                    app.comment.clear();
                    app.write = None;
                    // 操作は事実を作っただけ。状態は、読み直した事実から決まる
                    if !app.loading {
                        app.loading = true;
                        return load(app.loader.clone(), true);
                    }
                }
                Err(e) => app.notice = Some((e, false)),
            }
        }
        Msg::AskFix(n) => {
            app.fixing = Some(n);
            app.confirm = None;
        }
        Msg::Comment(t) => app.comment = t,
        Msg::AskMerge(n) => {
            app.confirm = Some(n);
            app.fixing = None;
        }
        Msg::WriteOpen => {
            let repos = repos_of(app);
            app.notice = None;
            app.write = Some(Write {
                repo: app
                    .repo
                    .clone()
                    .or_else(|| (repos.len() == 1).then(|| repos[0].clone())),
                ..Write::default()
            });
        }
        Msg::WriteRepo(repo) => {
            if let Some(w) = &mut app.write {
                w.repo = Some(repo);
            }
        }
        Msg::WriteTitle(t) => {
            if let Some(w) = &mut app.write {
                w.title = t;
            }
        }
        Msg::WriteBody(t) => {
            if let Some(w) = &mut app.write {
                w.body = t;
            }
        }
        Msg::Open(url) => open(&url),
        Msg::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            if modifiers.control() || modifiers.alt() || modifiers.logo() {
                return Task::none();
            }
            // 字を打っている間・中身を出している間は、Esc だけを受ける(打った字を奪わない)
            if app.write.is_some() || app.open {
                match key.as_ref() {
                    KeyCode::Named(Named::Escape) => {
                        app.open = false;
                        app.write = None;
                        forget(app);
                    }
                    // 同じ issue の、前の PR・次の PR へ(字を打っている間は動かさない)
                    KeyCode::Named(Named::ArrowRight)
                        if app.fixing.is_none() && app.write.is_none() =>
                    {
                        turn_pr(app, 1)
                    }
                    KeyCode::Named(Named::ArrowLeft)
                        if app.fixing.is_none() && app.write.is_none() =>
                    {
                        turn_pr(app, -1)
                    }
                    _ => {}
                }
                return Task::none();
            }
            match key.as_ref() {
                KeyCode::Named(Named::Tab) => app.filter = app.filter.next(),
                // 中身を出していれば閉じる。出していなければ、選ぶのをやめる
                KeyCode::Named(Named::Escape) => {
                    if app.open {
                        app.open = false;
                    } else {
                        app.selected = None;
                    }
                }
                KeyCode::Named(Named::ArrowDown) => step(app, 1),
                KeyCode::Named(Named::ArrowUp) => step(app, -1),
                // 中身を出す
                KeyCode::Named(Named::Enter) => app.open = app.selected.is_some(),
                KeyCode::Character("j") => step(app, 1),
                KeyCode::Character("k") => step(app, -1),
                KeyCode::Character("c") => return update(app, Msg::WriteOpen),
                KeyCode::Character("v") => {
                    app.look = match app.look {
                        Look::Board => Look::List,
                        Look::List => Look::Board,
                    }
                }
                KeyCode::Character("r") if !app.loading => {
                    app.loading = true;
                    return load(app.loader.clone(), true);
                }
                _ => {}
            }
        }
        Msg::Key(_) => {}
    }
    Task::none()
}

/// 書きかけの操作を忘れる(別の 1 件に移ったとき・閉じたとき)
fn forget(app: &mut App) {
    app.fixing = None;
    app.confirm = None;
    app.comment.clear();
    app.notice = None;
}

/// GitHub のページをブラウザで開く
fn open(url: &str) {
    use std::process::{Command, Stdio};
    let tries: [(&str, &[&str]); 5] = [
        ("wslview", &[]),
        ("xdg-open", &[]),
        ("open", &[]),
        ("rundll32.exe", &["url.dll,FileProtocolHandler"]),
        ("explorer.exe", &[]),
    ];
    for (program, args) in tries {
        if Command::new(program)
            .args(args)
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}

// ---- 何を出すか(描く前の並べ方)----

/// ボードの列。段階を 4 つにまとめる
const COLUMNS: [&str; 4] = ["Todo", "In progress", "In review", "Merged"];
/// リストで束を並べる順(人の手が要る順)
const LIST_ORDER: [usize; 4] = [2, 3, 1, 0];

fn column_of(stage: Stage) -> usize {
    match stage {
        Stage::Todo => 0,
        Stage::Doing | Stage::NeedsFix | Stage::Testing => 1,
        Stage::InReview | Stage::ReadyToMerge => 2,
        Stage::AwaitingShip | Stage::ShippedOpen => 3,
    }
}

type Item<'a> = (&'a RepoView, &'a Row);

/// 開いている部屋の、repo で絞ったあとの全部の行
fn items(app: &App) -> Vec<Item<'_>> {
    app.sections
        .iter()
        .enumerate()
        .filter(|(i, _)| app.room.is_none_or(|r| r == *i))
        .flat_map(|(_, s)| s.repos.iter().filter_map(|r| r.as_ref().ok()))
        .filter(|v| app.repo.as_ref().is_none_or(|r| *r == v.repo))
        .flat_map(|v| {
            v.groups
                .iter()
                .flat_map(move |g| g.rows.iter().map(move |r| (v, r)))
        })
        .collect()
}

/// 絞り込みまで当てたもの
fn shown(app: &App) -> Vec<Item<'_>> {
    items(app)
        .into_iter()
        .filter(|(v, r)| r.shows(app.filter, &v.viewer))
        .collect()
}

/// issue の段。人の手が要る順(In review → Merged → In progress → Todo)
fn lanes<'a>(shown: &[Item<'a>]) -> Vec<Item<'a>> {
    let mut out = Vec::new();
    for col in LIST_ORDER {
        out.extend(
            shown
                .iter()
                .filter(|(_, r)| r.kind != Kind::Pr && column_of(r.stage) == col),
        );
    }
    out
}

/// issue につながっていない PR。列の順
fn loose<'a>(shown: &[Item<'a>]) -> Vec<Item<'a>> {
    let mut out = Vec::new();
    for col in 0..4 {
        out.extend(
            shown
                .iter()
                .filter(|(_, r)| r.kind == Kind::Pr && column_of(r.stage) == col),
        );
    }
    out
}

/// PR 1 本が入る列。出さないもの(本番に出た・閉じた)は None
fn column_of_pr(mark: &str) -> Option<usize> {
    match mark {
        "…" | "!" | "~" => Some(1),
        "?" | "+" => Some(2),
        "↑" => Some(3),
        _ => None,
    }
}

fn key_of(view: &RepoView, row: &Row) -> Key {
    (view.repo.clone(), row.kind, row.number, row.title.clone())
}

fn current(app: &App) -> Option<Item<'_>> {
    let sel = app.selected.as_ref()?;
    items(app).into_iter().find(|(v, r)| key_of(v, r) == *sel)
}

/// 上下キーで、次の 1 件へ。issue の段 → つながっていない PR、の順
fn step(app: &mut App, by: i32) {
    let all = shown(app);
    let list: Vec<(Key, Option<u64>)> = lanes(&all)
        .iter()
        .map(|(v, r)| (key_of(v, r), None))
        .chain(
            loose(&all)
                .iter()
                .map(|(v, r)| (key_of(v, r), Some(r.number))),
        )
        .collect();
    if list.is_empty() {
        return;
    }
    let at = app
        .selected
        .as_ref()
        .and_then(|s| list.iter().position(|(k, _)| k == s));
    let next = match at {
        Some(i) => (i as i32 + by).clamp(0, list.len() as i32 - 1) as usize,
        None => 0,
    };
    app.selected = Some(list[next].0.clone());
    app.pr = list[next].1;
}

/// 同じ issue の中で、前の PR・次の PR へ
fn turn_pr(app: &mut App, by: i32) {
    let Some((_, r)) = current(app) else {
        return;
    };
    let numbers: Vec<u64> = r.prs.iter().map(|p| p.number).collect();
    let Some(at) = app.pr.and_then(|n| numbers.iter().position(|x| *x == n)) else {
        return;
    };
    let n = numbers.len() as i32;
    let next = numbers[((at as i32 + by).rem_euclid(n)) as usize];
    app.pr = Some(next);
    forget(app);
}

fn short(repo: &str) -> &str {
    repo.rsplit('/').next().unwrap_or(repo)
}

fn label_of(row: &Row) -> String {
    match row.kind {
        Kind::Task => format!("#{}", row.number),
        Kind::Pr => format!("PR {}", row.number),
        Kind::Local => "Local".to_string(),
    }
}

/// いま番を持っている PR の題。issue が漠然としていても、何で止まっているかが分かる
fn now_of(row: &Row) -> String {
    if row.kind != Kind::Task {
        return row
            .local
            .first()
            .map(|l| l.branch.clone())
            .unwrap_or_default();
    }
    row.prs
        .iter()
        .find(|p| !p.turn.is_empty())
        .map(|p| p.title.clone())
        .unwrap_or_default()
}

/// PR の札に出す言葉。GitHub が使っている言葉のまま
fn word_of(p: &asis::view::PrLine) -> String {
    match p.mark {
        "↑" => "Merged".to_string(),
        _ => p.why.clone(),
    }
}

// ---- 見た目。色は 1 色だけ(設定で選ぶ)。面を塗らず、線で区切る ----
const BG: Color = Color::from_rgb(0.051, 0.055, 0.051);
const PANEL: Color = Color::from_rgb(0.086, 0.094, 0.086);
const LINE: Color = Color::from_rgb(0.122, 0.129, 0.122);
const LINE2: Color = Color::from_rgb(0.2, 0.212, 0.2);
const BRIGHT: Color = Color::from_rgb(0.42, 0.44, 0.415);
const TEXT: Color = Color::from_rgb(0.925, 0.933, 0.914);
const DIM: Color = Color::from_rgb(0.553, 0.576, 0.549);
const BAD: Color = Color::from_rgb(0.91, 0.51, 0.435);

fn boxed(line: Color, fill: Color) -> impl Fn(&Theme) -> container::Style {
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

fn fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        ..container::Style::default()
    }
}

fn rule<'a>() -> Element<'a, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .style(fill(LINE))
        .into()
}

fn vrule<'a>() -> Element<'a, Msg> {
    container(Space::new().width(1).height(Length::Fill))
        .style(fill(LINE))
        .into()
}

fn cap<'a>(s: impl Into<String>) -> iced::widget::Text<'a> {
    text(s.into()).size(12).color(DIM)
}

/// 線の枠の、押せる字。選ばれているものは明るく
fn seg<'a>(label: String, on: bool, msg: Msg) -> Element<'a, Msg> {
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

fn turn_of<'a>(row: &Row, viewer: &str, accent: Color) -> iced::widget::Text<'a> {
    if row.is_mine(viewer) {
        text("You").size(13).color(accent)
    } else if row.turn.is_empty() {
        text("—").size(13).color(DIM)
    } else {
        text(row.turn.join("・")).size(13).color(DIM)
    }
}

fn chip<'a>(p: &asis::view::PrLine) -> Element<'a, Msg> {
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

const FRAME: Color = Color::from_rgb(0.165, 0.173, 0.165);

/// 押せる枠。自分の番は枠を色に、選んでいるものは明るい灰色の枠に
fn pressable<'a>(
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

/// ボードのカードの高さ。全部 同じ大きさにする(PR がまだ無い issue の 1 枚も同じ)
const CARD_H: f32 = 140.0;

fn card_box<'a>(body: Element<'a, Msg>, line: Color, on: bool, msg: Msg) -> Element<'a, Msg> {
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
fn pr_card<'a>(
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
            cap(r.age.clone()),
        ]
        .spacing(8),
        // 題は 3 行ぶんの高さに固定する。カードの形をそろえるため
        container(
            text(p.title.clone())
                .size(14)
                .color(TEXT)
                .wrapping(text::Wrapping::WordOrGlyph)
        )
        .height(58)
        .clip(true),
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
fn todo_card<'a>(app: &App, v: &'a RepoView, r: &'a Row) -> Element<'a, Msg> {
    let key = key_of(v, r);
    let mine = r.is_mine(&v.viewer);
    let local = r.local.first().map(|l| l.branch.clone());
    let body = column![
        cap(match (&local, r.kind) {
            (Some(b), _) => format!("Local: {b}"),
            (None, _) if r.stage == Stage::Doing => "Branch pushed · no PR yet".to_string(),
            _ => "No PR yet".to_string(),
        }),
        row![
            Space::new().width(Length::Fill),
            turn_of(r, &v.viewer, app.accent)
        ],
    ]
    .spacing(14);
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
fn middle<'a>(app: &'a App, width: f32) -> Element<'a, Msg> {
    let all = shown(app);
    let tasks = lanes(&all);
    let free = loose(&all);
    if all.is_empty() {
        return container(cap(if app.loading {
            "Loading from GitHub…"
        } else {
            "Nothing to show"
        }))
        .center(Length::Fill)
        .into();
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
                cap(r.age.clone()),
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
fn list_row<'a>(app: &App, v: &'a RepoView, r: &'a Row) -> Element<'a, Msg> {
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
        container(cap(r.age.clone())).width(48),
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

/// 押してほしいボタン。色の線の枠
fn primary<'a>(label: &str, accent: Color, msg: Option<Msg>) -> Element<'a, Msg> {
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

fn field<'a>(hint: &str, value: &str, on: fn(String) -> Msg, send: Msg) -> Element<'a, Msg> {
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

fn notice_of<'a>(app: &App) -> Option<Element<'a, Msg>> {
    app.notice.as_ref().map(|(t, ok)| {
        text(t.clone())
            .size(13)
            .color(if *ok { app.accent } else { BAD })
            .into()
    })
}

/// 選んだ 1 件の中身。前に大きく出す。
/// 段の頭を押したら issue 1 件、カードを押したら PR 1 本。操作は PR 1 本の中でする。
fn detail_of(app: &App) -> Option<Element<'_, Msg>> {
    let (v, r) = current(app)?;
    let key = key_of(v, r);
    let me = |turn: &[String]| -> (String, bool) {
        if turn.contains(&v.viewer) {
            ("you".to_string(), true)
        } else if turn.is_empty() {
            ("no one".to_string(), false)
        } else {
            (turn.join(", "), false)
        }
    };
    let why = |state: String, next: String| -> Element<'_, Msg> {
        container(
            text(if r.age.is_empty() {
                format!("{state} · Waiting on {next}")
            } else {
                format!("{state} · Waiting on {next} · idle {}", r.age)
            })
            .size(14)
            .color(TEXT)
            .wrapping(text::Wrapping::WordOrGlyph),
        )
        .padding([12, 14])
        .width(Length::Fill)
        .style(boxed(FRAME, Color::TRANSPARENT))
        .into()
    };
    let at = app
        .pr
        .and_then(|n| r.prs.iter().position(|p| p.number == n));
    let mut d = column![].spacing(14);
    match at {
        // ---- PR 1 本 ----
        Some(at) => {
            let p = &r.prs[at];
            let (next, _) = me(&p.turn);
            let mut top = row![cap(format!("{} · PR #{}", v.repo, p.number))]
                .spacing(10)
                .align_y(Alignment::Center);
            if r.kind == Kind::Task {
                top = top.push(seg(
                    format!("issue #{}", r.number),
                    false,
                    Msg::Select(key.clone(), None),
                ));
            } else {
                top = top.push(cap("No linked issue"));
            }
            top = top.push(Space::new().width(Length::Fill));
            let n = r.prs.len();
            if n > 1 {
                let go = |by: usize| Msg::Select(key.clone(), Some(r.prs[(at + by) % n].number));
                top = top
                    .push(seg("‹".into(), false, go(n - 1)))
                    .push(cap(format!("{} / {n}", at + 1)))
                    .push(seg("›".into(), false, go(1)));
            }
            d = d
                .push(top)
                .push(
                    text(p.title.clone())
                        .size(20)
                        .color(TEXT)
                        .wrapping(text::Wrapping::WordOrGlyph),
                )
                .push(why(p.why.clone(), next))
                .push(cap(format!("by {}", p.author)));
            if let Some(n) = notice_of(app) {
                d = d.push(n);
            }
            // その PR の今の事実で、できる操作だけを出す
            let repo = v.repo.clone();
            let number = p.number;
            let free = !app.busy;
            let merge = |strong: bool| -> Element<'_, Msg> {
                if app.confirm == Some(number) {
                    primary(
                        "Confirm merge",
                        app.accent,
                        free.then_some(Msg::Act(asis::act::Action::Merge {
                            repo: repo.clone(),
                            number,
                        })),
                    )
                } else if strong {
                    primary("Merge", app.accent, Some(Msg::AskMerge(number)))
                } else {
                    seg("Merge".into(), false, Msg::AskMerge(number))
                }
            };
            if p.mark == "?" {
                if app.fixing == Some(number) {
                    let send = Msg::Act(asis::act::Action::RequestChanges {
                        repo: repo.clone(),
                        number,
                        comment: app.comment.clone(),
                    });
                    d = d.push(
                        row![
                            field(
                                "What needs to change?",
                                &app.comment,
                                Msg::Comment,
                                send.clone()
                            ),
                            primary("Request changes", app.accent, free.then_some(send)),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                } else {
                    d = d.push(
                        row![
                            primary(
                                "Approve",
                                app.accent,
                                free.then_some(Msg::Act(asis::act::Action::Approve {
                                    repo: repo.clone(),
                                    number
                                }))
                            ),
                            seg("Request changes…".into(), false, Msg::AskFix(number)),
                            merge(false),
                        ]
                        .spacing(8),
                    );
                }
            } else if p.mark == "+" {
                d = d.push(merge(true));
            }
            for l in &r.local {
                d = d.push(cap(format!("Local  {}  {}", l.branch, l.panes.join(" / "))));
            }
            d = d.push(seg(
                format!("Open PR #{number} on GitHub"),
                false,
                Msg::Open(format!("https://github.com/{}/pull/{number}", v.repo)),
            ));
        }
        // ---- issue 1 件(手元の作業の行も、ここ)----
        None => {
            let (next, _) = me(&r.turn);
            d = d
                .push(cap(format!("{} · {}", v.repo, label_of(r))))
                .push(
                    text(r.title.clone())
                        .size(20)
                        .color(TEXT)
                        .wrapping(text::Wrapping::WordOrGlyph),
                )
                .push(why(r.why.clone(), next));
            if let Some(n) = notice_of(app) {
                d = d.push(n);
            }
            if r.kind == Kind::Task {
                d = d.push(cap("Description")).push(
                    text(if r.body.trim().is_empty() {
                        "No description".to_string()
                    } else {
                        r.body.clone()
                    })
                    .size(14)
                    .color(DIM)
                    .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            if r.prs.is_empty() {
                d = d.push(cap("No pull requests yet"));
            } else {
                d = d.push(cap(format!("{} pull requests", r.prs.len())));
                for p in &r.prs {
                    let (who, mine) = me(&p.turn);
                    let one = column![
                        row![
                            cap(format!("#{}", p.number)),
                            chip(p),
                            Space::new().width(Length::Fill),
                            text(who)
                                .size(13)
                                .color(if mine { app.accent } else { DIM }),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                        text(p.title.clone())
                            .size(15)
                            .color(TEXT)
                            .wrapping(text::Wrapping::WordOrGlyph),
                    ]
                    .spacing(6);
                    d = d.push(pressable(
                        one.into(),
                        FRAME,
                        false,
                        [11, 14],
                        Msg::Select(key.clone(), Some(p.number)),
                    ));
                }
            }
            for l in &r.local {
                d = d.push(cap(format!("Local  {}  {}", l.branch, l.panes.join(" / "))));
            }
            if !r.url.is_empty() {
                d = d.push(seg(
                    format!(
                        "Open {} on GitHub",
                        if r.kind == Kind::Task {
                            format!("issue #{}", r.number)
                        } else {
                            label_of(r)
                        }
                    ),
                    false,
                    Msg::Open(r.url.clone()),
                ));
            }
        }
    }
    Some(
        scrollable(container(d).padding(24).width(Length::Fill))
            .width(760)
            .into(),
    )
}

/// issue を書く。1 行でよい。型は無い
fn write_form(app: &App) -> Option<Element<'_, Msg>> {
    let w = app.write.as_ref()?;
    let repos: Vec<String> = repos_of(app);
    let send = Msg::Act(asis::act::Action::WriteIssue {
        repo: w.repo.clone().unwrap_or_default(),
        title: w.title.clone(),
        body: w.body.clone(),
    });
    let ready = !app.busy && w.repo.is_some() && !w.title.trim().is_empty();
    let mut picks = row![].spacing(6);
    for repo in repos {
        let on = w.repo.as_deref() == Some(repo.as_str());
        picks = picks.push(seg(short(&repo).to_string(), on, Msg::WriteRepo(repo)));
    }
    let mut d = column![
        text("New issue").size(18).color(TEXT),
        cap(app.t(
            "1 行でよい。型は無い。書いたものが、そのまま AI への指示になる。",
            "One line is enough. No template. What you write becomes the instruction for the AI."
        )),
        picks.wrap(),
        field("Title", &w.title, Msg::WriteTitle, send.clone()),
        field(
            "Description (optional)",
            &w.body,
            Msg::WriteBody,
            send.clone()
        ),
    ]
    .spacing(12);
    if let Some(n) = notice_of(app) {
        d = d.push(n);
    }
    d = d.push(
        row![
            Space::new().width(Length::Fill),
            seg("Cancel".into(), false, Msg::Close),
            primary("Create", app.accent, ready.then_some(send)),
        ]
        .spacing(8),
    );
    Some(container(d).padding(24).width(600).into())
}

/// 開いている部屋の repo
fn repos_of(app: &App) -> Vec<String> {
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

fn view(app: &App) -> Element<'_, Msg> {
    let all = items(app);
    let count = |f: Filter| all.iter().filter(|(v, r)| r.shows(f, &v.viewer)).count();

    // 左端:部屋の列。数字は、その部屋で自分の番の件数
    let tile = |label: String, on: bool, badge: usize, msg: Msg| -> Element<'_, Msg> {
        let accent = app.accent;
        let inner = column![text(label).size(12).color(if on { accent } else { TEXT })]
            .push(if badge > 0 {
                Element::from(text(badge.to_string()).size(11).color(accent))
            } else {
                Space::new().into()
            })
            .align_x(Alignment::Center);
        button(container(inner).center_x(40).center_y(40))
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
    let mut rail = column![tile("All".into(), app.room.is_none(), 0, Msg::Room(None))]
        .spacing(10)
        .align_x(Alignment::Center);
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
            text(
                section
                    .map(|s| s.project.clone())
                    .unwrap_or("All rooms".into())
            )
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

    let frame = row![
        container(rail).padding([14, 11]),
        vrule(),
        container(scrollable(side)).width(210),
        vrule(),
        column![
            container(bar).padding([14, 22]),
            rule(),
            container(notes).padding([6, 22]),
            responsive(move |size| middle(app, size.width)),
            rule(),
            foot,
        ]
        .width(Length::Fill),
    ]
    .height(Length::Fill);
    let base: Element<'_, Msg> = container(frame).style(fill(BG)).into();
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

fn subscription(_: &App) -> Subscription<Msg> {
    keyboard::listen().map(Msg::Key)
}

fn main() -> iced::Result {
    // 日本語が入っている、その OS の字
    let font = if cfg!(windows) {
        "Yu Gothic UI"
    } else if cfg!(target_os = "macos") {
        "Hiragino Sans"
    } else {
        "Noto Sans JP"
    };
    // WSL の Wayland は、窓を出す途中で切れることがある。X11 で出す。
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    // 窓の最初の大きさ。`ASIS_WINDOW=1366x768` で変えられる(いろいろな画面の形で確かめる用)
    let size = std::env::var("ASIS_WINDOW")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
        })
        .unwrap_or((1320.0, 820.0));
    iced::application(boot, update, view)
        .title("asis")
        .subscription(subscription)
        .default_font(Font::with_name(font))
        .window_size(size)
        .run()
}
