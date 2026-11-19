//! 何を出すか。描く前の並べ方(どの行を・どの列に・どの順で)

use super::*;

/// ボードの列。段階を 4 つにまとめる
pub const COLUMNS: [&str; 4] = ["Todo", "In progress", "In review", "Merged"];
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

pub fn key_of(view: &RepoView, row: &Row) -> Key {
    (view.repo.clone(), row.kind, row.number, row.title.clone())
}

pub fn current(app: &App) -> Option<Item<'_>> {
    let sel = app.selected.as_ref()?;
    items(app).into_iter().find(|(v, r)| key_of(v, r) == *sel)
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

/// いま番を持っている PR の題。issue が漠然としていても、何で止まっているかが分かる
pub fn now_of(row: &Row) -> String {
    if !matches!(row.kind, Kind::Task | Kind::Doc) {
        return row
            .local
            .first()
            .map(|l| l.branch.clone())
            .unwrap_or_default();
    }
    row.prs
        .iter()
        .find(|p| !p.turn.is_empty())
        .map(|p| p.title.clone())
        .unwrap_or_default()
}

/// PR の札に出す言葉。GitHub が使っている言葉のまま
pub fn word_of(p: &asis::view::PrLine) -> String {
    match p.mark {
        "↑" => "Merged".to_string(),
        _ => p.why.clone(),
    }
}
