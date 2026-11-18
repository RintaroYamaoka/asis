//! 説明の文と失敗の理由の言葉(設定の `lang`)。
//!
//! 画面の大事な単語(Room・Merge・Approve など)は、どちらでも英語のまま。ここで分けるのは、説明の文だけ。
//! 言葉は 1 つの窓・1 つのコマンドで 1 つなので、起動したときと設定を変えたときに `set` する。
use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// 言葉を決める。`lang = "en"` なら true
pub fn set(english: bool) {
    ENGLISH.store(english, Ordering::Relaxed);
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// 日本語と英語の 2 つから、今の言葉の方を返す
pub fn t<'a>(ja: &'a str, en: &'a str) -> &'a str {
    pick(english(), ja, en)
}

fn pick<'a>(english: bool, ja: &'a str, en: &'a str) -> &'a str {
    if english {
        en
    } else {
        ja
    }
}

/// `format!` の日本語と英語の 2 つから、今の言葉の方で字を作る。`say!("{n} 件", "{n} items")`
#[macro_export]
macro_rules! say {
    ($ja:literal, $en:literal $(,)?) => {
        if $crate::lang::english() {
            format!($en)
        } else {
            format!($ja)
        }
    };
}

// 何か所にも出る失敗の理由

pub fn not_repo(repo: &str) -> String {
    say!(
        "`{repo}` は owner/name の形ではありません",
        "`{repo}` is not in the form owner/name"
    )
}

pub fn no_gh(e: impl std::fmt::Display) -> String {
    say!("gh を実行できません({e})", "Cannot run gh ({e})")
}

pub fn bad_reply(e: impl std::fmt::Display) -> String {
    say!("返事を読めません({e})", "Cannot read the reply ({e})")
}

pub fn stopped() -> String {
    t("読み込みが途中で止まりました", "Loading stopped partway").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_by_the_setting() {
        assert_eq!(pick(false, "読めません", "Cannot read"), "読めません");
        assert_eq!(pick(true, "読めません", "Cannot read"), "Cannot read");
        // 何も決めなければ日本語(テストは言葉を変えない)
        let n = 3;
        assert_eq!(say!("{n} 件", "{n} items"), "3 件");
    }
}
