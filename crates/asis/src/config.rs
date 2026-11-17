//! プロジェクトの定義と、手元の設定。
//!
//! プロジェクトの定義(まとめる repo・役割・数えないチェック)は、repo の中の `.asis.toml` に置く。
//! 置いた repo を見られる人は、同じ定義を読む。手元の設定には「どの repo を見るか」だけを持つ。
use serde::Deserialize;
use std::path::PathBuf;

/// repo の中に置く定義ファイルの名前
pub const REPO_FILE: &str = ".asis.toml";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    pub name: String,
    pub repos: Vec<String>,
    /// 役割。GitHub のアカウント名か `author` の一覧。空なら既定(asis-core の Settings を参照)
    pub review: Vec<String>,
    pub merge: Vec<String>,
    pub deploy: Vec<String>,
    /// 状態を決めるときに数えないチェックの名前
    pub ignore_checks: Vec<String>,
}

/// 手元の設定
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Local {
    /// 見る repo。定義はその repo の `.asis.toml`
    pub watch: Vec<String>,
    /// repo にファイルを置かない場合の、手元だけの定義
    pub projects: Vec<Project>,
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
    project: Vec<RawProject>,
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
        review: raw.roles.review,
        merge: raw.roles.merge,
        deploy: raw.roles.deploy,
        ignore_checks: raw.ignore_checks,
    })
}

/// repo の中の `.asis.toml`。`home` はそれを置いた repo。
pub fn parse_repo_file(text: &str, home: &str) -> Result<Project, String> {
    let raw: RawProject = toml::from_str(text).map_err(|e| e.to_string())?;
    project(raw, Some(home))
}

/// 定義ファイルが無い repo。その repo 1 つを、既定の決まりで見る。
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
    Ok(Local {
        watch: raw.watch,
        projects,
    })
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

/// 見る repo を足す・外す。結果の一覧を返す。
pub fn edit_watch(repo: &str, add: bool) -> Result<Vec<String>, String> {
    check_repo(repo)?;
    let path = path().ok_or("設定ファイルの置き場が分かりません(HOME がありません)")?;
    let text = read(&path)?;
    let mut watch = parse_local(&text)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .watch;
    watch.retain(|r| r != repo);
    if add {
        watch.push(repo.to_string());
    }
    let next = with_watch(&text, &watch)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, next).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(watch)
}

/// repo に置く定義ファイルのひな形
pub const TEMPLATE: &str = r#"# asis のプロジェクトの定義。この repo を見られる人は、同じ定義を読みます。

# 画面に出す名前(省くと repo の名前)
# name = "shop"

# この repo と一緒に 1 つのプロジェクトとして見る repo
# repos = ["owner/api"]

# 状態を決めるときに数えないチェックの名前(落ちていても取り込みを止めないもの)
# ignore_checks = ["Preview"]

# 役割。GitHub のアカウント名か "author"(その PR を出した本人)。
# 省くと、確認は作者以外のメンバー、取り込みと本番は作者。
# [roles]
# review = ["alice"]
# merge = ["alice"]
# deploy = ["alice"]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_file_includes_its_home_and_reads_roles() {
        let p = parse_repo_file(
            r#"
name = "shop"
repos = ["o/api", "o/front"]
ignore_checks = ["Preview"]
[roles]
review = ["alice"]
merge = ["author"]
"#,
            "o/front",
        )
        .unwrap();
        assert_eq!(p.name, "shop");
        assert_eq!(p.repos, vec!["o/front", "o/api"]);
        assert_eq!(
            (p.review, p.merge, p.deploy.len()),
            (vec!["alice".to_string()], vec!["author".to_string()], 0)
        );
        assert_eq!(p.ignore_checks, vec!["Preview"]);
    }

    #[test]
    fn empty_repo_file_and_missing_file_mean_defaults() {
        let p = parse_repo_file("", "o/r").unwrap();
        assert_eq!(p, single("o/r"));
        assert_eq!((p.name.as_str(), p.repos.len()), ("o/r", 1));
        // ひな形は、そのまま置いても既定と同じ
        assert_eq!(parse_repo_file(TEMPLATE, "o/r").unwrap(), single("o/r"));
    }

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
            (l.projects[0].name.as_str(), l.projects[0].deploy.clone()),
            ("mine", vec!["me".to_string()])
        );
        assert_eq!(parse_local("").unwrap(), Local::default());
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
        assert!(parse_repo_file("repos = [\"a/b/c\"]", "o/r").is_err());
        assert!(parse_repo_file("typo = 1", "o/r").is_err());
        assert!(parse_repo_file("[roles]\nboss = [\"x\"]", "o/r").is_err());
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
