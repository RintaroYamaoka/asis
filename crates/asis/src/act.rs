//! 操作。求めないが、できる。
//!
//! 操作がやるのは、GitHub の上の事実を作ることだけ(issue ができる・承認が付く・取り込まれる)。
//! 状態は、その事実から今までどおり決まる。ここから状態を直接 変えることはできない。
use crate::{lang, say};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// issue を書く。題だけでよい
    WriteIssue {
        repo: String,
        title: String,
        body: String,
    },
    /// PR を承認する
    Approve { repo: String, number: u64 },
    /// PR に直しを頼む。何を直すかの一言が要る
    RequestChanges {
        repo: String,
        number: u64,
        comment: String,
    },
    /// PR を取り込む。取り込み方は、その repo の既定に従う
    Merge { repo: String, number: u64 },
}

fn check_repo(repo: &str) -> Result<(), String> {
    let ok = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    match repo.split_once('/') {
        Some((owner, name)) if ok(owner) && ok(name) => Ok(()),
        _ => Err(lang::not_repo(repo)),
    }
}

impl Action {
    /// `gh` に渡す引数。`merge_method` は取り込むときだけ使う(merge・squash・rebase)。
    pub fn args(&self, merge_method: &str) -> Result<Vec<String>, String> {
        let post = |path: String, fields: &[(&str, &str)]| {
            let mut a = vec!["api".to_string(), "-X".into(), "POST".into(), path];
            for (k, v) in fields {
                a.push("-f".into());
                a.push(format!("{k}={v}"));
            }
            a
        };
        match self {
            Action::WriteIssue { repo, title, body } => {
                check_repo(repo)?;
                if title.trim().is_empty() {
                    return Err(lang::t("題がありません", "The title is empty").into());
                }
                Ok(post(
                    format!("repos/{repo}/issues"),
                    &[("title", title.trim()), ("body", body.trim())],
                ))
            }
            Action::Approve { repo, number } => {
                check_repo(repo)?;
                Ok(post(
                    format!("repos/{repo}/pulls/{number}/reviews"),
                    &[("event", "APPROVE")],
                ))
            }
            Action::RequestChanges {
                repo,
                number,
                comment,
            } => {
                check_repo(repo)?;
                if comment.trim().is_empty() {
                    return Err(lang::t(
                        "何を直すかを、一言 書いてください",
                        "Write a word on what to fix",
                    )
                    .into());
                }
                Ok(post(
                    format!("repos/{repo}/pulls/{number}/reviews"),
                    &[("event", "REQUEST_CHANGES"), ("body", comment.trim())],
                ))
            }
            Action::Merge { repo, number } => {
                check_repo(repo)?;
                if !["merge", "squash", "rebase"].contains(&merge_method) {
                    return Err(say!(
                        "取り込み方が分かりません: `{merge_method}`",
                        "Unknown merge method: `{merge_method}`"
                    ));
                }
                Ok(vec![
                    "api".into(),
                    "-X".into(),
                    "PUT".into(),
                    format!("repos/{repo}/pulls/{number}/merge"),
                    "-f".into(),
                    format!("merge_method={merge_method}"),
                ])
            }
        }
    }

    /// 終わったあとに出す 1 行
    pub fn done(&self, reply: &str) -> String {
        match self {
            Action::WriteIssue { repo, .. } => {
                let number = serde_json::from_str::<serde_json::Value>(reply)
                    .ok()
                    .and_then(|v| v["number"].as_u64());
                match number {
                    Some(n) => format!("{repo}: issue #{n} opened"),
                    None => format!("{repo}: issue opened"),
                }
            }
            Action::Approve { number, .. } => format!("#{number}: Approved"),
            Action::RequestChanges { number, .. } => format!("#{number}: Changes requested"),
            Action::Merge { number, .. } => format!("#{number}: Merged"),
        }
    }
}

/// GitHub が返した失敗を、人が読める 1 行にする。
fn explain(stderr: &str, stdout: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| {
            let first = v["errors"][0]
                .as_str()
                .or(v["errors"][0]["message"].as_str())
                .map(str::to_string);
            first.or(v["message"].as_str().map(str::to_string))
        });
    let text = message.unwrap_or_else(|| stderr.trim().to_string());
    if text.contains("approve your own pull request") {
        lang::t(
            "自分の PR は、自分では Approve できません",
            "You cannot Approve your own PR",
        )
        .into()
    } else if text.contains("request changes on your own pull request") {
        lang::t(
            "自分の PR には、Request changes できません",
            "You cannot Request changes on your own PR",
        )
        .into()
    } else if text.contains("not mergeable") || text.contains("Merge conflict") {
        lang::t(
            "いまは Merge できません(conflict があるか、条件を満たしていません)",
            "Cannot Merge now (there is a conflict, or the requirements are not met)",
        )
        .into()
    } else {
        text
    }
}

fn gh(args: &[String]) -> Result<String, String> {
    let out = crate::command("gh")
        .args(args)
        .output()
        .map_err(lang::no_gh)?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        Err(explain(&String::from_utf8_lossy(&out.stderr), &stdout))
    }
}

/// その repo で、この人がふだん使う取り込み方(GitHub が覚えているもの)。
fn merge_method(repo: &str) -> Result<String, String> {
    check_repo(repo)?;
    let (owner, name) = repo.split_once('/').expect("形は確かめた");
    let reply = gh(&[
        "api".into(),
        "graphql".into(),
        "-f".into(),
        "query=query($o:String!,$n:String!){repository(owner:$o,name:$n){viewerDefaultMergeMethod}}"
            .into(),
        "-f".into(),
        format!("o={owner}"),
        "-f".into(),
        format!("n={name}"),
    ])?;
    let v: serde_json::Value = serde_json::from_str(&reply).map_err(lang::bad_reply)?;
    Ok(v["data"]["repository"]["viewerDefaultMergeMethod"]
        .as_str()
        .unwrap_or("MERGE")
        .to_lowercase())
}

/// 操作を 1 つ行う。うまくいけば、出す 1 行を返す。
pub fn run(action: &Action) -> Result<String, String> {
    let method = match action {
        Action::Merge { repo, .. } => merge_method(repo)?,
        _ => String::from("merge"),
    };
    let reply = gh(&action.args(&method)?)?;
    Ok(action.done(&reply))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn each_action_becomes_one_github_call() {
        let write = Action::WriteIssue {
            repo: "o/r".into(),
            title: " 検索が遅い ".into(),
            body: String::new(),
        };
        assert_eq!(
            write.args("merge").unwrap(),
            s(&[
                "api",
                "-X",
                "POST",
                "repos/o/r/issues",
                "-f",
                "title=検索が遅い",
                "-f",
                "body="
            ])
        );
        assert_eq!(
            Action::Approve {
                repo: "o/r".into(),
                number: 7
            }
            .args("merge")
            .unwrap(),
            s(&[
                "api",
                "-X",
                "POST",
                "repos/o/r/pulls/7/reviews",
                "-f",
                "event=APPROVE"
            ])
        );
        let fix = Action::RequestChanges {
            repo: "o/r".into(),
            number: 7,
            comment: "名前を直して".into(),
        };
        assert_eq!(
            fix.args("merge").unwrap(),
            s(&[
                "api",
                "-X",
                "POST",
                "repos/o/r/pulls/7/reviews",
                "-f",
                "event=REQUEST_CHANGES",
                "-f",
                "body=名前を直して"
            ])
        );
        assert_eq!(
            Action::Merge {
                repo: "o/r".into(),
                number: 7
            }
            .args("squash")
            .unwrap(),
            s(&[
                "api",
                "-X",
                "PUT",
                "repos/o/r/pulls/7/merge",
                "-f",
                "merge_method=squash"
            ])
        );
    }

    #[test]
    fn refuses_what_would_be_a_bad_call() {
        let no_title = Action::WriteIssue {
            repo: "o/r".into(),
            title: "  ".into(),
            body: "x".into(),
        };
        assert!(no_title.args("merge").unwrap_err().contains("題"));
        let no_word = Action::RequestChanges {
            repo: "o/r".into(),
            number: 1,
            comment: " ".into(),
        };
        assert!(no_word.args("merge").unwrap_err().contains("一言"));
        let merge = Action::Merge {
            repo: "o/r".into(),
            number: 1,
        };
        assert!(merge.args("fast").is_err());
        // repo の名前に、道を変える字や空白を入れさせない
        for bad in ["nope", "o/r/x", "o/ r", "../x/y", "o/r?x=1", ""] {
            assert!(
                Action::Approve {
                    repo: bad.into(),
                    number: 1
                }
                .args("merge")
                .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn says_what_happened_in_one_line() {
        let write = Action::WriteIssue {
            repo: "o/r".into(),
            title: "t".into(),
            body: String::new(),
        };
        assert_eq!(write.done(r#"{"number": 12}"#), "o/r: issue #12 opened");
        assert_eq!(write.done("???"), "o/r: issue opened");
        assert_eq!(
            Action::Merge {
                repo: "o/r".into(),
                number: 7
            }
            .done(""),
            "#7: Merged"
        );
    }

    #[test]
    fn failures_are_explained() {
        assert_eq!(
            explain(
                "",
                r#"{"message":"Unprocessable Entity","errors":["Can not approve your own pull request"]}"#
            ),
            "自分の PR は、自分では Approve できません"
        );
        assert!(
            explain("", r#"{"message":"Pull Request is not mergeable"}"#)
                .contains("Merge できません")
        );
        assert_eq!(
            explain("gh: Not Found (HTTP 404)\n", ""),
            "gh: Not Found (HTTP 404)"
        );
    }
}
