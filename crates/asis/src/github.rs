//! GitHub を読んで、事実の形にする。判断はしない(判断は asis-core)。
//! 認証は GitHub の公式コマンド `gh` のログインを借りる。
use crate::{lang, say};
use asis_core::facts::*;
use serde_json::Value;

/// 開いている PR。状態を決めるのに要るものを全部 取る。
const OPEN_QUERY: &str = r#"
query($owner:String!,$name:String!){
  viewer{login}
  repository(owner:$owner,name:$name){
    nameWithOwner
    open: pullRequests(states:OPEN, first:100, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{
      number title body isDraft state mergeable baseRefName headRefName createdAt updatedAt mergedAt
      author{login __typename} reviewDecision
      reviewRequests(first:10){nodes{requestedReviewer{... on User{login}}}}
      commits(last:1){nodes{commit{committedDate statusCheckRollup{contexts(first:50){nodes{
        __typename ... on CheckRun{name status conclusion completedAt} ... on StatusContext{context state createdAt}
      }}}}}}
      closingIssuesReferences(first:10){nodes{number repository{nameWithOwner}}}
      comments(last:30){nodes{author{login __typename} createdAt}}
      reviews(last:30){nodes{author{login __typename} state submittedAt}}
    }}
  }
}"#;

/// それ以外。取り込み済みの PR は、テストやコメントを見ない(状態に効かない)ので取らない。
const REST_QUERY: &str = r#"
query($owner:String!,$name:String!){
  repository(owner:$owner,name:$name){
    merged: pullRequests(states:MERGED, first:40, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{
      number title body state baseRefName headRefName createdAt updatedAt mergedAt author{login __typename}
      mergeCommit{oid}
      closingIssuesReferences(first:10){nodes{number repository{nameWithOwner}}}
    }}
    openIssues: issues(states:OPEN, first:100, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{...I}}
    closedIssues: issues(states:CLOSED, first:40, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{...I}}
    refs(refPrefix:"refs/heads/", first:100, orderBy:{field:TAG_COMMIT_DATE,direction:DESC}){nodes{name}}
    deployments(last:50){nodes{environment latestStatus{state} commit{oid committedDate}}}
    defaultBranchRef{name target{... on Commit{history(first:100){nodes{oid}}}}}
    assignableUsers(first:100){nodes{login}}
  }
}
fragment I on Issue { number title state updatedAt body author{login} assignees(first:5){nodes{login}} }"#;

pub struct RepoFacts {
    pub repo: String,
    pub viewer: String,
    pub prs: Vec<PrFacts>,
    /// PR ごとの最後に動いた時刻(ISO 8601)。prs と同じ順
    pub pr_updated: Vec<String>,
    /// PR ごとの、落ちているチェックの名前。prs と同じ順
    pub pr_failing: Vec<Vec<String>>,
    pub issues: Vec<IssueFacts>,
    pub branches: Vec<String>,
    /// 本番デプロイの記録が 1 件も無い
    pub no_deployments: bool,
    /// repo で動ける人(担当にできる人)。読めなければ、PR を出したことのある人(bot を除く)
    pub people: Vec<String>,
    /// 読み切れなかったことの注記。黙って欠けさせない
    pub notes: Vec<Cut>,
}

impl RepoFacts {
    /// チェックが動いている PR がある(終わりは PR の更新時刻に出ないので、次も読み直す)
    pub fn has_running_checks(&self) -> bool {
        self.prs
            .iter()
            .any(|p| p.lifecycle == Lifecycle::Open && p.checks == Checks::Pending)
    }
}

pub fn fetch(repo: &str, ignore_checks: &[String]) -> Result<RepoFacts, String> {
    let (owner, name) = repo.split_once('/').ok_or_else(|| lang::not_repo(repo))?;
    // 2 本を同時に聞く。
    let (open, rest) = std::thread::scope(|s| {
        let rest = s.spawn(|| ask(REST_QUERY, owner, name));
        (
            ask(OPEN_QUERY, owner, name),
            rest.join().unwrap_or_else(|_| Err(lang::stopped())),
        )
    });
    let (mut v, rest) = (
        open.map_err(|e| format!("{repo}: {e}"))?,
        rest.map_err(|e| format!("{repo}: {e}"))?,
    );
    let missing = || {
        say!(
            "{repo}: リポジトリが見つからないか、読む権限がありません",
            "{repo}: the repository does not exist, or you cannot read it"
        )
    };
    let target = v["data"]["repository"]
        .as_object_mut()
        .ok_or_else(missing)?;
    for (k, val) in rest["data"]["repository"].as_object().ok_or_else(missing)? {
        target.insert(k.clone(), val.clone());
    }
    parse(&v, ignore_checks).ok_or_else(missing)
}

/// 読み切れなかったもの(注記の言葉は、出すときに決める)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// 開いている PR が 100 本以上
    OpenPrs,
    /// 取り込み済みは新しい 40 本だけ読んでいて、それより前に本番に出ていないものが有りうる
    MergedPrs,
}

impl Cut {
    pub fn text(self) -> String {
        match self {
            Cut::OpenPrs => lang::t(
                "開いている PR が 100 本以上あります。新しく動いた 100 本だけを出しています",
                "There are 100 or more open PRs. Only the 100 most recently updated are shown",
            ),
            Cut::MergedPrs => lang::t(
                "取り込み済みは新しい 40 本だけを読んでいます。それより前に取り込んで本番に出ていない PR は出ていません",
                "Only the 40 most recent merged PRs are read. PRs merged before them and not yet deployed are not shown",
            ),
        }
        .to_string()
    }
}

/// gh が使えるかどうか
#[derive(Debug, Clone, PartialEq)]
pub enum Gh {
    /// 使える。GitHub のアカウント名
    Ready(String),
    /// gh が入っていない
    Missing,
    /// 入っているが、ログインしていない
    SignedOut,
    /// それ以外で聞けなかった(理由)
    Failed(String),
}

/// gh の返事から、どの状態かを決める
pub fn gh_state(ran: Result<(bool, String, String), std::io::ErrorKind>) -> Gh {
    match ran {
        Err(std::io::ErrorKind::NotFound) => Gh::Missing,
        Err(e) => Gh::Failed(lang::no_gh(e)),
        Ok((true, out, _)) if !out.trim().is_empty() => Gh::Ready(out.trim().to_string()),
        Ok((_, _, err)) if err.contains("auth login") || err.contains("not logged") => {
            Gh::SignedOut
        }
        Ok((_, _, err)) => Gh::Failed(err.trim().to_string()),
    }
}

/// gh が使えるか、誰としてログインしているかを聞く(GitHub に 1 回 聞く)
pub fn whoami() -> Gh {
    gh_state(
        crate::host::command("gh")
            .args(["api", "user", "--jq", ".login"])
            .output()
            .map(|o| {
                (
                    o.status.success(),
                    String::from_utf8_lossy(&o.stdout).into_owned(),
                    String::from_utf8_lossy(&o.stderr).into_owned(),
                )
            })
            .map_err(|e| e.kind()),
    )
}

fn ask(query: &str, owner: &str, name: &str) -> Result<Value, String> {
    let out = crate::host::command("gh")
        .args(["api", "graphql", "-f", &format!("query={query}"), "-f", &format!("owner={owner}"), "-f", &format!("name={name}")])
        .output()
        .map_err(|e| say!("gh を実行できません({e})。GitHub の公式コマンド gh を入れて `gh auth login` してください", "Cannot run gh ({e}). Install GitHub's official command gh and run `gh auth login`"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(lang::bad_reply)
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}
fn nodes(v: &Value) -> impl Iterator<Item = &Value> {
    v["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| !n.is_null())
}
fn is_bot(author: &Value) -> bool {
    author["__typename"] == "Bot"
        || author["login"]
            .as_str()
            .is_some_and(|l| l.ends_with("[bot]"))
}

pub fn parse(v: &Value, ignore_checks: &[String]) -> Option<RepoFacts> {
    let r = &v["data"]["repository"];
    let repo = r["nameWithOwner"].as_str()?.to_string();

    let production: Vec<&Value> = nodes(&r["deployments"])
        .filter(|d| {
            d["environment"]
                .as_str()
                .is_some_and(|e| e.to_ascii_lowercase().starts_with("prod"))
        })
        .collect();
    let succeeded: Vec<&Value> = production
        .iter()
        .filter(|d| d["latestStatus"]["state"] == "SUCCESS")
        .cloned()
        .collect();
    // 既定のブランチの commit の並び(新しい順)。本番に出たかは、時刻ではなくこの順番で決める。
    let default_branch = r["defaultBranchRef"]["name"].as_str().unwrap_or("");
    let history: Vec<&str> = nodes(&r["defaultBranchRef"]["target"]["history"])
        .filter_map(|c| c["oid"].as_str())
        .collect();
    let position = |oid: &str| history.iter().position(|h| *h == oid);
    // 成功した本番デプロイのうち、いちばん新しい commit の位置と時刻。
    let shipped_at = succeeded
        .iter()
        .filter_map(|d| d["commit"]["oid"].as_str().and_then(position))
        .min();
    let shipped_until = succeeded
        .iter()
        .filter_map(|d| d["commit"]["committedDate"].as_str())
        .max();
    let is_shipped = |p: &Value| -> bool {
        let Some(merged_at) = p["mergedAt"].as_str() else {
            return false;
        };
        let on_default = p["baseRefName"].as_str() == Some(default_branch);
        match (
            p["mergeCommit"]["oid"].as_str().and_then(position),
            shipped_at,
        ) {
            // どちらも並びの中にある: 取り込みの commit が、本番の commit と同じか、それより前
            (Some(merge), Some(shipped)) => merge >= shipped,
            // 取り込みが並びより古い(100 commit より前)で、本番の commit は並びの中にある
            (None, Some(_)) if on_default && !history.is_empty() => true,
            // 並びで決められないときは、時刻で比べる
            _ => shipped_until.is_some_and(|until| merged_at <= until),
        }
    };

    let mut prs = Vec::new();
    let mut pr_updated = Vec::new();
    let mut pr_failing = Vec::new();
    let mut people = Vec::new();
    for p in nodes(&r["open"]).chain(nodes(&r["merged"])) {
        let author = s(&p["author"]["login"]);
        if !is_bot(&p["author"]) && !author.is_empty() && !people.contains(&author) {
            people.push(author.clone());
        }
        let commit = &p["commits"]["nodes"][0]["commit"];
        let contexts = &commit["statusCheckRollup"]["contexts"];
        let (checks, failing) = checks(contexts, ignore_checks);
        let pushed = commit["committedDate"].as_str().unwrap_or("");
        let waiting_comment = first_unanswered(p, &author, pushed);
        let decision = p["reviewDecision"].as_str().unwrap_or("");
        let times = Times {
            created: s(&p["createdAt"]),
            pushed: pushed.to_string(),
            checked: nodes(contexts)
                .filter(|c| {
                    let name = c["name"].as_str().or(c["context"].as_str()).unwrap_or("");
                    !ignore_checks.iter().any(|i| i == name)
                })
                .filter_map(|c| c["completedAt"].as_str().or(c["createdAt"].as_str()))
                .max()
                .unwrap_or("")
                .to_string(),
            // 今の判定と同じ向きのレビューのうち、いちばん新しいもの
            reviewed: nodes(&p["reviews"])
                .filter(|r| !decision.is_empty() && r["state"] == decision)
                .filter_map(|r| r["submittedAt"].as_str())
                .max()
                .unwrap_or("")
                .to_string(),
            commented: waiting_comment.unwrap_or("").to_string(),
            merged: s(&p["mergedAt"]),
        };
        prs.push(PrFacts {
            repo: repo.clone(),
            number: p["number"].as_u64()?,
            title: s(&p["title"]),
            body: s(&p["body"]),
            author: author.clone(),
            head_ref: s(&p["headRefName"]),
            base_ref: s(&p["baseRefName"]),
            lifecycle: match p["state"].as_str()? {
                "MERGED" => Lifecycle::Merged,
                "CLOSED" => Lifecycle::Closed,
                _ => Lifecycle::Open,
            },
            is_draft: p["isDraft"] == true,
            has_conflict: p["mergeable"] == "CONFLICTING",
            checks,
            review: match p["reviewDecision"].as_str() {
                Some("APPROVED") => Review::Approved,
                Some("CHANGES_REQUESTED") => Review::ChangesRequested,
                _ => Review::None,
            },
            requested_reviewers: nodes(&p["reviewRequests"])
                .map(|n| s(&n["requestedReviewer"]["login"]))
                .filter(|l| !l.is_empty())
                .collect(),
            unanswered_comment: waiting_comment.is_some(),
            shipped: is_shipped(p),
            closes: nodes(&p["closingIssuesReferences"])
                .filter_map(|n| {
                    Some(IssueRef {
                        repo: n["repository"]["nameWithOwner"].as_str()?.to_string(),
                        number: n["number"].as_u64()?,
                    })
                })
                .collect(),
            times,
        });
        pr_updated.push(s(&p["updatedAt"]));
        pr_failing.push(failing);
    }

    let issues = nodes(&r["openIssues"])
        .chain(nodes(&r["closedIssues"]))
        .filter_map(|i| {
            Some(IssueFacts {
                repo: repo.clone(),
                number: i["number"].as_u64()?,
                title: s(&i["title"]),
                is_open: i["state"] == "OPEN",
                author: s(&i["author"]["login"]),
                assignees: nodes(&i["assignees"]).map(|a| s(&a["login"])).collect(),
                body: s(&i["body"]),
            })
        })
        .collect();

    // repo で動ける人。読めたらそちらを使う。
    let assignable: Vec<String> = nodes(&r["assignableUsers"])
        .map(|u| s(&u["login"]))
        .filter(|l| !l.is_empty())
        .collect();
    if !assignable.is_empty() {
        people = assignable;
    }

    let mut notes = Vec::new();
    if nodes(&r["open"]).count() >= 100 {
        notes.push(Cut::OpenPrs);
    }
    // 読んだ中でいちばん古い取り込みが、まだ本番に出ていない = それより前にも、出ていないものが有りうる。
    let merged: Vec<&PrFacts> = prs
        .iter()
        .filter(|p| p.lifecycle == Lifecycle::Merged)
        .collect();
    if !production.is_empty() && merged.len() >= 40 && merged.last().is_some_and(|p| !p.shipped) {
        notes.push(Cut::MergedPrs);
    }

    Some(RepoFacts {
        repo,
        viewer: s(&v["data"]["viewer"]["login"]),
        prs,
        pr_updated,
        pr_failing,
        issues,
        branches: nodes(&r["refs"]).map(|n| s(&n["name"])).collect(),
        no_deployments: production.is_empty(),
        people,
        notes,
    })
}

/// 変わったかどうかだけを見る印。全部の repo を 1 回の問い合わせで聞く。
/// 押し込み・PR・issue・本番デプロイのどれかが動くと、その repo の印が変わる。
pub fn stamps(repos: &[String]) -> Result<Vec<String>, String> {
    let mut query = String::from("{");
    for (i, repo) in repos.iter().enumerate() {
        let (owner, name) = repo.split_once('/').ok_or_else(|| lang::not_repo(repo))?;
        if repo.contains(['"', '\\']) {
            return Err(say!(
                "`{repo}` に使えない文字があります",
                "`{repo}` has characters that cannot be used"
            ));
        }
        query.push_str(&format!(
            r#"r{i}: repository(owner:"{owner}",name:"{name}"){{pushedAt pullRequests(first:1,orderBy:{{field:UPDATED_AT,direction:DESC}}){{nodes{{updatedAt}}}} issues(first:1,orderBy:{{field:UPDATED_AT,direction:DESC}}){{nodes{{updatedAt}}}} deployments(last:1){{nodes{{createdAt latestStatus{{state}}}}}} defaultBranchRef{{target{{oid}}}}}}"#
        ));
    }
    query.push('}');
    let out = crate::host::command("gh")
        .args(["api", "graphql", "-f", &format!("query={query}")])
        .output()
        .map_err(lang::no_gh)?;
    // 読めない repo が混ざると全体は失敗の印で返るが、読めた分の中身は入っているので、そのまま使う。
    let v: Value = serde_json::from_slice(&out.stdout).map_err(lang::bad_reply)?;
    if v["data"].is_null() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok((0..repos.len())
        .map(|i| v["data"][format!("r{i}")].to_string())
        .collect())
}

/// チェックを 1 つずつ見て、まとめる。数えないと決めた名前は外す。
/// 落ちているものがあれば失敗、無くて動いているものがあれば実行中、全部 通っていれば成功。
fn checks(contexts: &Value, ignore: &[String]) -> (Checks, Vec<String>) {
    let (mut failing, mut pending, mut total) = (Vec::new(), false, 0);
    for c in nodes(contexts) {
        let name = c["name"]
            .as_str()
            .or(c["context"].as_str())
            .unwrap_or_default();
        if ignore.iter().any(|i| i == name) {
            continue;
        }
        total += 1;
        let is_run = c["__typename"] == "CheckRun";
        let result = if is_run {
            c["conclusion"].as_str()
        } else {
            c["state"].as_str()
        };
        match result {
            _ if is_run && c["status"] != "COMPLETED" => pending = true,
            Some("SUCCESS") | Some("NEUTRAL") | Some("SKIPPED") => {}
            Some("PENDING") | Some("EXPECTED") => pending = true,
            _ => failing.push(name.to_string()),
        }
    }
    let state = match (total, failing.len(), pending) {
        (0, _, _) => Checks::None,
        (_, n, _) if n > 0 => Checks::Failing(n as u32),
        (_, _, true) => Checks::Pending,
        _ => Checks::Passing,
    };
    (state, failing)
}

/// 最後の push と作者の最後の発言のあとに、作者以外の人(bot を除く)のコメントがある。
/// 直しの依頼・承認は GitHub のレビューの判定で見るので、ここではコメントだけを数える。
/// 返していないコメントがあれば、その最初の 1 つの時刻を返す。
fn first_unanswered<'a>(p: &'a Value, author: &str, last_push: &str) -> Option<&'a str> {
    let events: Vec<(&str, &Value)> = nodes(&p["comments"])
        .filter_map(|c| Some((c["createdAt"].as_str()?, &c["author"])))
        .chain(
            nodes(&p["reviews"])
                .filter(|r| r["state"] == "COMMENTED")
                .filter_map(|r| Some((r["submittedAt"].as_str()?, &r["author"]))),
        )
        .collect();
    let author_last = events
        .iter()
        .filter(|(_, a)| a["login"] == author)
        .map(|(t, _)| *t)
        .max()
        .unwrap_or("");
    let since = author_last.max(last_push);
    events
        .iter()
        .filter(|(t, a)| *t > since && a["login"] != author && !is_bot(a) && a["login"].is_string())
        .map(|(t, _)| *t)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_state_tells_missing_signed_out_and_ready() {
        use std::io::ErrorKind;
        assert_eq!(gh_state(Err(ErrorKind::NotFound)), Gh::Missing);
        assert_eq!(
            gh_state(Ok((true, "alice\n".into(), String::new()))),
            Gh::Ready("alice".into())
        );
        assert_eq!(
            gh_state(Ok((
                false,
                String::new(),
                "To get started with GitHub CLI, please run:  gh auth login".into()
            ))),
            Gh::SignedOut
        );
        assert!(matches!(
            gh_state(Ok((
                false,
                String::new(),
                "error connecting to api.github.com".into()
            ))),
            Gh::Failed(_)
        ));
    }
    use serde_json::json;

    fn pr(extra: Value) -> Value {
        let mut p = json!({
            "number": 5, "title": "t", "body": "Refs #2", "isDraft": false, "state": "OPEN", "mergeable": "MERGEABLE",
            "baseRefName": "main", "headRefName": "2-x", "updatedAt": "2026-01-02T00:00:00Z", "mergedAt": null,
            "author": {"login": "ann", "__typename": "User"}, "reviewDecision": null,
            "reviewRequests": {"nodes": [{"requestedReviewer": {"login": "bob"}}, {"requestedReviewer": {}}]},
            "commits": {"nodes": [{"commit": {"committedDate": "2026-01-01T10:00:00Z", "statusCheckRollup": {"contexts": {"nodes": [{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"}]}}}}]},
            "closingIssuesReferences": {"nodes": [{"number": 2, "repository": {"nameWithOwner": "o/r"}}]},
            "comments": {"nodes": []}, "reviews": {"nodes": []}
        });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    }
    fn repo(open: Vec<Value>, merged: Vec<Value>, deployments: Value) -> Value {
        json!({"data": {"viewer": {"login": "bob"}, "repository": {
            "nameWithOwner": "o/r", "open": {"nodes": open}, "merged": {"nodes": merged},
            "openIssues": {"nodes": [{"number": 2, "title": "i", "state": "OPEN", "author": {"login": "cat"}, "assignees": {"nodes": [{"login": "ann"}]}}]},
            "closedIssues": {"nodes": [{"number": 1, "title": "old", "state": "CLOSED", "assignees": {"nodes": []}}]},
            "refs": {"nodes": [{"name": "main"}, {"name": "3-wip"}]},
            "deployments": {"nodes": deployments}
        }}})
    }

    /// 既定のブランチの並び(新しい順)と、担当にできる人を足す
    fn with_history(mut v: Value, oids: &[&str], users: &[&str]) -> Value {
        let r = &mut v["data"]["repository"];
        r["defaultBranchRef"] = json!({"name": "main", "target": {"history": {"nodes": oids.iter().map(|o| json!({"oid": o})).collect::<Vec<_>>()}}});
        r["assignableUsers"] =
            json!({"nodes": users.iter().map(|u| json!({"login": u})).collect::<Vec<_>>()});
        v
    }

    #[test]
    fn maps_an_open_pr() {
        let f = parse(&repo(vec![pr(json!({}))], vec![], json!([])), &[]).unwrap();
        let p = &f.prs[0];
        assert_eq!(
            (p.repo.as_str(), p.number, p.author.as_str()),
            ("o/r", 5, "ann")
        );
        assert_eq!(p.lifecycle, Lifecycle::Open);
        assert_eq!(p.checks, Checks::Passing);
        assert_eq!(p.requested_reviewers, vec!["bob"]);
        assert_eq!(
            p.closes,
            vec![IssueRef {
                repo: "o/r".into(),
                number: 2
            }]
        );
        assert!(!p.has_conflict && !p.unanswered_comment && !p.shipped);
        assert_eq!(f.viewer, "bob");
        assert_eq!(f.branches, vec!["main", "3-wip"]);
        assert_eq!(
            f.issues
                .iter()
                .map(|i| (i.number, i.is_open))
                .collect::<Vec<_>>(),
            vec![(2, true), (1, false)]
        );
        assert_eq!(f.issues[0].author, "cat");
        assert!(f.no_deployments);
        assert_eq!(f.people, vec!["ann"]);
    }

    #[test]
    fn maps_checks_review_and_conflict() {
        let run = |name: &str, status: &str, conclusion: Value| json!({"__typename": "CheckRun", "name": name, "status": status, "conclusion": conclusion});
        let status = |name: &str, state: &str| json!({"__typename": "StatusContext", "context": name, "state": state});
        let c = |contexts: Value| {
            pr(
                json!({"commits": {"nodes": [{"commit": {"committedDate": "x", "statusCheckRollup": {"contexts": {"nodes": contexts}}}}]}}),
            )
        };
        let prs = vec![
            c(json!([
                run("test", "COMPLETED", json!("SUCCESS")),
                run("lint", "COMPLETED", json!("FAILURE")),
                status("preview", "ERROR")
            ])),
            c(json!([
                run("test", "IN_PROGRESS", Value::Null),
                status("preview", "SUCCESS")
            ])),
            c(json!([
                run("test", "COMPLETED", json!("SKIPPED")),
                status("preview", "PENDING")
            ])),
            c(json!([
                run("test", "COMPLETED", json!("NEUTRAL")),
                status("preview", "FAILURE")
            ])),
            pr(
                json!({"commits": {"nodes": [{"commit": {"committedDate": "x", "statusCheckRollup": null}}]}}),
            ),
            pr(
                json!({"reviewDecision": "CHANGES_REQUESTED", "mergeable": "CONFLICTING", "isDraft": true}),
            ),
            pr(json!({"reviewDecision": "APPROVED"})),
        ];
        let f = parse(&repo(prs.clone(), vec![], json!([])), &[]).unwrap();
        assert_eq!(
            f.prs.iter().take(5).map(|p| p.checks).collect::<Vec<_>>(),
            vec![
                Checks::Failing(2),
                Checks::Pending,
                Checks::Pending,
                Checks::Failing(1),
                Checks::None
            ]
        );
        assert_eq!(f.pr_failing[0], vec!["lint", "preview"]);
        assert_eq!(
            (f.prs[5].review, f.prs[5].has_conflict, f.prs[5].is_draft),
            (Review::ChangesRequested, true, true)
        );
        assert_eq!(f.prs[6].review, Review::Approved);

        // 数えないと決めたチェックは外す。全部 外れたら「チェック無し」
        let f = parse(&repo(prs, vec![], json!([])), &["preview".to_string()]).unwrap();
        assert_eq!(
            f.prs.iter().take(4).map(|p| p.checks).collect::<Vec<_>>(),
            vec![
                Checks::Failing(1),
                Checks::Pending,
                Checks::Passing,
                Checks::Passing
            ]
        );
        assert_eq!(f.pr_failing[0], vec!["lint"]);
        let f = parse(
            &repo(
                vec![c(json!([status("preview", "FAILURE")]))],
                vec![],
                json!([]),
            ),
            &["preview".to_string()],
        )
        .unwrap();
        assert_eq!(f.prs[0].checks, Checks::None);
    }

    #[test]
    fn shipped_only_when_a_successful_production_deploy_covers_the_merge() {
        let merged = |at: &str| pr(json!({"state": "MERGED", "mergedAt": at}));
        let deps = json!([
            {"environment": "Preview", "latestStatus": {"state": "SUCCESS"}, "commit": {"committedDate": "2026-03-09T00:00:00Z"}},
            {"environment": "Production", "latestStatus": {"state": "SUCCESS"}, "commit": {"committedDate": "2026-03-05T00:00:00Z"}},
            {"environment": "Production", "latestStatus": {"state": "FAILURE"}, "commit": {"committedDate": "2026-03-08T00:00:00Z"}}
        ]);
        let f = parse(
            &repo(
                vec![],
                vec![
                    merged("2026-03-04T00:00:00Z"),
                    merged("2026-03-05T00:00:00Z"),
                    merged("2026-03-06T00:00:00Z"),
                ],
                deps,
            ),
            &[],
        )
        .unwrap();
        assert_eq!(
            f.prs.iter().map(|p| p.shipped).collect::<Vec<_>>(),
            vec![true, true, false]
        );
        assert!(!f.no_deployments);
    }

    #[test]
    fn shipped_follows_the_order_of_commits_not_their_times() {
        // 並び(新しい順): c4 c3 c2 c1。本番に出ているのは c2 まで。
        let merged = |oid: &str, at: &str, base: &str| {
            pr(
                json!({"state": "MERGED", "mergedAt": at, "mergeCommit": {"oid": oid}, "baseRefName": base}),
            )
        };
        let deps = json!([
            {"environment": "Production", "latestStatus": {"state": "SUCCESS"}, "commit": {"oid": "c2", "committedDate": "2026-03-05T00:00:00Z"}},
            {"environment": "Production", "latestStatus": {"state": "SUCCESS"}, "commit": {"oid": "c1", "committedDate": "2026-03-09T00:00:00Z"}}
        ]);
        let prs = vec![
            merged("c1", "2026-03-08T00:00:00Z", "main"), // 時刻は本番の commit より後だが、並びでは前 → 出た
            merged("c2", "2026-03-05T00:00:00Z", "main"), // 本番の commit そのもの → 出た
            merged("c3", "2026-03-01T00:00:00Z", "main"), // 時刻は前だが、並びでは後 → 出ていない
            merged("old", "2026-01-01T00:00:00Z", "main"), // 並びより古い取り込み → 出た
            merged("side", "2026-01-01T00:00:00Z", "feature"), // 別のブランチへの取り込みは、時刻で比べる
            merged("side2", "2026-03-10T00:00:00Z", "feature"),
        ];
        let f = parse(
            &with_history(repo(vec![], prs, deps), &["c4", "c3", "c2", "c1"], &[]),
            &[],
        )
        .unwrap();
        assert_eq!(
            f.prs.iter().map(|p| p.shipped).collect::<Vec<_>>(),
            vec![true, true, false, true, true, false]
        );
    }

    #[test]
    fn people_are_those_who_can_work_on_the_repo() {
        let f = parse(
            &with_history(
                repo(vec![pr(json!({}))], vec![], json!([])),
                &[],
                &["ann", "bob", "cat"],
            ),
            &[],
        )
        .unwrap();
        assert_eq!(f.people, vec!["ann", "bob", "cat"]);
        // 読めなければ、PR を出したことのある人
        let f = parse(&repo(vec![pr(json!({}))], vec![], json!([])), &[]).unwrap();
        assert_eq!(f.people, vec!["ann"]);
    }

    #[test]
    fn times_are_read_from_the_events() {
        let p = pr(json!({
            "createdAt": "2026-01-01T00:00:00Z",
            "reviewDecision": "CHANGES_REQUESTED",
            "commits": {"nodes": [{"commit": {"committedDate": "2026-01-01T10:00:00Z", "statusCheckRollup": {"contexts": {"nodes": [
                {"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS", "completedAt": "2026-01-01T10:05:00Z"},
                {"__typename": "CheckRun", "name": "preview", "status": "COMPLETED", "conclusion": "FAILURE", "completedAt": "2026-01-01T11:00:00Z"}
            ]}}}}]},
            "reviews": {"nodes": [
                {"author": {"login": "bob"}, "state": "APPROVED", "submittedAt": "2026-01-01T12:00:00Z"},
                {"author": {"login": "bob"}, "state": "CHANGES_REQUESTED", "submittedAt": "2026-01-01T13:00:00Z"}
            ]},
            "comments": {"nodes": [
                {"author": {"login": "bob", "__typename": "User"}, "createdAt": "2026-01-01T14:00:00Z"},
                {"author": {"login": "bob", "__typename": "User"}, "createdAt": "2026-01-01T15:00:00Z"}
            ]}
        }));
        let f = parse(&repo(vec![p], vec![], json!([])), &["preview".to_string()]).unwrap();
        let t = &f.prs[0].times;
        assert_eq!(t.created, "2026-01-01T00:00:00Z");
        assert_eq!(t.pushed, "2026-01-01T10:00:00Z");
        // 数えないチェックは、終わった時刻にも入れない
        assert_eq!(t.checked, "2026-01-01T10:05:00Z");
        // 今の判定と同じ向きのレビュー
        assert_eq!(t.reviewed, "2026-01-01T13:00:00Z");
        // 返していないコメントの、最初の 1 つ
        assert_eq!(t.commented, "2026-01-01T14:00:00Z");
    }

    #[test]
    fn notes_say_when_something_was_cut_off() {
        let f = parse(&repo(vec![pr(json!({}))], vec![], json!([])), &[]).unwrap();
        assert!(f.notes.is_empty() && !f.has_running_checks());
        let f = parse(
            &repo((0..100).map(|_| pr(json!({}))).collect(), vec![], json!([])),
            &[],
        )
        .unwrap();
        assert_eq!(f.notes, vec![Cut::OpenPrs]);
        // 読んだ中でいちばん古い取り込みが、まだ本番に出ていない
        let deps = json!([{"environment": "Production", "latestStatus": {"state": "SUCCESS"}, "commit": {"oid": "x", "committedDate": "2026-03-01T00:00:00Z"}}]);
        let unshipped: Vec<Value> = (0..40)
            .map(|_| pr(json!({"state": "MERGED", "mergedAt": "2026-03-02T00:00:00Z"})))
            .collect();
        let f = parse(&repo(vec![], unshipped, deps.clone()), &[]).unwrap();
        assert_eq!(f.notes, vec![Cut::MergedPrs]);
        let mut mixed: Vec<Value> = (0..39)
            .map(|_| pr(json!({"state": "MERGED", "mergedAt": "2026-03-02T00:00:00Z"})))
            .collect();
        mixed.push(pr(
            json!({"state": "MERGED", "mergedAt": "2026-02-01T00:00:00Z"}),
        ));
        assert!(parse(&repo(vec![], mixed, deps), &[])
            .unwrap()
            .notes
            .is_empty());
        // チェックが動いている PR
        let running = pr(
            json!({"commits": {"nodes": [{"commit": {"committedDate": "x", "statusCheckRollup": {"contexts": {"nodes": [{"__typename": "CheckRun", "name": "t", "status": "IN_PROGRESS", "conclusion": null}]}}}}]}}),
        );
        assert!(parse(&repo(vec![running], vec![], json!([])), &[])
            .unwrap()
            .has_running_checks());
    }

    #[test]
    fn unanswered_comment_rules() {
        let u = |comments: Value, reviews: Value| {
            let f = parse(
                &repo(
                    vec![pr(
                        json!({"comments": {"nodes": comments}, "reviews": {"nodes": reviews}}),
                    )],
                    vec![],
                    json!([]),
                ),
                &[],
            )
            .unwrap();
            f.prs[0].unanswered_comment
        };
        let c = |who: &str, kind: &str, at: &str| json!({"author": {"login": who, "__typename": kind}, "createdAt": at});
        // push(10:00)のあとの他人のコメント
        assert!(u(
            json!([c("bob", "User", "2026-01-01T11:00:00Z")]),
            json!([])
        ));
        // push より前のコメントは、直したものとして数えない
        assert!(!u(
            json!([c("bob", "User", "2026-01-01T09:00:00Z")]),
            json!([])
        ));
        // 作者があとで返した
        assert!(!u(
            json!([
                c("bob", "User", "2026-01-01T11:00:00Z"),
                c("ann", "User", "2026-01-01T12:00:00Z")
            ]),
            json!([])
        ));
        // bot と作者自身は数えない
        assert!(!u(
            json!([
                c("ci[bot]", "User", "2026-01-01T11:00:00Z"),
                c("dep", "Bot", "2026-01-01T11:00:00Z"),
                c("ann", "User", "2026-01-01T11:30:00Z")
            ]),
            json!([])
        ));
        // コメントだけのレビューは数える。承認は数えない
        let r = |state: &str| json!([{"author": {"login": "bob", "__typename": "User"}, "state": state, "submittedAt": "2026-01-01T11:00:00Z"}]);
        assert!(u(json!([]), r("COMMENTED")));
        assert!(!u(json!([]), r("APPROVED")));
    }

    #[test]
    fn missing_repository_is_none() {
        assert!(parse(&json!({"data": {"repository": null}}), &[]).is_none());
    }
}
