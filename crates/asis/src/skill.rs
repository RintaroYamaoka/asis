//! Claude Code に渡す手順書(`skill/asis/SKILL.md`)を、仕事場に入れる。
//!
//! issue は AI への指示になる。手順書があれば、Claude は「#12 を始めて」で issue から作業を始め、
//! PR を issue につなぐ。置き場は、仕事場の `~/.claude/skills/asis/SKILL.md`。
use std::path::{Path, PathBuf};

/// 手順書の中身。実行ファイルに入れてある
const SKILL: &str = include_str!("../../../skill/asis/SKILL.md");

/// 手順書を置くフォルダ(仕事場の `~/.claude/skills/asis`)。ホームが分からなければ None
pub fn dir() -> Option<PathBuf> {
    Some(
        crate::host::home_dir()?
            .join(".claude")
            .join("skills")
            .join("asis"),
    )
}

/// 入っているか
pub fn installed() -> bool {
    dir().is_some_and(|d| is_in(&d))
}

/// 入れる。入れた場所を返す
pub fn install() -> Result<PathBuf, String> {
    let dir = dir().ok_or_else(|| {
        crate::lang::t(
            "ホームの場所が分かりません",
            "Cannot find the home directory",
        )
        .to_string()
    })?;
    install_to(&dir)?;
    Ok(dir)
}

fn is_in(dir: &Path) -> bool {
    dir.join("SKILL.md").is_file()
}

/// もう入っていれば、触らない(人が手を入れたもの・別の場所へのリンクを、上書きしない)
fn install_to(dir: &Path) -> Result<(), String> {
    if is_in(dir) {
        return Ok(());
    }
    let fail = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(fail)?;
    std::fs::write(dir.join("SKILL.md"), SKILL).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_the_skill_once_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("asis-skill-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let at = dir.join("skills").join("asis");
        assert!(!is_in(&at));
        install_to(&at).unwrap();
        assert!(is_in(&at));
        let text = std::fs::read_to_string(at.join("SKILL.md")).unwrap();
        assert!(text.starts_with("---\nname: asis"));
        // 人が書き換えたものは、入れ直しても残る
        std::fs::write(at.join("SKILL.md"), "mine").unwrap();
        install_to(&at).unwrap();
        assert_eq!(
            std::fs::read_to_string(at.join("SKILL.md")).unwrap(),
            "mine"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
