//! この PC の中に残す記録。窓の版は文字を出す場所が無いので、落ちた理由・入れ替えの失敗をファイルに書く。
//! 外には送らない。repo の中身・鍵は書かない(書くのは、何が起きたかの 1 行だけ)。
use std::io::Write;
use std::path::{Path, PathBuf};

/// これを超えたら、古い分を 1 つだけ残して書き始め直す
const CAP: u64 = 1_000_000;

/// 記録の置き場。Windows は `%LOCALAPPDATA%\asis`、ほかは `$XDG_STATE_HOME/asis`(無ければ `~/.local/state/asis`)。
/// 仕事場が WSL の中でも、窓が動いている PC の側に置く(窓が落ちた理由は、窓の側で読む)
pub fn dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
    }?;
    Some(base.join("asis"))
}

pub fn path() -> Option<PathBuf> {
    dir().map(|d| d.join("asis.log"))
}

/// 1970 年からの秒を `2026-11-19T04:05:06Z` に(世界標準時)
pub fn stamp(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // 日数から年月日へ(グレゴリオ暦。3 月を年の始まりに置いて、うるう日を年の終わりに寄せる)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// 1 行 足す。`cap` を超えていたら、今の分を `.1` に退かしてから書く
fn append(path: &Path, line: &str, cap: u64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > cap) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    // 1 行に収める(改行が入ると、どこからが次の記録か分からなくなる)
    writeln!(file, "{}", line.replace(['\n', '\r'], " "))
}

/// 記録に 1 行 足す。書けなくても、動きは止めない
pub fn line(text: &str) {
    let Some(path) = path() else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = append(&path, &format!("{} {text}", stamp(now)), CAP);
}

/// 落ちたとき、理由と場所を記録に残す。もとの出し方(文字で出す)も、そのまま続ける
pub fn record_panics(app: &str, version: &str) {
    let label = format!("{app} {version}");
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        line(&format!("panic in {label}: {info}"));
        before(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_become_a_date_and_a_time() {
        assert_eq!(stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(stamp(86_399), "1970-01-01T23:59:59Z");
        // うるう年の 2 月 29 日と、その次の日
        assert_eq!(stamp(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(stamp(1_709_251_200), "2024-03-01T00:00:00Z");
        assert_eq!(stamp(1_795_061_106), "2026-11-19T04:05:06Z");
        // 100 で割れて 400 で割れない年は、うるう年ではない
        assert_eq!(stamp(4_107_456_000), "2100-02-28T00:00:00Z");
        assert_eq!(stamp(4_107_542_400), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn lines_are_appended_one_per_record_and_an_old_file_is_set_aside() {
        let dir = std::env::temp_dir().join(format!("asis-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("deep").join("asis.log");
        append(&path, "first", 20).unwrap();
        append(&path, "second\nwith a break", 20).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "first\nsecond with a break\n"
        );
        // 上限を超えていたら、今の分を退かして書き始め直す
        append(&path, "third", 20).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "third\n");
        assert_eq!(
            std::fs::read_to_string(path.with_extension("log.1")).unwrap(),
            "first\nsecond with a break\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
