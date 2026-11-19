//! 何を出すか。描く前の並べ方(どの行を・どの列に・どの順で)

use super::*;

/// ボードの列。段階を 4 つにまとめる
pub const COLUMNS: [&str; 4] = ["Todo", "In progress", "In review", "Merged"];
/// PR のカードを並べる列。まだ PR が無いものは、列ではなく行で出す
pub const PR_COLUMNS: [&str; 3] = ["In progress", "In review", "Merged"];
/// リストで束を並べる順(人の手が要る順)
pub const LIST_ORDER: [usize; 4] = [2, 3, 1, 0];

pub fn column_of(stage: Stage) -> usize {
    match stage {
        Stage::Todo => 0,
        Stage::Doing | Stage::NeedsFix | Stage::Testing => 1,
        Stage::InReview | Stage::ReadyToMerge => 2,
        Stage::AwaitingShip | Stage::ShippedOpen => 3,
    }
}

pub type Item<'a> = (&'a RepoView, &'a Row);

/// 開いている部屋
pub fn section(app: &App) -> Option<&Section> {
    app.room.and_then(|i| app.sections.get(i))
}

/// 開いている部屋の、読めた repository
pub fn views(app: &App) -> Vec<&RepoView> {
    section(app)
        .map(|s| s.repos.iter().filter_map(|r| r.as_ref().ok()).collect())
        .unwrap_or_default()
}

/// 1 つの repository の、出す行
pub fn rows_of(view: &RepoView) -> Vec<Item<'_>> {
    view.groups
        .iter()
        .flat_map(|g| g.rows.iter().map(move |r| (view, r)))
        .filter(|(v, r)| r.shows(Filter::All, &v.viewer))
        .collect()
}

/// 開いている部屋の、全部の行(カードの数字・持っている件数を数える用。合わせたボードは作らない)
pub fn room_items(app: &App) -> Vec<Item<'_>> {
    views(app).into_iter().flat_map(rows_of).collect()
}

/// いまボードを出している repository。ボードのページでなければ None
pub fn board_repo(app: &App) -> Option<String> {
    match (&app.area, &app.page) {
        (Area::Room, Page::Repo(repo)) if views(app).iter().any(|v| v.repo == *repo) => {
            Some(repo.clone())
        }
        _ => None,
    }
}

/// いま出しているボードの行。ボードは repository 1 つにつき 1 つ
pub fn items(app: &App) -> Vec<Item<'_>> {
    let Some(repo) = board_repo(app) else {
        return Vec::new();
    };
    views(app)
        .into_iter()
        .filter(|v| v.repo == repo)
        .flat_map(rows_of)
        .collect()
}

/// 絞り込みは置かない(自分の番は、カードの枠の色で分かる)
pub fn shown(app: &App) -> Vec<Item<'_>> {
    items(app)
}

/// issue の段。人の手が要る順(In review → Merged → In progress → Todo)
pub fn lanes<'a>(shown: &[Item<'a>]) -> Vec<Item<'a>> {
    let mut out = Vec::new();
    for col in LIST_ORDER {
        out.extend(
            shown
                .iter()
                .filter(|(_, r)| r.kind != Kind::Pr && column_of(r.stage) == col),
        );
    }
    out
}

/// issue につながっていない PR。列の順
pub fn loose<'a>(shown: &[Item<'a>]) -> Vec<Item<'a>> {
    let mut out = Vec::new();
    for col in 0..4 {
        out.extend(
            shown
                .iter()
                .filter(|(_, r)| r.kind == Kind::Pr && column_of(r.stage) == col),
        );
    }
    out
}

/// PR 1 本が入る列。出さないもの(本番に出た・閉じた)は None
pub fn column_of_pr(mark: &str) -> Option<usize> {
    match mark {
        "…" | "!" | "~" => Some(1),
        "?" | "+" => Some(2),
        "↑" => Some(3),
        _ => None,
    }
}

/// PR 1 本が入る列(`PR_COLUMNS` の何番目か)
pub fn pr_column(mark: &str) -> Option<usize> {
    column_of_pr(mark).map(|c| c - 1)
}

/// 束の名前。まだ PR が無いものは、状態の名前ではなく、そのことを言う
pub fn state_name(column: usize) -> &'static str {
    match column {
        0 => "No PR yet",
        i => COLUMNS[i],
    }
}

/// 1 件の、今の状態の言葉(Board の左の一覧に出す)
pub fn state_word(stage: Stage) -> &'static str {
    state_name(column_of(stage))
}

/// 一覧の名前。issue だけなら Issues。文書に書いたタスクが 1 つでもあれば、issue ではないので Plans
pub fn list_name(kinds: impl Iterator<Item = Kind>) -> &'static str {
    if kinds.into_iter().any(|k| k == Kind::Doc) {
        "Plans"
    } else {
        "Issues"
    }
}

/// その段を開いているか。カードが無ければ、開くものが無い。
/// 人が開け閉めしていなければ、Board は開いた形・List はたたんだ形
pub fn opened(app: &App, key: &Key, has_cards: bool) -> bool {
    has_cards && fold_state(app.fold.get(key).copied(), app.look)
}

pub fn fold_state(by_hand: Option<bool>, look: Look) -> bool {
    by_hand.unwrap_or(look == Look::Board)
}

/// いまボードを出している repository の、終わったタスク
pub fn closed_items(app: &App) -> Vec<Item<'_>> {
    let Some(repo) = board_repo(app) else {
        return Vec::new();
    };
    views(app)
        .into_iter()
        .filter(|v| v.repo == repo)
        .flat_map(|v| v.closed.iter().map(move |r| (v, r)))
        .collect()
}

pub fn key_of(view: &RepoView, row: &Row) -> Key {
    (view.repo.clone(), row.kind, row.number, row.title.clone())
}

pub fn current(app: &App) -> Option<Item<'_>> {
    let sel = app.selected.as_ref()?;
    items(app)
        .into_iter()
        .chain(closed_items(app))
        .find(|(v, r)| key_of(v, r) == *sel)
}

/// 上下キーで、次の 1 件へ。issue の段 → つながっていない PR、の順
pub fn step(app: &mut App, by: i32) {
    let all = shown(app);
    let list: Vec<(Key, Option<u64>)> = lanes(&all)
        .iter()
        .map(|(v, r)| (key_of(v, r), None))
        .chain(
            loose(&all)
                .iter()
                .map(|(v, r)| (key_of(v, r), Some(r.number))),
        )
        .collect();
    if list.is_empty() {
        return;
    }
    let at = app
        .selected
        .as_ref()
        .and_then(|s| list.iter().position(|(k, _)| k == s));
    let next = match at {
        Some(i) => (i as i32 + by).clamp(0, list.len() as i32 - 1) as usize,
        None => 0,
    };
    app.selected = Some(list[next].0.clone());
    app.pr = list[next].1;
}

/// 同じ issue の中で、前の PR・次の PR へ
pub fn turn_pr(app: &mut App, by: i32) {
    let Some((_, r)) = current(app) else {
        return;
    };
    let numbers: Vec<u64> = r.prs.iter().map(|p| p.number).collect();
    let Some(at) = app.pr.and_then(|n| numbers.iter().position(|x| *x == n)) else {
        return;
    };
    let n = numbers.len() as i32;
    let next = numbers[((at as i32 + by).rem_euclid(n)) as usize];
    app.pr = Some(next);
    forget(app);
}

pub fn short(repo: &str) -> &str {
    repo.rsplit('/').next().unwrap_or(repo)
}

pub fn label_of(row: &Row) -> String {
    match row.kind {
        Kind::Task => format!("#{}", row.number),
        Kind::Doc => row.label.clone(),
        Kind::Pr => format!("PR {}", row.number),
        Kind::Local => "Local".to_string(),
    }
}

/// PR の札に出す言葉。GitHub が使っている言葉のまま
pub fn word_of(p: &asis::view::PrLine) -> String {
    match p.mark {
        "↑" => "Merged".to_string(),
        _ => p.why.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lane_starts_open_on_the_board_and_folded_in_the_list_until_someone_toggles_it() {
        assert!(fold_state(None, Look::Board));
        assert!(!fold_state(None, Look::List));
        // 人が開け閉めしたら、そちらに従う
        assert!(!fold_state(Some(false), Look::Board));
        assert!(fold_state(Some(true), Look::List));
    }

    #[test]
    fn pull_requests_sit_in_three_columns_and_the_rest_is_named_by_what_is_missing() {
        assert_eq!(pr_column("…"), Some(0));
        assert_eq!(pr_column("!"), Some(0));
        assert_eq!(pr_column("?"), Some(1));
        assert_eq!(pr_column("+"), Some(1));
        assert_eq!(pr_column("↑"), Some(2));
        // 本番に出た・閉じた PR は、列に置かない
        assert_eq!(pr_column("✓"), None);
        assert_eq!(PR_COLUMNS[pr_column("?").unwrap()], "In review");
        assert_eq!(state_word(Stage::Todo), "No PR yet");
        assert_eq!(state_word(Stage::Doing), "In progress");
        assert_eq!(state_word(Stage::InReview), "In review");
        assert_eq!(state_word(Stage::AwaitingShip), "Merged");
    }

    #[test]
    fn the_list_is_called_issues_unless_a_task_comes_from_a_document() {
        assert_eq!(list_name([Kind::Task, Kind::Task].into_iter()), "Issues");
        assert_eq!(list_name([].into_iter()), "Issues");
        assert_eq!(list_name([Kind::Task, Kind::Doc].into_iter()), "Plans");
    }
}
