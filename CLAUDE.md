# Froxel

The editor is the Rust crate at the repository root. `cargo run` opens it.
The binary is `froxel`.

## Layout

`src/main.rs` is the window. `state/` is the session: `document` (palette and
camera), `files` (open and save), `objects` (bones, the preview pose, clips,
and undo), `pointer`,
`selection`, `drag`, `pick`, `geom`, and `vox/` (`.vox` read, write, the
`MVRG` rig chunk, and the `MVAN` clip chunk). `viewport.rs` is orbit and pointer input. `panels/` is the side
UI. `tools/` is brush shapes and generators. `mesh.rs` and `gpu.rs` draw the
edit-mode mesh. `trace/` is the CPU path tracer. `trace_gpu.rs` plus
`trace.wgsl` and `blit.wgsl` are the wgpu tracer used by Render mode.
`export/` writes glTF. `native_menu.rs` is the macOS menu. `mcp.rs`,
`remote.rs`, and `control.rs` are the MCP server and the `--control` port.
`bridge.rs` is the Godot/Blender loopback server (`froxel bridge` and
`--bridge`). Its `evaluate` runs inside `Session::transaction`: one undo step,
and the document is restored if a value or a script fails. The clients live in `godot/addons/froxel_bridge` and
`blender/froxel_bridge`. Unit tests live in `tests/` and are included from
the module they cover.

## .vox

Index 0 is empty. Hand sculpting is described in
`docs/hand-sculpting.md`. A loaded scene keeps its node graph (`VoxGraph` on the
session): groups, layers, every animation frame, and `_hidden`. Save writes
that graph back. Frame 0 is the edited pose. Objects created in the editor
are appended. Hidden objects stay in the file and are omitted from the mesh,
the tracer, and mesh export.

Cells of a rotated model are baked into world space for editing. `Volume.rot`,
`pivot`, and `model_dims` put them back into model space on save.

## Mesh export

`export_glb` merges coplanar faces of one palette slot, then
`split_junctions` cuts an edge where a corner of another face lands in the
open span. The same plane and the same slot is not a cut. Positions are glTF
Y-up `(x, z, -y)`. The menu export uses a palette texture. `COLOR_0` is
linear. MCP `export_mesh` leaves merge off unless the caller sets it.
`exportHidden` and `splitObjects` default to false. Clips are glTF
animations on the joint nodes. The preview pose is not the bind pose:
it is a local quaternion per bone, and an empty pose leaves the mesh and
the tracer unchanged.

## Render

Render mode dispatches `trace.wgsl`. The CPU tracer in `src/trace` is the
fallback and the tested algorithm; the shader uses the same sky and BSDF.
Atmospheric sky is Nishita single-scatter (Rayleigh and Mie), lit by a fixed
irradiance times the sun colour and Sky intensity, not by the Sun slider or
disc size. Below the horizon it fades into the flat sky's ground colour. The
sun disc is added in both skies. Off, the sky is the flat horizon gradient. Metal samples a GGX visible normal with probability
`metallic` and otherwise uses the diffuse estimator. Glass roughness samples
that normal, then Fresnel and Snell; roughness 0 is a single delta ray.
Absorption glass stays a tinted straight ray. Save Render writes the PNG once
the frame reaches Samples; leaving Render mode or an edit that restarts the
frame cancels it. Denoise is the edge-aware filter unless the crate is built
with `--features oidn`, which links Open Image Denoise and applies it to the
finished frame from the albedo and normal guides. That feature needs the
Open Image Denoise library on the build machine (`pkg-config`); the default
build does not. The Render panel names the denoiser that is linked. Yaw 0 places the camera on +X looking toward −X; a model facing −Y is face-on at yaw −90. `froxel render` is the same still with no window:
CPU by default, `--gpu` for the wgpu tracer. `froxel voxrt` checks a `.vox`
round trip, the save fixpoint, and the rotation codec. MCP `model_view` is the flat model
view: each pixel is the palette colour of the first cell hit, with no light
or tonemap. MCP `set_view` moves the window camera; its `orthographic` also
switches the edit viewport.

**V** **L** **B** **F** **U** **M** select brush, line, box, face, mutator, and
marquee. **T** **R** **G** select Add, Remove, and Paint, in Model mode only.
**Tab** switches Model and Object mode. In Object mode **X** **Y** **Z** rotate
and Shift flips.

## Tests

```sh
cargo test --offline --bin froxel -- --skip ping_round_trips
```

`ping_round_trips_with_the_token` needs a socket the sandbox does not allow.
Do not filter on the substring `ping`: `save_round_trips` matches it too.
The package is a binary, so `--lib` does not apply.

The bridge clients have their own checks:

```sh
python3 tests/bridge_protocol.py
cargo build && python3 tests/bridge_live.py
```

`bridge_live.py` starts `target/debug/froxel bridge` (or `FROXEL_BIN`) on the
frog and asks for a glb. `bridge_godot.py` bakes the lily pad through the Godot node
when `GODOT_PATH` names Godot 4.3+ (it skips without it) and checks the
scenes with the bridge stopped:

```sh
cargo build && GODOT_PATH=/path/to/godot python3 tests/bridge_godot.py
```

## Git

Commit straight to `main`. Do not create branches.

## Formatting

Style is `rustfmt.toml`. Run `cargo fmt` before every commit and keep
`cargo fmt --check` clean; do not hand-format against it.

Comments, docs, and UI strings are English.
