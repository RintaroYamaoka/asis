//! asis の窓の画面。読み込みと並べ方は asis の部品を使い、ここは描くだけ。
//!
//! 状態(App)→ 操作(Msg)→ 描く(view)の 3 つに分ける。TUI も同じ形にそろえる。
#![cfg_attr(windows, windows_subsystem = "windows")]

use asis::view::{Filter, Kind, RepoView, Row, Stage};
use asis::{Loader, Section};
use iced::widget::{button, column, container, mouse_area, row, scrollable, text, Space};
use iced::{Alignment, Color, Element, Font, Length, Task, Theme};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 開いたままのとき、何秒ごとに読み直すか
const EVERY: Duration = Duration::from_secs(30);

/// 行を指す鍵(repo・種類・番号・題)。読み直しても同じ行を選んだままにする
type Key = (String, Kind, u64, String);

struct App {
    loader: Arc<Mutex<Option<Loader>>>,
    sections: Arc<Vec<Section>>,
    tab: usize,
    filter: Filter,
    selected: Option<Key>,
    collapsed: HashSet<Stage>,
    loading: bool,
    synced: Option<Instant>,
    error: Option<String>,
}

#[derive(Clone)]
enum Msg {
    Loaded(Result<Arc<Vec<Section>>, String>),
    Tab(usize),
    Filter(Filter),
    Select(Key),
    Open(String),
    Toggle(Stage),
    Refresh,
    Again,
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
                *guard = Some(Loader::new(asis::default_source(&[])?));
            }
            Ok(Arc::new(guard.as_mut().expect("入れたばかり").load(force)))
        },
        Msg::Loaded,
    )
}

fn boot() -> (App, Task<Msg>) {
    let loader = Arc::new(Mutex::new(None));
    let app = App {
        loader: loader.clone(),
        sections: Arc::new(Vec::new()),
        tab: 0,
        filter: Filter::All,
        selected: None,
        collapsed: HashSet::new(),
        loading: true,
        synced: None,
        error: None,
    };
    (app, load(loader, true))
}

fn update(app: &mut App, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Loaded(result) => {
            app.loading = false;
            match result {
                Ok(sections) => {
                    app.sections = sections;
                    app.synced = Some(Instant::now());
                    app.error = None;
                    app.tab = app.tab.min(app.sections.len().saturating_sub(1));
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
        Msg::Tab(i) => {
            app.tab = i;
            app.selected = None;
        }
        Msg::Filter(f) => app.filter = f,
        Msg::Select(key) => app.selected = Some(key),
        Msg::Open(url) => open(&url),
        Msg::Toggle(stage) => {
            if !app.collapsed.remove(&stage) {
                app.collapsed.insert(stage);
            }
        }
    }
    Task::none()
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

// ---- 色。見た目は、動く骨組みを見てから決める。ここは仮の値。 ----
const BG: Color = Color::from_rgb(0.086, 0.094, 0.109);
const PANEL: Color = Color::from_rgb(0.118, 0.129, 0.149);
const LINE: Color = Color::from_rgb(0.2, 0.215, 0.243);
const TEXT: Color = Color::from_rgb(0.88, 0.89, 0.91);
const DIM: Color = Color::from_rgb(0.54, 0.57, 0.62);
const ACCENT: Color = Color::from_rgb(0.42, 0.66, 1.0);

fn stage_color(stage: Stage) -> Color {
    match stage {
        Stage::NeedsFix => Color::from_rgb(0.96, 0.45, 0.42),
        Stage::InReview => Color::from_rgb(0.98, 0.76, 0.33),
        Stage::ReadyToMerge => Color::from_rgb(0.45, 0.82, 0.55),
        Stage::Testing => Color::from_rgb(0.55, 0.7, 0.95),
        Stage::AwaitingShip => Color::from_rgb(0.72, 0.6, 0.95),
        Stage::ShippedOpen => Color::from_rgb(0.9, 0.6, 0.8),
        Stage::Doing => Color::from_rgb(0.6, 0.78, 0.85),
        Stage::Todo => DIM,
    }
}

fn fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        ..container::Style::default()
    }
}

/// 押せる文字。選ばれているものだけ色を付ける
fn tab<'a>(label: String, on: bool, msg: Msg) -> Element<'a, Msg> {
    button(text(label).size(14).color(if on { TEXT } else { DIM }))
        .padding([6, 12])
        .style(move |_, status| button::Style {
            background: Some(
                if on {
                    LINE
                } else if matches!(status, button::Status::Hovered) {
                    PANEL
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: TEXT,
            border: iced::Border {
                radius: 6.0.into(),
                ..iced::Border::default()
            },
            ..button::Style::default()
        })
        .on_press(msg)
        .into()
}

fn cell<'a>(content: impl Into<Element<'a, Msg>>, width: f32) -> Element<'a, Msg> {
    container(content)
        .width(Length::Fixed(width))
        .clip(true)
        .into()
}

const W_STAGE: f32 = 96.0;
const W_REPO: f32 = 132.0;
const W_NUM: f32 = 84.0;
const W_TURN: f32 = 132.0;
const W_AGE: f32 = 48.0;

fn short(repo: &str) -> &str {
    repo.rsplit('/').next().unwrap_or(repo)
}

fn turn_of(row: &Row, viewer: &str) -> (String, bool) {
    if row.is_mine(viewer) {
        ("あなた".to_string(), true)
    } else if row.turn.is_empty() {
        ("—".to_string(), false)
    } else {
        (row.turn.join("・"), false)
    }
}

fn key_of(view: &RepoView, row: &Row) -> Key {
    (view.repo.clone(), row.kind, row.number, row.title.clone())
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Task => "issue",
        Kind::Pr => "PR",
        Kind::Local => "手元",
    }
}

fn view(app: &App) -> Element<'_, Msg> {
    let section = app.sections.get(app.tab);
    let views: Vec<&RepoView> = section
        .map(|s| s.repos.iter().filter_map(|r| r.as_ref().ok()).collect())
        .unwrap_or_default();
    let all: Vec<(&RepoView, &Row)> = views
        .iter()
        .flat_map(|v| {
            v.groups
                .iter()
                .flat_map(move |g| g.rows.iter().map(move |r| (*v, r)))
        })
        .collect();
    let count = |f: Filter| all.iter().filter(|(v, r)| r.shows(f, &v.viewer)).count();

    // 上の段:部屋(プロジェクト)のタブと、読み直し
    let mut top = row![text("asis").size(18).color(TEXT), Space::new().width(16)]
        .align_y(Alignment::Center)
        .spacing(4);
    for (i, s) in app.sections.iter().enumerate() {
        top = top.push(tab(s.project.clone(), i == app.tab, Msg::Tab(i)));
    }
    let status = match (app.loading, app.synced) {
        (true, _) => "読み込み中…".to_string(),
        (false, Some(t)) => format!("{} 秒前に同期", t.elapsed().as_secs()),
        (false, None) => String::new(),
    };
    top = top
        .push(Space::new().width(Length::Fill))
        .push(text(status).size(13).color(DIM))
        .push(tab("読み直す".to_string(), false, Msg::Refresh));

    // 次の段:絞り込み
    let mut filters = row![].spacing(4);
    for f in [Filter::All, Filter::Mine, Filter::Others] {
        filters = filters.push(tab(
            format!("{}  {}", f.label(), count(f)),
            f == app.filter,
            Msg::Filter(f),
        ));
    }

    // 表の見出し
    let head = |label: &'static str| text(label).size(12).color(DIM);
    let header = row![
        cell(head("状態"), W_STAGE),
        cell(head("repo"), W_REPO),
        cell(head("番号"), W_NUM),
        container(head("題")).width(Length::Fill),
        cell(head("誰の番"), W_TURN),
        cell(head("経過"), W_AGE),
    ]
    .spacing(12)
    .padding([6, 16]);

    // 段階ごとの束
    let mut list = column![].spacing(2);
    for stage in Stage::ALL {
        let rows: Vec<&(&RepoView, &Row)> = all
            .iter()
            .filter(|(v, r)| r.stage == stage && r.shows(app.filter, &v.viewer))
            .collect();
        if rows.is_empty() {
            continue;
        }
        let closed = app.collapsed.contains(&stage);
        list = list.push(
            button(
                row![
                    text(if closed { "▸" } else { "▾" }).size(13).color(DIM),
                    text(stage.label()).size(14).color(stage_color(stage)),
                    text(rows.len().to_string()).size(13).color(DIM),
                ]
                .spacing(8),
            )
            .padding([8, 16])
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: None,
                text_color: TEXT,
                ..button::Style::default()
            })
            .on_press(Msg::Toggle(stage)),
        );
        if closed {
            continue;
        }
        for (v, r) in rows {
            let key = key_of(v, r);
            let on = app.selected.as_ref() == Some(&key);
            let (turn, mine) = turn_of(r, &v.viewer);
            let line = row![
                cell(
                    text(stage.label()).size(13).color(stage_color(stage)),
                    W_STAGE
                ),
                cell(text(short(&v.repo)).size(13).color(DIM), W_REPO),
                cell(
                    text(if r.number > 0 {
                        format!("{} #{}", kind_word(r.kind), r.number)
                    } else {
                        kind_word(r.kind).to_string()
                    })
                    .size(13)
                    .color(DIM),
                    W_NUM
                ),
                container(
                    text(r.title.clone())
                        .size(14)
                        .color(TEXT)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::None)
                )
                .width(Length::Fill)
                .height(20)
                .clip(true),
                cell(
                    text(turn).size(13).color(if mine { ACCENT } else { DIM }),
                    W_TURN
                ),
                cell(text(r.age.clone()).size(13).color(DIM), W_AGE),
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            let mut area = mouse_area(
                container(line)
                    .padding([7, 16])
                    .width(Length::Fill)
                    .style(fill(if on { LINE } else { Color::TRANSPARENT })),
            )
            .on_press(Msg::Select(key));
            if !r.url.is_empty() {
                area = area.on_double_click(Msg::Open(r.url.clone()));
            }
            list = list.push(area);
        }
    }
    if all.is_empty() && !app.loading {
        list = list.push(container(text("出すものがありません").color(DIM)).padding(16));
    }

    // 注記と、読めなかった理由
    let mut notes = column![].spacing(2);
    if let Some(e) = &app.error {
        notes = notes.push(text(e.clone()).size(13).color(stage_color(Stage::NeedsFix)));
    }
    for e in section
        .iter()
        .flat_map(|s| s.repos.iter().filter_map(|r| r.as_ref().err()))
    {
        notes = notes.push(text(e.clone()).size(13).color(stage_color(Stage::NeedsFix)));
    }
    for v in &views {
        for n in &v.notes {
            notes = notes.push(text(format!("{}: {n}", short(&v.repo))).size(12).color(DIM));
        }
    }

    // 下の段:選んだ行の中身
    let detail: Element<'_, Msg> = match all
        .iter()
        .find(|(v, r)| app.selected.as_ref() == Some(&key_of(v, r)))
    {
        Some((v, r)) => {
            let (turn, _) = turn_of(r, &v.viewer);
            let mut body = column![
                text(r.title.clone()).size(16).color(TEXT),
                text(format!("{}  ·  {}  ·  次は {turn}", short(&v.repo), r.why))
                    .size(13)
                    .color(DIM),
            ]
            .spacing(6);
            for p in &r.prs {
                body = body.push(
                    text(format!(
                        "PR #{}  {}  —  {}  ({})",
                        p.number, p.title, p.why, p.author
                    ))
                    .size(13)
                    .color(TEXT),
                );
            }
            for l in &r.local {
                body = body.push(
                    text(format!("手元  {}  {}", l.branch, l.panes.join(" / ")))
                        .size(13)
                        .color(DIM),
                );
            }
            if !r.url.is_empty() {
                body = body.push(tab(
                    "GitHub で開く".to_string(),
                    true,
                    Msg::Open(r.url.clone()),
                ));
            }
            container(body)
                .padding(16)
                .width(Length::Fill)
                .style(fill(PANEL))
                .into()
        }
        None => Space::new().into(),
    };

    container(
        column![
            container(top)
                .padding([10, 16])
                .width(Length::Fill)
                .style(fill(PANEL)),
            container(filters).padding([8, 16]),
            container(notes).padding([0, 16]),
            header,
            container(Space::new().width(Length::Fill).height(1)).style(fill(LINE)),
            scrollable(list).height(Length::Fill).width(Length::Fill),
            detail,
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .style(fill(BG))
    .into()
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
    iced::application(boot, update, view)
        .title("asis")
        .default_font(Font::with_name(font))
        .window_size((1100.0, 720.0))
        .run()
}
