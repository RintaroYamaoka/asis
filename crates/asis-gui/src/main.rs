//! asis の窓の画面。中身は lib にある(読み口を差し込んで開く別の入口からも使うため)。
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> iced::Result {
    asis_gui::run(None)
}
