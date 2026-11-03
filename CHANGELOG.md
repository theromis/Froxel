# Changelog

## Unreleased

### Added

- Contact sheets and camera presets: MCP `preview` and `froxel preview`
  take `inputs` (several `.vox` files, or the open document with
  parameters), `variants` (parameter sets of the scripted document), and a
  `reference` concept PNG, and lay them out in one labelled PNG grid.
  `preset` names a camera (`game`: orthographic, yaw −60, pitch 25), and
  `presets` / `--presets` define or override presets; `set_view` takes
  them too. The Render section has Sheet of files…, Sheet of variants…, and
  Reference…, and the Camera section a Game view button.

- Rhai strings: `"ledge_" + 4`, a number or bool on either side of `+`
  with a string, `+=`, `to_string`, and `len`, so loops name markers and
  objects; backtick interpolation (`` `ledge_${i}` ``) works too.

- Column relief: MCP `column_relief`, Rhai `column_relief(box, facing,
  palette, opts)`, and the Relief section break a cliff face into columns
  of random width that stand out of it and end at uneven heights around a
  crown, cutting the crown above short ones unless `lower` is false. The
  face is found in the cells, so swept, curved walls work.
  `examples/gorge_wall.rhai` uses it and keeps its ledge flat with
  `exclude`.

- Protected boxes: `exclude` on MCP and Rhai `scatter`, `joints`,
  `weather`, `moss`, `algae`, and `vines` (one inclusive box or a list of
  up to 64) keeps walking surfaces whole: no cell inside is cut, chipped,
  recoloured, or grown on, and no debris rests in one. The Protect section
  holds the boxes for the Scatter, Joints, Weather, and Growth sections.

- `examples/gorge_wall.rhai`: a fractured, mossy gorge wall swept along a
  curve, with algae, vines, and markers, and
  `examples/gorge_wall_variants.json` for variant export.

- Variant export: MCP `export_variants`, `froxel variants`, and the Export
  panel's Export variants… bake a scripted document once per parameter set
  and write a `.vox` and a `.glb` per set with the `export_models` mesh
  options, plus a manifest of each set's parameters, markers, and bounds.
  The document and its undo history stay as they were.

- Markers: MCP `set_marker`, `remove_marker`, and `list_markers`, Rhai
  `marker`, `collision_box`, and `connector`, and the Markers section name
  points, collision boxes, and connectors on an object. The `.vox` keeps
  them (`MVMK`), the export replies list them, and mesh export writes each as
  a glTF node with `extras.froxel` and the scene's `extras.froxelMarkers`.

- Sweep: MCP `sweep`, Rhai `sweep(points, profile, slots, opts)`, and the
  Sweep section fill a polygon or rounded-rectangle cross-section along a
  polyline or smooth curve, perpendicular to it, with twist and scale per
  point and strata bands, for curved walls, banks, and ledges.

- Growth: MCP `moss`, `algae`, and `vines`, the Rhai functions of the same
  names, and the Growth section grow plants on exposed faces filtered by
  face, facing, and a height band: moss cushions and ragged curtains, an
  algae band that thins upward in clumps, and vine strands graded to the tip
  with blooms. The caller gives the slots and their motion.

- Weather: MCP `weather`, Rhai `weather(x0, y0, z0, x1, y1, z1, opts)`, and
  the Weather section chip edges and corners, dent pits into flat faces,
  and add subtle colour noise on exposed stone; uncovered faces take a
  `broken` slot.

- Joints: MCP `joints`, Rhai `joints(x0, y0, z0, x1, y1, z1, pattern, opts)`,
  and the Joints section cut real joint gaps into stone: vertical columns of
  random widths, running bond courses, or a crack network, `width` wide and
  `depth` deep under the surface, with a `dirt` slot at the bottom.

- Script document tools: `get_voxels`, `set_voxels`, `set_palette` (alpha
  and motion), `set_object`, `remove_object`, `mirror_object`, `add_bone`,
  `set_bone`, `remove_bone`, `bind_object`, `list_bones`, `set_pose`,
  `clear_pose`, `add_clip`, `set_clip_keys`, `list_clips`, and
  `remove_clip`, plus `has_bone`, `has_clip`, and `has_object`. Object, rig,
  clip, and palette calls are recorded and replayed through the MCP tools
  after the script, in order, in the regenerate's undo step; a failing call
  names its line and puts the document back.

- Script shape tools: Rhai scripts have `fill_ellipsoid`, `fill_rounded_box`,
  `fill_tube`, `fill_box` with a mode, `carve_box`, `carve_ellipsoid`,
  `carve_rounded_box`, `carve_tube`, `smooth`, `break_end`, and `scatter`.
  They run the MCP tools' argument checks and shape code, with the MCP
  options in a trailing map such as `#{mode: "empty", aspect: 0.5}`. Scripts
  also gain `sqrt`, `pow`, `PI`, `min`, `max`, `clamp`, `ceil`, `round`,
  `tan`, and `push`, `pop`, `insert`, and `remove` on arrays.
  `examples/hollow_log.rhai` is a hollow log with roots and a snapped end.

- Colour atlas mesh export: MCP `export_mesh` and `export_models` take
  `colorAtlas`, the Export panel has **Colour atlas**, and `froxel bridge`
  takes `--color-atlas`. Coplanar faces merge whatever their palette slot, and
  each cell's colour goes into a per-mesh texture with nearest filtering.
  Faces of different motion or opacity still stay apart. Where a corner of
  another face lands on an edge, it becomes a vertex of the merged face's
  outline instead of cutting the face, so seams stay closed without
  multiplying quads. On noisy palettes this roughly halves the triangles
  (a pond bed 79.5k to 34.2k). One rigid mesh only: not with a skeleton,
  `splitObjects`, or `vertexColors`.

- Script list parameters: `param_floats`, `param_vec2s`, and `param_vec3s`
  declare a list of numbers or of 2D or 3D points with a range and a longest
  length, for a polyline or a curve. The `.vox` keeps them with the other
  parameters (`MVSC` kind 4). The Script panel edits a list as one line of
  text. MCP `set_params`, the bridge `evaluate`, Godot `set_parameter`, and
  `variants.json` take a JSON list and refuse one that is too long or out of
  range. The Godot inspector shows them as packed arrays and Blender as a
  JSON text field. Scripts can loop over an array with `for` and call `len()`.
- Hand sculpting is a named workflow, described in `docs/hand-sculpting.md`:
  block out with fills, refine with `set_voxels`, render after every step.
  The MCP server instructions point to it.
- The release workflow builds an Android arm64 APK, an iOS ipa, and an iOS
  simulator app. They are signed when the signing secrets in
  `docs/releasing.md` are set.
- Store builds: the release workflow makes a Google Play App Bundle and, with
  an App Store profile, an App Store ipa, and with `upload-stores` sends them
  to Google Play's internal track and TestFlight. The iOS app carries a
  privacy manifest. Both stores get the build number from
  `packaging/build_number.sh`. See `docs/releasing.md`.
- The release workflow's `submit-review` input sends the build for App Store
  review and Google Play production review, with release notes from this
  changelog.
- The Godot **FroxelBridge** node has Bake settings: **Bake Voxel Size**
  (metres per voxel), **Bake Origin** (Source, Center or Bottom Center of the
  geometry), **Bake Apply Transform** (keep the node's own transform) and
  **Bake Collision** (none, a box or an upright capsule around the geometry).
  With any of them set, the baked scene is ready to place: an unscaled root
  with a `Visual` child and a `Collision` StaticBody3D. The preview shows the
  voxel size and origin too. The headless `bake.gd` takes `--voxel-size`,
  `--origin` and `--collision`. The defaults bake the raw import as before.
- `tests/bridge_godot.py` bakes through the Godot node with `GODOT_PATH` and
  checks the scenes with the bridge stopped.

### Changed

- The Android package is `com.lonje.froxel`, as on iOS and macOS, and
  targets Android 16 (API 36). An APK from an earlier release installs beside
  it instead of updating.

### Fixed

- A failed Godot bake leaves the previous scene in place. The scene is written
  beside the target first and moved over it only when it is complete.
- The Godot plugin embeds the palette texture when it imports a model. In the
  editor it used to extract the texture as a file beside the glb, which failed
  outside `res://`, so the preview and **Bake standalone scene** came out
  untextured.
- The Godot inspector shows a script's parameters under one object heading
  instead of the same heading twice.
- `examples/lily_pad.rhai` lays the lily pad flat. It used to stand on its
  edge, because `.vox` has Z up.
- `froxel render --gpu` renders large frames such as 3840x1646 and 5244x2950.
  It used to panic, because the headless device kept wgpu's default buffer
  limits. It now asks for the adapter's limits, and a frame that still does
  not fit exits with an error that suggests the CPU tracer. Render mode
  falls back to the CPU tracer for such a frame instead of panicking.

## 0.2.0

Froxel models can now live inside Godot and Blender. Open a scripted `.vox`
with `froxel bridge`, and the Godot plugin or the Blender add-on shows its
parameters as sliders. Move one and the model is rebuilt and reimported, bones
and animation clips included. When it looks right, bake it into an ordinary
scene that no longer needs Froxel.

### Added

- **Godot and Blender bridge.** `froxel bridge --input model.vox` serves a
  document on `127.0.0.1`, and `froxel --bridge PORT` serves the open editor,
  so edits in the window show up in the next import. Each change is one undo
  step, and a rejected value or a failing script leaves the model as it was.
  See [docs/bridges.md](https://github.com/theromis/Froxel/blob/v0.2.0/docs/bridges.md).
- **Godot plugin**, `froxel-0.2.0-godot-addon.zip`. It adds a FroxelBridge node
  with live sliders and **Bake standalone scene**. A headless script bakes one
  `.tscn` per entry of a `variants.json`. Tested with Godot 4.5; needs 4.3+.
- **Blender add-on**, `froxel-0.2.0-blender-addon.zip`. It adds a Froxel tab in
  the 3D View sidebar, with sliders clamped to each parameter's range and
  **Bake into scene**, which keeps the rig working. Tested with Blender 5.2;
  needs 4.2+.
- `examples/lily_pad.vox`, a scripted lily pad with four parameters to try the
  bridge on.

### Fixed

- A click with the brush adds or removes one voxel. Held for a few frames, it
  used to stack a column toward the camera in Add mode, or dig a hole in
  Remove mode. A drag now paints one layer over the model as it was when the
  stroke began.
- The `--control` port now starts on Windows. Its token used to come from
  `/dev/urandom`, which Windows does not have.

## 0.1.0 – 2026-10-24

First release.

- Opens and saves MagicaVoxel `.vox`, including groups, layers, hidden objects,
  animation frames, and materials.
- Brush, shape, face, generator, mutator, and selection tools, plus object mode.
- Bones, poses, and animation clips, exported to glTF.
- Path-traced Render mode on the GPU (wgpu), with a CPU fallback.
  `froxel render` renders headless.
- glTF export with merged faces and a palette texture or vertex colours.
- Rhai generator scripts with sliders for their parameters.
- MCP server for AI agents, headless or connected to the window.
- Builds for macOS (universal, signed and notarized), Windows (x64, x86, ARM64),
  Linux (x86_64, aarch64, riscv64), and FreeBSD (x86_64).
