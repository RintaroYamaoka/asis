//! 画面に出す形。事実を asis-core に渡して、結果を並べるだけ。
//! 文字の出力も TUI も、ここで作った形を描く。
use crate::config::Project;
use crate::github::RepoFacts;
use asis_core::facts::*;
use asis_core::link::link_all;
use asis_core::pr::{judge_pr, PrState, PrVerdict, Reason};
use asis_core::task::{judge_task, TaskState};
use std::collections::BTreeMap;

pub struct RepoView {
    pub repo: String,
    pub viewer: String,
    pub note: Option<String>,
    pub groups: Vec<Group>,
}

pub struct Group {
    pub name: &'static str,
    pub rows: Vec<Row>,
}

pub struct Row {
    /// `#12`(タスク)か `?#34`(タスクにつながっていない PR)
    pub label: String,
    pub title: String,
    pub url: String,
    pub turn: Vec<String>,
    pub age: String,
    /// なぜこの状態か
    pub why: String,
    pub prs: Vec<PrLine>,
}

pub struct PrLine {
    pub mark: &'static str,
    pub number: u64,
    pub title: String,
    pub author: String,
    pub why: String,
    pub turn: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Mine,
    Others,
}

impl Filter {
    pub fn next(self) -> Filter {
        match self {
            Filter::All => Filter::Mine,
            Filter::Mine => Filter::Others,
            Filter::Others => Filter::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "すべて",
            Filter::Mine => "自分の番",
            Filter::Others => "相手の番",
        }
    }
}

impl Row {
    pub fn is_mine(&self, viewer: &str) -> bool {
        self.turn.iter().any(|t| t == viewer)
    }
    pub fn shows(&self, filter: Filter, viewer: &str) -> bool {
        match filter {
            Filter::All => true,
            Filter::Mine => self.is_mine(viewer),
            Filter::Others => !self.turn.is_empty() && !self.is_mine(viewer),
        }
    }
}

const ORDER: [TaskState; 5] = [
    TaskState::InReview,
    TaskState::AwaitingShip,
    TaskState::ShippedOpen,
    TaskState::Doing,
    TaskState::Todo,
];
pub const LOOSE: &str = "タスクにつながっていない PR";

pub fn build(f: &RepoFacts, project: &Project, now: i64) -> RepoView {
    let settings = Settings {
        members: f.people.clone(),
        merger: project.merger,
        deployer: project.deployer,
        merged_is_shipped: f.no_deployments,
    };
    let verdicts: Vec<PrVerdict> = f.prs.iter().map(|p| judge_pr(p, &settings)).collect();
    let links = link_all(&f.prs);
    let line = |i: usize| PrLine {
        mark: mark(verdicts[i].state),
        number: f.prs[i].number,
        title: f.prs[i].title.clone(),
        author: f.prs[i].author.clone(),
        why: why_pr(verdicts[i].reason, &f.pr_failing[i]),
        turn: verdicts[i].turn.clone(),
    };

    let mut by_issue: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, refs) in links.iter().enumerate() {
        for r in refs.iter().filter(|r| r.repo == f.repo) {
            by_issue.entry(r.number).or_default().push(i);
        }
    }

    let mut groups: Vec<Group> = ORDER
        .iter()
        .map(|s| Group {
            name: task_label(*s),
            rows: vec![],
        })
        .collect();
    for issue in &f.issues {
        let idx = by_issue.get(&issue.number).cloned().unwrap_or_default();
        let vs: Vec<PrVerdict> = idx.iter().map(|i| verdicts[*i].clone()).collect();
        let prefix = format!("{}-", issue.number);
        let t = judge_task(
            issue,
            &vs,
            f.branches.iter().any(|b| b.starts_with(&prefix)),
        );
        let Some(g) = ORDER.iter().position(|s| *s == t.state) else {
            continue;
        }; // 完了は出さない
        let newest = idx
            .iter()
            .map(|i| f.pr_updated[*i].as_str())
            .max()
            .unwrap_or("");
        groups[g].rows.push(Row {
            label: format!("#{}", issue.number),
            title: issue.title.clone(),
            url: format!("https://github.com/{}/issues/{}", f.repo, issue.number),
            turn: t.turn,
            age: age(newest, now),
            why: if t.mismatch {
                format!(
                    "{}(issue は閉じているが PR が残っている)",
                    task_label(t.state)
                )
            } else {
                task_label(t.state).to_string()
            },
            prs: idx.iter().map(|i| line(*i)).collect(),
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
            url: format!("https://github.com/{}/pull/{}", f.repo, p.number),
            turn: v.turn.clone(),
            age: age(&f.pr_updated[i], now),
            why: why_pr(v.reason, &f.pr_failing[i]),
            prs: vec![line(i)],
        });
    }
    groups.push(Group {
        name: LOOSE,
        rows: loose,
    });

    RepoView {
        repo: f.repo.clone(),
        viewer: f.viewer.clone(),
        note: f
            .no_deployments
            .then(|| "本番デプロイの記録が無いので、取り込みを本番として扱っています".to_string()),
        groups,
    }
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

fn why_pr(r: Reason, failing: &[String]) -> String {
    match r {
        Reason::Shipped => "本番に出た".into(),
        Reason::MergedNotShipped => "取り込み済み・まだ本番に出ていない".into(),
        Reason::Withdrawn => "取り下げ".into(),
        Reason::Draft => "下書き".into(),
        Reason::Conflict => "取り込み先とぶつかっている".into(),
        Reason::ChecksFailing(_) => format!("チェックが落ちている({})", failing.join("・")),
        Reason::ChangesRequested => "直しを頼まれている".into(),
        Reason::UnansweredComment => "返していないコメントがある".into(),
        Reason::ChecksRunning => "チェックが動いている".into(),
        Reason::Approved => "承認済み・取り込み待ち".into(),
        Reason::AwaitingReview => "確認待ち".into(),
    }
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
pub(crate) mod tests {
    use super::*;

    fn pr(number: u64, author: &str, head: &str) -> PrFacts {
        PrFacts {
            repo: "o/r".into(),
            number,
            title: format!("pr {number}"),
            body: String::new(),
            author: author.into(),
            head_ref: head.into(),
            base_ref: "main".into(),
            lifecycle: Lifecycle::Open,
            is_draft: false,
            has_conflict: false,
            checks: Checks::Passing,
            review: Review::None,
            requested_reviewers: vec![],
            unanswered_comment: false,
            shipped: false,
            closes: vec![],
        }
    }

    pub(crate) fn sample() -> RepoView {
        let mut failing = pr(21, "ann", "fix");
        failing.checks = Checks::Failing(1);
        let mut shipped = pr(22, "ann", "old");
        shipped.lifecycle = Lifecycle::Merged;
        shipped.shipped = true;
        let mut bobs = pr(23, "bob", "x");
        bobs.is_draft = true;
        let prs = vec![pr(20, "ann", "7-login"), failing, shipped, bobs];
        let n = prs.len();
        let f = RepoFacts {
            repo: "o/r".into(),
            viewer: "bob".into(),
            pr_updated: vec!["2026-11-18T07:34:56Z".into(); n],
            pr_failing: vec![vec![], vec!["lint".into()], vec![], vec![]],
            prs,
            issues: vec![
                IssueFacts {
                    repo: "o/r".into(),
                    number: 7,
                    title: "login".into(),
                    is_open: true,
                    assignees: vec![],
                },
                IssueFacts {
                    repo: "o/r".into(),
                    number: 8,
                    title: "later".into(),
                    is_open: true,
                    assignees: vec![],
                },
                IssueFacts {
                    repo: "o/r".into(),
                    number: 9,
                    title: "old".into(),
                    is_open: false,
                    assignees: vec![],
                },
            ],
            branches: vec![],
            no_deployments: false,
            people: vec!["ann".into(), "bob".into()],
        };
        let project = Project {
            name: "p".into(),
            repos: vec!["o/r".into()],
            merger: Role::Author,
            deployer: Role::Author,
            ignore_checks: vec![],
        };
        build(&f, &project, 1795005296)
    }

    fn rows<'a>(v: &'a RepoView, group: &str) -> Vec<&'a Row> {
        v.groups
            .iter()
            .filter(|g| g.name == group)
            .flat_map(|g| g.rows.iter())
            .collect()
    }

    #[test]
    fn tasks_are_grouped_by_state_and_carry_their_prs() {
        let v = sample();
        let review = rows(&v, "確認待ち");
        assert_eq!(review.len(), 1);
        assert_eq!(
            (review[0].label.as_str(), review[0].title.as_str()),
            ("#7", "login")
        );
        assert_eq!(
            review[0].prs.iter().map(|p| p.number).collect::<Vec<_>>(),
            vec![20]
        );
        assert_eq!(review[0].turn, vec!["bob"]);
        assert_eq!(review[0].url, "https://github.com/o/r/issues/7");
        assert_eq!(review[0].age, "5h");
        assert_eq!(
            rows(&v, "やること")
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            vec!["#8"]
        );
        // 完了したタスクは出さない
        assert!(v
            .groups
            .iter()
            .all(|g| g.rows.iter().all(|r| r.label != "#9")));
    }

    #[test]
    fn unlinked_prs_are_listed_unless_finished() {
        let v = sample();
        let loose = rows(&v, LOOSE);
        assert_eq!(
            loose.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            vec!["!#21", "…#23"]
        );
        assert_eq!(loose[0].why, "チェックが落ちている(lint)");
        assert_eq!(loose[0].url, "https://github.com/o/r/pull/21");
        assert!(v.note.is_none());
    }

    #[test]
    fn filter_by_whose_turn() {
        let v = sample();
        let shown = |f: Filter| {
            v.groups
                .iter()
                .flat_map(|g| g.rows.iter())
                .filter(|r| r.shows(f, &v.viewer))
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(Filter::All), vec!["#7", "#8", "!#21", "…#23"]);
        assert_eq!(shown(Filter::Mine), vec!["#7", "…#23"]);
        // 誰の番でもないもの(#8)は、相手の番にも出ない
        assert_eq!(shown(Filter::Others), vec!["!#21"]);
        assert_eq!(Filter::All.next().next().next(), Filter::All);
    }

    #[test]
    fn epoch_and_age() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2000-03-01T00:00:00Z"), Some(951868800));
        assert_eq!(epoch("2026-11-18T12:34:56Z"), Some(1795005296));
        assert_eq!(epoch("nope"), None);
        let now = 1795005296;
        assert_eq!(age("2026-11-18T12:04:56Z", now), "30m");
        assert_eq!(age("2026-11-15T12:34:56Z", now), "3d");
        assert_eq!(age("", now), "");
    }
}
