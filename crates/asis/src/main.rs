//! asis — development, as is.
//! 描くだけ。判断は asis-core、事実を読むのは github、並べるのは view。
mod config;
mod github;
mod tui;
mod view;

use config::Project;
use std::io::IsTerminal;
use std::time::{SystemTime, UNIX_EPOCH};
use view::{Filter, RepoView};

const HELP: &str = "asis — development, as is

使い方:
  asis                     設定ファイルのプロジェクトを開く
  asis <owner/repo>...     その repo を開く

  --plain                  画面を開かず、文字で出す
  --mine                   自分の番のものだけ出す
  --merger author|reviewer    承認のあと取り込む人(既定 author)
  --deployer author|reviewer  取り込みのあと本番に出す人(既定 author)
  --ignore-check <名前>    状態を決めるときに数えないチェック(何度でも指定できる)

GitHub の公式コマンド gh のログインを使います。何も保存しません。";

/// プロジェクト名と、その repo ごとの結果
pub struct Section {
    pub project: String,
    pub repos: Vec<Result<RepoView, String>>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut adhoc = Project {
        name: String::new(),
        repos: vec![],
        merger: asis_core::facts::Role::Author,
        deployer: asis_core::facts::Role::Author,
        ignore_checks: vec![],
    };
    let (mut plain, mut filter) = (false, Filter::All);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{a} に値がありません"));
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "--plain" => plain = true,
            "--mine" => filter = Filter::Mine,
            "--merger" => adhoc.merger = config::role(&value()?)?,
            "--deployer" => adhoc.deployer = config::role(&value()?)?,
            "--ignore-check" => adhoc.ignore_checks.push(value()?),
            _ if a.starts_with('-') => return Err(format!("知らない指定です: {a}\n\n{HELP}")),
            _ => adhoc.repos.push(a),
        }
    }
    let projects = if adhoc.repos.is_empty() {
        config::load()?
    } else {
        vec![adhoc]
    };
    if projects.is_empty() {
        let at = config::path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        println!("{HELP}\n\n設定ファイルがありません: {at}");
        return Ok(());
    }

    if plain || !std::io::stdout().is_terminal() {
        let sections = load(&projects);
        print!("{}", plain_text(&sections, filter));
        let failed = sections
            .iter()
            .flat_map(|s| &s.repos)
            .filter_map(|r| r.as_ref().err())
            .inspect(|e| eprintln!("{e}"))
            .count();
        return if failed > 0 {
            Err(format!("{failed} 個の repo を読めませんでした"))
        } else {
            Ok(())
        };
    }
    tui::run(projects, filter).map_err(|e| e.to_string())
}

/// 全部の repo を同時に読み、設定の順で返す。
pub fn load(projects: &[Project]) -> Vec<Section> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    std::thread::scope(|s| {
        let handles: Vec<Vec<_>> = projects
            .iter()
            .map(|p| {
                p.repos
                    .iter()
                    .map(|r| {
                        s.spawn(move || {
                            github::fetch(r, &p.ignore_checks).map(|f| view::build(&f, p, now))
                        })
                    })
                    .collect()
            })
            .collect();
        projects
            .iter()
            .zip(handles)
            .map(|(p, hs)| Section {
                project: p.name.clone(),
                repos: hs
                    .into_iter()
                    .map(|h| {
                        h.join()
                            .unwrap_or_else(|_| Err("読み込みが途中で止まりました".into()))
                    })
                    .collect(),
            })
            .collect()
    })
}

fn plain_text(sections: &[Section], filter: Filter) -> String {
    let mut out = String::new();
    for v in sections.iter().flat_map(|s| &s.repos).flatten() {
        out.push_str(&format!("\n{}\n", v.repo));
        if let Some(note) = &v.note {
            out.push_str(&format!("  ({note})\n"));
        }
        let mut shown = 0;
        for g in &v.groups {
            let rows: Vec<_> = g
                .rows
                .iter()
                .filter(|r| r.shows(filter, &v.viewer))
                .collect();
            if rows.is_empty() {
                continue;
            }
            out.push_str(&format!("\n  {}  {}\n", g.name, rows.len()));
            for r in rows {
                shown += 1;
                let prs = if r.label.starts_with('#') {
                    r.prs
                        .iter()
                        .map(|p| format!("{}#{}", p.mark, p.number))
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    r.why.clone()
                };
                out.push_str(&format!(
                    "    {:<7} {}  [{}] {}  {prs}\n",
                    r.label,
                    clip(&r.title, 46),
                    turn_text(&r.turn, &v.viewer),
                    r.age
                ));
            }
        }
        if shown == 0 {
            out.push_str(if filter == Filter::Mine {
                "\n  あなたの番のものはありません\n"
            } else {
                "\n  動いているものはありません\n"
            });
        }
    }
    out
}

pub fn turn_text(turn: &[String], viewer: &str) -> String {
    match turn {
        [] => "-".to_string(),
        t if t.iter().any(|x| x == viewer) => "あなたの番".to_string(),
        t => t.join(","),
    }
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).chain(['…']).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_short_and_cuts_long() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("あいうえおか", 5), "あいうえ…");
    }

    #[test]
    fn plain_text_lists_groups_and_respects_the_filter() {
        let sections = vec![Section {
            project: "p".into(),
            repos: vec![Ok(view::tests::sample()), Err("x".into())],
        }];
        let all = plain_text(&sections, Filter::All);
        assert!(all.contains("o/r\n"));
        assert!(all.contains("確認待ち  1"));
        assert!(all.contains("#7      login  [あなたの番] 5h  ?#20"));
        assert!(all.contains("!#21    pr 21  [ann] 5h  チェックが落ちている(lint)"));
        let mine = plain_text(&sections, Filter::Mine);
        assert!(mine.contains("#7 ") && !mine.contains("#21") && !mine.contains("#8"));
    }

    #[test]
    fn turn_text_forms() {
        assert_eq!(turn_text(&[], "me"), "-");
        assert_eq!(turn_text(&["a".into(), "me".into()], "me"), "あなたの番");
        assert_eq!(turn_text(&["a".into(), "b".into()], "me"), "a,b");
    }
}
