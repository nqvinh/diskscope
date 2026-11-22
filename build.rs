//! Embeds the app icon (assets/icon/diskscope.ico) into diskscope.exe and diskscope-cli.exe, so Explorer,
//! the taskbar and Alt+Tab show it. Needs the Windows SDK resource compiler; without it the build still
//! succeeds, only without the icon (a cargo warning says so).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/diskscope.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon/diskscope.ico");
    if let Err(e) = res.compile() {
        println!("cargo:warning=app icon not embedded (Windows SDK resource compiler missing?): {e}");
    }
}
