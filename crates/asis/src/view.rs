//! 画面に出す形。事実を asis-core に渡して、結果を並べるだけ。
//! 文字の出力も TUI も、ここで作った形を描く。
use crate::config::Project;
use crate::extra::FileTask;
use crate::github::RepoFacts;
use crate::local::Work;
use asis_core::facts::*;
use asis_core::link::{link_all, ref_in_branch};
use asis_core::pr::{judge_pr, PrState, PrVerdict, Reason};
use asis_core::task::{judge_task_in, TaskState};
use std::collections::BTreeMap;

pub struct RepoView {
    pub repo: String,
    pub viewer: String,
    /// この人たちの分だけを出している。空なら全部
    pub only: Vec<String>,
    /// 読み方についての注記(本番の扱い・読み切れなかったこと)
    pub notes: Vec<String>,
    pub groups: Vec<Group>,
    /// 終わったタスク(issue が閉じていて、終わっていない PR が無いもの)。
    /// 今の状態を持たないので、`stage` は見ない(`why` は `Closed`)
    pub closed: Vec<Row>,
}

pub struct Group {
    pub name: &'static str,
    pub rows: Vec<Row>,
}

/// 行が何か
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// GitHub の issue
    Task,
    /// repo の中のファイルに書かれたタスク(差し込みの読み口が足したもの。`extra` を参照)
    Doc,
    /// タスクにつながっていない PR
    Pr,
    /// この PC の上の、まだ PR が無い作業
    Local,
}

/// いまどの段階か。タスクにも、タスクにつながっていない PR にも、同じ言葉を使う。
/// 並びは、人の手が要る順。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    NeedsFix,
    InReview,
    ReadyToMerge,
    Testing,
    AwaitingShip,
    ShippedOpen,
    Doing,
    Todo,
}

impl Stage {
    pub const ALL: [Stage; 8] = [
        Stage::NeedsFix,
        Stage::InReview,
        Stage::ReadyToMerge,
        Stage::Testing,
        Stage::AwaitingShip,
        Stage::ShippedOpen,
        Stage::Doing,
        Stage::Todo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Stage::NeedsFix => "Needs action",
            Stage::InReview => "Review required",
            Stage::ReadyToMerge => "Ready to merge",
            Stage::Testing => "Checks pending",
            Stage::AwaitingShip => "Merged",
            Stage::ShippedOpen => "Deployed",
            Stage::Doing => "In progress",
            Stage::Todo => "Todo",
        }
    }

    fn of_task(state: TaskState, prs: &[PrVerdict]) -> Option<Stage> {
        // つながる PR に直しが要れば、タスクも「直しが要る」。
        if prs.iter().any(|v| v.state == PrState::NeedsFix) {
            return Some(Stage::NeedsFix);
        }
        Some(match state {
            TaskState::InReview => Stage::InReview,
            TaskState::AwaitingShip => Stage::AwaitingShip,
            TaskState::ShippedOpen => Stage::ShippedOpen,
            TaskState::Doing => Stage::Doing,
            TaskState::Todo => Stage::Todo,
            TaskState::Done => return None,
        })
    }

    fn of_pr(state: PrState) -> Option<Stage> {
        Some(match state {
            PrState::NeedsFix => Stage::NeedsFix,
            PrState::InReview => Stage::InReview,
            PrState::ReadyToMerge => Stage::ReadyToMerge,
            PrState::Testing => Stage::Testing,
            PrState::Merged => Stage::AwaitingShip,
            PrState::Draft => Stage::Doing,
            PrState::Shipped | PrState::Withdrawn => return None,
        })
    }
}

pub struct Row {
    pub kind: Kind,
    pub stage: Stage,
    /// issue か PR の番号。手元の作業は 0
    pub number: u64,
    /// `#12`(タスク)か `?#34`(タスクにつながっていない PR)
    pub label: String,
    pub title: String,
    pub url: String,
    pub turn: Vec<String>,
    /// 番が付いてからの時間。番の付いた PR が何本かあれば、いちばん長く待たせているもの
    pub age: String,
    /// 番が付いてからの秒。待たせている順に並べるためのもの。番が無ければ 0
    pub waiting: i64,
    /// 番が付いたまま、3 日 動きが無い
    pub stale: bool,
    /// なぜこの状態か
    pub why: String,
    /// issue の本文。書かれたまま。issue でなければ空
    pub body: String,
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
    /// 番が付いてからの時間
    pub age: String,
    /// 番が付いたまま、3 日 動きが無い
    pub stale: bool,
    /// PR の本文。書かれたまま
    pub body: String,
    /// どのブランチから、どのブランチへ(`feat/x → main`)
    pub branch: String,
    /// 足した行・消した行・変えたファイルの数
    pub size: (u64, u64, u64),
    /// 確認を頼まれている人
    pub reviewers: Vec<String>,
    /// 落ちているチェックの名前
    pub failing: Vec<String>,
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
            Filter::All => "All",
            Filter::Mine => "Your turn",
            Filter::Others => "Their turn",
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
/// 動きが無いと印を付ける長さ(3 日)
const STALE: i64 = 3 * 86400;
pub const LOOSE: &str = "Unlinked PRs";
/// 終わったタスクの理由の欄
pub const CLOSED: &str = "Closed";
pub const LOCAL: &str = "Local work (no PR yet)";

/// `only` が空でなければ、その人たちが出した PR と、その人たちのタスクだけを出す。
/// ほかの人の PR でも、その人たちが GitHub で確認を頼まれていれば出す(番が来ているものを隠さない)。
/// `works` は、この repo の、この PC の上の worktree。
pub fn build(
    f: &RepoFacts,
    project: &Project,
    only: &[String],
    works: &[Work],
    extra: &[FileTask],
    now: i64,
) -> RepoView {
    let wanted = |who: &str| only.is_empty() || only.iter().any(|o| o == who);
    // その PR を、出す人たちが出したか、出す人たちが確認を頼まれているか。
    let wanted_pr =
        |p: &PrFacts| wanted(&p.author) || p.requested_reviewers.iter().any(|r| wanted(r));
    // 確認する人が決まらないときに回す先。部屋ならそのメンバー、部屋でなければ repo に書ける人。
    let members = if project.members.is_empty() {
        f.people.clone()
    } else {
        project.members.clone()
    };
    let settings = Settings {
        members,
        plan: project.plan.clone(),
        implement: project.implement.clone(),
        review: project.review.clone(),
        merge: project.merge.clone(),
        deploy: project.deploy.clone(),
        merged_is_shipped: f.no_deployments,
    };
    let verdicts: Vec<PrVerdict> = f.prs.iter().map(|p| judge_pr(p, &settings)).collect();
    let links = link_all(&f.prs);
    // 番が付いてからの時間・秒・3 日 動きが無いか。番の付いた PR のうち、いちばん長く待たせているもので決める。
    // 番の付いた PR が無ければ、最後に動いてからの時間だけを出す
    let clock = |idx: &[usize]| -> (String, i64, bool) {
        let newest = idx.iter().map(|i| f.pr_updated[*i].as_str()).max();
        let since = idx
            .iter()
            .filter(|i| !verdicts[**i].turn.is_empty())
            .map(|i| match verdicts[*i].since.as_str() {
                "" => f.pr_updated[*i].as_str(),
                s => s,
            })
            .min();
        match (since.and_then(epoch), newest.and_then(epoch)) {
            (Some(t), Some(moved)) => (
                age(since.unwrap_or(""), now),
                (now - t).max(0),
                now - moved >= STALE,
            ),
            _ => (age(newest.unwrap_or(""), now), 0, false),
        }
    };
    let line = |i: usize| {
        let (age, _, stale) = clock(&[i]);
        PrLine {
            mark: mark(verdicts[i].state),
            number: f.prs[i].number,
            title: f.prs[i].title.clone(),
            author: f.prs[i].author.clone(),
            why: why_pr(verdicts[i].reason, &f.pr_failing[i]),
            turn: verdicts[i].turn.clone(),
            age,
            stale,
            body: f.prs[i].body.clone(),
            branch: format!("{} → {}", f.prs[i].head_ref, f.prs[i].base_ref),
            size: f.pr_size.get(i).copied().unwrap_or_default(),
            reviewers: f.prs[i].requested_reviewers.clone(),
            failing: f.pr_failing[i].clone(),
        }
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

    let mut closed: Vec<Row> = Vec::new();
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
        let t = judge_task_in(issue, &vs, has_branch, &settings);
        // 起票した人・担当・つながる PR の作者か確認を頼まれた人の、どれかが当たれば出す。
        let related = wanted(&issue.author)
            || issue.assignees.iter().any(|a| wanted(a))
            || idx.iter().any(|i| wanted_pr(&f.prs[*i]));
        if !related {
            continue;
        }
        // 完了は、今の一覧には出さず、終わったものの一覧に入れる。
        // つながる PR は「入った」ことにしない(今までどおり、まだ出すものは下で出る)
        let g = ORDER.iter().position(|s| *s == t.state);
        if g.is_some() {
            idx.iter().for_each(|i| claimed[*i] = true);
        }
        let (age, waiting, stale) = clock(&idx);
        let into = match g {
            Some(g) => &mut groups[g].rows,
            None => &mut closed,
        };
        into.push(Row {
            kind: Kind::Task,
            stage: Stage::of_task(t.state, &vs).unwrap_or(Stage::Todo),
            number: issue.number,
            label: format!("#{}", issue.number),
            title: issue.title.clone(),
            url: format!("https://github.com/{}/issues/{}", f.repo, issue.number),
            turn: t.turn,
            age,
            waiting,
            stale,
            why: if g.is_none() {
                CLOSED.to_string()
            } else if t.mismatch {
                format!("{} · issue closed, PR still open", task_label(t.state))
            } else {
                task_label(t.state).to_string()
            },
            body: issue.body.clone(),
            prs: idx.iter().map(|i| line(*i)).collect(),
            local,
        });
    }

    // ファイルに書かれたタスク(差し込み)。issue と同じ規則で、つながる PR の事実から状態を決める。
    for t in extra {
        let idx: Vec<usize> = t.prs.iter().copied().filter(|i| *i < f.prs.len()).collect();
        let vs: Vec<PrVerdict> = idx.iter().map(|i| verdicts[*i].clone()).collect();
        let is_open = t
            .open
            .unwrap_or_else(|| idx.iter().any(|i| f.prs[*i].lifecycle == Lifecycle::Open));
        let local = local_for(&|b| {
            idx.iter().any(|i| open_branch(*i) == Some(b)) || t.branches.iter().any(|x| x == b)
        });
        let as_issue = IssueFacts {
            repo: f.repo.clone(),
            number: 0,
            title: t.title.clone(),
            is_open,
            author: String::new(),
            assignees: vec![],
            body: t.body.clone(),
        };
        let v = judge_task_in(
            &as_issue,
            &vs,
            !local.is_empty() || !t.branches.is_empty(),
            &settings,
        );
        // PR がまだ無いものは誰のものでもないので出す。PR があれば、PR で絞る
        if !idx.is_empty() && !idx.iter().any(|i| wanted_pr(&f.prs[*i])) {
            continue;
        }
        let g = ORDER.iter().position(|s| *s == v.state);
        if g.is_some() {
            idx.iter().for_each(|i| claimed[*i] = true);
        }
        let (age, waiting, stale) = clock(&idx);
        let file = t.path.rsplit('/').next().unwrap_or("");
        let into = match g {
            Some(g) => &mut groups[g].rows,
            None => &mut closed,
        };
        into.push(Row {
            kind: Kind::Doc,
            stage: Stage::of_task(v.state, &vs).unwrap_or(Stage::Todo),
            number: 0,
            label: t.label.clone(),
            title: t.title.clone(),
            url: if t.path.is_empty() {
                String::new()
            } else {
                format!("https://github.com/{}/blob/HEAD/{}", f.repo, t.path)
            },
            turn: v.turn,
            age,
            waiting,
            stale,
            why: match v.state {
                TaskState::Done => CLOSED.to_string(),
                TaskState::ShippedOpen if file.is_empty() => "Deployed".to_string(),
                TaskState::ShippedOpen => format!("Deployed · still open in {file}"),
                TaskState::Doing if vs.is_empty() && !t.branches.is_empty() => {
                    format!("In progress · {} · no PR yet", t.branches.join(", "))
                }
                s => task_label(s).to_string(),
            },
            body: t.body.clone(),
            prs: idx.iter().map(|i| line(*i)).collect(),
            local,
        });
    }

    // タスクにつながっていない PR。本番に出たもの・取り下げたものは出さない。
    let mut loose = Vec::new();
    for (i, p) in f.prs.iter().enumerate() {
        let v = &verdicts[i];
        if claimed[i] || !wanted_pr(p) || matches!(v.state, PrState::Shipped | PrState::Withdrawn) {
            continue;
        }
        loose.push(Row {
            kind: Kind::Pr,
            stage: Stage::of_pr(v.state).unwrap_or(Stage::Doing),
            number: p.number,
            label: format!("{}#{}", mark(v.state), p.number),
            title: p.title.clone(),
            url: format!("https://github.com/{}/pull/{}", f.repo, p.number),
            turn: v.turn.clone(),
            age: clock(&[i]).0,
            waiting: clock(&[i]).1,
            stale: clock(&[i]).2,
            why: why_pr(v.reason, &f.pr_failing[i]),
            body: String::new(),
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
            kind: Kind::Local,
            stage: Stage::Doing,
            number: 0,
            label: "Local".into(),
            title: w.branch.clone(),
            url: String::new(),
            turn: vec![f.viewer.clone()],
            age: String::new(),
            waiting: 0,
            stale: false,
            why: "Working locally · no PR yet".into(),
            body: String::new(),
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
            .then(|| {
                crate::lang::t(
                    "本番デプロイの記録が無いので、取り込みを本番として扱っています",
                    "There is no record of production deploys, so a merge counts as deployed",
                )
                .to_string()
            })
            .into_iter()
            .chain(f.notes.iter().map(|c| c.text()))
            .collect(),
        groups,
        closed,
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

/// 画面に出す言葉は、GitHub が使っている言葉にそろえる(Todo・In progress・In review・Merged …)。
fn task_label(s: TaskState) -> &'static str {
    match s {
        TaskState::Todo => "Todo",
        TaskState::Doing => "In progress",
        TaskState::InReview => "In review",
        TaskState::AwaitingShip => "Merged",
        TaskState::ShippedOpen => "Deployed · issue open",
        TaskState::Done => "Done",
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
        Reason::Shipped => "Deployed".into(),
        Reason::MergedNotShipped => "Merged · not deployed".into(),
        Reason::Withdrawn => "Closed".into(),
        Reason::Draft => "Draft".into(),
        Reason::Conflict => "Merge conflict".into(),
        Reason::ChecksFailing(_) => format!("Checks failing ({})", failing.join(", ")),
        Reason::ChangesRequested => "Changes requested".into(),
        Reason::UnansweredComment => "Unresolved comment".into(),
        Reason::ChecksRunning => "Checks pending".into(),
        Reason::Approved => "Approved".into(),
        Reason::AwaitingReview => "Review required".into(),
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
            times: Times::default(),
        }
    }

    #[test]
    fn file_tasks_follow_the_same_rules_as_issues() {
        let task = |label: &str, open: Option<bool>, prs: &[usize]| FileTask {
            label: label.into(),
            title: format!("{label} title"),
            open,
            path: if open.is_some() {
                "docs/plan.md".into()
            } else {
                String::new()
            },
            body: String::new(),
            prs: prs.to_vec(),
            branches: vec![],
        };
        let mut pushed = task("T-5", Some(true), &[]); // PR は無いが、ブランチは push 済み
        pushed.branches = vec!["feat/t5-start".into()];
        let extra = [
            task("T-1", Some(true), &[1]), // 落ちているチェックの PR #21
            task("T-4", Some(true), &[]),  // PR がまだ無い
            task("T-2", None, &[2]),       // 番号だけ。PR は本番に出た
            task("T-3", Some(false), &[]), // 済んだ
            pushed,
        ];
        let project = Project {
            name: "p".into(),
            repos: vec!["o/r".into()],
            ..Project::default()
        };
        let v = build(&sample_facts(&[]), &project, &[], &[], &extra, 1795005296);
        let rows: Vec<&Row> = v.groups.iter().flat_map(|g| &g.rows).collect();
        let doc = |l: &str| rows.iter().find(|r| r.kind == Kind::Doc && r.label == l);
        assert_eq!(doc("T-1").map(|r| r.stage), Some(Stage::NeedsFix));
        assert_eq!(
            doc("T-1").unwrap().url,
            "https://github.com/o/r/blob/HEAD/docs/plan.md"
        );
        assert_eq!(doc("T-4").map(|r| r.stage), Some(Stage::Todo));
        let t5 = doc("T-5").unwrap();
        assert_eq!(t5.stage, Stage::Doing);
        assert_eq!(t5.why, "In progress · feat/t5-start · no PR yet");
        // 本番に出た番号だけのタスクと、済んだタスクは出さない
        assert!(doc("T-2").is_none() && doc("T-3").is_none());
        // ファイルのタスクに入った PR は、つながっていない PR の段に出ない
        assert!(!rows.iter().any(|r| r.kind == Kind::Pr && r.number == 21));
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
        let project = Project {
            name: "p".into(),
            repos: vec!["o/r".into()],
            ..Project::default()
        };
        build(
            &sample_facts(bodies),
            &project,
            &only,
            works,
            &[],
            1795005296,
        )
    }

    fn sample_facts(bodies: &[(u64, &str)]) -> RepoFacts {
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
        RepoFacts {
            repo: "o/r".into(),
            viewer: "bob".into(),
            pr_updated: vec!["2026-11-18T07:34:56Z".into(); n],
            pr_failing: vec![vec![], vec!["lint".into()], vec![], vec![]],
            pr_size: vec![(120, 30, 5), (0, 0, 0), (0, 0, 0), (0, 0, 0)],
            prs,
            issues: vec![
                IssueFacts {
                    repo: "o/r".into(),
                    number: 7,
                    title: "login".into(),
                    is_open: true,
                    author: "cat".into(),
                    assignees: vec![],
                    body: String::new(),
                },
                IssueFacts {
                    repo: "o/r".into(),
                    number: 8,
                    title: "later".into(),
                    is_open: true,
                    author: "bob".into(),
                    assignees: vec![],
                    body: String::new(),
                },
                IssueFacts {
                    repo: "o/r".into(),
                    number: 9,
                    title: "old".into(),
                    is_open: false,
                    author: "ann".into(),
                    assignees: vec![],
                    body: String::new(),
                },
            ],
            branches: vec![],
            no_deployments: false,
            people: vec!["ann".into(), "bob".into()],
            notes: vec![],
        }
    }

    fn rows<'a>(v: &'a RepoView, group: &str) -> Vec<&'a Row> {
        v.groups
            .iter()
            .filter(|g| g.name == group)
            .flat_map(|g| g.rows.iter())
            .collect()
    }

    #[test]
    fn age_counts_from_when_the_turn_began_and_marks_three_quiet_days() {
        let project = Project {
            name: "p".into(),
            repos: vec!["o/r".into()],
            ..Project::default()
        };
        // 今は 2026-11-18T12:34:56Z
        let mut f = sample_facts(&[]);
        // #20(確認待ち): 2 時間前にチェックが終わった。PR 自体は 5 時間前にも動いている
        f.prs[0].times.checked = "2026-11-18T10:34:56Z".into();
        // #21(チェックが落ちている): 4 日前に落ちて、それから動いていない
        f.prs[1].times.checked = "2026-11-14T12:34:56Z".into();
        f.pr_updated[1] = "2026-11-14T12:34:56Z".into();
        let v = build(&f, &project, &[], &[], &[], 1795005296);
        let all: Vec<&Row> = v.groups.iter().flat_map(|g| &g.rows).collect();
        let row = |n: u64| {
            *all.iter()
                .find(|r| r.prs.iter().any(|p| p.number == n))
                .unwrap()
        };
        assert_eq!((row(20).age.as_str(), row(20).stale), ("2h", false));
        assert_eq!(row(20).waiting, 2 * 3600);
        assert_eq!((row(21).age.as_str(), row(21).stale), ("4d", true));
        assert!(row(21).prs[0].stale);
        // 時刻が分からなければ、最後に動いたときから数える
        assert_eq!(row(23).age, "5h");
    }

    #[test]
    fn a_pull_request_carries_what_its_page_would_show() {
        let v = sample_refs(&[(20, "Closes #7\n\nWhy and how.")]);
        let all: Vec<&Row> = v.groups.iter().flat_map(|g| &g.rows).collect();
        let pr = |n: u64| {
            all.iter()
                .flat_map(|r| &r.prs)
                .find(|p| p.number == n)
                .unwrap()
        };
        assert_eq!(pr(20).body, "Closes #7\n\nWhy and how.");
        assert_eq!(pr(20).branch, "7-login → main");
        assert_eq!(pr(20).size, (120, 30, 5));
        assert_eq!(pr(21).failing, vec!["lint"]);
        assert!(pr(21).body.is_empty());
    }

    #[test]
    fn finished_tasks_are_kept_apart_from_the_open_ones() {
        let v = sample();
        // #9 は閉じていて、終わっていない PR が無い
        assert_eq!(v.closed.len(), 1);
        assert_eq!((v.closed[0].number, v.closed[0].why.as_str()), (9, CLOSED));
        assert!(v.groups.iter().flat_map(|g| &g.rows).all(|r| r.number != 9));
        // 出す人を絞っているときは、その人たちに関わるものだけ(#9 は ann が起票した)
        assert!(sample_only(&["bob"]).closed.is_empty());
        assert_eq!(sample_only(&["ann"]).closed.len(), 1);
    }

    #[test]
    fn tasks_are_grouped_by_state_and_carry_their_prs() {
        let v = sample();
        let review = rows(&v, "In review");
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
            rows(&v, "Todo")
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
        assert_eq!(loose[0].why, "Checks failing (lint)");
        // タスクにも PR にも、同じ段階の言葉が付く
        assert_eq!(
            loose
                .iter()
                .map(|r| (r.kind, r.stage, r.number))
                .collect::<Vec<_>>(),
            vec![
                (Kind::Pr, Stage::NeedsFix, 21),
                (Kind::Pr, Stage::Doing, 23)
            ]
        );
        let review = &rows(&v, "In review")[0];
        assert_eq!(
            (review.kind, review.stage, review.number),
            (Kind::Task, Stage::InReview, 7)
        );
        assert_eq!(rows(&v, "Todo")[0].stage, Stage::Todo);
        assert!(Stage::ALL.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(Stage::NeedsFix.label(), "Needs action");
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
        assert!(row.why.contains("issue closed, PR still open"));
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

    #[test]
    fn only_keeps_prs_that_ask_the_named_people_to_review() {
        // ann の PR #21 で bob が確認を頼まれている。bob の分だけを見ていても、#21 は隠さない
        let mut f = sample_facts(&[]);
        f.prs[1].checks = Checks::Passing;
        f.prs[1].requested_reviewers = vec!["bob".into()];
        let project = Project::default();
        let v = build(&f, &project, &["bob".to_string()], &[], &[], 1795005296);
        let labels: Vec<&str> = v
            .groups
            .iter()
            .flat_map(|g| g.rows.iter())
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(labels, vec!["#8", "?#21", "…#23"]);
        assert!(v
            .groups
            .iter()
            .flat_map(|g| g.rows.iter())
            .any(|r| r.label == "?#21" && r.is_mine("bob")));
    }

    #[test]
    fn review_falls_back_to_room_members_not_everyone_who_can_write() {
        // GitHub では ann・bob・cat・dan が書ける。部屋のメンバーは ann と cat だけ
        let mut f = sample_facts(&[]);
        f.people = vec!["ann".into(), "bob".into(), "cat".into(), "dan".into()];
        let turn = |project: &Project| {
            let v = build(&f, project, &[], &[], &[], 1795005296);
            v.groups
                .iter()
                .flat_map(|g| g.rows.iter())
                .find(|r| r.label == "#7")
                .unwrap()
                .turn
                .clone()
        };
        // 部屋でなければ、作者(ann)以外の書ける人
        assert_eq!(turn(&Project::default()), vec!["bob", "cat", "dan"]);
        let room = Project {
            members: vec!["ann".into(), "cat".into()],
            ..Project::default()
        };
        assert_eq!(turn(&room), vec!["cat"]);
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
            rows(&v, "In progress")
                .iter()
                .map(|r| (r.label.as_str(), r.local_mark()))
                .collect::<Vec<_>>(),
            vec![("#8", "◇")]
        );
        assert!(rows(&v, "Todo").is_empty());
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
            ("Local", "▶", vec!["bob".to_string()])
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
