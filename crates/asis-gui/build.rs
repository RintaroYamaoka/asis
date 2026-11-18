//! Windows の exe に印(アイコン)を入れる。ほかの OS では何もしない。
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            // 印が入らなくても、アプリは動く
            println!("cargo:warning=アイコンを入れられませんでした: {e}");
        }
    }
}
