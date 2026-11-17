//! 入力の形。GitHub から読んだ事実をそのまま持つ。判断は入れない。

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IssueRef {
    /// `owner/name`
    pub repo: String,
    pub number: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    /// テストの設定が無い
    None,
    Pending,
    Failing(u32),
    Passing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Review {
    None,
    Approved,
    ChangesRequested,
}

#[derive(Debug, Clone)]
pub struct PrFacts {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub head_ref: String,
    pub base_ref: String,
    pub lifecycle: Lifecycle,
    pub is_draft: bool,
    pub has_conflict: bool,
    pub checks: Checks,
    pub review: Review,
    pub requested_reviewers: Vec<String>,
    /// 最後の push のあとに、作者以外の人のコメントがあり、作者がまだ返していない
    pub unanswered_comment: bool,
    /// 取り込みの commit が、成功した最新の本番デプロイに含まれている
    pub shipped: bool,
    /// GitHub が持つ PR と issue のリンク
    pub closes: Vec<IssueRef>,
}

#[derive(Debug, Clone)]
pub struct IssueFacts {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub is_open: bool,
    pub assignees: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Author,
    Reviewer,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub members: Vec<String>,
    pub merger: Role,
    pub deployer: Role,
    /// デプロイの記録が無いプロジェクトで、取り込みを本番として扱う
    pub merged_is_shipped: bool,
}
