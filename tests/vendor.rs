//! The vendored copies in `vendor/` carry fixes the App Store and iOS need.
//! `[patch.crates-io]` only applies while the locked version is the one the
//! copy has, so a lock that takes one of them from crates.io would silently
//! drop a fix.

#[test]
fn the_patched_crates_come_from_vendor_not_the_registry() {
    let lock = include_str!("../Cargo.lock");
    for name in ["eframe", "egui-wgpu", "winit"] {
        let packages: Vec<&str> = lock
            .split("[[package]]")
            .filter(|package| package.contains(&format!("\nname = \"{name}\"\n")))
            .collect();
        assert!(!packages.is_empty(), "{name} is not in Cargo.lock");
        for package in packages {
            assert!(
                !package.contains("\nsource = "),
                "{name} is locked from a registry, so the vendor/ patch is not used:{package}"
            );
        }
    }
}
