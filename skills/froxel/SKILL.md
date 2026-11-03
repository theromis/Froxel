---
name: froxel
description: Model, rig, animate, render, and export voxel art through the Froxel MCP server. Use when the user wants a voxel model, a MagicaVoxel .vox file, a voxel creature or prop, a turnaround of a voxel model, a path-traced voxel render, or a rigged .glb for Godot, Blender, or a game engine, and the froxel MCP tools (open_file, fill_box, set_voxels, preview, render, model_view, export_mesh) are available.
---

# Froxel

Froxel is a voxel editor with a path tracer. Its MCP server (`froxel mcp`)
edits one document: a `.vox` scene of objects, a 255-colour palette, bones,
and animation clips. Every edit is one undo step.

If the tools are missing, the server is not configured. Point the user to
`docs/mcp.md` in the Froxel repository: `froxel mcp` runs headless, and
`froxel mcp --connect 127.0.0.1:7979 --launch` drives the editor window.

## Choose an approach

- **Hand sculpting** for one specific shape that must match a reference: a
  creature, a character, a hero prop. This is the default for living things.
- **A Rhai script** (`set_script`, `set_params`, `regenerate`) for a family of
  variants or large regular geometry: buildings, terrain, fences, rocks.
  A curve or polyline is one `param_vec2s` / `param_vec3s` list parameter,
  set as `[[x, y], …]` through `set_params`, not one float per coordinate.
  Scripts have the MCP tools too, by the same names, with optional
  arguments in a trailing map such as `#{mode: "empty"}`: the sculpt tools
  (`fill_tube`, `carve_*`, `smooth`, `break_end`, `scatter`, `joints`, `weather`, `moss`, `algae`, `vines`, `sweep`, and `marker`,
  `collision_box`, `connector`),
  `get_voxels`/`set_voxels`, and the object, bone, pose, clip, and palette
  tools (`mirror_object`, `add_bone`, `bind_object`, `set_clip_keys`,
  `set_palette`, …). The document tools apply after the script in order;
  guard `add_bone`, `add_clip`, and `mirror_object` with `has_bone`,
  `has_clip`, and `has_object` so a rerun does not fail. Only file-writing
  tools have no script form. See `docs/scripting.md` and
  `examples/hollow_log.rhai`.

- **Markers** for game data: `set_marker` names a ledge point, a collision
  `box`, or a `connector` (a joining end) on an object; scripts call
  `marker`, `collision_box`, and `connector`. They are saved in the `.vox`,
  listed in export replies, and exported as glTF nodes with
  `extras.froxel`, so collision never drifts from the model.
- **Variants**: `export_variants` bakes a scripted document once per
  parameter set and writes `<name>.vox`, `<name>.glb` (with the
  `export_mesh` options), and a manifest of parameters, markers, and bounds.
  `froxel variants` is the command-line form. `examples/gorge_wall.rhai`
  shows the stone tools and markers together.

## Coordinates and rules

- `x` and `y` are horizontal, `z` is up. A model faces −Y.
- Voxel tools write the **active object**, in its **local** cells. World cell
  = local cell + the object's `position`. Switch with `set_active_object`.
- Palette `0` erases. Slots `1..255` are colours; set them with `set_palette`
  (`entries` sets many at once) before you fill.
- An object is at most 256 cells per axis. A cell lies in `[-1024, 1024)`.
- `fill_box`, `fill_rounded_box`, `fill_ellipsoid`, and `fill_tube` write
  **every** cell in the shape, empty ones too. `mode: "empty"` adds without
  overwriting; `mode: "paint"` recolours only existing cells.
- `set_voxels` takes `[x, y, z, palette]` rows, up to 200000 per call. Batch
  detail into few calls.
- Call `inspect_document` when you do not know the objects, bounds, or
  palette. Do not guess indices. Its `palette` lists only the slots in use;
  pass `fullPalette: true` to see all 256.

## Hand sculpting loop

1. `new_document`, then `set_palette` with the colours you need.
2. **Block out** the large forms with `fill_rounded_box` and
   `fill_ellipsoid`. Grow limbs, roots, branches, and twigs with `fill_tube`
   (a radius per point, so it tapers; `curve: "smooth"` bends it). Cut
   hollows, tunnels, and pits with `carve`. Snap wood with `break_end` (a
   ragged face with splinters) instead of cutting it clean, sweep curved
   walls, banks, and ledges with `sweep` (a profile along a smooth path),
   and pile debris
   with `scatter`. Cut real joints into stone with `joints` (columns,
   running bond courses, or cracks, with dirt at the bottom) instead of
   painting dark lines, then `weather` for chipped edges, pits, and
   subtle colour noise, then `moss` (caps and curtains), `algae` (a
   waterline band), and `vines` (hanging strands with blooms) with your own
   slots and `set_palette` motion. Do not compute tubes or carves in
   an outside script. Get the silhouette and proportions right first.
3. **Look.** Use `preview` for a quick lit look at shape and framing after
   every edit: it is the viewport's light, rasterised in well under a second,
   and `object` frames one part. Use `model_view` to compare shape and colour with a reference: it
   is flat palette colour, orthographic, with no lighting. Use `view` (`front`,
   `back`, `left`, `right`, `top`, `bottom`) and its `views` list to write a
   whole turnaround in one call. Use `render` to judge lighting and material.
4. **Soften** a blocky region with `smooth` (it keeps colours and removes
   anything one cell thick, so leave twigs out of its box), then **refine**
   with `set_voxels`: cut the snout, place the eyes, add the colour breaks.
5. Repeat steps 3 and 4. A first blockout is never the result. Look after
   every step and read the PNGs; do not report a model you have not viewed.
6. `save_file` with a `path`.

Build each rig part (head, jaw, body segments, limbs, tail) as its own object
at world `[0, 0, 0]` with `add_object`, so local cells are world cells. Sculpt
paired parts once and copy them with `mirror_object`. Keep the rest pose
straight and symmetrical; show poses with bones instead of sculpting them.

## Rig and animate

1. `add_bone` with a `pivot` in fractional world coordinates, so a joint can
   sit between cells. Give children a `parent`.
2. `bind_object` each part to one bone. Skinning is rigid: one bone, weight 1.
3. Check the rig with `render` or `model_view` using `pose` and
   `showBones: true`. That poses one image without changing the document.
   `set_pose` previews in the window and is an edit; `clear_pose` removes it.
4. `add_clip`, then `set_clip_keys` per bone. Keys are local TRS in voxel
   space; `translation` is the full offset from the parent, not a delta.
   Times are increasing and start at 0 or later.

Clips are saved in the `.vox` and exported as glTF animations.

## Render

`render` path-traces a PNG and blocks until it is written. Defaults: 256×256,
16 samples, denoised, yaw 30°, pitch 25°. Yaw 0 puts the camera on +X; a model
facing −Y is face-on at yaw −90. Raise `samples` and the size for a final
image. `views` writes extra angles in one call. The camera fits the visible
cells in the frame at any angle; `zoom` magnifies, `target` (an object index,
`active`, or a `{min, max}` cell box) frames one part, and giving only `width`
makes the height follow the model (up to 4096 per side). `model_view` takes
`pixelsPerVoxel` to size the picture to the model. Unknown arguments are
refused.

## Export

- `export_mesh` writes a `.glb` (glTF 2.0, Y-up). `merge` is off unless you set
  it; turn it on for game assets to merge coplanar faces. `exportHidden` and
  `splitObjects` default to false. `aoStrength` (0..1, off by default) bakes
  ambient occlusion into `COLOR_0` so crevices and overhang undersides come
  out darker with no run-time cost; `aoRadius` (default 3 cells) sets how far
  it reaches. 0.6 to 0.8 suits a sky-lit look. `aoTarget` `texture` (the
  default with `colorAtlas`) paints it into the atlas and adds no triangles;
  `vertex` (the default without) writes `COLOR_0` and leaves faces that shade
  differently unmerged; `both` writes the two. The reply lists the nodes, meshes, skin, and
  animations: check it before you report success.
- `export_models` writes up to 16 `.glb` files, each from its own `.vox`
  `source`, without touching the open document.

## Window

When the server runs with `--connect`, edits appear in the open editor.
`set_view` moves its camera, `screenshot_viewport` captures it, and
`ui_inspect` with `ui_click` drive the controls. In a headless session these
tools fail; use `preview`, `render`, and `model_view` instead.

## Mistakes to avoid

- Filling over existing detail with a box when you meant to recolour.
- Writing world coordinates into an object whose `position` is not
  `[0, 0, 0]`.
- Sculpting a curled or posed creature instead of rigging a straight one.
- Using `screenshot_viewport` to check colours. The viewport is lit; use
  `model_view`.
- Removing an object and reusing old indices. Later indices shift down by
  one, as the reply says.
