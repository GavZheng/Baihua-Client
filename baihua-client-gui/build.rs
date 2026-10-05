//! Windows-only resource stamping: the executable icon plus version metadata, so
//! Explorer, the taskbar and installed shortcuts all show Baihua's own artwork.

fn main() {
    println!("cargo:rerun-if-changed=assets/baihua.ico");
    println!("cargo:rerun-if-changed=assets/images/icon.png");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resources = winresource::WindowsResource::new();
    resources.set_icon("assets/baihua.ico");
    resources.set(
        "ProductName",
        "Baihua - instant messenger, graphical client",
    );
    resources.set("FileDescription", "Baihua graphical client");
    resources.set("LegalCopyright", "Copyright (c) ChepleBob 2026");
    if let Err(error) = resources.compile() {
        panic!("the Windows resource set must compile: {error}");
    }
}
