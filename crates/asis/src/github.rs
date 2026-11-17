//! GitHub を読んで、事実の形にする。判断はしない(判断は asis-core)。
//! 認証は GitHub の公式コマンド `gh` のログインを借りる。
use asis_core::facts::*;
use serde_json::Value;
use std::process::Command;

const QUERY: &str = r#"
query($owner:String!,$name:String!){
  viewer{login}
  repository(owner:$owner,name:$name){
    nameWithOwner
    open: pullRequests(states:OPEN, first:100, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{...P}}
    merged: pullRequests(states:MERGED, first:40, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{...P}}
    openIssues: issues(states:OPEN, first:100, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{...I}}
    closedIssues: issues(states:CLOSED, first:40, orderBy:{field:UPDATED_AT,direction:DESC}){nodes{...I}}
    refs(refPrefix:"refs/heads/", first:100, orderBy:{field:TAG_COMMIT_DATE,direction:DESC}){nodes{name}}
    deployments(last:50){nodes{environment latestStatus{state} commit{committedDate}}}
  }
}
fragment I on Issue { number title state updatedAt assignees(first:5){nodes{login}} }
fragment P on PullRequest {
  number title body isDraft state mergeable baseRefName headRefName updatedAt mergedAt
  author{login __typename} reviewDecision
  reviewRequests(first:10){nodes{requestedReviewer{... on User{login}}}}
  commits(last:1){nodes{commit{committedDate statusCheckRollup{state}}}}
  closingIssuesReferences(first:10){nodes{number repository{nameWithOwner}}}
  comments(last:30){nodes{author{login __typename} createdAt}}
  reviews(last:30){nodes{author{login __typename} state submittedAt}}
}"#;

pub struct RepoFacts {
    pub repo: String,
    pub viewer: String,
    pub prs: Vec<PrFacts>,
    /// PR ごとの最後に動いた時刻(ISO 8601)。prs と同じ順
    pub pr_updated: Vec<String>,
    pub issues: Vec<IssueFacts>,
    pub branches: Vec<String>,
    /// 本番デプロイの記録が 1 件も無い
    pub no_deployments: bool,
    /// PR を出したことのある人(bot を除く)
    pub people: Vec<String>,
}

pub fn fetch(repo: &str) -> Result<RepoFacts, String> {
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| format!("`{repo}` は owner/name の形ではありません"))?;
    let out = Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={QUERY}"), "-f", &format!("owner={owner}"), "-f", &format!("name={name}")])
        .output()
        .map_err(|e| format!("gh を実行できません({e})。GitHub の公式コマンド gh を入れて `gh auth login` してください"))?;
    if !out.status.success() {
        return Err(format!(
            "{repo}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("{repo}: 返事を読めません({e})"))?;
    parse(&v).ok_or_else(|| format!("{repo}: リポジトリが見つからないか、読む権限がありません"))
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

pub fn parse(v: &Value) -> Option<RepoFacts> {
    let r = &v["data"]["repository"];
    let repo = r["nameWithOwner"].as_str()?.to_string();

    // 成功した本番デプロイのうち、いちばん新しい commit の時刻。
    let production: Vec<&Value> = nodes(&r["deployments"])
        .filter(|d| {
            d["environment"]
                .as_str()
                .is_some_and(|e| e.to_ascii_lowercase().starts_with("prod"))
        })
        .collect();
    let shipped_until = production
        .iter()
        .filter(|d| d["latestStatus"]["state"] == "SUCCESS")
        .filter_map(|d| d["commit"]["committedDate"].as_str())
        .max()
        .map(str::to_string);

    let mut prs = Vec::new();
    let mut pr_updated = Vec::new();
    let mut people = Vec::new();
    for p in nodes(&r["open"]).chain(nodes(&r["merged"])) {
        let author = s(&p["author"]["login"]);
        if !is_bot(&p["author"]) && !author.is_empty() && !people.contains(&author) {
            people.push(author.clone());
        }
        let commit = &p["commits"]["nodes"][0]["commit"];
        let merged_at = p["mergedAt"].as_str();
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
            checks: match commit["statusCheckRollup"]["state"].as_str() {
                None => Checks::None,
                Some("SUCCESS") => Checks::Passing,
                Some("FAILURE") | Some("ERROR") => Checks::Failing(0),
                Some(_) => Checks::Pending,
            },
            review: match p["reviewDecision"].as_str() {
                Some("APPROVED") => Review::Approved,
                Some("CHANGES_REQUESTED") => Review::ChangesRequested,
                _ => Review::None,
            },
            requested_reviewers: nodes(&p["reviewRequests"]).map(|n| s(&n["requestedReviewer"]["login"])).filter(|l| !l.is_empty()).collect(),
            unanswered_comment: unanswered(p, &author, commit["committedDate"].as_str().unwrap_or("")),
            // 取り込みの時刻が、本番に出た最新の commit の時刻以前なら、本番に含まれている。
            shipped: matches!((merged_at, &shipped_until), (Some(m), Some(until)) if m <= until.as_str()),
            closes: nodes(&p["closingIssuesReferences"])
                .filter_map(|n| Some(IssueRef { repo: n["repository"]["nameWithOwner"].as_str()?.to_string(), number: n["number"].as_u64()? }))
                .collect(),
        });
        pr_updated.push(s(&p["updatedAt"]));
    }

    let issues = nodes(&r["openIssues"])
        .chain(nodes(&r["closedIssues"]))
        .filter_map(|i| {
            Some(IssueFacts {
                repo: repo.clone(),
                number: i["number"].as_u64()?,
                title: s(&i["title"]),
                is_open: i["state"] == "OPEN",
                assignees: nodes(&i["assignees"]).map(|a| s(&a["login"])).collect(),
            })
        })
        .collect();

    Some(RepoFacts {
        repo,
        viewer: s(&v["data"]["viewer"]["login"]),
        prs,
        pr_updated,
        issues,
        branches: nodes(&r["refs"]).map(|n| s(&n["name"])).collect(),
        no_deployments: production.is_empty(),
        people,
    })
}

/// 最後の push と作者の最後の発言のあとに、作者以外の人(bot を除く)のコメントがある。
/// 直しの依頼・承認は GitHub のレビューの判定で見るので、ここではコメントだけを数える。
fn unanswered(p: &Value, author: &str, last_push: &str) -> bool {
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
        .any(|(t, a)| *t > since && a["login"] != author && !is_bot(a) && a["login"].is_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pr(extra: Value) -> Value {
        let mut p = json!({
            "number": 5, "title": "t", "body": "Refs #2", "isDraft": false, "state": "OPEN", "mergeable": "MERGEABLE",
            "baseRefName": "main", "headRefName": "2-x", "updatedAt": "2026-01-02T00:00:00Z", "mergedAt": null,
            "author": {"login": "ann", "__typename": "User"}, "reviewDecision": null,
            "reviewRequests": {"nodes": [{"requestedReviewer": {"login": "bob"}}, {"requestedReviewer": {}}]},
            "commits": {"nodes": [{"commit": {"committedDate": "2026-01-01T10:00:00Z", "statusCheckRollup": {"state": "SUCCESS"}}}]},
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
            "openIssues": {"nodes": [{"number": 2, "title": "i", "state": "OPEN", "assignees": {"nodes": [{"login": "ann"}]}}]},
            "closedIssues": {"nodes": [{"number": 1, "title": "old", "state": "CLOSED", "assignees": {"nodes": []}}]},
            "refs": {"nodes": [{"name": "main"}, {"name": "3-wip"}]},
            "deployments": {"nodes": deployments}
        }}})
    }

    #[test]
    fn maps_an_open_pr() {
        let f = parse(&repo(vec![pr(json!({}))], vec![], json!([]))).unwrap();
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
        assert!(f.no_deployments);
        assert_eq!(f.people, vec!["ann"]);
    }

    #[test]
    fn maps_checks_review_and_conflict() {
        let c = |state: Value| {
            pr(
                json!({"commits": {"nodes": [{"commit": {"committedDate": "x", "statusCheckRollup": state}}]}}),
            )
        };
        let f = parse(&repo(
            vec![
                c(json!({"state": "FAILURE"})),
                c(json!({"state": "PENDING"})),
                c(Value::Null),
                pr(json!({"reviewDecision": "CHANGES_REQUESTED", "mergeable": "CONFLICTING", "isDraft": true})),
                pr(json!({"reviewDecision": "APPROVED"})),
            ],
            vec![],
            json!([]),
        ))
        .unwrap();
        assert_eq!(
            f.prs.iter().take(3).map(|p| p.checks).collect::<Vec<_>>(),
            vec![Checks::Failing(0), Checks::Pending, Checks::None]
        );
        assert_eq!(
            (f.prs[3].review, f.prs[3].has_conflict, f.prs[3].is_draft),
            (Review::ChangesRequested, true, true)
        );
        assert_eq!(f.prs[4].review, Review::Approved);
    }

    #[test]
    fn shipped_only_when_a_successful_production_deploy_covers_the_merge() {
        let merged = |at: &str| pr(json!({"state": "MERGED", "mergedAt": at}));
        let deps = json!([
            {"environment": "Preview", "latestStatus": {"state": "SUCCESS"}, "commit": {"committedDate": "2026-03-09T00:00:00Z"}},
            {"environment": "Production", "latestStatus": {"state": "SUCCESS"}, "commit": {"committedDate": "2026-03-05T00:00:00Z"}},
            {"environment": "Production", "latestStatus": {"state": "FAILURE"}, "commit": {"committedDate": "2026-03-08T00:00:00Z"}}
        ]);
        let f = parse(&repo(
            vec![],
            vec![
                merged("2026-03-04T00:00:00Z"),
                merged("2026-03-05T00:00:00Z"),
                merged("2026-03-06T00:00:00Z"),
            ],
            deps,
        ))
        .unwrap();
        assert_eq!(
            f.prs.iter().map(|p| p.shipped).collect::<Vec<_>>(),
            vec![true, true, false]
        );
        assert!(!f.no_deployments);
    }

    #[test]
    fn unanswered_comment_rules() {
        let u = |comments: Value, reviews: Value| {
            let f = parse(&repo(
                vec![pr(
                    json!({"comments": {"nodes": comments}, "reviews": {"nodes": reviews}}),
                )],
                vec![],
                json!([]),
            ))
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
        assert!(parse(&json!({"data": {"repository": null}})).is_none());
    }
}
