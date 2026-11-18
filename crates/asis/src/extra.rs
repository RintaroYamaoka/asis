//! 差し込み口。repo の中のファイルに書かれたタスクを、issue と同じタスクとして足すためのもの。
//!
//! asis 本体は、ファイルの書き方を何も知らない。どのファイルを読むか、どうタスクに直すかは、差し込む側が決める。
//! 本体がするのは 2 つだけ:選ばれたファイルを main から読むこと(main が動いたときだけ読み直す)と、
//! 返ってきたタスクを issue と同じ規則(つながる PR の事実)で並べること。
//! 差し込まなければ、今までどおり issue と PR だけで動く。
use serde_json::Value;
use std::process::Command;

/// 差し込む側が作るもの
pub trait Reader: Send + Sync {
    /// main にあるファイルの場所を見て、読むかを決める
    fn wants(&self, path: &str) -> bool;
    /// 読んだファイル(場所, 中身)と、PR の題(並びは repo の PR と同じ)と、
    /// PR がまだ無いブランチの名前から、足すタスクを返す
    fn tasks(
        &self,
        files: &[(String, String)],
        pr_titles: &[&str],
        branches: &[&str],
    ) -> Vec<FileTask>;
}

/// ファイルに書かれたタスク 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct FileTask {
    /// 画面の番号の欄に出す字(`T-12` など)
    pub label: String,
    pub title: String,
    /// まだ済んでいないか。`None` は、つながる PR が開いているあいだだけ開いている
    pub open: Option<bool>,
    /// 書かれているファイルの場所。無ければ空
    pub path: String,
    /// 書かれたまま
    pub body: String,
    /// つながる PR。`pr_titles` の何番目か
    pub prs: Vec<usize>,
    /// このタスクの作業をしている、PR がまだ無いブランチ(`branches` から選んだもの)
    pub branches: Vec<String>,
}

/// repo に push されたブランチのうち、PR がまだ無いもの(既定のブランチは除く)
pub fn branches_without_pr<'a>(branches: &'a [String], pr_heads: &[&str]) -> Vec<&'a str> {
    branches
        .iter()
        .map(String::as_str)
        .filter(|b| !matches!(*b, "main" | "master") && !pr_heads.contains(b))
        .collect()
}

/// `stamps` の 1 件から、main の今の commit を取り出す
pub fn main_oid(stamp: &str) -> Option<String> {
    let v: Value = serde_json::from_str(stamp).ok()?;
    v["defaultBranchRef"]["target"]["oid"]
        .as_str()
        .map(str::to_string)
}

/// URL の道の 1 区切りに入れられる形にする(`/` は区切りとして残す)
fn encode_path(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn gh(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| format!("gh を実行できません({e})"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

/// ファイルの一覧(木)から、ファイルの場所だけを取り出す
pub fn paths_in_tree(tree: &Value) -> Vec<String> {
    tree["tree"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["type"] == "blob")
        .filter_map(|e| e["path"].as_str().map(str::to_string))
        .collect()
}

/// main の、その commit のファイルのうち、`wants` が選んだものを読む
pub fn fetch_files(
    repo: &str,
    oid: &str,
    wants: &dyn Fn(&str) -> bool,
) -> Result<Vec<(String, String)>, String> {
    let tree: Value = serde_json::from_slice(&gh(&[
        "api",
        &format!("repos/{repo}/git/trees/{oid}?recursive=1"),
    ])?)
    .map_err(|e| format!("ファイルの一覧を読めません({e})"))?;
    let mut out = Vec::new();
    for path in paths_in_tree(&tree).into_iter().filter(|p| wants(p)) {
        let body = gh(&[
            "api",
            "-H",
            "Accept: application/vnd.github.raw+json",
            &format!("repos/{repo}/contents/{}?ref={oid}", encode_path(&path)),
        ])?;
        out.push((path, String::from_utf8_lossy(&body).into_owned()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_oid_is_read_from_the_stamp() {
        assert_eq!(
            main_oid(r#"{"pushedAt":"x","defaultBranchRef":{"target":{"oid":"abc"}}}"#).as_deref(),
            Some("abc")
        );
        assert_eq!(main_oid("null"), None);
    }

    #[test]
    fn paths_are_encoded_and_listed() {
        assert_eq!(
            encode_path("docs/a b/メモ.md"),
            "docs/a%20b/%E3%83%A1%E3%83%A2.md"
        );
        let tree = serde_json::json!({"tree": [
            {"path": "docs", "type": "tree"},
            {"path": "docs/メモ.md", "type": "blob"}
        ]});
        assert_eq!(paths_in_tree(&tree), vec!["docs/メモ.md"]);
    }

    #[test]
    fn only_branches_without_a_pr_are_passed() {
        let branches = ["main".to_string(), "feat/a".into(), "feat/b".into()];
        assert_eq!(branches_without_pr(&branches, &["feat/a"]), vec!["feat/b"]);
    }
}
