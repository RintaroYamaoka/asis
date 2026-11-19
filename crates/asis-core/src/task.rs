//! タスクの状態。つながっている PR の束から決める。設計書 §6。
use crate::facts::*;
use crate::pr::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Todo,
    Doing,
    InReview,
    AwaitingShip,
    /// 全部 本番に出たが、issue が開いている
    ShippedOpen,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskVerdict {
    pub state: TaskState,
    pub turn: Vec<String>,
    /// issue は閉じているのに、終わっていない PR が残っている
    pub mismatch: bool,
}

pub fn judge_task(issue: &IssueFacts, prs: &[PrVerdict], has_branch: bool) -> TaskVerdict {
    use PrState::*;
    // 取り下げた PR は、無かったものとして扱う。
    let live: Vec<&PrVerdict> = prs.iter().filter(|p| p.state != Withdrawn).collect();
    let any = |states: &[PrState]| live.iter().any(|p| states.contains(&p.state));
    let unfinished = live.iter().any(|p| p.state != Shipped);

    let state = if !unfinished && !issue.is_open {
        TaskState::Done
    } else if live.is_empty() {
        if has_branch {
            TaskState::Doing
        } else {
            TaskState::Todo
        }
    } else if !unfinished {
        TaskState::ShippedOpen
    } else if any(&[Draft, NeedsFix, Testing]) {
        TaskState::Doing
    } else if any(&[InReview, ReadyToMerge]) {
        TaskState::InReview
    } else {
        TaskState::AwaitingShip
    };

    // PR が出ていれば PR の番。まだ無ければ、開いているタスクは担当の番。
    let mut turn: Vec<String> = if live.is_empty() && issue.is_open {
        issue.assignees.clone()
    } else {
        live.iter().flat_map(|p| p.turn.iter().cloned()).collect()
    };
    turn.sort();
    turn.dedup();

    TaskVerdict {
        state,
        turn,
        mismatch: !issue.is_open && unfinished,
    }
}

/// 役の人に番を回す。事実から番が決まらないときだけ(担当が居る・PR の番がある、を上書きしない)。
/// - まだ誰も始めていない(PR もブランチも担当も無い)→ 実装する人
/// - 全部 本番に出たのに issue が開いている → 計画する人(確かめて閉じる)
pub fn judge_task_in(
    issue: &IssueFacts,
    prs: &[PrVerdict],
    has_branch: bool,
    settings: &Settings,
) -> TaskVerdict {
    let mut verdict = judge_task(issue, prs, has_branch);
    if verdict.turn.is_empty() {
        verdict.turn = match verdict.state {
            TaskState::Todo => settings.implement.clone(),
            TaskState::ShippedOpen => settings.plan.clone(),
            _ => Vec::new(),
        };
        verdict.turn.sort();
        verdict.turn.dedup();
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_take_the_turn_only_when_the_facts_name_nobody() {
        let team = Settings {
            members: vec![],
            plan: vec!["pat".into()],
            implement: vec!["ivy".into(), "ian".into()],
            review: vec![],
            merge: vec![],
            deploy: vec![],
            merged_is_shipped: false,
        };
        let turn = |i: &IssueFacts, prs: &[PrVerdict], branch: bool| {
            judge_task_in(i, prs, branch, &team).turn
        };
        // まだ誰も始めていない → 実装する人
        assert_eq!(turn(&issue(true), &[], false), vec!["ian", "ivy"]);
        // 担当が居れば、担当のまま
        let mut mine = issue(true);
        mine.assignees = vec!["ann".into()];
        assert_eq!(turn(&mine, &[], false), vec!["ann"]);
        // ブランチがあれば、もう誰かが始めている。役の人に回さない
        assert!(turn(&issue(true), &[], true).is_empty());
        // 全部 本番に出たのに開いている → 計画する人
        assert_eq!(
            turn(&issue(true), &[v(PrState::Shipped, &[])], false),
            vec!["pat"]
        );
        // PR の番があれば、そのまま
        assert_eq!(
            turn(&issue(true), &[v(PrState::InReview, &["bob"])], false),
            vec!["bob"]
        );
        // 役の人が居なければ、今までどおり誰の番でもない
        let none = Settings {
            plan: vec![],
            implement: vec![],
            ..team.clone()
        };
        assert!(judge_task_in(&issue(true), &[], false, &none)
            .turn
            .is_empty());
    }

    fn issue(open: bool) -> IssueFacts {
        IssueFacts {
            repo: "o/r".into(),
            number: 12,
            title: "t".into(),
            is_open: open,
            author: String::new(),
            assignees: vec![],
            body: String::new(),
        }
    }
    fn v(state: PrState, turn: &[&str]) -> PrVerdict {
        PrVerdict {
            since: String::new(),
            state,
            turn: turn.iter().map(|s| s.to_string()).collect(),
            reason: Reason::Draft,
        }
    }
    fn st(open: bool, prs: &[PrVerdict], branch: bool) -> TaskState {
        judge_task(&issue(open), prs, branch).state
    }

    #[test]
    fn nothing_yet() {
        assert_eq!(st(true, &[], false), TaskState::Todo);
        assert_eq!(st(true, &[], true), TaskState::Doing);
    }

    #[test]
    fn a_task_without_prs_is_the_assignees() {
        let mut i = issue(true);
        i.assignees = vec!["cat".into(), "ann".into()];
        // PR がまだ無いタスクは、担当の番。担当が居なければ誰の番でもない
        assert_eq!(judge_task(&i, &[], false).turn, vec!["ann", "cat"]);
        assert_eq!(judge_task(&i, &[], true).turn, vec!["ann", "cat"]);
        assert_eq!(
            judge_task(&i, &[v(PrState::Withdrawn, &[])], false).turn,
            vec!["ann", "cat"]
        );
        assert!(judge_task(&issue(true), &[], false).turn.is_empty());
        // PR が出たら、PR の番
        assert_eq!(
            judge_task(&i, &[v(PrState::InReview, &["bob"])], false).turn,
            vec!["bob"]
        );
        // 閉じたタスクは誰の番でもない
        i.is_open = false;
        assert!(judge_task(&i, &[], false).turn.is_empty());
    }

    #[test]
    fn withdrawn_prs_do_not_count() {
        assert_eq!(
            st(true, &[v(PrState::Withdrawn, &[])], false),
            TaskState::Todo
        );
    }

    #[test]
    fn any_pr_being_worked_on_means_doing() {
        for s in [PrState::Draft, PrState::NeedsFix, PrState::Testing] {
            assert_eq!(
                st(
                    true,
                    &[
                        v(PrState::Shipped, &[]),
                        v(PrState::InReview, &["bob"]),
                        v(s, &["ann"])
                    ],
                    false
                ),
                TaskState::Doing
            );
        }
    }

    #[test]
    fn review_then_ship_then_done() {
        assert_eq!(
            st(
                true,
                &[
                    v(PrState::Shipped, &[]),
                    v(PrState::Merged, &["bob"]),
                    v(PrState::InReview, &["bob"])
                ],
                false
            ),
            TaskState::InReview
        );
        assert_eq!(
            st(true, &[v(PrState::ReadyToMerge, &["bob"])], false),
            TaskState::InReview
        );
        assert_eq!(
            st(
                true,
                &[v(PrState::Shipped, &[]), v(PrState::Merged, &["bob"])],
                false
            ),
            TaskState::AwaitingShip
        );
        assert_eq!(
            st(
                true,
                &[v(PrState::Shipped, &[]), v(PrState::Withdrawn, &[])],
                false
            ),
            TaskState::ShippedOpen
        );
        assert_eq!(
            st(false, &[v(PrState::Shipped, &[])], false),
            TaskState::Done
        );
    }

    #[test]
    fn closed_issue_without_prs_is_done() {
        assert_eq!(st(false, &[], false), TaskState::Done);
        assert_eq!(
            st(false, &[v(PrState::Withdrawn, &[])], true),
            TaskState::Done
        );
    }

    #[test]
    fn closed_issue_with_unfinished_prs_is_a_mismatch() {
        let t = judge_task(&issue(false), &[v(PrState::InReview, &["bob"])], false);
        assert_eq!(t.state, TaskState::InReview);
        assert!(t.mismatch);
        let t = judge_task(&issue(false), &[v(PrState::Shipped, &[])], false);
        assert!(!t.mismatch);
        let t = judge_task(&issue(true), &[v(PrState::InReview, &["bob"])], false);
        assert!(!t.mismatch);
    }

    #[test]
    fn turn_is_everyone_holding_a_pr_once() {
        let t = judge_task(
            &issue(true),
            &[
                v(PrState::InReview, &["bob", "cat"]),
                v(PrState::NeedsFix, &["ann"]),
                v(PrState::Merged, &["bob"]),
            ],
            false,
        );
        assert_eq!(t.turn, vec!["ann", "bob", "cat"]);
        assert!(judge_task(&issue(true), &[], false).turn.is_empty());
    }
}
