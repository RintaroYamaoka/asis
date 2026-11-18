//! この PC の上の作業を読む。worktree・ブランチ・ターミナルのペイン。
//! PR になる前の作業は GitHub からは見えないので、ここで拾う。判断はしない。
use serde_json::Value;
use std::path::{Path, PathBuf};

/// ターミナルのペイン
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pane {
    pub cwd: PathBuf,
    /// 動いている AI の名前。無ければ空
    pub agent: String,
    /// working / idle / done など。AI が居なければ空
    pub status: String,
    pub title: String,
}

/// 手元の worktree 1 つ
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Work {
    /// `owner/name`
    pub repo: String,
    pub path: PathBuf,
    pub branch: String,
    /// この worktree を開いているペイン
    pub panes: Vec<Pane>,
}

impl Work {
    /// AI が今 動いている
    pub fn working(&self) -> bool {
        self.panes.iter().any(|p| p.status == "working")
    }
}

/// `git@github.com:o/r.git` / `https://github.com/o/r` / `ssh://git@github.com/o/r.git` → `o/r`
pub fn repo_of_url(url: &str) -> Option<String> {
    let rest = url.trim().split_once("github.com")?.1;
    let rest = rest.trim_start_matches([':', '/']).trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| format!("{owner}/{name}"))
}

/// `.git/config` の中の origin の URL
pub fn origin_in_config(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
        } else if in_origin {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "url" {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

/// `git worktree list --porcelain` の出力 → (場所, ブランチ)。ブランチに居ない worktree は除く。
pub fn parse_worktrees(porcelain: &str) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if let Some(p) = path.take() {
                out.push((p, b.to_string()));
            }
        }
    }
    out
}

/// ターミナルの道具が出すペインの一覧(JSON)
pub fn parse_panes(json: &Value) -> Vec<Pane> {
    let panes = json["result"]["panes"].as_array();
    panes
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let text = |k: &str| p[k].as_str().unwrap_or_default().to_string();
            let cwd = p["foreground_cwd"].as_str().or(p["cwd"].as_str())?;
            Some(Pane {
                cwd: PathBuf::from(cwd),
                agent: text("agent"),
                status: text("agent_status"),
                title: text("terminal_title_stripped"),
            })
        })
        .collect()
}

/// ペインを、それが開いている worktree に付ける。いちばん深い worktree に 1 回だけ付く。
pub fn attach(mut works: Vec<Work>, panes: &[Pane]) -> Vec<Work> {
    for pane in panes {
        let deepest = works
            .iter_mut()
            .filter(|w| pane.cwd.starts_with(&w.path))
            .max_by_key(|w| w.path.components().count());
        if let Some(w) = deepest {
            w.panes.push(pane.clone());
        }
    }
    works
}

fn panes() -> Vec<Pane> {
    // ターミナルの道具が無い・動いていないときは、ペイン無しとして進む。
    let out = crate::host::command("herdr")
        .args(["pane", "list"])
        .output();
    let Ok(out) = out else { return vec![] };
    if !out.status.success() {
        return vec![];
    }
    serde_json::from_slice(&out.stdout)
        .map(|v| parse_panes(&v))
        .unwrap_or_default()
}

/// その場所を含む repo の、いちばん上のフォルダ(worktree なら、その worktree の上)。
fn toplevel(dir: &Path) -> Option<PathBuf> {
    let out = crate::host::command("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

fn origin(dir: &Path) -> Option<String> {
    let out = crate::host::command("git")
        .arg("-C")
        .arg(dir)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
        .and_then(|u| repo_of_url(&u))
}

/// 見ている repo の、この PC の上の worktree を集める。
/// 仕事場が WSL の中なら(窓は Windows)、WSL の中の asis に集めさせる。
/// フォルダを歩くのは、その中でないとできない。
pub fn gather(repos: &[String], roots: &[PathBuf]) -> Vec<Work> {
    if crate::host::wsl().is_some() {
        return gather_in_wsl(repos);
    }
    gather_here(repos, roots)
}

/// WSL の中の `asis local` に集めさせて、返事を読む。WSL や asis が無ければ、手元の作業は無しとして進む。
/// 探すフォルダ(`roots`)は、WSL の中の設定のものを使う。
fn gather_in_wsl(repos: &[String]) -> Vec<Work> {
    if repos.is_empty() {
        return vec![];
    }
    // ログインのシェルを通す(asis の置き場が PATH に入るのは、そのときなので)
    let out = crate::host::command("bash")
        .args(["-lc", r#"asis local "$@""#, "asis"])
        .args(repos)
        .output();
    match out {
        Ok(out) if out.status.success() => works_of_json(&out.stdout),
        _ => vec![],
    }
}

/// `asis local` の返事を読む
pub fn works_of_json(reply: &[u8]) -> Vec<Work> {
    serde_json::from_slice(reply).unwrap_or_default()
}

/// 探す起点は、ペインが開いている場所・今の場所・設定の `roots` の直下のフォルダ。
fn gather_here(repos: &[String], roots: &[PathBuf]) -> Vec<Work> {
    let panes = panes();
    let mut seeds: Vec<PathBuf> = panes.iter().map(|p| p.cwd.clone()).collect();
    seeds.extend(std::env::current_dir().ok());
    for root in roots {
        let children = std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path());
        // 直下のフォルダのうち、origin が見ている repo のものだけ(設定を文字で読むので速い)。
        seeds.extend(children.filter(|dir| {
            let config =
                std::fs::read_to_string(dir.join(".git").join("config")).unwrap_or_default();
            origin_in_config(&config)
                .and_then(|u| repo_of_url(&u))
                .is_some_and(|r| repos.contains(&r))
        }));
    }

    let mut works: Vec<Work> = Vec::new();
    let mut seen_tops: Vec<PathBuf> = Vec::new();
    for seed in seeds {
        let Some(top) = toplevel(&seed) else { continue };
        if seen_tops.contains(&top) {
            continue;
        }
        seen_tops.push(top.clone());
        let Some(repo) = origin(&top).filter(|r| repos.contains(r)) else {
            continue;
        };
        let Ok(out) = crate::host::command("git")
            .arg("-C")
            .arg(&top)
            .args(["worktree", "list", "--porcelain"])
            .output()
        else {
            continue;
        };
        for (path, branch) in parse_worktrees(&String::from_utf8_lossy(&out.stdout)) {
            if !works.iter().any(|w| w.path == path) {
                works.push(Work {
                    repo: repo.clone(),
                    path,
                    branch,
                    panes: vec![],
                });
            }
        }
    }
    attach(works, &panes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn works_survive_the_trip_through_json() {
        let works = vec![Work {
            repo: "o/r".into(),
            path: "/home/a/r".into(),
            branch: "12-x".into(),
            panes: vec![Pane {
                cwd: "/home/a/r/src".into(),
                agent: "claude".into(),
                status: "working".into(),
                title: "t".into(),
            }],
        }];
        let json = serde_json::to_vec(&works).unwrap();
        assert_eq!(works_of_json(&json), works);
        // 読めない返事は、手元の作業 無しとして進む
        assert!(works_of_json(b"bash: asis: command not found").is_empty());
    }

    #[test]
    fn repo_from_remote_urls() {
        for url in [
            "git@github.com:o/r.git",
            "https://github.com/o/r",
            "https://github.com/o/r.git\n",
            "ssh://git@github.com/o/r.git",
            "https://user@github.com/o/r/",
        ] {
            assert_eq!(repo_of_url(url).as_deref(), Some("o/r"), "{url}");
        }
        for url in [
            "https://gitlab.com/o/r",
            "git@github.com:o",
            "https://github.com/o/r/extra",
            "",
        ] {
            assert_eq!(repo_of_url(url), None, "{url}");
        }
    }

    #[test]
    fn origin_url_in_git_config() {
        let config = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://github.com/x/y\n[remote \"origin\"]\n\tfetch = +refs\n\turl = git@github.com:o/r.git\n[branch \"main\"]\n\tremote = origin\n";
        assert_eq!(
            origin_in_config(config).as_deref(),
            Some("git@github.com:o/r.git")
        );
        assert_eq!(origin_in_config("[core]\n\turl = nope\n"), None);
    }

    #[test]
    fn worktree_list() {
        let porcelain = "worktree /src/r\nHEAD abc\nbranch refs/heads/main\n\nworktree /src/_wt/12-login\nHEAD def\nbranch refs/heads/12-login\n\nworktree /src/_wt/detached\nHEAD 123\ndetached\n\nworktree /src/_wt/feat\nHEAD 456\nbranch refs/heads/feat/x\n";
        assert_eq!(
            parse_worktrees(porcelain),
            vec![
                (PathBuf::from("/src/r"), "main".to_string()),
                (PathBuf::from("/src/_wt/12-login"), "12-login".to_string()),
                (PathBuf::from("/src/_wt/feat"), "feat/x".to_string())
            ]
        );
        assert!(parse_worktrees("").is_empty());
    }

    #[test]
    fn panes_from_json() {
        let v = json!({"result": {"panes": [
            {"cwd": "/a", "foreground_cwd": "/a/sub", "agent": "claude", "agent_status": "working", "terminal_title_stripped": "login"},
            {"cwd": "/b"},
            {"agent": "x"}
        ]}});
        let panes = parse_panes(&v);
        assert_eq!(panes.len(), 2);
        assert_eq!(
            panes[0],
            Pane {
                cwd: "/a/sub".into(),
                agent: "claude".into(),
                status: "working".into(),
                title: "login".into()
            }
        );
        assert_eq!(
            (panes[1].cwd.clone(), panes[1].agent.as_str()),
            (PathBuf::from("/b"), "")
        );
        assert!(parse_panes(&json!({"error": "not running"})).is_empty());
    }

    #[test]
    fn panes_attach_to_the_deepest_worktree() {
        let work = |path: &str| Work {
            repo: "o/r".into(),
            path: path.into(),
            branch: "b".into(),
            panes: vec![],
        };
        let pane = |cwd: &str, status: &str| Pane {
            cwd: cwd.into(),
            agent: "claude".into(),
            status: status.into(),
            title: String::new(),
        };
        let works = attach(
            vec![
                work("/src/r"),
                work("/src/r/_wt/12-login"),
                work("/src/other"),
            ],
            &[
                pane("/src/r/_wt/12-login/src", "working"),
                pane("/src/r", "idle"),
                pane("/elsewhere", "working"),
            ],
        );
        assert_eq!(
            works.iter().map(|w| w.panes.len()).collect::<Vec<_>>(),
            vec![1, 1, 0]
        );
        assert!(works[1].working() && !works[0].working());
        // 名前が前から一致するだけの別のフォルダには付けない
        let works = attach(vec![work("/src/r")], &[pane("/src/r2", "working")]);
        assert!(works[0].panes.is_empty());
    }
}
