# Froxel

The editor is the Rust crate at the repository root. `cargo run` opens it.
The binary is `froxel`.

## Layout

`src/main.rs` is the window. `state/` is the session: `document` (palette and
camera), `files` (open and save), `objects` (bones, the preview pose, clips,
and undo), `joints` (MCP `place_joints`, pivots at the joints between rig
parts), `pointer`,
`selection`, `drag`, `pick`, `geom`, and `vox/` (`.vox` read, write, the
`MVRG` rig chunk, and the `MVAN` clip chunk). `viewport.rs` is orbit and pointer input. `panels/` is the side
UI. `tools/` is brush shapes, generators, and the MCP sculpt shapes
(`sculpt.rs`: `fill_tube`, `carve`, `smooth`; `loft.rs`, `deform.rs`, and
`brush.rs`: `loft`, `deform`, and the sculpt `brush`; `collision.rs`: the box cover of MCP
`collision_from_voxels`; `fit.rs`: MCP `fit_to_views`, the views-fitting
search; `sheet_measure.rs`: MCP `measure_sheet`, a views sheet's crops,
cube pitch, checks, and parts; `mask_edit.rs`: the mask edits of `compare_silhouette` and the views,
`excludeColors`, `flood`, `mask`, `erase`, `keepLargest`; `paint_views.rs`: MCP `paint_from_views`; `pixelate.rs`: MCP `pixelate_views` and
the `downscale` of `carve_from_views` and `paint_from_views`;
`reduce.rs`: MCP `reduce_palette` and `import_mesh` `clean`; `colors.rs`: Oklab, median cut,
k-means, and the accent guard they share; `rig_split.rs`: MCP `split_parts`, the geodesic
split into rig parts; `upright.rs`: `import_mesh` `orient: "upright"`, the PCA stand-up). `mesh.rs` and `gpu.rs` draw the
edit-mode mesh. `trace/` is the CPU path tracer. `trace_gpu.rs` plus
`trace.wgsl` and `blit.wgsl` are the wgpu tracer used by Render mode.
`import/` reads `.glb`, `.gltf`, and `.obj` meshes and voxelizes them
(MCP `import_mesh`, `remote/import.rs`; Rhai `voxelize`, `script/import.rs`;
fixtures and their generator in `tests/fixtures/mesh`). `preview.rs` is the fast lit CPU raster of the viewport mesh behind MCP
`preview` and `froxel preview` (`docs/preview.md`). `export/` writes glTF. `native_menu.rs` is the macOS menu. `skills/froxel` is the agent skill for the MCP tools. `mcp.rs`,
`remote.rs`, and `control.rs` are the MCP server and the `--control` port;
`mcp/reload.rs` is MCP `reload` and `froxel mcp --watch` (the server
re-executes a rebuilt binary in place, keeps the document, and sends
`notifications/tools/list_changed`).
`bridge.rs` is the Godot/Blender loopback server (`froxel bridge` and
`--bridge`). Its `evaluate` runs inside `Session::transaction`: one undo step,
and the document is restored if a value or a script fails. The clients live in `godot/addons/froxel_bridge` and
`blender/froxel_bridge`. Unit tests live in `tests/` and are included from
the module they cover.

## .vox

Index 0 is empty. Hand sculpting is described in
`docs/hand-sculpting.md`. A loaded scene keeps its node graph (`VoxGraph` on the
session): groups, layers, every animation frame, and `_hidden`. Save writes
that graph back. Froxel saves also write `MVOR` v1 (the editor-to-file shift),
so their world cells and markers do not recenter on reopening. Files without
it retain import centring/floor placement, unless they carry `MVSC` or `MVMK`
(older Froxel files, which keep their node positions). A nonzero shift is stated
as `sceneShift` on open and export. Local cells may still rebase to the SIZE
corner. Frame 0 is the edited pose. Objects created in the editor
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
`colorAtlas` (`color_atlas`, `froxel bridge --color-atlas`) merges across
slots of one motion and opacity, paints cells into a per-mesh atlas, and
closes T-junctions by inserting the corner into the face outline and
ear-clipping it (`emit_atlas`), not by `split_junctions`.
`exportHidden` and `splitObjects` default to false. `aoStrength` (off at 0)
bakes per-corner ambient occlusion (`export/occlusion.rs`) into `COLOR_0`:
multiplied into vertex colours, or a grey `COLOR_0` beside the texture. A
shade is a function of the lattice point and the face only, and greedy merge
keeps apart faces whose corner shades differ. `aoTarget` `texture` (the
default with `colorAtlas`) darkens each cell's atlas texel instead (mean of
the corners), so the atlas merge and triangle count stay as without it;
`vertex` and `both` are the other choices. `froxel export` is the same
export from the command line. `bake` (`export/bake.rs`, with `colorAtlas`)
paints occlusion from real rays and a sun with shadows into the atlas texels,
for a game to draw unshaded: the material is `KHR_materials_unlit`, every face
uses one fixed ray pattern so there is no noise, and off leaves the file byte
for byte. `export/scene.rs` is MCP `bake_scene` and `froxel bake-scene`: placed
`.vox` files, in the frame of the glb they export to, plus occluders, into one
merged mesh and atlas; rays enter a placement through its inverse transform, over
one grid per source and a BVH. Clips are glTF
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

**V** **L** **B** **F** **S** **U** **M** select brush, line, box, face, sculpt,
mutator, and marquee. **T** **R** **G** select Add, Remove, and Paint, in Model mode only.
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

`python3 tests/mcp_reload.py` (after `cargo build`) reloads a running
`froxel mcp` by the tool and by `--watch` and checks the document and the
`tools/list_changed` notification survive.

`bridge_live.py` starts `target/debug/froxel bridge` (or `FROXEL_BIN`) on the
frog and asks for a glb. `bridge_godot.py` bakes the lily pad through the Godot node
when `GODOT_PATH` names Godot 4.3+ (it skips without it) and checks the
scenes with the bridge stopped:

```sh
cargo build && GODOT_PATH=/path/to/godot python3 tests/bridge_godot.py
```

## MCP parity

A capability added to MCP goes, in the same change, into:
- Rhai scripting, on the same code path;
- the editor UI;
- everything that describes it: UI tooltips and hints, the MCP server
  instructions and tool descriptions, `froxel --help` and command help, docs
  (mcp.md, scripting.md, hand-sculpting.md), the skill, and examples.

MCP-only plumbing (screenshots, ui_inspect, ui_click, session handling) is
exempt, and so are the tools that write files (render, model_view, preview,
export_mesh, export_models, bake_scene): scripts never write files. A feature is not done
until all of these exist. (User decision, 2026-11-02.)

## Git

Commit straight to `main`. Do not create branches.

## Formatting

Style is `rustfmt.toml`. Run `cargo fmt` before every commit and keep
`cargo fmt --check` clean; do not hand-format against it.

Comments, docs, and UI strings are English.
