//! どこで仕事をするか。
//!
//! 画面は、画面のある OS で動かす。仕事(gh・git を呼ぶ、設定を読む)は、仕事のある所でする。
//! ふつうは、どちらもこの PC。違うのは、Windows の窓が WSL の中の仕事を見るとき:
//! WSL から開くと、窓は Windows 版が出て(`--wsl <distro>` が付く)、
//! 外のコマンドは WSL の中で動く。gh のログインも、手元の作業も、WSL のものがそのまま使われる。
//!
//! (WSL の中で窓を出すこともできるが、画面を Windows へ運ぶぶん、どうしても動きが落ちる)
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// 起動のときの指定から決まったもの
struct Host {
    /// 仕事場が WSL の中なら、その distro の名前
    wsl: Option<String>,
    /// `--wsl` を除いた、残りの指定
    args: Vec<String>,
}

static HOST: OnceLock<Host> = OnceLock::new();

fn host() -> &'static Host {
    HOST.get_or_init(|| {
        let (wsl, args) = split_args(std::env::args().skip(1).collect());
        Host {
            // WSL の中の仕事を見るのは、Windows の窓だけ
            wsl: wsl.filter(|_| cfg!(windows)),
            args,
        }
    })
}

/// 指定から `--wsl <distro>` を抜き出す
fn split_args(mut args: Vec<String>) -> (Option<String>, Vec<String>) {
    match args.iter().position(|a| a == "--wsl") {
        Some(i) if i + 1 < args.len() => {
            let distro = args.remove(i + 1);
            args.remove(i);
            (Some(distro), args)
        }
        _ => (None, args),
    }
}

/// 起動のときの指定(`--wsl <distro>` は除く)
pub fn args() -> Vec<String> {
    host().args.clone()
}

/// 仕事場が WSL の中なら、その distro の名前
pub fn wsl() -> Option<&'static str> {
    host().wsl.as_deref()
}

/// 仕事場が WSL の中のとき、その中の設定の置き場を、Windows から見える書き方で返す
/// (`\\wsl.localhost\Ubuntu\home\…\.config\asis`)。聞くのは 1 回だけ
pub fn wsl_config_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        wsl()?;
        let out = command("sh")
            .args([
                "-c",
                r#"wslpath -w "${XDG_CONFIG_HOME:-$HOME/.config}/asis""#,
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!dir.is_empty()).then(|| PathBuf::from(dir))
    })
    .clone()
}

/// 仕事場のホーム。仕事場が WSL の中なら、その中のホームを Windows から見える書き方で返す。聞くのは 1 回だけ
pub fn home_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        if wsl().is_some() {
            let out = command("sh")
                .args(["-c", r#"wslpath -w "$HOME""#])
                .output()
                .ok()
                .filter(|o| o.status.success())?;
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return (!dir.is_empty()).then(|| PathBuf::from(dir));
        }
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    })
    .clone()
}

/// 仕事場のコマンド(gh・git・herdr など)
pub fn command(program: &str) -> Command {
    match wsl() {
        Some(distro) => {
            let mut c = local("wsl.exe");
            c.args(["-d", distro, "-e", program]);
            c
        }
        None => local(program),
    }
}

/// この PC のコマンド(ブラウザを開く・OS の設定を読む)。
/// Windows の窓のアプリから呼ぶと、呼ぶたびに黒い窓が一瞬 出るので、出さない印を付ける
pub fn local(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wsl_flag_is_taken_out_of_the_arguments() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(
            split_args(args(&["--wsl", "Ubuntu", "o/r"])),
            (Some("Ubuntu".into()), args(&["o/r"]))
        );
        assert_eq!(
            split_args(args(&["o/r", "--plain", "--wsl", "Debian"])),
            (Some("Debian".into()), args(&["o/r", "--plain"]))
        );
        assert_eq!(split_args(args(&["o/r"])), (None, args(&["o/r"])));
        // 名前の無い `--wsl` は、そのまま残す(知らない指定として断られる)
        assert_eq!(split_args(args(&["--wsl"])), (None, args(&["--wsl"])));
    }
}
