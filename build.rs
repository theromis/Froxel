//! Gives the Windows executable the Froxel icon.

fn main() {
    println!("cargo:rerun-if-changed=packaging/icons/froxel.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/icons/froxel.ico");
        res.compile().expect("embed the Windows icon");
    }
}
