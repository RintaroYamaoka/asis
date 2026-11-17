//! 設定ファイル。プロジェクト = repo のまとまりと、その決まり。
use asis_core::facts::Role;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub name: String,
    pub repos: Vec<String>,
    pub merger: Role,
    pub deployer: Role,
    /// 状態を決めるときに数えないチェックの名前
    pub ignore_checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    project: Vec<RawProject>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: String,
    repos: Vec<String>,
    merger: Option<String>,
    deployer: Option<String>,
    #[serde(default)]
    ignore_checks: Vec<String>,
}

pub fn role(s: &str) -> Result<Role, String> {
    match s {
        "author" => Ok(Role::Author),
        "reviewer" => Ok(Role::Reviewer),
        other => Err(format!(
            "`{other}` は author か reviewer で指定してください"
        )),
    }
}

pub fn parse(text: &str) -> Result<Vec<Project>, String> {
    let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
    file.project
        .into_iter()
        .map(|p| {
            if p.repos.is_empty() {
                return Err(format!("プロジェクト `{}` に repos がありません", p.name));
            }
            Ok(Project {
                merger: p
                    .merger
                    .as_deref()
                    .map(role)
                    .transpose()?
                    .unwrap_or(Role::Author),
                deployer: p
                    .deployer
                    .as_deref()
                    .map(role)
                    .transpose()?
                    .unwrap_or(Role::Author),
                name: p.name,
                repos: p.repos,
                ignore_checks: p.ignore_checks,
            })
        })
        .collect()
}

/// `$XDG_CONFIG_HOME/asis/config.toml`、無ければ `~/.config/asis/config.toml`(Windows は `%APPDATA%`)。
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("asis").join("config.toml"))
}

/// 設定ファイルが無ければ空。読めない・形が違うときはエラー。
pub fn load() -> Result<Vec<Project>, String> {
    let Some(path) = path() else {
        return Ok(vec![]);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_projects_with_defaults() {
        let got = parse(
            r#"
[[project]]
name = "shop"
repos = ["o/front", "o/api"]
merger = "reviewer"
ignore_checks = ["Preview"]

[[project]]
name = "solo"
repos = ["me/tool"]
"#,
        )
        .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].name.as_str(), got[0].repos.len()), ("shop", 2));
        assert_eq!(
            (got[0].merger, got[0].deployer),
            (Role::Reviewer, Role::Author)
        );
        assert_eq!(got[0].ignore_checks, vec!["Preview"]);
        assert_eq!(
            (got[1].merger, got[1].ignore_checks.len()),
            (Role::Author, 0)
        );
    }

    #[test]
    fn empty_file_is_no_projects() {
        assert_eq!(parse("").unwrap(), vec![]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("[[project]]\nname = \"x\"\nrepos = []")
            .unwrap_err()
            .contains("repos"));
        assert!(
            parse("[[project]]\nname = \"x\"\nrepos = [\"o/r\"]\nmerger = \"boss\"")
                .unwrap_err()
                .contains("boss")
        );
        assert!(parse("[[project]]\nname = \"x\"\nrepos = [\"o/r\"]\ntypo = 1").is_err());
        assert!(parse("[[project]]\nrepos = [\"o/r\"]").is_err());
    }
}
