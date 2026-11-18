//! 手元の設定。
//!
//! どの repo を見るか・手元の clone の場所・部屋を置く DB の場所を持つ。
//! 人と共有する定義(repo・メンバー・役割)は、ここではなく部屋に置く。
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    pub name: String,
    pub repos: Vec<String>,
    /// 部屋のメンバー。確認する人が決まらないときに回す先。空なら、GitHub でその repo に書ける人
    pub members: Vec<String>,
    /// 役割。GitHub のアカウント名か `author` の一覧。空なら既定(asis-core の Settings を参照)
    pub review: Vec<String>,
    pub merge: Vec<String>,
    pub deploy: Vec<String>,
    /// 状態を決めるときに数えないチェックの名前
    pub ignore_checks: Vec<String>,
    /// 部屋から来た定義なら、その部屋の番号
    pub room: Option<String>,
}

/// 手元の設定
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Local {
    /// 見る repo。その repo 1 つを、既定の決まりで見る
    pub watch: Vec<String>,
    /// 手元の clone を探すフォルダ。直下にある、見ている repo の clone と、その worktree を読む
    pub roots: Vec<String>,
    /// repo ごとの絞り込み。その人たちの分だけを出す
    pub only: BTreeMap<String, Vec<String>>,
    /// 手元だけの定義(`[[project]]`)。部屋を使わずに、repo をまとめて役割を決める
    pub projects: Vec<Project>,
    /// 部屋を置く DB。書かなければ、部屋を使わない
    pub db: Option<Db>,
    /// 画面で使う 1 色(`#rrggbb`)。書かなければ既定の色
    pub accent: Option<String>,
    /// 説明の文の言葉。`ja`(既定)か `en`。大事な単語は、どちらでも英語
    pub lang: Option<String>,
}

/// 部屋を置く DB(Supabase)の場所と、公開用の鍵
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Db {
    pub url: String,
    pub key: String,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Roles {
    #[serde(default)]
    review: Vec<String>,
    #[serde(default)]
    merge: Vec<String>,
    #[serde(default)]
    deploy: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: Option<String>,
    #[serde(default)]
    repos: Vec<String>,
    #[serde(default)]
    roles: Roles,
    #[serde(default)]
    ignore_checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocal {
    #[serde(default)]
    watch: Vec<String>,
    #[serde(default)]
    roots: Vec<String>,
    #[serde(default)]
    only: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    project: Vec<RawProject>,
    db: Option<Db>,
    accent: Option<String>,
    lang: Option<String>,
}

fn check_repo(repo: &str) -> Result<(), String> {
    match repo.split_once('/') {
        Some((owner, name)) if !owner.is_empty() && !name.is_empty() && !name.contains('/') => {
            Ok(())
        }
        _ => Err(format!("`{repo}` は owner/name の形ではありません")),
    }
}

fn project(raw: RawProject, home: Option<&str>) -> Result<Project, String> {
    // 定義を置いた repo は、書かなくても含める。
    let mut repos: Vec<String> = home.into_iter().map(str::to_string).collect();
    for r in raw.repos {
        check_repo(&r)?;
        if !repos.contains(&r) {
            repos.push(r);
        }
    }
    let name = raw
        .name
        .or_else(|| home.map(str::to_string))
        .ok_or("name がありません")?;
    if repos.is_empty() {
        return Err(format!("プロジェクト `{name}` に repos がありません"));
    }
    Ok(Project {
        name,
        repos,
        members: Vec::new(),
        review: raw.roles.review,
        merge: raw.roles.merge,
        deploy: raw.roles.deploy,
        ignore_checks: raw.ignore_checks,
        room: None,
    })
}

/// その場で指定した repo。その repo 1 つを、既定の決まりで見る。
pub fn single(repo: &str) -> Project {
    Project {
        name: repo.to_string(),
        repos: vec![repo.to_string()],
        ..Project::default()
    }
}

pub fn parse_local(text: &str) -> Result<Local, String> {
    let raw: RawLocal = toml::from_str(text).map_err(|e| e.to_string())?;
    raw.watch.iter().try_for_each(|r| check_repo(r))?;
    let projects = raw
        .project
        .into_iter()
        .map(|p| project(p, None))
        .collect::<Result<_, _>>()?;
    raw.only.keys().try_for_each(|r| check_repo(r))?;
    let db = raw.db.map(|db| Db {
        url: db.url.trim_end_matches('/').to_string(),
        key: db.key,
    });
    if db
        .as_ref()
        .is_some_and(|db| !db.url.starts_with("https://"))
    {
        return Err("[db] の url は https:// で始めてください".into());
    }
    if let Some(c) = &raw.accent {
        if parse_color(c).is_none() {
            return Err(format!("accent は `#rrggbb` の形で書いてください: `{c}`"));
        }
    }
    if raw.lang.as_deref().is_some_and(|l| l != "ja" && l != "en") {
        return Err("lang は `ja` か `en` で書いてください".into());
    }
    Ok(Local {
        watch: raw.watch,
        roots: raw.roots,
        only: raw.only,
        projects,
        db,
        accent: raw.accent,
        lang: raw.lang,
    })
}

/// `#rrggbb` を、赤・緑・青の 3 つの数にする。
pub fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let part = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

/// `$XDG_CONFIG_HOME/asis/config.toml`、無ければ `~/.config/asis/config.toml`(Windows は `%APPDATA%`)。
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("asis").join("config.toml"))
}

fn read(path: &PathBuf) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// 設定ファイルが無ければ空。読めない・形が違うときはエラー。
pub fn load() -> Result<Local, String> {
    let Some(path) = path() else {
        return Ok(Local::default());
    };
    parse_local(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// `watch` の行だけを書き換えた設定ファイルの中身を返す。ほかの行(手元の定義やコメント)は触らない。
pub fn with_watch(text: &str, watch: &[String]) -> Result<String, String> {
    let is_watch = |line: &str| {
        line.trim_start()
            .strip_prefix("watch")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    if text
        .lines()
        .any(|l| is_watch(l) && !l.trim_end().ends_with(']'))
    {
        return Err("watch が複数行で書かれています。設定ファイルを手で直してください".into());
    }
    let rest: Vec<&str> = text.lines().filter(|l| !is_watch(l)).collect();
    let line = format!(
        "watch = [{}]",
        watch
            .iter()
            .map(|r| format!("\"{r}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let body = rest.join("\n");
    Ok(if body.trim().is_empty() {
        format!("{line}\n")
    } else {
        format!("{line}\n{}\n", body.trim_start_matches('\n').trim_end())
    })
}

/// `[only]` の表の、その repo の行だけを書き換えた中身を返す。`people` が空なら行を消す。
pub fn with_only(text: &str, repo: &str, people: &[String]) -> String {
    let key = format!("\"{repo}\"");
    let is_entry = |line: &str| {
        line.trim_start()
            .strip_prefix(&key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !is_entry(l))
        .map(str::to_string)
        .collect();
    if !people.is_empty() {
        let entry = format!(
            "{key} = [{}]",
            people
                .iter()
                .map(|p| format!("\"{p}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
        match lines.iter().position(|l| l.trim() == "[only]") {
            Some(at) => lines.insert(at + 1, entry),
            None => {
                // 表の見出しは、ほかの表より前に置く(`watch` の行のすぐ後)。
                let at = lines
                    .iter()
                    .position(|l| l.trim_start().starts_with('['))
                    .unwrap_or(lines.len());
                lines.splice(
                    at..at,
                    [String::new(), "[only]".to_string(), entry, String::new()],
                );
            }
        }
    }
    // 中身が無くなった表の見出しは消す。
    if let Some(at) = lines.iter().position(|l| l.trim() == "[only]") {
        let next = lines[at + 1..].iter().find(|l| !l.trim().is_empty());
        if next.is_none_or(|l| l.trim_start().starts_with('[')) {
            lines.remove(at);
        }
    }
    let mut out = lines.join("\n").trim_end().to_string();
    out.push('\n');
    out
}

fn is_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

fn is_key(line: &str, key: &str) -> bool {
    line.trim_start()
        .strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// 一番上の段(表の見出しより前)の `key = "value"` を書き換えた中身を返す。`None` なら行を消す。
/// ほかの行(表やコメント)は触らない。値は呼ぶ側で確かめておく。
pub fn with_value(text: &str, key: &str, value: Option<&str>) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let top = lines
        .iter()
        .position(|l| is_header(l))
        .unwrap_or(lines.len());
    let keep: Vec<String> = lines[..top]
        .iter()
        .filter(|l| !is_key(l, key))
        .cloned()
        .collect();
    let rest = lines.split_off(top);
    let mut out = keep;
    if let Some(v) = value {
        // 一番上の段の、最後の中身のある行のすぐ後に置く
        let at = out
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .map_or(0, |i| i + 1);
        out.insert(at, format!("{key} = \"{v}\""));
    }
    if !rest.is_empty() && out.last().is_some_and(|l| !l.trim().is_empty()) {
        out.push(String::new());
    }
    out.extend(rest);
    let body = out.join("\n");
    let body = body.trim_matches('\n');
    if body.is_empty() {
        String::new()
    } else {
        format!("{body}\n")
    }
}

/// `[db]` の表だけを書き換えた中身を返す。
pub fn with_db(text: &str, db: &Db) -> Result<String, String> {
    if !db.url.starts_with("https://") {
        return Err("DB の URL は https:// で始めてください".into());
    }
    let plain = |s: &str| !s.is_empty() && !s.contains(['"', '\\', '\n']);
    if !plain(&db.url) || !plain(&db.key) {
        return Err("DB の URL と鍵を、空にせず、記号の \" \\ を入れずに書いてください".into());
    }
    let mut out: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if is_header(line) {
            inside = line.trim() == "[db]";
        }
        if !inside {
            out.push(line);
        }
    }
    let body = out.join("\n");
    let body = body.trim_end();
    let table = format!(
        "[db]\nurl = \"{}\"\nkey = \"{}\"\n",
        db.url.trim_end_matches('/'),
        db.key
    );
    Ok(if body.is_empty() {
        table
    } else {
        format!("{body}\n\n{table}")
    })
}

/// 設定ファイルの中身を、読めることを確かめてから書く。
pub fn save(change: impl FnOnce(&str) -> Result<String, String>) -> Result<Local, String> {
    let path = path().ok_or("設定ファイルの置き場が分かりません(HOME がありません)")?;
    let next = change(&read(&path)?)?;
    let local = parse_local(&next).map_err(|e| {
        format!("書き換えた設定を読めません({e})。設定ファイルを手で直してください")
    })?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, next).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(local)
}

/// 見る repo を足す・外す。足すときは、その repo で誰の分を出すか(空なら全部)も書く。結果を返す。
pub fn edit_watch(repo: &str, add: bool, only: &[String]) -> Result<Local, String> {
    check_repo(repo)?;
    save(|text| {
        let mut watch = parse_local(text)?.watch;
        watch.retain(|r| r != repo);
        if add {
            watch.push(repo.to_string());
        }
        Ok(with_only(
            &with_watch(text, &watch)?,
            repo,
            if add { only } else { &[] },
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_config_has_watch_and_local_projects() {
        let l = parse_local(
            r#"
watch = ["o/a", "o/b"]

[[project]]
name = "mine"
repos = ["me/tool"]
[project.roles]
deploy = ["me"]
"#,
        )
        .unwrap();
        assert_eq!(l.watch, vec!["o/a", "o/b"]);
        assert_eq!(
            parse_local("roots = [\"~/src\"]").unwrap().roots,
            vec!["~/src"]
        );
        assert_eq!(
            (l.projects[0].name.as_str(), l.projects[0].deploy.clone()),
            ("mine", vec!["me".to_string()])
        );
        assert_eq!(parse_local("").unwrap(), Local::default());
    }

    #[test]
    fn accent_is_an_optional_hex_color() {
        assert_eq!(parse_local("").unwrap().accent, None);
        assert_eq!(
            parse_local("accent = \"#f2b63c\"")
                .unwrap()
                .accent
                .as_deref(),
            Some("#f2b63c")
        );
        assert_eq!(parse_color("#f2b63c"), Some([0xf2, 0xb6, 0x3c]));
        assert_eq!(parse_color("f2b63c"), None);
        assert_eq!(parse_color("#f2b63"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        assert!(parse_local("accent = \"orange\"").is_err());
        // 言葉は ja か en
        assert_eq!(
            parse_local("lang = \"en\"").unwrap().lang.as_deref(),
            Some("en")
        );
        assert!(parse_local("lang = \"fr\"").is_err());
    }

    #[test]
    fn db_is_optional_and_must_be_https() {
        assert_eq!(parse_local("watch = []").unwrap().db, None);
        let l = parse_local("[db]\nurl = \"https://x.example/\"\nkey = \"k\"").unwrap();
        assert_eq!(
            l.db,
            Some(Db {
                url: "https://x.example".into(),
                key: "k".into()
            })
        );
        assert!(parse_local("[db]\nurl = \"http://x.example\"\nkey = \"k\"").is_err());
        assert!(parse_local("[db]\nurl = \"https://x.example\"").is_err());
    }

    #[test]
    fn only_table_is_read_and_rewritten() {
        let w = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let l =
            parse_local("watch = [\"o/a\"]\n\n[only]\n\"o/a\" = [\"alice\", \"bob\"]\n").unwrap();
        assert_eq!(l.only.get("o/a"), Some(&w(&["alice", "bob"])));

        // 表が無ければ作る。手元の定義より前に置く
        let text = "watch = [\"o/a\"]\n[[project]]\nname = \"x\"\nrepos = [\"o/r\"]\n";
        let next = with_only(text, "o/a", &w(&["alice"]));
        let l = parse_local(&next).unwrap();
        assert_eq!(
            (l.only.get("o/a"), l.projects.len()),
            (Some(&w(&["alice"])), 1)
        );
        // 2 つ目を足す・1 つ目を書き換える・消す
        let next = with_only(&next, "o/b", &w(&["bob"]));
        let next = with_only(&next, "o/a", &w(&["cat"]));
        let l = parse_local(&next).unwrap();
        assert_eq!(
            (l.only.get("o/a"), l.only.get("o/b")),
            (Some(&w(&["cat"])), Some(&w(&["bob"])))
        );
        let next = with_only(&with_only(&next, "o/a", &[]), "o/b", &[]);
        let l = parse_local(&next).unwrap();
        assert!(l.only.is_empty());
        assert!(!next.contains("[only]"), "{next}");
        assert_eq!((l.watch, l.projects.len()), (w(&["o/a"]), 1));
        assert!(parse_local("[only]\nnope = [\"x\"]").is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_local("[[project]]\nname = \"x\"")
            .unwrap_err()
            .contains("repos"));
        assert!(parse_local("[[project]]\nrepos = [\"o/r\"]")
            .unwrap_err()
            .contains("name"));
        assert!(parse_local("watch = [\"nope\"]")
            .unwrap_err()
            .contains("owner/name"));
        assert!(parse_local("[[project]]\nname = \"x\"\nrepos = [\"a/b/c\"]").is_err());
        assert!(parse_local("typo = 1").is_err());
        assert!(parse_local(
            "[[project]]\nname = \"x\"\nrepos = [\"o/r\"]\n[project.roles]\nboss = [\"x\"]"
        )
        .is_err());
    }

    #[test]
    fn with_value_rewrites_a_top_line_and_keeps_tables() {
        let text = "watch = [\"o/a\"]\naccent = \"#000000\"\n\n[db]\nurl = \"https://x.example\"\nkey = \"k\"\n";
        let next = with_value(text, "accent", Some("#58e0bd"));
        let l = parse_local(&next).unwrap();
        assert_eq!(l.accent.as_deref(), Some("#58e0bd"));
        assert_eq!(l.db.unwrap().key, "k");
        assert_eq!(l.watch, vec!["o/a"]);
        // 無ければ、表より前に足す(表の中の同じ名前の行は触らない)
        let next = with_value(&next, "lang", Some("en"));
        assert!(
            next.find("lang").unwrap() < next.find("[db]").unwrap(),
            "{next}"
        );
        assert_eq!(parse_local(&next).unwrap().lang.as_deref(), Some("en"));
        let next = with_value(&next, "key", Some("x"));
        assert!(parse_local(&next).is_err());
        // 空の設定にも書ける・消せる
        assert_eq!(with_value("", "lang", Some("ja")), "lang = \"ja\"\n");
        let gone = with_value(text, "accent", None);
        assert_eq!(parse_local(&gone).unwrap().accent, None);
        assert!(gone.contains("[db]"));
    }

    #[test]
    fn with_db_replaces_only_the_db_table() {
        let text = "lang = \"en\"\n\n[db]\nurl = \"https://old.example\"\nkey = \"old\"\n\n[only]\n\"o/a\" = [\"bob\"]\n";
        let db = Db {
            url: "https://new.example".into(),
            key: "new".into(),
        };
        let next = with_db(text, &db).unwrap();
        let l = parse_local(&next).unwrap();
        assert_eq!(l.db, Some(db.clone()));
        assert_eq!(l.lang.as_deref(), Some("en"));
        assert_eq!(l.only.len(), 1);
        assert_eq!(next.matches("[db]").count(), 1, "{next}");
        assert_eq!(
            parse_local(&with_db("", &db).unwrap()).unwrap().db,
            Some(db)
        );
        let bad = |url: &str, key: &str| {
            with_db(
                "",
                &Db {
                    url: url.into(),
                    key: key.into(),
                },
            )
            .is_err()
        };
        assert!(bad("http://x.example", "k"));
        assert!(bad("https://x.example", ""));
        assert!(bad("https://x.example", "a\"b"));
    }

    #[test]
    fn with_watch_rewrites_only_that_line() {
        let w = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(with_watch("", &w(&["o/a"])).unwrap(), "watch = [\"o/a\"]\n");
        let text =
            "watch = [\"o/a\"]\n\n# 手元の定義\n[[project]]\nname = \"x\"\nrepos = [\"o/r\"]\n";
        let next = with_watch(text, &w(&["o/a", "o/b"])).unwrap();
        assert_eq!(next, "watch = [\"o/a\", \"o/b\"]\n# 手元の定義\n[[project]]\nname = \"x\"\nrepos = [\"o/r\"]\n");
        assert_eq!(parse_local(&next).unwrap().watch, w(&["o/a", "o/b"]));
        assert_eq!(parse_local(&next).unwrap().projects.len(), 1);
        assert_eq!(
            with_watch(&next, &[]).unwrap().lines().next(),
            Some("watch = []")
        );
        assert!(with_watch("watch = [\n  \"o/a\",\n]\n", &[]).is_err());
    }
}
