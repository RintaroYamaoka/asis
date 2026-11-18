//! asis のコマンド。中身は lib にある。
fn main() {
    if let Err(e) = asis::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
