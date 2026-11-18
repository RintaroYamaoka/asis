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
    /// いつ何が起きたか
    pub times: Times,
}

/// PR に起きたことの時刻(ISO 8601 の UTC。同じ形なので、字のまま比べられる)。分からなければ空
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Times {
    /// PR を出した
    pub created: String,
    /// 最後の push
    pub pushed: String,
    /// チェックが終わった(いちばん遅いもの)
    pub checked: String,
    /// 今の判定(承認・直しの依頼)になったレビュー
    pub reviewed: String,
    /// 作者がまだ返していない、人のコメントの最初の 1 つ
    pub commented: String,
    /// 取り込まれた
    pub merged: String,
}

#[derive(Debug, Clone)]
pub struct IssueFacts {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub is_open: bool,
    /// 起票した人
    pub author: String,
    pub assignees: Vec<String>,
    /// 本文。書かれたまま(型は求めない)。状態の判断には使わない
    pub body: String,
}

/// 役割の欄に書ける特別な名前。その PR を出した本人を指す。
pub const AUTHOR: &str = "author";

/// プロジェクトの決まり。役割は GitHub のアカウント名か [`AUTHOR`] の一覧で、空なら既定。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// その repo で動いている人
    pub members: Vec<String>,
    /// 確認する人。空なら、作者以外のメンバー
    pub review: Vec<String>,
    /// 承認のあと取り込む人。空なら作者
    pub merge: Vec<String>,
    /// 取り込みのあと本番に出す人。空なら作者
    pub deploy: Vec<String>,
    /// デプロイの記録が無いプロジェクトで、取り込みを本番として扱う
    pub merged_is_shipped: bool,
}
