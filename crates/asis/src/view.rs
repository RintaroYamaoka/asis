//! 画面に出す形。事実を asis-core に渡して、結果を並べるだけ。
//! 文字の出力も TUI も、ここで作った形を描く。
use crate::config::Project;
use crate::github::RepoFacts;
use crate::local::Work;
use asis_core::facts::*;
use asis_core::link::{link_all, ref_in_branch};
use asis_core::pr::{judge_pr, PrState, PrVerdict, Reason};
use asis_core::task::{judge_task, TaskState};
use std::collections::BTreeMap;

pub struct RepoView {
    pub repo: String,
    pub viewer: String,
    /// この人たちの分だけを出している。空なら全部
    pub only: Vec<String>,
    /// 読み方についての注記(本番の扱い・読み切れなかったこと)
    pub notes: Vec<String>,
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
    /// この PC の上で、この行の作業をしている worktree
    pub local: Vec<LocalLine>,
}

pub struct LocalLine {
    pub path: String,
    pub branch: String,
    /// 開いているペインの説明(AI の名前・状態・題)。無ければ空
    pub panes: Vec<String>,
    pub working: bool,
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
    /// 手元の作業の印。AI が動いている / ペインが開いている / worktree だけある
    pub fn local_mark(&self) -> &'static str {
        if self.local.iter().any(|l| l.working) {
            "▶"
        } else if self.local.iter().any(|l| !l.panes.is_empty()) {
            "◆"
        } else if !self.local.is_empty() {
            "◇"
        } else {
            ""
        }
    }
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
pub const LOCAL: &str = "手元の作業(PR はまだ)";

/// `only` が空でなければ、その人たちが出した PR と、その人たちのタスクだけを出す。
/// `works` は、この repo の、この PC の上の worktree。
pub fn build(
    f: &RepoFacts,
    project: &Project,
    only: &[String],
    works: &[Work],
    now: i64,
) -> RepoView {
    let wanted = |who: &str| only.is_empty() || only.iter().any(|o| o == who);
    let settings = Settings {
        members: f.people.clone(),
        review: project.review.clone(),
        merge: project.merge.clone(),
        deploy: project.deploy.clone(),
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

    // 手元の worktree を、ブランチで行に付ける。付いたものには印を付けておく。
    let mut used = vec![false; works.len()];
    let mut local_for = |matches: &dyn Fn(&str) -> bool| -> Vec<LocalLine> {
        let mut out = Vec::new();
        for (i, w) in works.iter().enumerate() {
            if matches(&w.branch) {
                used[i] = true;
                out.push(local_line(w));
            }
        }
        out
    };
    let open_branch =
        |i: usize| (f.prs[i].lifecycle == Lifecycle::Open).then(|| f.prs[i].head_ref.as_str());

    // タスクの行に入った PR。入らなかった PR は、つながる先を書いていても「つながっていない PR」に出す
    // (読めていない issue を指す PR を、黙って消さない)。
    let mut claimed = vec![false; f.prs.len()];
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
        // この issue の番号で始まるブランチか(規則は asis-core の link と同じ)。
        let numbered =
            |b: &str| ref_in_branch(b, &f.repo).is_some_and(|r| r.number == issue.number);
        // このタスクの worktree = 番号で始まるブランチか、つながる開いた PR のブランチ。
        let local = local_for(&|b| numbered(b) || idx.iter().any(|i| open_branch(*i) == Some(b)));
        let has_branch = !local.is_empty() || f.branches.iter().any(|b| numbered(b));
        let t = judge_task(issue, &vs, has_branch);
        // 起票した人・担当・つながる PR の作者のどれかが当たれば出す。
        let related = wanted(&issue.author)
            || issue.assignees.iter().any(|a| wanted(a))
            || idx.iter().any(|i| wanted(&f.prs[*i].author));
        if !related {
            continue;
        }
        let Some(g) = ORDER.iter().position(|s| *s == t.state) else {
            continue;
        }; // 完了は出さない
        idx.iter().for_each(|i| claimed[*i] = true);
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
            local,
        });
    }

    // タスクにつながっていない PR。本番に出たもの・取り下げたものは出さない。
    let mut loose = Vec::new();
    for (i, p) in f.prs.iter().enumerate() {
        let v = &verdicts[i];
        if claimed[i]
            || !wanted(&p.author)
            || matches!(v.state, PrState::Shipped | PrState::Withdrawn)
        {
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
            local: local_for(&|b| open_branch(i) == Some(b)),
        });
    }

    // どの行にも付かなかった worktree のうち、ペインが開いているもの(今 触っているもの)。
    // 古い worktree が並ばないように、開いていないものは出さない。
    let mut fresh = Vec::new();
    for (w, _) in works.iter().zip(&used).filter(|(w, used)| {
        !**used && !w.panes.is_empty() && !matches!(w.branch.as_str(), "main" | "master")
    }) {
        if !wanted(&f.viewer) {
            break;
        }
        let l = local_line(w);
        fresh.push(Row {
            label: "手元".into(),
            title: w.branch.clone(),
            url: String::new(),
            turn: vec![f.viewer.clone()],
            age: String::new(),
            why: "手元で作業中・まだ PR が無い".into(),
            prs: vec![],
            local: vec![l],
        });
    }
    groups.push(Group {
        name: LOCAL,
        rows: fresh,
    });
    groups.push(Group {
        name: LOOSE,
        rows: loose,
    });

    RepoView {
        repo: f.repo.clone(),
        viewer: f.viewer.clone(),
        only: only.to_vec(),
        notes: f
            .no_deployments
            .then(|| "本番デプロイの記録が無いので、取り込みを本番として扱っています".to_string())
            .into_iter()
            .chain(f.notes.iter().cloned())
            .collect(),
        groups,
    }
}

fn local_line(w: &Work) -> LocalLine {
    LocalLine {
        path: w.path.display().to_string(),
        branch: w.branch.clone(),
        panes: w
            .panes
            .iter()
            .map(|p| {
                [p.agent.as_str(), p.status.as_str(), p.title.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect(),
        working: w.working(),
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
        sample_only(&[])
    }

    fn sample_only(only: &[&str]) -> RepoView {
        sample_with(only, &[])
    }

    fn sample_refs(bodies: &[(u64, &str)]) -> RepoView {
        sample_full(&[], &[], bodies)
    }

    fn sample_with(only: &[&str], works: &[Work]) -> RepoView {
        sample_full(only, works, &[])
    }

    fn sample_full(only: &[&str], works: &[Work], bodies: &[(u64, &str)]) -> RepoView {
        let only: Vec<String> = only.iter().map(|s| s.to_string()).collect();
        let mut failing = pr(21, "ann", "fix");
        failing.checks = Checks::Failing(1);
        let mut shipped = pr(22, "ann", "old");
        shipped.lifecycle = Lifecycle::Merged;
        shipped.shipped = true;
        let mut bobs = pr(23, "bob", "x");
        bobs.is_draft = true;
        let mut prs = vec![pr(20, "ann", "7-login"), failing, shipped, bobs];
        for (number, body) in bodies {
            prs.iter_mut().find(|p| p.number == *number).unwrap().body = body.to_string();
        }
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
                    author: "cat".into(),
                    assignees: vec![],
                },
                IssueFacts {
                    repo: "o/r".into(),
                    number: 8,
                    title: "later".into(),
                    is_open: true,
                    author: "bob".into(),
                    assignees: vec![],
                },
                IssueFacts {
                    repo: "o/r".into(),
                    number: 9,
                    title: "old".into(),
                    is_open: false,
                    author: "ann".into(),
                    assignees: vec![],
                },
            ],
            branches: vec![],
            no_deployments: false,
            people: vec!["ann".into(), "bob".into()],
            notes: vec![],
        };
        let project = Project {
            name: "p".into(),
            repos: vec!["o/r".into()],
            ..Project::default()
        };
        build(&f, &project, &only, works, 1795005296)
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
        assert!(v.notes.is_empty());
    }

    #[test]
    fn a_pr_pointing_at_an_unknown_issue_is_still_listed() {
        // 読めていない issue(古く閉じたもの・別の repo のもの)を指す PR を、黙って消さない
        let v = sample_refs(&[(21, "Refs #999"), (23, "Refs other/repo#5")]);
        assert_eq!(
            rows(&v, LOOSE)
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            vec!["!#21", "…#23"]
        );
        // 完了したタスク(#9)を指す開いた PR は、ずれとしてタスクの側に出る
        let v = sample_refs(&[(21, "Refs #9")]);
        let row = v
            .groups
            .iter()
            .flat_map(|g| g.rows.iter())
            .find(|r| r.label == "#9")
            .unwrap();
        assert!(row.why.contains("issue は閉じているが PR が残っている"));
        assert_eq!(
            rows(&v, LOOSE)
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            vec!["…#23"]
        );
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
    fn only_shows_the_named_peoples_work() {
        let labels = |v: &RepoView| {
            v.groups
                .iter()
                .flat_map(|g| g.rows.iter())
                .map(|r| r.label.clone())
                .collect::<Vec<_>>()
        };
        // ann の PR(#20 は タスク #7 につながる・#21 はつながらない)。bob の下書き #23 と bob のタスク #8 は出ない
        let v = sample_only(&["ann"]);
        assert_eq!(labels(&v), vec!["#7", "!#21"]);
        assert_eq!(v.only, vec!["ann"]);
        // bob が起票したタスク #8 と、bob の PR
        assert_eq!(labels(&sample_only(&["bob"])), vec!["#8", "…#23"]);
        // cat は PR を出していないが、タスク #7 を起票している
        assert_eq!(labels(&sample_only(&["cat"])), vec!["#7"]);
        assert_eq!(labels(&sample_only(&["ann", "bob"])), labels(&sample()));
        assert!(labels(&sample_only(&["nobody"])).is_empty());
    }

    fn work(branch: &str, pane_status: Option<&str>) -> Work {
        use crate::local::Pane;
        let panes = pane_status
            .into_iter()
            .map(|s| Pane {
                cwd: "/w".into(),
                agent: "claude".into(),
                status: s.into(),
                title: "t".into(),
            })
            .collect();
        Work {
            repo: "o/r".into(),
            path: format!("/w/{branch}").into(),
            branch: branch.into(),
            panes,
        }
    }

    #[test]
    fn local_work_attaches_to_tasks_and_prs() {
        // 7-login = タスク #7 の PR のブランチ。8-wip = まだ PR の無いタスク #8。fix = つながっていない PR #21
        let v = sample_with(
            &[],
            &[
                work("7-login", Some("working")),
                work("feature/8-wip", None),
                work("fix", Some("idle")),
                work("main", Some("idle")),
            ],
        );
        let row = |label: &str| {
            v.groups
                .iter()
                .flat_map(|g| g.rows.iter())
                .find(|r| r.label == label)
                .unwrap()
        };
        assert_eq!(row("#7").local_mark(), "▶");
        assert_eq!(row("#7").local[0].panes, vec!["claude working t"]);
        assert_eq!(row("!#21").local_mark(), "◆");
        // 手元にブランチがあるだけで、タスクは実装中になる(push する前から見える)
        assert_eq!(
            rows(&v, "実装中")
                .iter()
                .map(|r| (r.label.as_str(), r.local_mark()))
                .collect::<Vec<_>>(),
            vec![("#8", "◇")]
        );
        assert!(rows(&v, "やること").is_empty());
        // 既定のブランチの worktree は出さない
        assert!(rows(&v, LOCAL).is_empty());
        assert_eq!(row("…#23").local_mark(), "");
    }

    #[test]
    fn unlinked_local_work_is_listed_only_while_a_pane_is_open() {
        let v = sample_with(
            &[],
            &[
                work("spike", Some("working")),
                work("old-idea", None),
                work("99-unknown", Some("idle")),
            ],
        );
        let local = rows(&v, LOCAL);
        assert_eq!(
            local.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["spike", "99-unknown"]
        );
        assert_eq!(
            (
                local[0].label.as_str(),
                local[0].local_mark(),
                local[0].turn.clone()
            ),
            ("手元", "▶", vec!["bob".to_string()])
        );
        assert!(local[0].url.is_empty());
        // ほかの人の分だけを見ているときは、自分の手元の作業は出さない
        assert!(rows(
            &sample_with(&["ann"], &[work("spike", Some("working"))]),
            LOCAL
        )
        .is_empty());
        assert_eq!(
            rows(
                &sample_with(&["bob"], &[work("spike", Some("working"))]),
                LOCAL
            )
            .len(),
            1
        );
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
