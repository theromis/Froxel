# Changelog

## Unreleased

### Added

- The release workflow builds an Android arm64 APK, an iOS ipa, and an iOS
  simulator app. They are signed when the signing secrets in
  `docs/releasing.md` are set.
- Store builds: the release workflow makes a Google Play App Bundle and, with
  an App Store profile, an App Store ipa, and with `upload-stores` sends them
  to Google Play's internal track and TestFlight. The iOS app carries a
  privacy manifest. Both stores get the build number from
  `packaging/build_number.sh`. See `docs/releasing.md`.
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
