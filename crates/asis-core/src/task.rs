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

    let mut turn: Vec<String> = live.iter().flat_map(|p| p.turn.iter().cloned()).collect();
    turn.sort();
    turn.dedup();

    TaskVerdict {
        state,
        turn,
        mismatch: !issue.is_open && unfinished,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(open: bool) -> IssueFacts {
        IssueFacts {
            repo: "o/r".into(),
            number: 12,
            title: "t".into(),
            is_open: open,
            assignees: vec![],
        }
    }
    fn v(state: PrState, turn: &[&str]) -> PrVerdict {
        PrVerdict {
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
