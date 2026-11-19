//! asis — development, as is.
//! 判断は asis-core、事実を読むのは github、並べるのは view、部屋は room。
//! 画面(TUI・GUI)は、ここの `Loader` が返すものを描くだけ。
pub mod act;
pub mod config;
pub mod extra;
pub mod github;
pub mod host;
pub mod lang;
pub mod local;
pub mod log;
pub mod room;
pub mod skill;
pub mod tui;
pub mod update;
pub mod view;

use config::Project;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use view::{Filter, RepoView};

const HELP: &str = "asis — development, as is

使い方:
  asis                     見ると決めた repo を開く
  asis <owner/repo>...     その repo を開く
  asis add <owner/repo>    見る repo に足す(--only を付けると、その人の分だけ出す)
  asis remove <owner/repo> 見る repo から外す
  asis login               GitHub でログインする(部屋を使うとき)
  asis room                部屋(repo・メンバー・役割をまとめたもの)。`asis room help` で一覧

  --plain                  画面を開かず、文字で出す
  --mine                   自分の番のものだけ出す
  --only <名前,名前>       その人たちが出した PR と、その人たちのタスクだけ出す

repo・メンバー・役割をまとめて人と共有するときは、部屋を使います。
部屋が無くても、ログインしなくても、その repo 1 つを既定の決まりで見られます。

GitHub の公式コマンド gh のログインを使います。読んだものは保存しません。";

/// 何を見るか。
pub struct Source {
    /// 1 つずつ、既定の決まりで見る repo
    pub watch: Vec<String>,
    /// repo ごとの絞り込み。その人たちの分だけを出す
    pub only: std::collections::BTreeMap<String, Vec<String>>,
    /// 手元の clone を探すフォルダ
    pub roots: Vec<std::path::PathBuf>,
    /// 手元だけの定義と、入っている部屋
    pub local: Vec<Project>,
}

/// プロジェクト名と、その repo ごとの結果
pub struct Section {
    pub project: String,
    /// 部屋なら、その部屋の番号
    pub room: Option<String>,
    /// 部屋のメンバーと、その役割(Review・Merge・Deploy)。部屋でなければ空
    pub people: Vec<(String, String)>,
    pub repos: Vec<Result<RepoView, String>>,
}

/// コマンドとして動かす。
pub fn run() -> Result<(), String> {
    let mut repos = Vec::new();
    let (mut plain, mut filter, mut only) = (false, Filter::All, Vec::new());
    let raw: Vec<String> = host::args();
    if matches!(
        raw.first().map(String::as_str),
        Some("login" | "logout" | "room")
    ) {
        return room::cli(&raw);
    }
    // `asis local <owner/repo>...`: この PC の上の作業を JSON で出す。
    // Windows で開いた窓が、WSL の中の作業を知るために呼ぶ(人が打つためのものではない)
    if raw.first().map(String::as_str) == Some("local") {
        let roots: Vec<std::path::PathBuf> = config::load()
            .map(|l| l.roots)
            .unwrap_or_default()
            .iter()
            .map(|r| expand_home(r))
            .collect();
        let works = local::gather(&raw[1..], &roots);
        println!(
            "{}",
            serde_json::to_string(&works).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let mut args = raw.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "--plain" => plain = true,
            "--mine" => filter = Filter::Mine,
            "--only" => {
                let names = args
                    .next()
                    .ok_or(lang::t("--only に名前がありません", "--only needs names"))?;
                only.extend(
                    names
                        .split(',')
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                        .map(str::to_string),
                );
            }
            _ if a.starts_with('-') => {
                return Err(say!(
                    "知らない指定です: {a}\n\n{HELP}",
                    "Unknown option: {a}\n\n{HELP}"
                ))
            }
            _ => repos.push(a),
        }
    }
    if let Some(cmd @ ("add" | "remove")) = repos.first().map(String::as_str) {
        let [_, repo] = repos.as_slice() else {
            return Err(say!(
                "asis {cmd} <owner/repo> の形で指定してください",
                "Use the form asis {cmd} <owner/repo>"
            ));
        };
        let local = config::edit_watch(repo, cmd == "add", &only)?;
        if local.watch.is_empty() {
            println!("見る repo はありません");
        }
        for r in &local.watch {
            match local.only.get(r) {
                Some(people) => println!("{r}  (only {})", people.join(", ")),
                None => println!("{r}"),
            }
        }
        return Ok(());
    }
    let only_flag = only.clone();
    let source = if repos.is_empty() {
        default_source(&only_flag)?
    } else {
        let only = if only.is_empty() {
            Default::default()
        } else {
            repos.iter().map(|r| (r.clone(), only.clone())).collect()
        };
        // その場で repo を指定したときも、設定の探すフォルダは使う。
        let roots = config::load().map(|l| l.roots).unwrap_or_default();
        Source {
            watch: repos,
            only,
            roots: roots.iter().map(|r| expand_home(r)).collect(),
            local: vec![],
        }
    };
    if source.watch.is_empty() && source.local.is_empty() {
        println!(
            "{HELP}\n\nまだ見る repo がありません。`asis add <owner/repo>` で足してください。"
        );
        return Ok(());
    }

    if plain || !std::io::stdout().is_terminal() {
        let sections = Loader::new(source).load(true);
        print!("{}", plain_text(&sections, filter));
        let failed = sections
            .iter()
            .flat_map(|s| &s.repos)
            .filter_map(|r| r.as_ref().err())
            .inspect(|e| eprintln!("{e}"))
            .count();
        return if failed > 0 {
            Err(say!(
                "{failed} 個の repo を読めませんでした",
                "Could not read {failed} repos"
            ))
        } else {
            Ok(())
        };
    }
    tui::run(source, filter).map_err(|e| e.to_string())
}

/// 入っている部屋を読んだ結果
#[derive(Debug, Clone, PartialEq)]
pub enum Rooms {
    /// 部屋を置く DB が設定に無い
    NoDb,
    /// DB はあるが、ログインしていない
    SignedOut,
    Read(Vec<room::Room>),
    /// 読めなかった(理由)
    Failed(String),
}

/// 手元の設定にある DB から、入っている部屋を読む。
pub fn read_rooms(local: &config::Local) -> Rooms {
    let Some(db) = &local.db else {
        return Rooms::NoDb;
    };
    match room::Client::open(db) {
        Ok(None) => Rooms::SignedOut,
        Ok(Some(c)) => c.rooms().map_or_else(Rooms::Failed, Rooms::Read),
        Err(e) => Rooms::Failed(e),
    }
}

/// 手元の設定と、入っている部屋から、見るものを決める。
/// `only_flag` は、その場で指定した絞り込み。あれば、設定や部屋の絞り込みより優先する。
pub fn source_of(local: config::Local, rooms: &[room::Room], only_flag: &[String]) -> Source {
    // その場の --only は、設定の絞り込みより優先して全部の repo に当てる。
    let only = if only_flag.is_empty() {
        local.only
    } else {
        local
            .watch
            .iter()
            .map(|r| (r.clone(), only_flag.to_vec()))
            .collect()
    };
    let mut source = Source {
        watch: local.watch,
        only,
        roots: local.roots.iter().map(|r| expand_home(r)).collect(),
        local: local.projects,
    };
    add_rooms(&mut source, rooms, only_flag);
    source
}

/// 手元の設定と、入っている部屋から、見るものを決める。部屋を読めなくても、手元の設定の分は出す。
pub fn default_source(only_flag: &[String]) -> Result<Source, String> {
    let local = config::load()?;
    let rooms = match read_rooms(&local) {
        Rooms::Read(rooms) => rooms,
        Rooms::Failed(e) => {
            eprintln!(
                "{}",
                say!(
                    "部屋を読めませんでした: {e}",
                    "Could not read the Rooms: {e}"
                )
            );
            Vec::new()
        }
        Rooms::NoDb | Rooms::SignedOut => Vec::new(),
    };
    Ok(source_of(local, &rooms, only_flag))
}

/// 部屋のメンバーと、その役割を言葉にしたもの。役割が無い人は空。
pub fn people_of(p: &Project) -> Vec<(String, String)> {
    p.members
        .iter()
        .map(|m| {
            let roles: Vec<&str> = [
                (&p.review, "Review"),
                (&p.merge, "Merge"),
                (&p.deploy, "Deploy"),
            ]
            .iter()
            .filter(|(who, _)| who.contains(m))
            .map(|(_, word)| *word)
            .collect();
            (m.clone(), roles.join("・"))
        })
        .collect()
}

/// 部屋を、見るものに足す。repo がまだ無い部屋も足す。部屋の repo は部屋の決まりで見るので、手元の「見る repo」からは外す。
/// 出すのは部屋のメンバーの分だけ。その場の `--only` があれば、そちらを優先する。
fn add_rooms(source: &mut Source, rooms: &[room::Room], only_flag: &[String]) {
    for room in rooms {
        for repo in &room.repos {
            source.watch.retain(|w| w != repo);
            let people = if only_flag.is_empty() {
                room.people(repo)
            } else {
                only_flag.to_vec()
            };
            // 空は「全員」。絞り込みを置かない。
            source.only.remove(repo);
            if !people.is_empty() {
                source.only.insert(repo.clone(), people);
            }
        }
        source.local.push(room.project());
    }
}

/// `~/src` → ホームの下の場所
fn expand_home(path: &str) -> std::path::PathBuf {
    match (
        path.strip_prefix("~/"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    ) {
        (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
        _ => std::path::PathBuf::from(path),
    }
}

/// 何分かに 1 回は、変わったかどうかに関係なく全部 読み直す(取りこぼしに備える)。
const FULL_EVERY: Duration = Duration::from_secs(600);

/// 読んだときの印と、読んだ中身か、読めなかった理由
type Read<T> = (String, Result<T, String>);

/// 読んだものを覚えておき、変わった repo だけ読み直す。
/// GitHub に聞ける量には 1 時間あたりの上限があり、同じアカウントで動くほかの道具と分け合うので、
/// 毎回 全部は読まない。変わったかどうかは、全部の repo を 1 回の軽い問い合わせで聞く。
pub struct Loader {
    source: Source,
    /// repo の事実。キーは repo と数えないチェックの組。(読んだときの印, 事実か、読めなかった理由)
    facts: HashMap<String, Read<github::RepoFacts>>,
    last_full: Option<Instant>,
    /// 差し込まれた読み口(repo の中のファイルに書かれたタスク)。無ければ issue と PR だけ
    reader: Option<std::sync::Arc<dyn extra::Reader>>,
    /// 差し込みが読んだファイル。キーは repo。(main の commit, ファイルか、読めなかった理由)
    files: HashMap<String, Read<Vec<(String, String)>>>,
}

/// 覚えているものを捨てて読み直すか。
/// 覚えが無い・前に読めなかった・印が変わった・印を聞けなかった・チェックが動いていた、のどれかなら読み直す。
fn needs_fetch(force: bool, cached: Option<(&str, bool, bool)>, current: Option<&str>) -> bool {
    match (cached, current) {
        (Some((stamp, ok, running)), Some(now)) => force || !ok || running || stamp != now,
        _ => true,
    }
}

/// 読み直しの記録。環境変数 `ASIS_LOG` に書き出し先があるときだけ書く(調べるとき用)。
fn log(line: &str) {
    use std::io::Write;
    let Some(path) = std::env::var_os("ASIS_LOG") else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

fn facts_key(repo: &str, ignore_checks: &[String]) -> String {
    format!("{repo}\n{}", ignore_checks.join("\n"))
}

impl Loader {
    pub fn new(source: Source) -> Loader {
        Loader {
            source,
            facts: HashMap::new(),
            last_full: None,
            reader: None,
            files: HashMap::new(),
        }
    }

    /// 読み口を差し込む(`extra` を参照)
    pub fn with_reader(mut self, reader: Option<std::sync::Arc<dyn extra::Reader>>) -> Loader {
        self.reader = reader;
        self
    }

    /// 見るものを替える。読んだ事実は覚えたままにする(同じ repo は読み直さない)。
    pub fn set_source(&mut self, source: Source) {
        self.source = source;
    }

    /// 定義を読み、変わった repo を同時に読み、指定の順で返す。`force` なら全部 読み直す。
    pub fn load(&mut self, force: bool) -> Vec<Section> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let stopped = lang::stopped;
        let force = force || self.last_full.is_none_or(|t| t.elapsed() > FULL_EVERY);
        if force {
            self.last_full = Some(Instant::now());
        }

        // 1. 見るもの。その場で指定した repo は、その repo 1 つを既定の決まりで見る。部屋と手元の定義は、その決まりで見る。
        let singles: Vec<Project> = self
            .source
            .watch
            .iter()
            .map(|r| config::single(r))
            .collect();
        let projects: Vec<&Project> = singles.iter().chain(self.source.local.iter()).collect();

        // 変わったかどうかの印を、全部の repo について 1 回で聞く。
        let mut known: Vec<String> = Vec::new();
        for repo in projects.iter().flat_map(|p| p.repos.iter()) {
            if !known.contains(repo) {
                known.push(repo.clone());
            }
        }
        let stamps: HashMap<String, String> = github::stamps(&known)
            .map(|s| known.iter().cloned().zip(s).collect())
            .unwrap_or_default();

        // 2. repo の事実。変わったものだけ同時に読む。
        let mut wanted: Vec<(String, &str, &[String])> = Vec::new();
        for p in &projects {
            for repo in &p.repos {
                let key = facts_key(repo, &p.ignore_checks);
                let cached = self.facts.get(&key).map(|(stamp, f)| {
                    (
                        stamp.as_str(),
                        f.is_ok(),
                        f.as_ref().is_ok_and(|f| f.has_running_checks()),
                    )
                });
                if needs_fetch(force, cached, stamps.get(repo).map(String::as_str))
                    && !wanted.iter().any(|(k, _, _)| *k == key)
                {
                    wanted.push((key, repo.as_str(), p.ignore_checks.as_slice()));
                }
            }
        }
        log(&format!(
            "load force={force} known={} stamps={} facts_read=[{}]",
            known.len(),
            stamps.len(),
            wanted
                .iter()
                .map(|(_, repo, _)| *repo)
                .collect::<Vec<_>>()
                .join(" ")
        ));
        let fetched: Vec<Result<github::RepoFacts, String>> = std::thread::scope(|s| {
            let handles: Vec<_> = wanted
                .iter()
                .map(|(_, repo, ignore)| s.spawn(move || github::fetch(repo, ignore)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Err(stopped())))
                .collect()
        });
        let fresh: Vec<(String, Read<github::RepoFacts>)> = wanted
            .iter()
            .zip(fetched)
            .map(|((key, repo, _), f)| {
                (
                    key.clone(),
                    (stamps.get(*repo).cloned().unwrap_or_default(), f),
                )
            })
            .collect();
        drop(wanted);

        // 2b. 差し込みが読むファイル。main が動いた repo だけ読み直す。
        if let Some(reader) = self.reader.clone() {
            let stale: Vec<(String, String)> = known
                .iter()
                .filter_map(|repo| {
                    let oid = stamps.get(repo).and_then(|s| extra::main_oid(s))?;
                    let same = self
                        .files
                        .get(repo)
                        .is_some_and(|(at, f)| *at == oid && f.is_ok());
                    (!same).then(|| (repo.clone(), oid))
                })
                .collect();
            let read: Vec<Result<Vec<(String, String)>, String>> = std::thread::scope(|s| {
                let handles: Vec<_> = stale
                    .iter()
                    .map(|(repo, oid)| {
                        let r = reader.clone();
                        s.spawn(move || extra::fetch_files(repo, oid, &|p| r.wants(p)))
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap_or_else(|_| Err(stopped())))
                    .collect()
            });
            for ((repo, oid), f) in stale.into_iter().zip(read) {
                self.files.insert(repo, (oid, f));
            }
        }

        // 3. この PC の上の作業(worktree・ペイン)は、毎回 読む(GitHub には聞かない)。
        let repos: Vec<String> = projects
            .iter()
            .flat_map(|p| p.repos.iter().cloned())
            .collect();
        let works = local::gather(&repos, &self.source.roots);

        // 4. 並べる。
        let facts: HashMap<&str, &Result<github::RepoFacts, String>> = self
            .facts
            .iter()
            .map(|(k, (_, f))| (k.as_str(), f))
            .chain(fresh.iter().map(|(k, (_, f))| (k.as_str(), f)))
            .collect();
        let sections = projects
            .iter()
            .map(|p| Section {
                project: p.name.clone(),
                room: p.room.clone(),
                people: people_of(p),
                repos: p
                    .repos
                    .iter()
                    .map(
                        |repo| match facts.get(facts_key(repo, &p.ignore_checks).as_str()) {
                            Some(Ok(f)) => {
                                let only =
                                    self.source.only.get(repo).map(Vec::as_slice).unwrap_or(&[]);
                                let mine: Vec<local::Work> =
                                    works.iter().filter(|w| w.repo == *repo).cloned().collect();
                                // 差し込みの読み口があれば、ファイルに書かれたタスクを足す
                                let (tasks, note) = match (&self.reader, self.files.get(repo)) {
                                    (Some(r), Some((_, Ok(files)))) => {
                                        let titles: Vec<&str> =
                                            f.prs.iter().map(|p| p.title.as_str()).collect();
                                        let heads: Vec<&str> =
                                            f.prs.iter().map(|p| p.head_ref.as_str()).collect();
                                        let lone = extra::branches_without_pr(&f.branches, &heads);
                                        (r.tasks(files, &titles, &lone), None)
                                    }
                                    (Some(_), Some((_, Err(e)))) => (
                                        vec![],
                                        Some(say!(
                                            "docs を読めませんでした: {e}",
                                            "Could not read the docs: {e}"
                                        )),
                                    ),
                                    _ => (vec![], None),
                                };
                                let mut v = view::build(f, p, only, &mine, &tasks, now);
                                v.notes.extend(note);
                                Ok(v)
                            }
                            Some(Err(e)) => Err(e.clone()),
                            None => Err(stopped()),
                        },
                    )
                    .collect(),
            })
            .collect();
        drop(facts);
        drop(projects);
        self.facts.extend(fresh);
        sections
    }
}

fn plain_text(sections: &[Section], filter: Filter) -> String {
    let mut out = String::new();
    for v in sections.iter().flat_map(|s| &s.repos).flatten() {
        out.push_str(&format!("\n{}{}\n", v.repo, only_text(&v.only)));
        for note in &v.notes {
            out.push_str(&format!("  ({note})\n"));
        }
        let mut shown = 0;
        for g in &v.groups {
            let rows: Vec<_> = g
                .rows
                .iter()
                .filter(|r| r.shows(filter, &v.viewer))
                .collect();
            if rows.is_empty() {
                continue;
            }
            out.push_str(&format!("\n  {}  {}\n", g.name, rows.len()));
            for r in rows {
                shown += 1;
                let prs = if r.label.starts_with('#') {
                    r.prs
                        .iter()
                        .map(|p| format!("{}#{}", p.mark, p.number))
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    r.why.clone()
                };
                let local = if r.local.is_empty() {
                    String::new()
                } else {
                    format!("  {} 手元", r.local_mark())
                };
                out.push_str(&format!(
                    "    {:<7} {}  [{}] {}  {prs}{local}\n",
                    r.label,
                    clip(&r.title, 46),
                    turn_text(&r.turn, &v.viewer),
                    r.age
                ));
            }
        }
        if shown == 0 {
            out.push_str(if filter == Filter::Mine {
                "\n  Nothing is waiting on you\n"
            } else {
                "\n  動いているものはありません\n"
            });
        }
    }
    out
}

/// 絞り込んでいることを、repo の名前の横に出す。
pub fn only_text(only: &[String]) -> String {
    if only.is_empty() {
        String::new()
    } else {
        format!("  (only {})", only.join(", "))
    }
}

pub fn turn_text(turn: &[String], viewer: &str) -> String {
    match turn {
        [] => "-".to_string(),
        t if t.iter().any(|x| x == viewer) => "Your turn".to_string(),
        t => t.join(","),
    }
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).chain(['…']).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_carry_their_roles_in_words() {
        let p = Project {
            name: "team".into(),
            members: vec!["alice".into(), "bob".into()],
            review: vec!["alice".into()],
            deploy: vec!["alice".into(), "author".into()],
            ..Project::default()
        };
        assert_eq!(
            people_of(&p),
            vec![
                ("alice".to_string(), "Review・Deploy".to_string()),
                ("bob".to_string(), String::new())
            ]
        );
        assert!(people_of(&Project::default()).is_empty());
    }

    #[test]
    fn rooms_replace_watched_repos_and_show_only_members() {
        let rooms = room::parse_rooms(
            r#"[{"id":"1","name":"team","merge_by_author":false,"deploy_by_author":false,"ignore_checks":[],
              "room_repos":[{"repo":"o/a","everyone":false},{"repo":"o/all","everyone":true}],
              "room_members":[{"github_id":1,"login":"alice","is_admin":true,"review":true,"merge":true,"deploy":true}]},
             {"id":"2","name":"empty","merge_by_author":false,"deploy_by_author":false,"ignore_checks":[],
              "room_repos":[],"room_members":[]}]"#,
        )
        .unwrap();
        let mut source = Source {
            watch: vec!["o/a".into(), "o/b".into()],
            only: [
                ("o/a".to_string(), vec!["zed".to_string()]),
                ("o/all".to_string(), vec!["zed".to_string()]),
            ]
            .into(),
            roots: vec![],
            local: vec![],
        };
        add_rooms(&mut source, &rooms, &[]);
        assert_eq!(source.watch, vec!["o/b"]);
        // repo がまだ無い部屋も出す(作ったばかりの部屋が消えないように)
        assert_eq!(source.local.len(), 2);
        assert!(source.local[1].repos.is_empty());
        assert_eq!(source.local[0].review, vec!["alice"]);
        assert_eq!(source.only["o/a"], vec!["alice"]);
        // 「全員」にした repo は絞らない。手元の絞り込みが残っていても外す
        assert!(!source.only.contains_key("o/all"));
        add_rooms(&mut source, &rooms, &["bob".to_string()]);
        assert_eq!(source.only["o/a"], vec!["bob"]);
    }

    #[test]
    fn clip_keeps_short_and_cuts_long() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("あいうえおか", 5), "あいうえ…");
    }

    #[test]
    fn plain_text_lists_groups_and_respects_the_filter() {
        let sections = vec![Section {
            room: None,
            project: "p".into(),
            people: vec![],
            repos: vec![Ok(view::tests::sample()), Err("x".into())],
        }];
        let all = plain_text(&sections, Filter::All);
        assert!(all.contains("o/r\n"));
        assert_eq!(only_text(&[]), "");
        assert_eq!(only_text(&["a".into(), "b".into()]), "  (only a, b)");
        assert!(all.contains("In review  1"));
        assert!(all.contains("#7      login  [Your turn] 5h  ?#20"));
        assert!(all.contains("!#21    pr 21  [ann] 5h  Checks failing (lint)"));
        let mine = plain_text(&sections, Filter::Mine);
        assert!(mine.contains("#7 ") && !mine.contains("#21") && !mine.contains("#8"));
    }

    #[test]
    fn refetch_only_what_changed() {
        // 印が同じで、前に読めていて、チェックも止まっていれば、読み直さない
        assert!(!needs_fetch(false, Some(("a", true, false)), Some("a")));
        // 印が変わった
        assert!(needs_fetch(false, Some(("a", true, false)), Some("b")));
        // 前に読めなかった・チェックが動いていた(終わりは印に出ない)
        assert!(needs_fetch(false, Some(("a", false, false)), Some("a")));
        assert!(needs_fetch(false, Some(("a", true, true)), Some("a")));
        // 覚えが無い・印を聞けなかった・全部 読み直すとき
        assert!(needs_fetch(false, None, Some("a")));
        assert!(needs_fetch(false, Some(("a", true, false)), None));
        assert!(needs_fetch(true, Some(("a", true, false)), Some("a")));
        assert_ne!(facts_key("o/r", &[]), facts_key("o/r", &["x".to_string()]));
    }

    #[test]
    fn turn_text_forms() {
        assert_eq!(turn_text(&[], "me"), "-");
        assert_eq!(turn_text(&["a".into(), "me".into()], "me"), "Your turn");
        assert_eq!(turn_text(&["a".into(), "b".into()], "me"), "a,b");
    }
}
