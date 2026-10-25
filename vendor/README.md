# Patched crates

`eframe` and `egui-wgpu` 0.37.0 from crates.io, with one change each so the
iOS build does not stretch: they size the wgpu surface from
`Window::inner_size`, which on iOS is only the safe area, while egui-winit
sizes egui's screen from `Window::outer_size` (the whole display). The patch
uses `outer_size` on iOS in both places and changes nothing elsewhere.

- `eframe/src/native/wgpu_integration.rs`: `surface_size` for `Resized`.
- `egui-wgpu/src/winit.rs`: the two surface creations.

`Cargo.toml` points at these copies through `[patch.crates-io]`. Drop them
once upstream sizes the surface the same way.
