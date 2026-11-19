//! asis の窓の画面。読み込みと並べ方は asis の部品を使い、ここは描くだけ。
//!
//! 状態(App)→ 操作(Msg)→ 描く(view)の 3 つに分ける。TUI も同じ形にそろえる。
//! 骨組み:左端に部屋の列 → repo とメンバー → 真ん中(ボードかリスト)→ 右に詳細。
mod arrange;
mod board;
mod chrome;
mod detail;
mod pages;
mod panels;
mod screen;
mod style;

use arrange::*;
use board::*;
use detail::*;
use pages::*;
use screen::*;
use style::*;

use asis::github::Gh;
use asis::room::{Role, Room};
use asis::view::{Filter, Kind, RepoView, Row, Stage};
use asis::{Loader, Rooms, Section};
use iced::keyboard::{self, key::Named, Key as KeyCode};
use iced::widget::{
    button, container, mouse_area, opaque, responsive, scrollable, stack, text, text_input, Space,
};
use iced::{Alignment, Border, Color, Element, Font, Length, Subscription, Task, Theme};
use panels::*;
use std::sync::{Arc, Mutex, OnceLock};

/// 窓の出し方(`run` で 1 回 決まる)
static PLATFORM: OnceLock<chrome::Platform> = OnceLock::new();

/// 差し込まれた読み口(repo の中のファイルに書かれたタスク)。`run` で決まり、読み込みのたびに使う
static READER: OnceLock<Option<Arc<dyn asis::extra::Reader>>> = OnceLock::new();
/// 新しい版を確かめた結果。確かめる側(裏の 1 本)が書き、画面が読む
static UPDATE: Mutex<Option<asis::update::Outcome>> = Mutex::new(None);
/// 画面に出す版。配る側が渡した版、無ければこの部品の版
static VERSION: OnceLock<String> = OnceLock::new();

pub fn version() -> &'static str {
    VERSION
        .get()
        .map_or(env!("CARGO_PKG_VERSION"), |v| v.as_str())
}

fn reader() -> Option<Arc<dyn asis::extra::Reader>> {
    READER.get().cloned().flatten()
}
use std::time::{Duration, Instant};

/// 開いたままのとき、何秒ごとに読み直すか
const EVERY: Duration = Duration::from_secs(30);

/// 行を指す鍵(repo・種類・番号・題)。読み直しても同じ行を選んだままにする
type Key = (String, Kind, u64, String);

/// 部屋の中の、いま居るページ。ページ 1 つに役目 1 つ
#[derive(Clone, PartialEq)]
enum Page {
    /// repository のカードの一覧。部屋を開くと、最初に出る
    Repos,
    /// その repository のボード。ボードは repository 1 つにつき 1 つ
    Repo(String),
    /// その repository の設定(ボードの奥)
    RepoSettings(String),
    /// 人のカードの一覧
    Members,
    /// その人の Roles(GitHub の番号で指す)
    Member(i64),
    RoomSettings,
}

/// 左の欄に何を出しているか。部屋の中か、この PC の設定か
#[derive(Clone, Copy, PartialEq)]
enum Area {
    Room,
    Settings(SetTab),
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Board,
    List,
}

/// 設定の画面で打っている途中の字
#[derive(Default)]
struct Form {
    room_name: String,
    invite: String,
    repo: String,
    check: String,
    color: String,
    db_url: String,
    db_key: String,
    root: String,
}

#[derive(Clone, Copy)]
enum Field {
    RoomName,
    Invite,
    Repo,
    Check,
    Color,
    DbUrl,
    DbKey,
    Root,
}

/// 部屋を変える操作。DB に送る
#[derive(Clone)]
enum Op {
    Create(String),
    AddRepo(String, String),
    Everyone(String, String, bool),
    DropRepo(String, String),
    Invite(String, String),
    Role(String, i64, Role, bool),
    Kick(String, i64),
    Leave(String),
    Checks(String, Vec<String>),
    Delete(String),
}

/// 読み込んだもの。部屋の画面と、部屋の一覧
#[derive(Clone)]
struct Snapshot {
    sections: Arc<Vec<Section>>,
    rooms: Rooms,
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
    /// 開いている部屋。部屋が 1 つでもあれば、いつもどれかを開いている
    room: Option<usize>,
    area: Area,
    page: Page,
    look: Look,
    /// 人が開け閉めした段(true = 開いている)。Board / List を切り替えると、忘れる
    fold: std::collections::HashMap<Key, bool>,
    /// issue につながっていない PR の束を、人が開け閉めしたか
    fold_loose: Option<bool>,
    /// Board の左の一覧で、Closed を出しているか
    closed_tab: bool,
    /// Claude Code の手順書が、仕事場に入っているか
    skill: bool,
    /// 手元の clone を探すフォルダ(設定)
    roots: Vec<String>,
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
    /// Sign in のリンク(待っている間だけ)。開いたブラウザが GitHub に入っていないときに、別のブラウザへ貼る用
    sign_url: Arc<Mutex<Option<String>>>,
    /// 読み込みの最中に読み直しを頼まれた。今のが終わったら、もう 1 回 読む
    again: bool,
    /// 読み直しの合図の番号。最後に出した合図だけを受ける(合図が増えていかないように)
    tick: u64,
    /// 自分で描く上の帯(WSL のとき)
    bar: chrome::Bar,
    synced: Option<Instant>,
    error: Option<String>,
    accent: Color,
    /// 説明の文を英語で出すか(大事な単語は、どちらでも英語)
    english: bool,
    /// gh が使えるか。None は確かめている途中
    gh: Option<Gh>,
    /// 入っている部屋(DB から読んだもの)
    rooms: Rooms,
    /// 前に出している設定の画面
    pop: Option<Pop>,
    form: Form,
    /// 作ったばかりの部屋。読み込みに出てきたら、そこを開く
    want_room: Option<String>,
    /// 部屋を消すのを 1 回 押した
    confirm_delete: bool,
    /// ブラウザでのログインを待っている
    signing_in: bool,
    /// 部屋を置く DB の場所(設定)
    db_url: Option<String>,
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
    /// 自分で描いた上の帯からの、窓の操作
    Win(chrome::Win),
    Loaded(Result<Snapshot, String>),
    GhChecked(Gh),
    CheckGh,
    Pop(Option<Pop>),
    Input(Field, String),
    RoomOp(Op),
    RoomDone(Result<String, String>),
    SignIn,
    SignedIn(Result<(), String>),
    SignOut,
    CopySignIn,
    Accent(String),
    Lang(bool),
    SaveDb,
    AskDelete,
    Room(Option<usize>),
    Go(Page),
    Area(Area),
    Look(Look),
    /// 段を開け閉めする
    Fold(Key),
    FoldLoose,
    /// Board の左の一覧を、Open か Closed に
    IssuesTab(bool),
    SkillInstall,
    AddRoot,
    DropRoot(String),
    Select(Key, Option<u64>),
    Close,
    Act(asis::act::Action),
    Acted(Result<String, String>),
    AskFix(u64),
    Comment(String),
    AskMerge(u64),
    WriteOpen,
    WriteTitle(String),
    WriteBody(String),
    Open(String),
    Refresh,
    Again(u64),
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
            let mut guard = loader.lock().map_err(|_| asis::lang::stopped())?;
            // `asis-gui owner/repo` と打てば、その repo だけを既定の決まりで開く
            let repos: Vec<String> = asis::host::args();
            let rooms = if repos.is_empty() {
                // 部屋は毎回 読み直す(招かれた部屋・変えた役割が、開いたまま入るように)
                let local = asis::config::load()?;
                let rooms = asis::read_rooms(&local);
                // 部屋を読めなかったときは、前に読んだ部屋のまま見る(画面から消さない)
                if !(matches!(rooms, Rooms::Failed(_)) && guard.is_some()) {
                    let list = match &rooms {
                        Rooms::Read(r) => r.as_slice(),
                        _ => &[],
                    };
                    // Sign in していない間は、何も出さない(はじめの画面で Sign in を求める)。
                    // 手元の設定に見る repo が残っていても、窓では Room に入っている人にだけ見せる
                    let source = if matches!(rooms, Rooms::SignedOut) {
                        asis::Source {
                            watch: vec![],
                            only: Default::default(),
                            roots: vec![],
                            local: vec![],
                        }
                    } else {
                        asis::source_of(local.clone(), list, &[])
                    };
                    match guard.as_mut() {
                        Some(l) => l.set_source(source),
                        None => *guard = Some(Loader::new(source).with_reader(reader())),
                    }
                }
                rooms
            } else {
                if guard.is_none() {
                    *guard = Some(
                        Loader::new(asis::Source {
                            watch: repos,
                            only: Default::default(),
                            roots: vec![],
                            local: vec![],
                        })
                        .with_reader(reader()),
                    );
                }
                Rooms::NoDb
            };
            let sections = Arc::new(guard.as_mut().expect("入れたばかり").load(force));
            Ok(Snapshot { sections, rooms })
        },
        Msg::Loaded,
    )
}

/// 少し待ってから、読み直しの合図を出す。
/// 待つ間は、画面の仕事を回す場所(数に限りがある)を使わず、別に立てた 1 本で寝る。
fn later(n: u64) -> Task<Msg> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(EVERY);
                let _ = tx.send(());
            });
            let _ = rx.await;
            n
        },
        Msg::Again,
    )
}

fn check_gh() -> Task<Msg> {
    Task::perform(async { asis::github::whoami() }, Msg::GhChecked)
}

/// 部屋の 1 つ 1 つを指す名前(部屋なら番号、手元の定義なら名前)。読み直しても同じ部屋を開いたままにする
fn section_key(s: &Section) -> String {
    s.room.clone().unwrap_or_else(|| s.project.clone())
}

fn color_of(hex: &str) -> Option<Color> {
    asis::config::parse_color(hex).map(|[r, g, b]| Color::from_rgb8(r, g, b))
}

fn boot() -> (App, Task<Msg>) {
    let loader = Arc::new(Mutex::new(None));
    let local = asis::config::load().unwrap_or_default();
    // 1 色は、手元の設定で選べる。書いていなければ既定の色
    let accent = local
        .accent
        .as_deref()
        .and_then(color_of)
        .unwrap_or(Color::from_rgb8(0xf2, 0xb6, 0x3c));
    let english = local.lang.as_deref() == Some("en");
    let app = App {
        loader: loader.clone(),
        sections: Arc::new(Vec::new()),
        room: None,
        area: Area::Room,
        page: Page::Repos,
        look: Look::Board,
        fold: std::collections::HashMap::new(),
        fold_loose: None,
        closed_tab: false,
        skill: asis::skill::installed(),
        roots: local.roots.clone(),
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
        again: false,
        sign_url: Arc::default(),
        tick: 0,
        bar: chrome::Bar::default(),
        synced: None,
        error: None,
        accent,
        english,
        gh: None,
        rooms: Rooms::NoDb,
        pop: None,
        form: Form::default(),
        want_room: None,
        confirm_delete: false,
        // 作るとき用:Sign in を待っている画面を出した状態で開く(`ASIS_SIGNING=1`。debug のみ)
        signing_in: cfg!(debug_assertions) && std::env::var_os("ASIS_SIGNING").is_some(),
        db_url: local.db.map(|d| d.url),
    };
    (app, Task::batch([check_gh(), load(loader, true)]))
}

fn update(app: &mut App, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Loaded(result) => {
            app.loading = false;
            match result {
                Ok(Snapshot { sections, rooms }) => {
                    // 開いていた部屋を、読み直したあとも開いたままにする(並びが変わっても)
                    let was = app.room.and_then(|i| app.sections.get(i)).map(section_key);
                    app.room = was.and_then(|k| sections.iter().position(|s| section_key(s) == k));
                    // 作ったばかりの部屋が出てきたら、そこを開く
                    if let Some(id) = &app.want_room {
                        if let Some(i) = sections.iter().position(|s| s.room.as_ref() == Some(id)) {
                            app.room = Some(i);
                            app.page = Page::Repos;
                            app.want_room = None;
                        }
                    }
                    // いつも、どれか 1 つの部屋を開いている(開いていた部屋が無くなったら、最初の部屋)
                    if app.room.is_none() && !sections.is_empty() {
                        app.room = Some(0);
                        app.page = Page::Repos;
                    }
                    app.sections = sections;
                    app.rooms = rooms;
                    // 作るとき用:そのページを出した状態で開く
                    // (`ASIS_POP=repos|board|rset|members|member|room|look|account|claude|local|db|about|new`)
                    #[cfg(debug_assertions)]
                    if app.synced.is_none() {
                        let first = views(app).first().map(|v| v.repo.clone());
                        let who = panels::open_room(app)
                            .and_then(|r| r.members.first())
                            .map(|m| m.github_id);
                        let set = |t| Area::Settings(t);
                        match std::env::var("ASIS_POP").as_deref() {
                            Ok("board") => {
                                // `ASIS_REPO=名前の一部` でその repository、`ASIS_LOOK=list`、`ASIS_TAB=closed`
                                let want = std::env::var("ASIS_REPO").ok();
                                let repo = views(app)
                                    .iter()
                                    .map(|v| v.repo.clone())
                                    .find(|r| want.as_deref().is_some_and(|w| r.contains(w)))
                                    .or(first);
                                app.page = repo.map_or(Page::Repos, Page::Repo);
                                if std::env::var("ASIS_LOOK").as_deref() == Ok("list") {
                                    app.look = Look::List;
                                }
                                app.closed_tab =
                                    std::env::var("ASIS_TAB").as_deref() == Ok("closed");
                            }
                            Ok("rset") => app.page = first.map_or(Page::Repos, Page::RepoSettings),
                            Ok("members") => app.page = Page::Members,
                            Ok("member") => app.page = who.map_or(Page::Members, Page::Member),
                            Ok("room") => app.page = Page::RoomSettings,
                            Ok("look") => app.area = set(SetTab::Look),
                            Ok("account") => app.area = set(SetTab::Account),
                            Ok("claude") => app.area = set(SetTab::Claude),
                            Ok("local") => app.area = set(SetTab::Local),
                            Ok("db") => app.area = set(SetTab::Db),
                            Ok("about") => app.area = set(SetTab::About),
                            Ok("new") => app.pop = Some(Pop::NewRoom),
                            _ => {}
                        }
                    }
                    // 作るとき用:最初の 1 件の中身を出した状態で開く(画面を撮って確かめるため)
                    #[cfg(debug_assertions)]
                    if app.synced.is_none() && std::env::var_os("ASIS_OPEN_FIRST").is_some() {
                        if let Some(v) = views(app).first().map(|v| v.repo.clone()) {
                            app.page = Page::Repo(v);
                        }
                        step(app, 1);
                        app.open = app.selected.is_some();
                    }
                    app.synced = Some(Instant::now());
                    app.error = None;
                }
                Err(e) => app.error = Some(e),
            }
            app.tick += 1;
            // 読んでいる間に頼まれた読み直し(Sign out・部屋の変更など)を、落とさない
            if std::mem::take(&mut app.again) {
                app.loading = true;
                return load(app.loader.clone(), true);
            }
            return later(app.tick);
        }
        Msg::GhChecked(gh) => app.gh = Some(gh),
        Msg::CheckGh => {
            app.gh = None;
            return Task::batch([check_gh(), update(app, Msg::Refresh)]);
        }
        Msg::Pop(pop) => {
            app.pop = pop;
            app.notice = None;
            app.confirm_delete = false;
            app.open = false;
            app.write = None;
        }
        Msg::Input(field, value) => {
            let f = &mut app.form;
            *match field {
                Field::RoomName => &mut f.room_name,
                Field::Invite => &mut f.invite,
                Field::Repo => &mut f.repo,
                Field::Check => &mut f.check,
                Field::Color => &mut f.color,
                Field::DbUrl => &mut f.db_url,
                Field::DbKey => &mut f.db_key,
                Field::Root => &mut f.root,
            } = value;
        }
        Msg::RoomOp(op) => {
            if app.busy {
                return Task::none();
            }
            if let Some(e) = panels::refuse(app, &op) {
                app.notice = Some((e, false));
                return Task::none();
            }
            app.busy = true;
            app.notice = None;
            return Task::perform(async move { run_op(op) }, Msg::RoomDone);
        }
        Msg::RoomDone(result) => {
            app.busy = false;
            app.confirm_delete = false;
            match result {
                Ok(done) => {
                    if let Some(id) = done.strip_prefix("created:") {
                        app.want_room = Some(id.to_string());
                        app.pop = None;
                        app.area = Area::Room;
                        app.page = Page::Repos;
                        app.notice = Some((
                            app.t(
                                "Room を作りました。repository を足してください",
                                "Room created. Add repositories next",
                            )
                            .into(),
                            true,
                        ));
                    } else if done == "gone" {
                        app.pop = None;
                        app.room = None;
                        app.page = Page::Repos;
                        app.notice = None;
                    } else {
                        app.notice = (!done.is_empty()).then_some((done, true));
                    }
                    app.form = Form::default();
                    if !app.loading {
                        app.loading = true;
                        return load(app.loader.clone(), false);
                    }
                    // 読んでいる最中なら、終わってからもう 1 回 読む
                    app.again = true;
                }
                Err(e) => app.notice = Some((e, false)),
            }
        }
        Msg::SignIn => {
            let Some(db) = asis::config::load().ok().and_then(|l| l.db) else {
                app.area = Area::Settings(SetTab::Db);
                app.pop = None;
                return Task::none();
            };
            if app.signing_in {
                return Task::none();
            }
            app.signing_in = true;
            app.notice = None;
            let slot = app.sign_url.clone();
            if let Ok(mut url) = slot.lock() {
                *url = None;
            }
            return Task::perform(
                async move {
                    asis::room::sign_in(&db, &|url| {
                        if let Ok(mut slot) = slot.lock() {
                            *slot = Some(url.to_string());
                        }
                    })
                },
                Msg::SignedIn,
            );
        }
        Msg::CopySignIn => {
            let url = app.sign_url.lock().ok().and_then(|url| url.clone());
            if let Some(url) = url {
                app.notice = Some((
                    app.t(
                        "リンクをコピーしました。GitHub に入っているブラウザに貼ってください",
                        "Link copied. Paste it into the browser where you are signed in to GitHub",
                    )
                    .into(),
                    true,
                ));
                return iced::clipboard::write(url);
            }
        }
        Msg::SignedIn(result) => {
            app.signing_in = false;
            match result {
                Ok(()) => {
                    app.notice = Some((app.t("Sign in しました", "Signed in").into(), true));
                    return update(app, Msg::Refresh);
                }
                Err(e) => app.notice = Some((e, false)),
            }
        }
        Msg::SignOut => match asis::room::sign_out() {
            Ok(_) => {
                app.room = None;
                // 読み直しを待たずに、その場で Sign out した画面(はじめの画面)にする
                app.rooms = Rooms::SignedOut;
                app.sections = Arc::new(Vec::new());
                app.page = Page::Repos;
                app.selected = None;
                app.open = false;
                app.pop = None;
                app.notice = Some((app.t("Sign out しました", "Signed out").into(), true));
                return update(app, Msg::Refresh);
            }
            Err(e) => app.notice = Some((e, false)),
        },
        Msg::Accent(hex) => {
            let hex = hex.trim().to_lowercase();
            match color_of(&hex) {
                None => {
                    app.notice = Some((
                        app.t("#rrggbb の形で入れてください", "Use the form #rrggbb")
                            .into(),
                        false,
                    ))
                }
                Some(color) => match asis::config::save(|t| {
                    Ok(asis::config::with_value(t, "accent", Some(&hex)))
                }) {
                    Ok(_) => {
                        app.accent = color;
                        app.form.color.clear();
                        app.notice = None;
                    }
                    Err(e) => app.notice = Some((e, false)),
                },
            }
        }
        Msg::Lang(english) => {
            let lang = if english { "en" } else { "ja" };
            match asis::config::save(|t| Ok(asis::config::with_value(t, "lang", Some(lang)))) {
                Ok(_) => {
                    app.english = english;
                    app.notice = None;
                    // 注記を新しい言葉で作り直す(読んだ事実は手元にあるので、GitHub には聞き直さない)
                    return load(app.loader.clone(), false);
                }
                Err(e) => app.notice = Some((e, false)),
            }
        }
        Msg::SaveDb => {
            let db = asis::config::Db {
                url: app.form.db_url.trim().trim_end_matches('/').to_string(),
                key: app.form.db_key.trim().to_string(),
            };
            match asis::config::save(|t| asis::config::with_db(t, &db)) {
                Ok(_) => {
                    // ログインの記録は DB ごと。別の DB に替えたら、前のものは使えない
                    if app.db_url.as_deref() != Some(db.url.as_str()) {
                        let _ = asis::room::sign_out();
                        app.room = None;
                    }
                    app.db_url = Some(db.url);
                    app.form.db_url.clear();
                    app.form.db_key.clear();
                    app.notice = Some((
                        app.t("つなぎ先を変えました", "Database changed").into(),
                        true,
                    ));
                    return update(app, Msg::Refresh);
                }
                Err(e) => app.notice = Some((e, false)),
            }
        }
        Msg::AskDelete => app.confirm_delete = true,
        // 古い合図は捨てる。読み込み中の合図も捨てる(読み終われば、次の合図が出る)
        Msg::Again(n) if n != app.tick => {}
        Msg::Again(_) if app.loading => {}
        Msg::Refresh if app.loading => app.again = true,
        Msg::Again(_) => {
            app.loading = true;
            return load(app.loader.clone(), false);
        }
        Msg::Refresh => {
            app.loading = true;
            return load(app.loader.clone(), true);
        }
        Msg::Room(room) => {
            app.room = room;
            app.area = Area::Room;
            app.page = Page::Repos;
            app.selected = None;
            app.open = false;
            app.notice = None;
            app.confirm_delete = false;
        }
        Msg::Go(page) => {
            app.area = Area::Room;
            app.page = page;
            app.selected = None;
            app.open = false;
            app.notice = None;
            app.confirm_delete = false;
        }
        Msg::Area(area) => {
            app.area = area;
            app.open = false;
            app.write = None;
            app.notice = None;
            app.confirm_delete = false;
            // 開いたときに、入っているかを確かめ直す(外で入れた・消したかもしれない)
            if area == Area::Settings(SetTab::Claude) {
                app.skill = asis::skill::installed();
            }
        }
        Msg::Look(look) => set_look(app, look),
        Msg::Fold(key) => {
            let now = fold_state(app.fold.get(&key).copied(), app.look);
            app.fold.insert(key, !now);
        }
        Msg::FoldLoose => app.fold_loose = Some(!fold_state(app.fold_loose, app.look)),
        Msg::IssuesTab(closed) => app.closed_tab = closed,
        Msg::SkillInstall => match asis::skill::install() {
            Ok(dir) => {
                app.skill = true;
                app.notice = Some((
                    if app.english {
                        format!("Installed to {}", dir.display())
                    } else {
                        format!("{} に入れました", dir.display())
                    },
                    true,
                ));
            }
            Err(e) => app.notice = Some((e, false)),
        },
        Msg::AddRoot => {
            let root = app.form.root.trim().to_string();
            if root.is_empty() || app.roots.contains(&root) {
                return Task::none();
            }
            let mut roots = app.roots.clone();
            roots.push(root);
            return save_roots(app, roots);
        }
        Msg::DropRoot(root) => {
            let roots = app.roots.iter().filter(|r| **r != root).cloned().collect();
            return save_roots(app, roots);
        }
        Msg::Select(key, pr) => {
            app.selected = Some(key);
            app.pr = pr;
            app.open = true;
            forget(app);
        }
        Msg::Win(op) => return app.bar.perform(op),
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
                    // 読んでいる最中なら、終わってからもう 1 回 読む
                    app.again = true;
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
            // New issue は、ボードを出している repository にだけ出す
            let Some(repo) = board_repo(app) else {
                return Task::none();
            };
            app.notice = None;
            app.write = Some(Write {
                repo: Some(repo),
                ..Write::default()
            });
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
            // 設定の画面を出している間は、Esc で閉じるだけ(打った字を奪わない)
            if app.pop.is_some() {
                if key.as_ref() == KeyCode::Named(Named::Escape) {
                    return update(app, Msg::Pop(None));
                }
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
            // ここから下は、ボードの上だけ(ほかのページでは、打った字を奪わない)
            let Some(here) = board_repo(app) else {
                return Task::none();
            };
            match key.as_ref() {
                // 次の repository のボードへ
                KeyCode::Named(Named::Tab) => {
                    let all: Vec<String> = views(app).iter().map(|v| v.repo.clone()).collect();
                    if let Some(at) = all.iter().position(|r| *r == here) {
                        app.page = Page::Repo(all[(at + 1) % all.len()].clone());
                        app.selected = None;
                    }
                }
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
                    let next = match app.look {
                        Look::Board => Look::List,
                        Look::List => Look::Board,
                    };
                    set_look(app, next);
                }
                // 選んでいる段を開く・たたむ
                KeyCode::Named(Named::ArrowRight) => {
                    if let Some(key) = app.selected.clone() {
                        app.fold.insert(key, true);
                    }
                }
                KeyCode::Named(Named::ArrowLeft) => {
                    if let Some(key) = app.selected.clone() {
                        app.fold.insert(key, false);
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

/// 手元の clone を探すフォルダを、設定に書く
fn save_roots(app: &mut App, roots: Vec<String>) -> Task<Msg> {
    match asis::config::save(|t| asis::config::with_roots(t, &roots)) {
        Ok(_) => {
            app.roots = roots;
            app.form.root.clear();
            app.notice = None;
            // 探す場所が変わったので、手元の作業を読み直す
            update(app, Msg::Refresh)
        }
        Err(e) => {
            app.notice = Some((e, false));
            Task::none()
        }
    }
}

/// 部屋を変える操作を、DB に送る。作ったときは `created:<番号>`、自分が居なくなる操作は `gone` を返す
fn run_op(op: Op) -> Result<String, String> {
    use asis::{lang::t, say};
    let local = asis::config::load()?;
    let db = local.db.ok_or(t(
        "部屋を置く DB が設定にありません",
        "There is no database for Rooms in the settings",
    ))?;
    let c =
        asis::room::Client::open(&db)?.ok_or(t("Sign in していません", "You are not signed in"))?;
    Ok(match op {
        Op::Create(name) => format!("created:{}", c.create(&name)?),
        Op::AddRepo(room, repo) => {
            c.add_repo(&room, &repo, false)?;
            let repo = repo.trim();
            say!("{repo} を足しました", "Added {repo}")
        }
        Op::Everyone(room, repo, on) => {
            c.add_repo(&room, &repo, on)?;
            String::new()
        }
        Op::DropRepo(room, repo) => {
            c.drop_repo(&room, &repo)?;
            say!("{repo} を外しました", "Removed {repo}")
        }
        Op::Invite(room, login) => {
            let login = c.invite(&room, &login, &[])?;
            say!("{login} を Invite しました", "Invited {login}")
        }
        Op::Role(room, id, role, on) => {
            c.set_role(&room, id, role, on)?;
            String::new()
        }
        Op::Kick(room, id) => {
            c.kick(&room, id)?;
            String::new()
        }
        Op::Leave(room) => {
            c.leave(&room)?;
            "gone".into()
        }
        Op::Checks(room, list) => {
            c.set_ignore(&room, &list)?;
            String::new()
        }
        Op::Delete(room) => {
            c.delete(&room)?;
            "gone".into()
        }
    })
}

/// 書きかけの操作を忘れる(別の 1 件に移ったとき・閉じたとき)
/// Board と List を切り替える。1 件ずつの開け閉めは忘れる(Board = 開いた形、List = たたんだ形から始める)
fn set_look(app: &mut App, look: Look) {
    app.look = look;
    app.fold.clear();
    app.fold_loose = None;
}

fn forget(app: &mut App) {
    app.fixing = None;
    app.confirm = None;
    app.comment.clear();
    app.notice = None;
}

/// GitHub のページをブラウザで開く
fn open(url: &str) {
    use std::process::Stdio;
    let tries: [(&str, &[&str]); 5] = [
        ("wslview", &[]),
        ("xdg-open", &[]),
        ("open", &[]),
        ("rundll32.exe", &["url.dll,FileProtocolHandler"]),
        ("explorer.exe", &[]),
    ];
    for (program, args) in tries {
        if asis::host::local(program)
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

/// 受けるのは「キーが押された」だけ。
/// WSL の画面は、ホイールを回すたびに Shift などの押され具合を知らせてくる。それも受けると、
/// そのたびに画面を全部 組み立て直すことになり、スクロールが重くなる(1 回のホイールで 2 回)。
fn subscription(_: &App) -> Subscription<Msg> {
    iced::event::listen_with(|event, status, _| match event {
        iced::Event::Keyboard(e @ keyboard::Event::KeyPressed { .. })
            if status == iced::event::Status::Ignored =>
        {
            Some(Msg::Key(e))
        }
        _ => None,
    })
}

/// 新しい版を確かめる間隔
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 3600);

/// 動いている exe のフォルダで、新しい版を確かめ続ける。新しければ裏で入れ替える(次に開くと、その版で動く)。
/// 最初の 1 回は、読み込みの邪魔をしないよう少し待つ
fn watch_updates(app: asis::update::App) {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    // 前の入れ替えで退かした古いファイルを消す(動いている間は消せなかったもの)
    asis::update::clean(&dir);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        loop {
            if let Some(db) = asis::config::load().ok().and_then(|local| local.db) {
                let outcome = asis::update::run_once(&app, &dir, &db);
                if let Ok(mut slot) = UPDATE.lock() {
                    *slot = Some(outcome);
                }
            }
            std::thread::sleep(UPDATE_EVERY);
        }
    });
}

/// いちばん最近 確かめた結果(画面に 1 行で出す)
pub fn update_outcome() -> Option<asis::update::Outcome> {
    UPDATE.lock().ok().and_then(|slot| slot.clone())
}

/// 窓を開く。`reader` を渡すと、repo の中のファイルに書かれたタスクも出す(`asis::extra` を参照)
pub fn run(reader: Option<Arc<dyn asis::extra::Reader>>) -> iced::Result {
    run_with(reader, None)
}

/// 窓を開く。`app` を渡すと、その名前で配られている新しい版に、自分で入れ替える(`asis::update` を参照)。
/// 渡さなければ、入れ替えない(自分でビルドして使う人のため)
pub fn run_with(
    reader: Option<Arc<dyn asis::extra::Reader>>,
    app: Option<asis::update::App>,
) -> iced::Result {
    let _ = READER.set(reader);
    let version = app
        .as_ref()
        .map_or(env!("CARGO_PKG_VERSION"), |a| a.version.as_str())
        .to_string();
    let name = app
        .as_ref()
        .map_or("asis-gui", |a| a.name.as_str())
        .to_string();
    asis::log::record_panics(&name, &version);
    let _ = VERSION.set(version);
    // 日本語が入っている、その OS の字
    let font = if cfg!(windows) {
        "Yu Gothic UI"
    } else if cfg!(target_os = "macos") {
        "Hiragino Sans"
    } else {
        "Noto Sans JP"
    };
    // WSL から開かれて、Windows 版が入っていれば、窓はそちらに任せる
    if let Some(status) = chrome::hand_off_to_windows() {
        std::process::exit(status.code().unwrap_or(0));
    }
    if let Some(app) = app {
        watch_updates(app);
    }
    let platform = *PLATFORM.get_or_init(chrome::Platform::detect);
    // 窓の最初の大きさ。`ASIS_WINDOW=1366x768` で変えられる(いろいろな画面の形で確かめる用)
    let size = std::env::var("ASIS_WINDOW")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
        })
        .unwrap_or((1320.0, 820.0));
    iced::application(boot, update, view)
        .title(|_: &App| std::env::var("ASIS_TITLE").unwrap_or_else(|_| "AsIs".into()))
        .subscription(subscription)
        .default_font(Font::with_name(font))
        .scale_factor(move |_| platform.scale)
        .window(iced::window::Settings {
            size: iced::Size::new(size.0, size.1),
            // 窓の左上とタスクバーに出る印
            icon: iced::window::icon::from_rgba(
                include_bytes!("../assets/icon-64.rgba").to_vec(),
                64,
                64,
            )
            .ok(),
            decorations: !platform.own_bar,
            ..iced::window::Settings::default()
        })
        .run()
}
