//! asis — development, as is.
//! 描くだけ。判断は asis-core、事実を読むのは github、並べるのは view。
mod config;
mod github;
mod local;
mod tui;
mod view;

use config::Project;
use std::io::IsTerminal;
use std::time::{SystemTime, UNIX_EPOCH};
use view::{Filter, RepoView};

const HELP: &str = "asis — development, as is

使い方:
  asis                     見ると決めた repo を開く
  asis <owner/repo>...     その repo を開く
  asis add <owner/repo>    見る repo に足す(--only を付けると、その人の分だけ出す)
  asis remove <owner/repo> 見る repo から外す
  asis init                repo に置く定義ファイル(.asis.toml)のひな形を出す

  --plain                  画面を開かず、文字で出す
  --mine                   自分の番のものだけ出す
  --only <名前,名前>       その人たちが出した PR と、その人たちのタスクだけ出す

プロジェクトの定義(まとめる repo・役割・数えないチェック)は、repo の中の .asis.toml に置きます。
その repo を見られる人は、同じ定義を読みます。置かなければ、その repo 1 つを既定の決まりで見ます。

GitHub の公式コマンド gh のログインを使います。読んだものは保存しません。";

/// 何を見るか。読み直すたびに、定義ファイルも読み直す。
pub struct Source {
    /// 定義を repo の中のファイルから読む repo
    pub watch: Vec<String>,
    /// repo ごとの絞り込み。その人たちの分だけを出す
    pub only: std::collections::BTreeMap<String, Vec<String>>,
    /// 手元の clone を探すフォルダ
    pub roots: Vec<std::path::PathBuf>,
    /// 手元だけの定義
    pub local: Vec<Project>,
}

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
    let mut repos = Vec::new();
    let (mut plain, mut filter, mut only) = (false, Filter::All, Vec::new());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "--plain" => plain = true,
            "--mine" => filter = Filter::Mine,
            "--only" => {
                let names = args.next().ok_or("--only に名前がありません")?;
                only.extend(
                    names
                        .split(',')
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                        .map(str::to_string),
                );
            }
            _ if a.starts_with('-') => return Err(format!("知らない指定です: {a}\n\n{HELP}")),
            _ => repos.push(a),
        }
    }
    match repos.first().map(String::as_str) {
        Some("init") => {
            print!("{}", config::TEMPLATE);
            return Ok(());
        }
        Some(cmd @ ("add" | "remove")) => {
            let [_, repo] = repos.as_slice() else {
                return Err(format!("asis {cmd} <owner/repo> の形で指定してください"));
            };
            let local = config::edit_watch(repo, cmd == "add", &only)?;
            if local.watch.is_empty() {
                println!("見る repo はありません");
            }
            for r in &local.watch {
                match local.only.get(r) {
                    Some(people) => println!("{r}  ({} の分だけ)", people.join("・")),
                    None => println!("{r}"),
                }
            }
            return Ok(());
        }
        _ => {}
    }
    let source = if repos.is_empty() {
        let local = config::load()?;
        // その場の --only は、設定の絞り込みより優先して全部の repo に当てる。
        let only = if only.is_empty() {
            local.only
        } else {
            local
                .watch
                .iter()
                .map(|r| (r.clone(), only.clone()))
                .collect()
        };
        Source {
            watch: local.watch,
            only,
            roots: local.roots.iter().map(|r| expand_home(r)).collect(),
            local: local.projects,
        }
    } else {
        let only = if only.is_empty() {
            Default::default()
        } else {
            repos.iter().map(|r| (r.clone(), only.clone())).collect()
        };
        // その場で repo を指定したときも、設定の探すフォルダは使う。
        let roots = config::load().map(|l| l.roots).unwrap_or_default();
        Source {
            watch: repos,
            only,
            roots: roots.iter().map(|r| expand_home(r)).collect(),
            local: vec![],
        }
    };
    if source.watch.is_empty() && source.local.is_empty() {
        println!(
            "{HELP}\n\nまだ見る repo がありません。`asis add <owner/repo>` で足してください。"
        );
        return Ok(());
    }

    if plain || !std::io::stdout().is_terminal() {
        let sections = load(&source);
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
    tui::run(source, filter).map_err(|e| e.to_string())
}

/// `~/src` → ホームの下の場所
fn expand_home(path: &str) -> std::path::PathBuf {
    match (
        path.strip_prefix("~/"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    ) {
        (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
        _ => std::path::PathBuf::from(path),
    }
}

/// 見る repo の定義を読む。定義ファイルが無ければ、その repo 1 つを既定の決まりで見る。
fn resolve(repo: &str) -> Result<Project, String> {
    match github::project_file(repo)? {
        Some(text) => config::parse_repo_file(&text, repo)
            .map_err(|e| format!("{repo} の {}: {e}", config::REPO_FILE)),
        None => Ok(config::single(repo)),
    }
}

/// 定義を読み、全部の repo を同時に読み、指定の順で返す。
pub fn load(source: &Source) -> Vec<Section> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let stopped = || "読み込みが途中で止まりました".to_string();
    // 1. 見る repo の定義を同時に読む。
    let resolved: Vec<Result<Project, String>> = std::thread::scope(|s| {
        let handles: Vec<_> = source
            .watch
            .iter()
            .map(|r| s.spawn(move || resolve(r)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(stopped())))
            .collect()
    });
    // 定義を読めなかった repo は、その名前で理由を出す。
    let projects: Vec<(&str, Result<&Project, String>)> = source
        .watch
        .iter()
        .zip(&resolved)
        .map(|(repo, r)| (repo.as_str(), r.as_ref().map_err(String::clone)))
        .chain(source.local.iter().map(|p| (p.name.as_str(), Ok(p))))
        .collect();
    // 2. この PC の上の作業(worktree・ペイン)を読む。
    let repos: Vec<String> = projects
        .iter()
        .filter_map(|(_, p)| p.as_ref().ok())
        .flat_map(|p| p.repos.iter().cloned())
        .collect();
    let works = local::gather(&repos, &source.roots);
    let works = &works;
    // 3. 全部の repo を同時に読む。
    std::thread::scope(|s| {
        let handles: Vec<_> = projects
            .into_iter()
            .map(|(label, p)| match p {
                Ok(p) => Ok((
                    p,
                    p.repos
                        .iter()
                        .map(|r| {
                            s.spawn(move || {
                                github::fetch(r, &p.ignore_checks).map(|f| {
                                    let only = source.only.get(r).map(Vec::as_slice).unwrap_or(&[]);
                                    let mine: Vec<local::Work> =
                                        works.iter().filter(|w| w.repo == *r).cloned().collect();
                                    view::build(&f, p, only, &mine, now)
                                })
                            })
                        })
                        .collect::<Vec<_>>(),
                )),
                Err(e) => Err((label, e)),
            })
            .collect();
        handles
            .into_iter()
            .map(|h| match h {
                Ok((p, hs)) => Section {
                    project: p.name.clone(),
                    repos: hs
                        .into_iter()
                        .map(|h| h.join().unwrap_or_else(|_| Err(stopped())))
                        .collect(),
                },
                Err((label, e)) => Section {
                    project: label.to_string(),
                    repos: vec![Err(e)],
                },
            })
            .collect()
    })
}

fn plain_text(sections: &[Section], filter: Filter) -> String {
    let mut out = String::new();
    for v in sections.iter().flat_map(|s| &s.repos).flatten() {
        out.push_str(&format!("\n{}{}\n", v.repo, only_text(&v.only)));
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
                let local = if r.local.is_empty() {
                    String::new()
                } else {
                    format!("  {} 手元", r.local_mark())
                };
                out.push_str(&format!(
                    "    {:<7} {}  [{}] {}  {prs}{local}\n",
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

/// 絞り込んでいることを、repo の名前の横に出す。
pub fn only_text(only: &[String]) -> String {
    if only.is_empty() {
        String::new()
    } else {
        format!("  ({} の分だけ)", only.join("・"))
    }
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
        assert_eq!(only_text(&[]), "");
        assert_eq!(only_text(&["a".into(), "b".into()]), "  (a・b の分だけ)");
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
