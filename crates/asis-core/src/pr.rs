//! PR 1 本の状態と番。設計書 §5 の表を上から順に当てる。
use crate::facts::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Shipped,
    Merged,
    Withdrawn,
    Draft,
    NeedsFix,
    Testing,
    ReadyToMerge,
    InReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Shipped,
    MergedNotShipped,
    Withdrawn,
    Draft,
    Conflict,
    ChecksFailing(u32),
    ChangesRequested,
    UnansweredComment,
    ChecksRunning,
    Approved,
    AwaitingReview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrVerdict {
    pub state: PrState,
    /// 次に進められる人。誰の番でもなければ空
    pub turn: Vec<String>,
    pub reason: Reason,
    /// この状態になった時刻(番が付いた時刻)。分からなければ空
    pub since: String,
}

/// この理由の行に当たるようになった時刻。理由を作った出来事のうち、いちばん遅いもの。
/// ぶつかりは、いつ起きたかが分からないので、分かっている出来事のいちばん遅いものにする
fn since(pr: &PrFacts, reason: Reason) -> String {
    let t = &pr.times;
    let latest = |xs: &[&String]| xs.iter().max().map(|x| x.to_string()).unwrap_or_default();
    match reason {
        Reason::Shipped | Reason::Withdrawn => String::new(),
        Reason::MergedNotShipped => t.merged.clone(),
        Reason::Draft => t.created.clone(),
        Reason::Conflict => latest(&[&t.created, &t.pushed, &t.checked, &t.reviewed, &t.commented]),
        Reason::ChecksFailing(_) => latest(&[&t.pushed, &t.checked]),
        Reason::ChangesRequested => latest(&[&t.reviewed]),
        Reason::UnansweredComment => latest(&[&t.commented]),
        Reason::ChecksRunning => t.pushed.clone(),
        Reason::Approved => latest(&[&t.reviewed, &t.checked]),
        Reason::AwaitingReview => latest(&[&t.created, &t.pushed, &t.checked]),
    }
}

pub fn judge_pr(pr: &PrFacts, s: &Settings) -> PrVerdict {
    let author = || vec![pr.author.clone()];
    let (state, turn, reason) = match pr.lifecycle {
        Lifecycle::Merged if pr.shipped || s.merged_is_shipped => {
            (PrState::Shipped, vec![], Reason::Shipped)
        }
        Lifecycle::Merged => (
            PrState::Merged,
            named(&s.deploy, pr),
            Reason::MergedNotShipped,
        ),
        Lifecycle::Closed => (PrState::Withdrawn, vec![], Reason::Withdrawn),
        Lifecycle::Open => {
            if pr.is_draft {
                (PrState::Draft, author(), Reason::Draft)
            } else if pr.has_conflict {
                (PrState::NeedsFix, author(), Reason::Conflict)
            } else if let Checks::Failing(n) = pr.checks {
                (PrState::NeedsFix, author(), Reason::ChecksFailing(n))
            } else if pr.review == Review::ChangesRequested {
                (PrState::NeedsFix, author(), Reason::ChangesRequested)
            } else if pr.unanswered_comment {
                (PrState::NeedsFix, author(), Reason::UnansweredComment)
            } else if pr.checks == Checks::Pending {
                (PrState::Testing, vec![], Reason::ChecksRunning)
            } else if pr.review == Review::Approved {
                (PrState::ReadyToMerge, named(&s.merge, pr), Reason::Approved)
            } else {
                (PrState::InReview, reviewers(pr, s), Reason::AwaitingReview)
            }
        }
    };
    PrVerdict {
        state,
        turn,
        reason,
        since: since(pr, reason),
    }
}

/// 役割の欄を、その PR での人の一覧にする。空なら作者。`author` は作者に読み替える。
fn named(role: &[String], pr: &PrFacts) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in role {
        let who = if name == AUTHOR { &pr.author } else { name };
        if !out.contains(who) {
            out.push(who.clone());
        }
    }
    if out.is_empty() {
        out.push(pr.author.clone());
    }
    out
}

/// 確認する人。頼まれた人 → 決まりに書いた人 → 作者以外のメンバー。
/// 自分の PR は自分では確認しないので、作者は外す。外して誰も残らなければ作者
/// (1 人のプロジェクト、または確認する人に書かれた本人の PR)。
/// 決まりに確認する人が書いてあるときは、書かれていないメンバーへは回さない。
fn reviewers(pr: &PrFacts, s: &Settings) -> Vec<String> {
    if !pr.requested_reviewers.is_empty() {
        return pr.requested_reviewers.clone();
    }
    let pool = if s.review.is_empty() {
        &s.members
    } else {
        &s.review
    };
    let others: Vec<String> = pool
        .iter()
        .filter(|m| **m != pr.author && *m != AUTHOR)
        .cloned()
        .collect();
    if others.is_empty() {
        vec![pr.author.clone()]
    } else {
        others
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn pr() -> PrFacts {
        PrFacts {
            repo: "o/r".into(),
            number: 1,
            title: "t".into(),
            body: String::new(),
            author: "ann".into(),
            head_ref: "feature".into(),
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

    pub(crate) fn team() -> Settings {
        Settings {
            members: vec!["ann".into(), "bob".into()],
            plan: vec![],
            implement: vec![],
            review: vec![],
            merge: vec!["bob".into()],
            deploy: vec!["bob".into()],
            merged_is_shipped: false,
        }
    }

    fn j(p: PrFacts) -> PrVerdict {
        judge_pr(&p, &team())
    }

    #[test]
    fn since_is_the_latest_event_that_made_the_turn() {
        let mut p = pr();
        p.times = Times {
            created: "2026-01-01T00:00:00Z".into(),
            pushed: "2026-01-02T00:00:00Z".into(),
            checked: "2026-01-02T00:10:00Z".into(),
            reviewed: "2026-01-03T00:00:00Z".into(),
            commented: "2026-01-04T00:00:00Z".into(),
            merged: "2026-01-05T00:00:00Z".into(),
        };
        // 確認待ち: チェックが終わったとき(push と作成より後)
        assert_eq!(j(p.clone()).since, "2026-01-02T00:10:00Z");
        // 直しの依頼・承認: そのレビューのとき
        p.review = Review::ChangesRequested;
        assert_eq!(j(p.clone()).since, "2026-01-03T00:00:00Z");
        p.review = Review::None;
        // 返していないコメント: 最初のコメントのとき
        p.unanswered_comment = true;
        assert_eq!(j(p.clone()).since, "2026-01-04T00:00:00Z");
        p.unanswered_comment = false;
        // 下書き: PR を出したとき。取り込み済み: 取り込まれたとき
        p.is_draft = true;
        assert_eq!(j(p.clone()).since, "2026-01-01T00:00:00Z");
        p.lifecycle = Lifecycle::Merged;
        assert_eq!(j(p).since, "2026-01-05T00:00:00Z");
    }

    #[test]
    fn open_green_unreviewed_waits_for_the_other_member() {
        let v = j(pr());
        assert_eq!(v.state, PrState::InReview);
        assert_eq!(v.turn, vec!["bob"]);
        assert_eq!(v.reason, Reason::AwaitingReview);
    }

    #[test]
    fn requested_reviewer_wins_over_members() {
        let mut p = pr();
        p.requested_reviewers = vec!["cat".into()];
        assert_eq!(j(p).turn, vec!["cat"]);
    }

    #[test]
    fn solo_project_reviews_itself() {
        let mut s = team();
        s.members = vec!["ann".into()];
        assert_eq!(judge_pr(&pr(), &s).turn, vec!["ann"]);
        s.members = vec![];
        assert_eq!(judge_pr(&pr(), &s).turn, vec!["ann"]);
    }

    #[test]
    fn roles_can_name_people_or_the_author() {
        let mut s = team();
        s.members = vec!["ann".into(), "bob".into(), "cat".into()];
        s.review = vec!["cat".into()];
        s.merge = vec!["author".into(), "dan".into(), "ann".into()];
        s.deploy = vec!["dan".into()];
        assert_eq!(judge_pr(&pr(), &s).turn, vec!["cat"]);
        let mut p = pr();
        p.review = Review::Approved;
        assert_eq!(judge_pr(&p, &s).turn, vec!["ann", "dan"]);
        p.lifecycle = Lifecycle::Merged;
        assert_eq!(judge_pr(&p, &s).turn, vec!["dan"]);
    }

    #[test]
    fn the_author_does_not_review_their_own_pr() {
        let mut s = team();
        s.members = vec!["ann".into(), "bob".into(), "cat".into()];
        // 確認する人が何人か書いてあれば、作者を外した残りが確認する
        s.review = vec!["ann".into(), "cat".into()];
        assert_eq!(judge_pr(&pr(), &s).turn, vec!["cat"]);
        // 決まりが無ければ、作者以外のメンバー
        s.review = vec![];
        assert_eq!(judge_pr(&pr(), &s).turn, vec!["bob", "cat"]);
    }

    #[test]
    fn a_sole_named_reviewer_keeps_their_own_pr() {
        // 確認する人に書かれた本人が出した PR は、本人の番のまま。
        // 確認する人に書かれていないメンバーへは回さない。
        let mut s = team();
        s.review = vec!["ann".into()];
        assert_eq!(judge_pr(&pr(), &s).turn, vec!["ann"]);
        s.review = vec!["author".into()];
        assert_eq!(judge_pr(&pr(), &s).turn, vec!["ann"]);
    }

    #[test]
    fn draft_is_the_authors() {
        let mut p = pr();
        p.is_draft = true;
        let v = j(p);
        assert_eq!((v.state, v.reason), (PrState::Draft, Reason::Draft));
        assert_eq!(v.turn, vec!["ann"]);
    }

    #[test]
    fn conflict_failing_changes_and_comment_go_back_to_author() {
        type Change = fn(&mut PrFacts);
        let cases: Vec<(Change, Reason)> = vec![
            (|p| p.has_conflict = true, Reason::Conflict),
            (|p| p.checks = Checks::Failing(2), Reason::ChecksFailing(2)),
            (
                |p| p.review = Review::ChangesRequested,
                Reason::ChangesRequested,
            ),
            (|p| p.unanswered_comment = true, Reason::UnansweredComment),
        ];
        for (f, reason) in cases {
            let mut p = pr();
            f(&mut p);
            let v = j(p);
            assert_eq!(v.state, PrState::NeedsFix, "{reason:?}");
            assert_eq!(v.reason, reason);
            assert_eq!(v.turn, vec!["ann"]);
        }
    }

    #[test]
    fn running_checks_are_nobodys_turn() {
        let mut p = pr();
        p.checks = Checks::Pending;
        let v = j(p);
        assert_eq!(
            (v.state, v.reason),
            (PrState::Testing, Reason::ChecksRunning)
        );
        assert!(v.turn.is_empty());
    }

    #[test]
    fn approved_waits_for_the_merger() {
        let mut p = pr();
        p.review = Review::Approved;
        let v = j(p.clone());
        assert_eq!(
            (v.state, v.reason),
            (PrState::ReadyToMerge, Reason::Approved)
        );
        assert_eq!(v.turn, vec!["bob"]);
        let mut s = team();
        s.merge = vec![];
        assert_eq!(judge_pr(&p, &s).turn, vec!["ann"]);
    }

    #[test]
    fn no_checks_configured_counts_as_ready() {
        let mut p = pr();
        p.checks = Checks::None;
        assert_eq!(j(p).state, PrState::InReview);
    }

    #[test]
    fn merged_but_not_shipped_waits_for_the_deployer() {
        let mut p = pr();
        p.lifecycle = Lifecycle::Merged;
        let v = j(p.clone());
        assert_eq!(
            (v.state, v.reason),
            (PrState::Merged, Reason::MergedNotShipped)
        );
        assert_eq!(v.turn, vec!["bob"]);
        let mut s = team();
        s.merged_is_shipped = true;
        let v = judge_pr(&p, &s);
        assert_eq!(v.state, PrState::Shipped);
        assert!(v.turn.is_empty());
    }

    #[test]
    fn shipped_and_withdrawn_are_finished() {
        let mut p = pr();
        p.lifecycle = Lifecycle::Merged;
        p.shipped = true;
        let v = j(p);
        assert_eq!(v.state, PrState::Shipped);
        assert!(v.turn.is_empty());
        let mut p = pr();
        p.lifecycle = Lifecycle::Closed;
        let v = j(p);
        assert_eq!((v.state, v.reason), (PrState::Withdrawn, Reason::Withdrawn));
        assert!(v.turn.is_empty());
    }

    // 表の行の順番。上の行が下の行に勝つ。
    #[test]
    fn row_order() {
        // 取り込み済みは、残っている指摘やテストの赤より先
        let mut p = pr();
        p.lifecycle = Lifecycle::Merged;
        p.checks = Checks::Failing(1);
        p.review = Review::ChangesRequested;
        assert_eq!(j(p).state, PrState::Merged);
        // 下書きは、ぶつかりや赤より先
        let mut p = pr();
        p.is_draft = true;
        p.has_conflict = true;
        p.checks = Checks::Failing(1);
        assert_eq!(j(p).reason, Reason::Draft);
        // ぶつかり > テストの赤 > 直しの依頼
        let mut p = pr();
        p.has_conflict = true;
        p.checks = Checks::Failing(1);
        assert_eq!(j(p).reason, Reason::Conflict);
        let mut p = pr();
        p.checks = Checks::Failing(1);
        p.review = Review::ChangesRequested;
        assert_eq!(j(p).reason, Reason::ChecksFailing(1));
        // 直しの依頼は、テスト中より先(テストが終わっても作者の番のまま)
        let mut p = pr();
        p.review = Review::ChangesRequested;
        p.checks = Checks::Pending;
        assert_eq!(j(p).state, PrState::NeedsFix);
        // テスト中は、承認より先(通るまで取り込めない)
        let mut p = pr();
        p.review = Review::Approved;
        p.checks = Checks::Pending;
        assert_eq!(j(p).state, PrState::Testing);
        // 承認済みでも、返していないコメントがあれば作者の番
        let mut p = pr();
        p.review = Review::Approved;
        p.unanswered_comment = true;
        assert_eq!(j(p).state, PrState::NeedsFix);
    }
}
