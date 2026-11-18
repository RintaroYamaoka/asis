//! どこで仕事をするか。
//!
//! 画面は、画面のある OS で動かす。仕事(gh・git を呼ぶ、設定を読む)は、仕事のある所でする。
//! ふつうは、どちらもこの PC。違うのは、Windows の窓が WSL の中の仕事を見るとき:
//! WSL から開くと、窓は Windows 版が出て(`ASIS_WSL` に distro の名前が入る)、
//! 外のコマンドは WSL の中で動く。gh のログインも、手元の作業も、WSL のものがそのまま使われる。
//!
//! (WSL の中で窓を出すこともできるが、画面を Windows へ運ぶぶん、どうしても動きが落ちる)
use std::process::Command;

/// 仕事場が WSL の中なら、その distro の名前
pub fn wsl() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var("ASIS_WSL").ok().filter(|d| !d.is_empty())
}

/// 仕事場のコマンド(gh・git・herdr など)
pub fn command(program: &str) -> Command {
    match wsl() {
        Some(distro) => {
            let mut c = local("wsl.exe");
            c.args(["-d", &distro, "-e", program]);
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
