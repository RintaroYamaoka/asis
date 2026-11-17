//! asis — development, as is.
//! 描くだけ。判断は asis-core、事実を読むのは github。
mod github;

use asis_core::facts::*;
use asis_core::link::link_all;
use asis_core::pr::{judge_pr, PrState, PrVerdict, Reason};
use asis_core::task::{judge_task, TaskState};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const HELP: &str = "asis — development, as is

使い方:
  asis <owner/repo>... [--merger author|reviewer] [--deployer author|reviewer] [--mine]

  --merger    承認のあと取り込む人(既定 author)
  --deployer  取り込みのあと本番に出す人(既定 author)
  --mine      自分の番のものだけ出す

GitHub の公式コマンド gh のログインを使います。何も保存しません。";

fn main() {
    let mut repos = Vec::new();
    let (mut merger, mut deployer, mut mine) = (Role::Author, Role::Author, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => return println!("{HELP}"),
            "--mine" => mine = true,
            "--merger" | "--deployer" => {
                let role = match args.next().as_deref() {
                    Some("author") => Role::Author,
                    Some("reviewer") => Role::Reviewer,
                    other => {
                        return eprintln!(
                            "{a} には author か reviewer を指定してください(指定: {other:?})"
                        )
                    }
                };
                if a == "--merger" {
                    merger = role
                } else {
                    deployer = role
                }
            }
            _ if a.starts_with('-') => return eprintln!("知らない指定です: {a}\n\n{HELP}"),
            _ => repos.push(a),
        }
    }
    if repos.is_empty() {
        return println!("{HELP}");
    }
    // repo ごとに同時に読み、指定された順で出す。
    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = repos
            .iter()
            .map(|r| s.spawn(move || github::fetch(r)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("読み込みが途中で止まりました".into()))
            })
            .collect()
    });
    let mut failed = false;
    for result in results {
        match result {
            Ok(f) => print!("{}", render(&f, merger, deployer, mine, now())),
            Err(e) => {
                failed = true;
                eprintln!("{e}");
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

struct Row {
    label: String,
    title: String,
    prs: String,
    note: String,
    turn: Vec<String>,
    age: String,
}

fn render(f: &github::RepoFacts, merger: Role, deployer: Role, mine: bool, now: i64) -> String {
    let settings = Settings {
        members: f.people.clone(),
        merger,
        deployer,
        merged_is_shipped: f.no_deployments,
    };
    let verdicts: Vec<PrVerdict> = f.prs.iter().map(|p| judge_pr(p, &settings)).collect();
    let links = link_all(&f.prs);

    // タスクごとに、つながる PR の番号を集める。
    let mut by_issue: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, refs) in links.iter().enumerate() {
        for r in refs.iter().filter(|r| r.repo == f.repo) {
            by_issue.entry(r.number).or_default().push(i);
        }
    }

    let order = [
        TaskState::InReview,
        TaskState::AwaitingShip,
        TaskState::ShippedOpen,
        TaskState::Doing,
        TaskState::Todo,
    ];
    let mut groups: Vec<(String, Vec<Row>)> = order
        .iter()
        .map(|s| (task_label(*s).to_string(), vec![]))
        .collect();
    for issue in &f.issues {
        let idx = by_issue.get(&issue.number).cloned().unwrap_or_default();
        let vs: Vec<PrVerdict> = idx.iter().map(|i| verdicts[*i].clone()).collect();
        let prefix = format!("{}-", issue.number);
        let has_branch = f.branches.iter().any(|b| b.starts_with(&prefix));
        let t = judge_task(issue, &vs, has_branch);
        let Some(g) = order.iter().position(|s| *s == t.state) else {
            continue;
        }; // 完了は出さない
        let newest = idx
            .iter()
            .map(|i| f.pr_updated[*i].as_str())
            .max()
            .unwrap_or("");
        groups[g].1.push(Row {
            label: format!("#{}", issue.number),
            title: issue.title.clone(),
            prs: idx
                .iter()
                .map(|i| format!("{}#{}", mark(verdicts[*i].state), f.prs[*i].number))
                .collect::<Vec<_>>()
                .join(" "),
            note: if t.mismatch {
                "issue は閉じているが PR が残っている".into()
            } else {
                String::new()
            },
            turn: t.turn,
            age: age(newest, now),
        });
    }

    // タスクにつながっていない PR。本番に出たもの・取り下げたものは出さない。
    let mut loose = Vec::new();
    for (i, p) in f.prs.iter().enumerate() {
        let v = &verdicts[i];
        if !links[i].is_empty() || matches!(v.state, PrState::Shipped | PrState::Withdrawn) {
            continue;
        }
        loose.push(Row {
            label: format!("{}#{}", mark(v.state), p.number),
            title: p.title.clone(),
            prs: String::new(),
            note: reason_text(v.reason),
            turn: v.turn.clone(),
            age: age(&f.pr_updated[i], now),
        });
    }
    groups.push(("タスクにつながっていない PR".to_string(), loose));

    let mut out = format!("\n{}\n", f.repo);
    if f.no_deployments {
        out.push_str("  (本番デプロイの記録が無いので、取り込みを本番として扱っています)\n");
    }
    let mut shown = 0;
    for (name, rows) in groups {
        let rows: Vec<&Row> = rows
            .iter()
            .filter(|r| !mine || r.turn.contains(&f.viewer))
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("\n  {name}  {}\n", rows.len()));
        for r in rows {
            shown += 1;
            let turn = match r.turn.as_slice() {
                [] => "-".to_string(),
                t if t.contains(&f.viewer) => "あなたの番".to_string(),
                t => t.join(","),
            };
            let extra = [r.prs.as_str(), r.note.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("  ");
            out.push_str(&format!(
                "    {:<7} {}  [{turn}] {}  {extra}\n",
                r.label,
                clip(&r.title, 46),
                r.age
            ));
        }
    }
    if shown == 0 {
        out.push_str(if mine {
            "\n  あなたの番のものはありません\n"
        } else {
            "\n  動いているものはありません\n"
        });
    }
    out
}

fn task_label(s: TaskState) -> &'static str {
    match s {
        TaskState::Todo => "やること",
        TaskState::Doing => "実装中",
        TaskState::InReview => "確認待ち",
        TaskState::AwaitingShip => "本番待ち",
        TaskState::ShippedOpen => "本番に出た・閉じていない",
        TaskState::Done => "完了",
    }
}

fn mark(s: PrState) -> &'static str {
    match s {
        PrState::Shipped => "✓",
        PrState::Merged => "↑",
        PrState::Withdrawn => "×",
        PrState::Draft => "…",
        PrState::NeedsFix => "!",
        PrState::Testing => "~",
        PrState::ReadyToMerge => "+",
        PrState::InReview => "?",
    }
}

fn reason_text(r: Reason) -> String {
    match r {
        Reason::Shipped => "本番に出た",
        Reason::MergedNotShipped => "取り込み済み・まだ本番に出ていない",
        Reason::Withdrawn => "取り下げ",
        Reason::Draft => "下書き",
        Reason::Conflict => "取り込み先とぶつかっている",
        Reason::ChecksFailing(_) => "テストが落ちている",
        Reason::ChangesRequested => "直しを頼まれている",
        Reason::UnansweredComment => "返していないコメントがある",
        Reason::ChecksRunning => "テストが動いている",
        Reason::Approved => "承認済み・取り込み待ち",
        Reason::AwaitingReview => "確認待ち",
    }
    .to_string()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max - 1).chain(['…']).collect()
}

/// `2026-01-02T03:04:05Z` からの経過。読めなければ空。
fn age(iso: &str, now: i64) -> String {
    let Some(t) = epoch(iso) else {
        return String::new();
    };
    let d = (now - t).max(0);
    match d {
        0..=3599 => format!("{}m", d / 60),
        3600..=86399 => format!("{}h", d / 3600),
        _ => format!("{}d", d / 86400),
    }
}

fn epoch(iso: &str) -> Option<i64> {
    let n = |a: usize, b: usize| iso.get(a..b)?.parse::<i64>().ok();
    let (y, m, d, h, mi, s) = (
        n(0, 4)?,
        n(5, 7)?,
        n(8, 10)?,
        n(11, 13)?,
        n(14, 16)?,
        n(17, 19)?,
    );
    // 暦の日付 → 1970-01-01 からの日数
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_age() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2000-03-01T00:00:00Z"), Some(951868800));
        assert_eq!(epoch("2026-11-18T12:34:56Z"), Some(1795005296));
        assert_eq!(epoch("nope"), None);
        let now = 1795005296;
        assert_eq!(age("2026-11-18T12:04:56Z", now), "30m");
        assert_eq!(age("2026-11-18T07:34:56Z", now), "5h");
        assert_eq!(age("2026-11-15T12:34:56Z", now), "3d");
        assert_eq!(age("", now), "");
    }

    #[test]
    fn clip_keeps_short_and_cuts_long() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("あいうえおか", 5), "あいうえ…");
    }
}
