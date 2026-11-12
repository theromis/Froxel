# Patched crates

`eframe` and `egui-wgpu` 0.37.0 from crates.io, with one change each so the
iOS build does not stretch: they size the wgpu surface from
`Window::inner_size`, which on iOS is only the safe area, while egui-winit
sizes egui's screen from `Window::outer_size` (the whole display). The patch
uses `outer_size` on iOS in both places and changes nothing elsewhere.

- `eframe/src/native/wgpu_integration.rs`: `surface_size` for `Resized`.
- `egui-wgpu/src/winit.rs`: the two surface creations.

`winit` 0.30.13 from crates.io, with the UIScene lifecycle added. iOS 27
stops an app built with the iOS 26 SDK or later at launch unless it adopts
UIScene, and winit 0.30 has none (eframe 0.37 needs winit 0.30).

- `winit/src/platform_impl/ios/scene.rs`: `WinitSceneDelegate`, the scene
  delegate that `packaging/ios/bundle.sh` names in the `Info.plist`
  `UIApplicationSceneManifest`. When UIKit connects the scene, winit windows
  created before then move into it and are made key; later windows join it
  at creation (`window.rs`). `event_loop.rs` registers the class before
  `UIApplicationMain`.
- The examples and tests are left out of the copy and its `Cargo.toml`.

`Cargo.toml` points at these copies through `[patch.crates-io]`. Drop the
eframe and egui-wgpu copies once upstream sizes the surface the same way,
and the winit copy once eframe moves to a winit with UIScene support.
