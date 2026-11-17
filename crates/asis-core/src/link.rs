//! PR をタスクにつなぐ。設計書 §4。推測はしない。
use crate::facts::*;

const KEYWORDS: &[&str] = &[
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved", "ref",
    "refs",
];

/// 本文の `Closes #12` / `Refs #12` / `Part of owner/repo#12` を拾う。
/// 合図の語の直後にある参照だけを取る(本文にただ書かれた `#12` はつながない)。
pub fn refs_in_body(body: &str, repo: &str) -> Vec<IssueRef> {
    let words: Vec<&str> = body.split_whitespace().collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let key = w.trim_end_matches(':').to_ascii_lowercase();
        let is_keyword = KEYWORDS.contains(&key.as_str())
            || (key == "of" && i > 0 && words[i - 1].eq_ignore_ascii_case("part"));
        if !is_keyword {
            continue;
        }
        let found = words.get(i + 1).and_then(|next| parse_ref(next, repo));
        out.extend(found.filter(|r| !out.contains(r)));
    }
    out
}

/// `#12` か `owner/repo#12`。後ろの句読点は捨てる。
fn parse_ref(word: &str, repo: &str) -> Option<IssueRef> {
    let word = word.trim_end_matches(|c: char| !c.is_ascii_digit());
    let (prefix, digits) = word.split_once('#')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: u64 = digits.parse().ok().filter(|n| *n > 0)?;
    let repo = match prefix {
        "" => repo.to_string(),
        p if p.split('/').count() == 2 && p.split('/').all(|s| !s.is_empty()) => p.to_string(),
        _ => return None,
    };
    Some(IssueRef { repo, number })
}

/// ブランチ名が issue の番号で始まる(`12-short-name`)。
/// 種類を前に付ける決まりの repo に合わせて、最後の区切りの頭でも読む(`feature/12-short-name`)。
pub fn ref_in_branch(head_ref: &str, repo: &str) -> Option<IssueRef> {
    let last = head_ref.rsplit('/').next()?;
    let (digits, rest) = last.split_once('-')?;
    if rest.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: u64 = digits.parse().ok().filter(|n| *n > 0)?;
    Some(IssueRef {
        repo: repo.to_string(),
        number,
    })
}

/// 各 PR がつながるタスク。入力と同じ順で返す。
/// GitHub のリンク・本文・ブランチ名で直接つながり、積んだ PR は下の PR のつながりを継ぐ。
pub fn link_all(prs: &[PrFacts]) -> Vec<Vec<IssueRef>> {
    let mut links: Vec<Vec<IssueRef>> = prs
        .iter()
        .map(|p| {
            let mut v = p.closes.clone();
            v.extend(refs_in_body(&p.body, &p.repo));
            v.extend(ref_in_branch(&p.head_ref, &p.repo));
            v
        })
        .collect();
    // 積み上げは何段でもあるので、増えなくなるまで回す。
    loop {
        let mut grew = false;
        for i in 0..prs.len() {
            let Some(below) = prs
                .iter()
                .position(|q| q.repo == prs[i].repo && q.head_ref == prs[i].base_ref)
            else {
                continue;
            };
            for r in links[below].clone() {
                if !links[i].contains(&r) {
                    links[i].push(r);
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    for v in &mut links {
        v.sort();
        v.dedup();
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pr::tests::pr;

    fn r(repo: &str, n: u64) -> IssueRef {
        IssueRef {
            repo: repo.into(),
            number: n,
        }
    }

    #[test]
    fn body_keywords() {
        assert_eq!(refs_in_body("Closes #12", "o/r"), vec![r("o/r", 12)]);
        assert_eq!(
            refs_in_body("refs: #7 and Fixes #8.", "o/r"),
            vec![r("o/r", 7), r("o/r", 8)]
        );
        assert_eq!(
            refs_in_body("Part of o/other#3\nresolved #4", "o/r"),
            vec![r("o/other", 3), r("o/r", 4)]
        );
        assert_eq!(
            refs_in_body("Refs #12, Closes #12", "o/r"),
            vec![r("o/r", 12)]
        );
    }

    #[test]
    fn body_without_keyword_is_not_a_link() {
        assert!(refs_in_body("see #12 for context, like PR #9", "o/r").is_empty());
        assert!(refs_in_body("closes the gap in #12", "o/r").is_empty());
        assert!(refs_in_body("Closes #abc, Refs #", "o/r").is_empty());
        assert!(refs_in_body("", "o/r").is_empty());
    }

    #[test]
    fn branch_prefix() {
        assert_eq!(ref_in_branch("12-add-login", "o/r"), Some(r("o/r", 12)));
        assert_eq!(ref_in_branch("12", "o/r"), None);
        // 種類を前に付ける決まりの repo(`feature/12-add`)でも読む。番号は最後の区切りの頭
        assert_eq!(ref_in_branch("feature/12-add", "o/r"), Some(r("o/r", 12)));
        assert_eq!(ref_in_branch("user/fix/7-x", "o/r"), Some(r("o/r", 7)));
        assert_eq!(ref_in_branch("12-add/extra", "o/r"), None);
        assert_eq!(ref_in_branch("feature/add-12", "o/r"), None);
        assert_eq!(ref_in_branch("v2-rewrite", "o/r"), None);
        assert_eq!(ref_in_branch("0-zero", "o/r"), None);
    }

    #[test]
    fn link_sources_are_merged_and_stacks_inherit() {
        let mut a = pr();
        a.number = 1;
        a.head_ref = "12-base".into();
        a.closes = vec![r("o/r", 30)];
        let mut b = pr();
        b.number = 2;
        b.head_ref = "more".into();
        b.base_ref = "12-base".into();
        let mut c = pr();
        c.number = 3;
        c.head_ref = "top".into();
        c.base_ref = "more".into();
        c.body = "Refs #40".into();
        let mut d = pr();
        d.number = 4;
        d.head_ref = "chore".into();
        let got = link_all(&[c.clone(), a.clone(), b.clone(), d.clone()]);
        assert_eq!(got[1], vec![r("o/r", 12), r("o/r", 30)]);
        assert_eq!(got[2], vec![r("o/r", 12), r("o/r", 30)]);
        assert_eq!(got[0], vec![r("o/r", 12), r("o/r", 30), r("o/r", 40)]);
        assert!(got[3].is_empty());
    }

    #[test]
    fn stack_does_not_cross_repos() {
        let mut a = pr();
        a.head_ref = "12-base".into();
        let mut b = pr();
        b.repo = "o/other".into();
        b.head_ref = "x".into();
        b.base_ref = "12-base".into();
        let got = link_all(&[a, b]);
        assert!(got[1].is_empty());
    }
}
