//! 新しい版に入れ替える。
//! 何を配るか(名前と今の版)は、起こす側が渡す。本体は、どこの誰が配っているかを知らない。
//! 版の一覧とファイルは部屋の DB に置く(`supabase/migrations/0005_releases.sql`)。
//! ここは、版を比べる・中身を確かめる・ファイルを入れ替える、だけを持つ。DB と話すのは `room`。
use crate::say;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// 配るもの。起こす側が渡す
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    /// DB の `app_releases.app` と同じ名前
    pub name: String,
    /// いま動いている版(`0.2.0`)
    pub version: String,
    /// 自分でビルドして使っている人のための、作り直しのコマンド(プログラムと引数)。
    /// これがあるときは、配っている実行ファイルには入れ替えない。新しい版が出たら、このコマンドで
    /// ソースから作り直す(自分でビルドした版を、黙って別の実行ファイルにしないため)
    pub rebuild: Option<Vec<String>>,
}

/// DB にある、いちばん新しい版
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Release {
    pub version: String,
    /// これより古い版は、もう使えない
    pub min_version: String,
    pub files: Vec<File>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct File {
    /// 入れる先での名前(`asis-gui.exe`)。フォルダは含まない
    pub name: String,
    /// 置き場の中での場所
    pub path: String,
    /// 中身の照合値(SHA-256 を 16 進で)
    pub sha256: String,
}

/// `1.2.3` を数字 3 つに。形が違えば `None`
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// いまの版でよい
    Current,
    /// 新しい版がある。いまの版も使える
    Newer,
    /// いまの版は、もう使えない
    TooOld,
}

/// いまの版と、DB の版を比べる。読めない版は、何もしない側に倒す(勝手に入れ替えない)
pub fn decide(current: &str, release: &Release) -> Verdict {
    let (Some(now), Some(latest)) = (parse_version(current), parse_version(&release.version))
    else {
        return Verdict::Current;
    };
    if parse_version(&release.min_version).is_some_and(|min| now < min) {
        Verdict::TooOld
    } else if now < latest {
        Verdict::Newer
    } else {
        Verdict::Current
    }
}

/// DB の返事(行の並び)から、いちばん新しい版を読む。行が無ければ `None`
pub fn parse_release(text: &str) -> Result<Option<Release>, String> {
    let rows: Vec<Release> = serde_json::from_str(text).map_err(|e| {
        say!(
            "版の一覧を読めません: {e}",
            "Cannot read the list of versions: {e}"
        )
    })?;
    Ok(rows.into_iter().next())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 入れ替えてよい名前か。フォルダをまたぐ名前・実行ファイルでない名前は入れない
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.starts_with('.')
        && (name.ends_with(".exe") || !name.contains('.'))
}

fn with_suffix(dir: &Path, name: &str, suffix: &str) -> PathBuf {
    dir.join(format!("{name}{suffix}"))
}

/// 入れ替えた版を覚えておくファイル。動いている版は古いままなので、次に確かめたとき、同じ版をもう一度 取らないため
const MARK: &str = "asis.version";

/// このフォルダに、もう入れ替え済みの版(次に開くと、その版で動く)
pub fn staged_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(MARK)).ok()?;
    parse_version(&text).map(|_| text.trim().to_string())
}

/// 前の入れ替えで退かした古いファイルを消す。動いている最中は消せないので、起動のたびに試す
pub fn clean(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".old") || name.ends_with(".new") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// `dir` の中のファイルを、新しい版に入れ替える。
/// 1. このフォルダに今あるファイルだけを対象にする(無いものは足さない)
/// 2. 全部 取って、全部の照合値が合ってから、入れ替えを始める(途中で止まっても、古い版がそのまま残る)
/// 3. 動いているファイルは上書きできないので、別名に退かしてから新しいものを置く。失敗したら元に戻す
///
/// 返すのは、入れ替えたファイルの名前
pub fn apply(
    dir: &Path,
    release: &Release,
    fetch: &dyn Fn(&File) -> Result<Vec<u8>, String>,
) -> Result<Vec<String>, String> {
    let io = |what: &Path, e: std::io::Error| format!("{}: {e}", what.display());
    let wanted: Vec<&File> = release
        .files
        .iter()
        .filter(|f| safe_name(&f.name) && dir.join(&f.name).is_file())
        .collect();
    if wanted.is_empty() {
        return Err(say!(
            "新しい版に、このフォルダのファイルがありません",
            "The new version has no file for this folder"
        ));
    }
    for file in &wanted {
        let bytes = fetch(file)?;
        let got = sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(file.sha256.trim()) {
            clean(dir);
            let name = &file.name;
            return Err(say!(
                "{name} の中身が、版の記録と合いません。入れ替えません",
                "{name} does not match the record of the version. Not replacing it"
            ));
        }
        let new = with_suffix(dir, &file.name, ".new");
        std::fs::write(&new, &bytes).map_err(|e| io(&new, e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755));
        }
    }
    let mut done: Vec<&File> = Vec::new();
    for file in &wanted {
        let live = dir.join(&file.name);
        let old = with_suffix(dir, &file.name, ".old");
        let new = with_suffix(dir, &file.name, ".new");
        let _ = std::fs::remove_file(&old);
        let swapped = std::fs::rename(&live, &old)
            .map_err(|e| io(&live, e))
            .and_then(|()| {
                std::fs::rename(&new, &live).map_err(|e| {
                    // 新しいものを置けなかった。退かした古いものを戻す
                    let _ = std::fs::rename(&old, &live);
                    io(&new, e)
                })
            });
        if let Err(e) = swapped {
            // ここまでに入れ替えたものも、古い版に戻す(版が混ざったまま動かさない)
            for file in done {
                let live = dir.join(&file.name);
                let old = with_suffix(dir, &file.name, ".old");
                let _ = std::fs::remove_file(&live);
                let _ = std::fs::rename(&old, &live);
            }
            clean(dir);
            return Err(e);
        }
        done.push(file);
    }
    let mark = dir.join(MARK);
    std::fs::write(&mark, &release.version).map_err(|e| io(&mark, e))?;
    Ok(wanted.iter().map(|f| f.name.clone()).collect())
}

/// 確かめた結果、次にすること
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// 何もしない
    Nothing,
    /// もう入れ替えてある。次に開くと、その版で動く
    Ready(String),
    /// 取って入れ替える。`must` は、いま動いている版がもう使えないとき
    Fetch { must: bool },
}

/// いま動いている版・このフォルダに入れ替え済みの版・DB の版から、次にすることを決める
pub fn plan(running: &str, staged: Option<&str>, release: &Release) -> Step {
    let must = decide(running, release) == Verdict::TooOld;
    // 入れ替え済みの版が DB の版に追いついていれば、もう取らない
    if let Some(staged) = staged {
        if decide(staged, release) == Verdict::Current
            && decide(running, release) != Verdict::Current
        {
            return Step::Ready(staged.to_string());
        }
    }
    match decide(running, release) {
        Verdict::Current => Step::Nothing,
        Verdict::Newer | Verdict::TooOld => Step::Fetch { must },
    }
}

/// 画面に出す結果
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Nothing,
    /// 入れ替えた。次に開くと、この版で動く
    Ready(String),
    /// いまの版はもう使えないのに、入れ替えられなかった(理由つき)
    Blocked(String),
    /// 新しい版が出ている。ソースから作り直す(自分でビルドして使っている人)
    Available(String),
}

/// 作り直しを、その版でもう頼んだか。頼んだ版を覚えておき、同じ版で何度も作り直さない
/// (作り直しに失敗したとき、開くたびに繰り返さないため。もう一度やるのは、人が押したとき)
const ASKED: &str = "asis.rebuild";

pub fn rebuild_asked(dir: &Path, version: &str) -> bool {
    std::fs::read_to_string(dir.join(ASKED)).is_ok_and(|t| t.trim() == version)
}

/// 作り直しのコマンドを起こす。待たない(作り直しは数分かかる。終わると、次に開いたとき新しい版で動く)
pub fn rebuild(app: &App, dir: &Path, version: &str) -> Result<(), String> {
    let Some([program, args @ ..]) = app.rebuild.as_deref() else {
        return Err(say!(
            "作り直しのコマンドがありません",
            "There is no command to rebuild with"
        ));
    };
    let _ = std::fs::write(dir.join(ASKED), version);
    crate::log::line(&format!(
        "update: {} {} -> {version}: rebuilding from source ({program})",
        app.name, app.version
    ));
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{program}: {e}"))
}

/// 1 回 確かめて、新しければ入れ替える。Sign in していない・DB が無い・つながらないときは、何もしない。
/// 失敗は記録に残す(`crate::log`)。動きは止めない
pub fn run_once(app: &App, dir: &Path, db: &crate::config::Db) -> Outcome {
    let client = match crate::room::Client::open(db) {
        Ok(Some(client)) => client,
        Ok(None) => return Outcome::Nothing,
        Err(e) => {
            crate::log::line(&format!("update: {e}"));
            return Outcome::Nothing;
        }
    };
    let release = match client.latest_release(&app.name) {
        Ok(Some(release)) => release,
        Ok(None) => return Outcome::Nothing,
        Err(e) => {
            crate::log::line(&format!("update: {e}"));
            return Outcome::Nothing;
        }
    };
    if app.rebuild.is_some() {
        return match decide(&app.version, &release) {
            Verdict::Current => Outcome::Nothing,
            Verdict::Newer | Verdict::TooOld => Outcome::Available(release.version),
        };
    }
    match plan(&app.version, staged_version(dir).as_deref(), &release) {
        Step::Nothing => Outcome::Nothing,
        Step::Ready(version) => Outcome::Ready(version),
        Step::Fetch { must } => {
            let fetch = |file: &File| client.release_file(&file.path);
            match apply(dir, &release, &fetch) {
                Ok(names) => {
                    crate::log::line(&format!(
                        "update: {} {} -> {} ({})",
                        app.name,
                        app.version,
                        release.version,
                        names.join(", ")
                    ));
                    Outcome::Ready(release.version)
                }
                Err(e) => {
                    crate::log::line(&format!(
                        "update: {} {} -> {} failed: {e}",
                        app.name, app.version, release.version
                    ));
                    if must {
                        Outcome::Blocked(e)
                    } else {
                        Outcome::Nothing
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rebuild_is_asked_once_per_version() {
        let dir = folder("ask", &[]);
        let app = App {
            name: "my-app".into(),
            version: "0.1.0".into(),
            // 何もしないで終わるコマンド
            rebuild: Some(vec![
                std::env::current_exe().unwrap().display().to_string(),
                "--list".into(),
            ]),
        };
        assert!(!rebuild_asked(&dir, "0.2.0"));
        rebuild(&app, &dir, "0.2.0").unwrap();
        assert!(rebuild_asked(&dir, "0.2.0"));
        // 次の版が出たら、また頼む
        assert!(!rebuild_asked(&dir, "0.3.0"));
        // コマンドが無ければ、頼めない
        let none = App {
            rebuild: None,
            ..app.clone()
        };
        assert!(rebuild(&none, &dir, "0.3.0").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_next_step_follows_the_running_the_staged_and_the_released_version() {
        let r = release("0.3.0", "0.2.0", &[]);
        assert_eq!(plan("0.3.0", None, &r), Step::Nothing);
        assert_eq!(plan("0.2.0", None, &r), Step::Fetch { must: false });
        assert_eq!(plan("0.1.0", None, &r), Step::Fetch { must: true });
        // もう入れ替えてあれば、取り直さない
        assert_eq!(
            plan("0.2.0", Some("0.3.0"), &r),
            Step::Ready("0.3.0".into())
        );
        assert_eq!(
            plan("0.1.0", Some("0.3.0"), &r),
            Step::Ready("0.3.0".into())
        );
        // 入れ替えてあるのが 1 つ前の版なら、取り直す
        assert_eq!(plan("0.1.0", Some("0.2.0"), &r), Step::Fetch { must: true });
        // 開き直して新しい版で動いていれば、前の印が残っていても何もしない
        assert_eq!(plan("0.3.0", Some("0.3.0"), &r), Step::Nothing);
    }

    fn release(version: &str, min: &str, files: &[(&str, &[u8])]) -> Release {
        Release {
            version: version.into(),
            min_version: min.into(),
            files: files
                .iter()
                .map(|(name, bytes)| File {
                    name: name.to_string(),
                    path: format!("app/{version}/{name}"),
                    sha256: sha256_hex(bytes),
                })
                .collect(),
        }
    }

    fn folder(tag: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("asis-update-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        dir
    }

    fn read(dir: &Path, name: &str) -> Vec<u8> {
        std::fs::read(dir.join(name)).unwrap()
    }

    #[test]
    fn versions_are_three_numbers() {
        assert_eq!(parse_version("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version(" 1.10.3\n"), Some((1, 10, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2.x"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn a_newer_version_is_offered_and_a_too_old_one_is_refused() {
        let r = release("0.3.0", "0.2.0", &[]);
        assert_eq!(decide("0.3.0", &r), Verdict::Current);
        assert_eq!(decide("0.4.0", &r), Verdict::Current);
        assert_eq!(decide("0.2.0", &r), Verdict::Newer);
        assert_eq!(decide("0.2.9", &r), Verdict::Newer);
        assert_eq!(decide("0.1.9", &r), Verdict::TooOld);
        // 数字として比べる(字の順だと 0.10.0 が 0.9.0 より前になる)
        assert_eq!(
            decide("0.9.0", &release("0.10.0", "0.1.0", &[])),
            Verdict::Newer
        );
        // 読めない版は、何もしない
        assert_eq!(decide("dev", &r), Verdict::Current);
        assert_eq!(
            decide("0.1.0", &release("next", "0.0.0", &[])),
            Verdict::Current
        );
    }

    #[test]
    fn the_newest_row_is_read_and_no_row_is_none() {
        let text = r#"[{"version":"0.2.0","min_version":"0.1.0",
            "files":[{"name":"a.exe","path":"app/0.2.0/a.exe","sha256":"ab"}]}]"#;
        let r = parse_release(text).unwrap().unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.files[0].path, "app/0.2.0/a.exe");
        assert_eq!(parse_release("[]").unwrap(), None);
        assert!(parse_release("{").is_err());
    }

    #[test]
    fn files_in_the_folder_are_replaced_and_the_old_ones_are_set_aside() {
        let dir = folder("ok", &[("a.exe", b"old a"), ("b.exe", b"old b")]);
        // c.exe はこのフォルダに無いので、足さない
        let r = release(
            "0.2.0",
            "0.1.0",
            &[
                ("a.exe", b"new a"),
                ("b.exe", b"new b"),
                ("c.exe", b"new c"),
            ],
        );
        let fetch = |f: &File| Ok(format!("new {}", &f.name[..1]).into_bytes());
        assert_eq!(apply(&dir, &r, &fetch).unwrap(), vec!["a.exe", "b.exe"]);
        assert_eq!(read(&dir, "a.exe"), b"new a");
        assert_eq!(read(&dir, "b.exe"), b"new b");
        assert_eq!(read(&dir, "a.exe.old"), b"old a");
        assert!(!dir.join("c.exe").exists());
        assert_eq!(staged_version(&dir).as_deref(), Some("0.2.0"));
        // 次に開いたとき、退かしたものを消す
        clean(&dir);
        assert!(!dir.join("a.exe.old").exists());
        assert_eq!(read(&dir, "a.exe"), b"new a");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nothing_is_replaced_when_one_file_does_not_match_its_record() {
        let dir = folder("bad", &[("a.exe", b"old a"), ("b.exe", b"old b")]);
        let r = release(
            "0.2.0",
            "0.1.0",
            &[("a.exe", b"new a"), ("b.exe", b"new b")],
        );
        // b.exe だけ、途中で中身が変わった
        let fetch = |f: &File| {
            Ok(if f.name == "b.exe" {
                b"tampered".to_vec()
            } else {
                b"new a".to_vec()
            })
        };
        assert!(apply(&dir, &r, &fetch).unwrap_err().contains("b.exe"));
        assert_eq!(read(&dir, "a.exe"), b"old a");
        assert_eq!(read(&dir, "b.exe"), b"old b");
        assert!(!dir.join("a.exe.new").exists());
        assert_eq!(staged_version(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nothing_is_replaced_when_a_download_fails() {
        let dir = folder("net", &[("a.exe", b"old a"), ("b.exe", b"old b")]);
        let r = release(
            "0.2.0",
            "0.1.0",
            &[("a.exe", b"new a"), ("b.exe", b"new b")],
        );
        let fetch = |f: &File| {
            if f.name == "b.exe" {
                Err("offline".to_string())
            } else {
                Ok(b"new a".to_vec())
            }
        };
        assert_eq!(apply(&dir, &r, &fetch).unwrap_err(), "offline");
        assert_eq!(read(&dir, "a.exe"), b"old a");
        assert_eq!(staged_version(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_that_leave_the_folder_or_are_not_programs_are_ignored() {
        assert!(safe_name("asis-gui.exe"));
        assert!(safe_name("asis"));
        assert!(!safe_name("../asis.exe"));
        assert!(!safe_name("sub/asis.exe"));
        assert!(!safe_name("sub\\asis.exe"));
        assert!(!safe_name(".profile"));
        assert!(!safe_name("config.toml"));
        assert!(!safe_name(""));
        let dir = folder("names", &[("config.toml", b"mine")]);
        let r = release("0.2.0", "0.1.0", &[("config.toml", b"theirs")]);
        assert!(apply(&dir, &r, &|_| Ok(b"theirs".to_vec())).is_err());
        assert_eq!(read(&dir, "config.toml"), b"mine");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
