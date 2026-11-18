//! ターミナルの画面。描くことと、キーを受けることだけ。判断は持たない。
use crate::view::{Filter, Row};
use crate::{clip, only_text, turn_text, Loader, Section, Source};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(30);
const MINE: Color = Color::Indexed(214);
const DIM: Color = Color::Indexed(245);
const FAINT: Color = Color::Indexed(240);

pub struct App {
    pub sections: Vec<Section>,
    pub filter: Filter,
    /// 見えている行のうち、何番目を選んでいるか
    pub cursor: usize,
    pub loading: bool,
    pub synced: Option<Instant>,
}

/// 見えている行。(その行, 見ている人)
fn visible(app: &App) -> Vec<(&Row, &str)> {
    let views = app.sections.iter().flat_map(|s| &s.repos).flatten();
    views
        .flat_map(|v| {
            v.groups
                .iter()
                .flat_map(|g| &g.rows)
                .filter(|r| r.shows(app.filter, &v.viewer))
                .map(|r| (r, v.viewer.as_str()))
        })
        .collect()
}

fn count(app: &App, filter: Filter) -> usize {
    let views = app.sections.iter().flat_map(|s| &s.repos).flatten();
    views
        .map(|v| {
            v.groups
                .iter()
                .flat_map(|g| &g.rows)
                .filter(|r| r.shows(filter, &v.viewer))
                .count()
        })
        .sum()
}

pub fn run(source: Source, filter: Filter) -> std::io::Result<()> {
    // 読み込みは別の流れで回す。画面は止めない。
    let (tx, rx) = mpsc::channel::<Vec<Section>>();
    let (kick, kicked) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut loader = Loader::new(source);
        // 自動の読み直しは変わった repo だけ。キーで頼まれたら全部。
        let mut force = false;
        loop {
            if tx.send(loader.load(force)).is_err() {
                break;
            }
            force = match kicked.recv_timeout(REFRESH) {
                Ok(()) => true,
                Err(mpsc::RecvTimeoutError::Timeout) => false,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
        }
    });

    let mut app = App {
        sections: vec![],
        filter,
        cursor: 0,
        loading: true,
        synced: None,
    };
    let mut terminal = ratatui::init();
    let result = (|| loop {
        if let Ok(sections) = rx.try_recv() {
            app.sections = sections;
            app.loading = false;
            app.synced = Some(Instant::now());
        }
        let rows = visible(&app).len();
        app.cursor = app.cursor.min(rows.saturating_sub(1));
        terminal.draw(|f| draw(f, &app))?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
            KeyCode::Down | KeyCode::Char('j') => {
                app.cursor = (app.cursor + 1).min(rows.saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => app.cursor = app.cursor.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => app.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => app.cursor = rows.saturating_sub(1),
            KeyCode::Tab => {
                app.filter = app.filter.next();
                app.cursor = 0;
            }
            KeyCode::Char('r') => {
                app.loading = true;
                let _ = kick.send(());
            }
            KeyCode::Enter | KeyCode::Char('o') => {
                if let Some((row, _)) = visible(&app)
                    .get(app.cursor)
                    .filter(|(row, _)| !row.url.is_empty())
                {
                    open(&row.url);
                }
            }
            _ => {}
        }
    })();
    ratatui::restore();
    result
}

/// GitHub のページをブラウザで開く。その環境にある開き方を順に試す。
fn open(url: &str) {
    use std::process::Stdio;
    for opener in ["wslview", "xdg-open", "open"] {
        if crate::command(opener)
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    let _ = crate::command("cmd")
        .args(["/C", "start", "", url])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub fn draw(f: &mut Frame, app: &App) {
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());
    draw_head(f, head, app);
    let side = body.width >= 100;
    let [list, detail] = if side {
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(body)
    } else {
        Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(9.min(body.height / 2)),
        ])
        .areas(body)
    };
    draw_list(f, list, app);
    draw_detail(f, detail, app, side);
    let keys = "↑↓ 移動   Tab 絞り込み   Enter GitHub で開く   r 読み直す   q 終わる      ▶ AI が動いている  ◆ ペインあり  ◇ worktree あり";
    f.render_widget(
        Paragraph::new(Span::styled(keys, Style::new().fg(DIM))),
        foot,
    );
}

fn draw_head(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![
        Span::styled("asis", Style::new().fg(MINE).add_modifier(Modifier::BOLD)),
        Span::raw("   "),
    ];
    for filter in [Filter::All, Filter::Mine, Filter::Others] {
        let n = count(app, filter);
        let style = if filter == app.filter {
            Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::new().fg(DIM)
        };
        spans.push(Span::styled(format!(" {} {n} ", filter.label()), style));
        spans.push(Span::raw(" "));
    }
    let status = match (app.loading, app.synced) {
        (true, _) => "読み込み中…".to_string(),
        (false, Some(t)) => format!("{} 秒前に同期", t.elapsed().as_secs()),
        (false, None) => String::new(),
    };
    spans.push(Span::styled(format!("  {status}"), Style::new().fg(DIM)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_list(f: &mut Frame, area: Rect, app: &App) {
    let width = area.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let (mut index, mut selected_line) = (0, 0);
    for section in &app.sections {
        for repo in &section.repos {
            let v = match repo {
                Ok(v) => v,
                Err(e) => {
                    lines.push(Line::styled(
                        format!("読めませんでした: {e}"),
                        Style::new().fg(Color::Red),
                    ));
                    lines.push(Line::raw(""));
                    continue;
                }
            };
            let title = if section.project.is_empty() {
                v.repo.clone()
            } else {
                format!("{}  {}", section.project, v.repo)
            };
            lines.push(Line::from(vec![
                Span::styled(title, Style::new().add_modifier(Modifier::BOLD)),
                Span::styled(only_text(&v.only), Style::new().fg(DIM)),
            ]));
            for note in &v.notes {
                lines.push(Line::styled(format!("  {note}"), Style::new().fg(FAINT)));
            }
            let mut any = false;
            for g in &v.groups {
                let rows: Vec<&Row> = g
                    .rows
                    .iter()
                    .filter(|r| r.shows(app.filter, &v.viewer))
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                any = true;
                lines.push(Line::styled(
                    format!("  {}  {}", g.name, rows.len()),
                    Style::new().fg(DIM),
                ));
                for r in rows {
                    let mine = r.is_mine(&v.viewer);
                    let turn = turn_text(&r.turn, &v.viewer);
                    let marks: String = if r.label.starts_with('#') {
                        r.prs.iter().map(|p| p.mark).collect()
                    } else {
                        String::new()
                    };
                    // 題の幅 = 残り全部。右に 番・経過 を置く。
                    let marks = format!("{marks} {}", r.local_mark()).trim().to_string();
                    let right = format!(" {marks} {turn} {:>3}", r.age);
                    let room = width.saturating_sub(4 + 8 + display_width(&right));
                    let title = pad(&clip_width(&r.title, room), room);
                    let base = if index == app.cursor {
                        Style::new().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::new()
                    };
                    let turn_style = if mine {
                        base.fg(MINE).add_modifier(Modifier::BOLD)
                    } else {
                        base.fg(DIM)
                    };
                    if index == app.cursor {
                        selected_line = lines.len();
                    }
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("    {:<8}", r.label),
                            base.fg(if mine { MINE } else { DIM }),
                        ),
                        Span::styled(title, base),
                        Span::styled(format!(" {marks} "), base.fg(DIM)),
                        Span::styled(turn, turn_style),
                        Span::styled(format!(" {:>3}", r.age), base.fg(DIM)),
                    ]));
                    index += 1;
                }
            }
            if !any {
                lines.push(Line::styled(
                    "  出すものはありません",
                    Style::new().fg(FAINT),
                ));
            }
            lines.push(Line::raw(""));
        }
    }
    if app.sections.is_empty() {
        lines.push(Line::styled("GitHub を読んでいます…", Style::new().fg(DIM)));
    }
    // 選んでいる行が見える所まで送る。
    let height = area.height as usize;
    let scroll = (selected_line + 3).saturating_sub(height);
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), area);
}

fn draw_detail(f: &mut Frame, area: Rect, app: &App, side: bool) {
    let block = Block::new()
        .borders(if side { Borders::LEFT } else { Borders::TOP })
        .border_style(Style::new().fg(FAINT));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = visible(app);
    let Some((row, viewer)) = rows.get(app.cursor) else {
        return;
    };
    let mine = row.is_mine(viewer);
    let mut lines = vec![
        Line::styled(format!(" {}", row.label), Style::new().fg(DIM)),
        Line::styled(
            format!(" {}", row.title),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::raw(format!(" {}", row.why)),
            Span::styled(
                format!("  ・  {}", turn_text(&row.turn, viewer)),
                if mine {
                    Style::new().fg(MINE).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(DIM)
                },
            ),
            Span::styled(format!("  ・  {}", row.age), Style::new().fg(DIM)),
        ]),
        Line::raw(""),
    ];
    if row.label.starts_with('#') {
        lines.push(Line::styled(
            format!(" PR {} 本", row.prs.len()),
            Style::new().fg(DIM),
        ));
    }
    for p in &row.prs {
        if row.label.starts_with('#') {
            lines.push(Line::from(vec![
                Span::raw(format!(" {} #{}  ", p.mark, p.number)),
                Span::raw(clip(&p.title, 60)),
            ]));
        }
        lines.push(Line::styled(
            format!(
                "     {} ・ {} ・ {}",
                p.why,
                p.author,
                turn_text(&p.turn, viewer)
            ),
            Style::new().fg(DIM),
        ));
    }
    if !row.local.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(" この PC の上の作業", Style::new().fg(DIM)));
        for l in &row.local {
            lines.push(Line::raw(format!(
                " {} {}",
                if l.working {
                    "▶"
                } else if l.panes.is_empty() {
                    "◇"
                } else {
                    "◆"
                },
                l.branch
            )));
            lines.push(Line::styled(
                format!("     {}", l.path),
                Style::new().fg(DIM),
            ));
            for pane in &l.panes {
                lines.push(Line::styled(format!("     {pane}"), Style::new().fg(DIM)));
            }
        }
    }
    if !row.url.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!(" {}", row.url),
            Style::new().fg(FAINT),
        ));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// 画面上の幅(全角は 2 桁)。
fn display_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

fn clip_width(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = display_width(c.encode_utf8(&mut [0; 4]));
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

fn pad(s: &str, width: usize) -> String {
    format!("{s}{}", " ".repeat(width.saturating_sub(display_width(s))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::sample;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app(filter: Filter, cursor: usize) -> App {
        App {
            sections: vec![Section {
                room: None,
                project: "shop".into(),
                people: vec![],
                repos: vec![Ok(sample()), Err("o/x: 権限がありません".into())],
            }],
            filter,
            cursor,
            loading: false,
            synced: None,
        }
    }

    fn screen(app: &App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, app)).unwrap();
        let b = t.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_counts_rows_and_the_selected_detail() {
        let s = screen(&app(Filter::All, 0), 130, 24);
        // 全角の文字は 2 桁を使い、2 桁目は空になるので、空白を抜いて照らす
        let s: String = s.chars().filter(|c| *c != ' ').collect();
        assert!(s.contains("All4"), "{s}");
        assert!(s.contains("Yourturn2"));
        assert!(s.contains("Theirturn1"));
        assert!(s.contains("shopo/r"));
        assert!(s.contains("Inreview1"));
        assert!(s.contains("#7login"));
        assert!(s.contains("Yourturn"));
        assert!(s.contains("https://github.com/o/r/issues/7"));
        assert!(s.contains("読めませんでした:o/x:権限がありません"));
    }

    #[test]
    fn filter_and_cursor_change_what_is_shown() {
        let s: String = screen(&app(Filter::Others, 0), 130, 24)
            .chars()
            .filter(|c| *c != ' ')
            .collect();
        assert!(s.contains("!#21pr21") && !s.contains("#7login"));
        assert!(s.contains("https://github.com/o/r/pull/21"));
        let s: String = screen(&app(Filter::All, 3), 130, 24)
            .chars()
            .filter(|c| *c != ' ')
            .collect();
        assert!(s.contains("https://github.com/o/r/pull/23"));
    }

    #[test]
    fn narrow_and_empty_screens_do_not_panic() {
        let _ = screen(&app(Filter::All, 0), 40, 8);
        let _ = screen(&app(Filter::All, 0), 1, 1);
        let empty = App {
            sections: vec![],
            filter: Filter::All,
            cursor: 0,
            loading: true,
            synced: None,
        };
        let s: String = screen(&empty, 80, 10)
            .chars()
            .filter(|c| *c != ' ')
            .collect();
        assert!(s.contains("読み込み中"));
    }

    #[test]
    fn widths() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("あa"), 3);
        assert_eq!(clip_width("あいうえお", 7), "あいう…");
        assert_eq!(clip_width("abc", 7), "abc");
        assert_eq!(pad("あ", 4), "あ  ");
    }
}
