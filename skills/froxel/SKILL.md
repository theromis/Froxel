---
name: froxel
description: Model, rig, animate, render, and export voxel art through the Froxel MCP server. Use when the user wants a voxel model, a MagicaVoxel .vox file, a voxel creature or prop, a turnaround of a voxel model, a path-traced voxel render, or a rigged .glb for Godot, Blender, or a game engine, and the froxel MCP tools (open_file, fill_box, set_voxels, render, model_view, export_mesh) are available.
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

## Coordinates and rules

- `x` and `y` are horizontal, `z` is up. A model faces −Y.
- Voxel tools write the **active object**, in its **local** cells. World cell
  = local cell + the object's `position`. Switch with `set_active_object`.
- Palette `0` erases. Slots `1..255` are colours; set them with `set_palette`
  (`entries` sets many at once) before you fill.
- An object is at most 256 cells per axis. A cell lies in `[-1024, 1024)`.
- `fill_box`, `fill_rounded_box`, and `fill_ellipsoid` write **every** cell in
  the shape, empty ones too. To recolour only existing cells, read them with
  `get_voxels` and write them with `set_voxels`.
- `set_voxels` takes `[x, y, z, palette]` rows, up to 200000 per call. Batch
  detail into few calls.
- Call `inspect_document` when you do not know the objects, bounds, or
  palette. Do not guess indices.

## Hand sculpting loop

1. `new_document`, then `set_palette` with the colours you need.
2. **Block out** the large forms with `fill_rounded_box` and
   `fill_ellipsoid`. Get the silhouette and proportions right first.
3. **Look.** Use `model_view` to compare shape and colour with a reference: it
   is flat palette colour, orthographic, with no lighting. Use `view` (`front`,
   `back`, `left`, `right`, `top`, `bottom`) and its `views` list to write a
   whole turnaround in one call. Use `render` to judge lighting and material.
4. **Refine** with `set_voxels`: cut the snout, place the eyes, add the colour
   breaks.
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
image. `views` writes extra angles in one call. Hidden objects still count
when the shot is framed; lower `distance` if the subject looks small.

## Export

- `export_mesh` writes a `.glb` (glTF 2.0, Y-up). `merge` is off unless you set
  it; turn it on for game assets to merge coplanar faces. `exportHidden` and
  `splitObjects` default to false. The reply lists the nodes, meshes, skin, and
  animations: check it before you report success.
- `export_models` writes up to 16 `.glb` files, each from its own `.vox`
  `source`, without touching the open document.

## Window

When the server runs with `--connect`, edits appear in the open editor.
`set_view` moves its camera, `screenshot_viewport` captures it, and
`ui_inspect` with `ui_click` drive the controls. In a headless session these
tools fail; use `render` and `model_view` instead.

## Mistakes to avoid

- Filling over existing detail with a box when you meant to recolour.
- Writing world coordinates into an object whose `position` is not
  `[0, 0, 0]`.
- Sculpting a curled or posed creature instead of rigging a straight one.
- Using `screenshot_viewport` to check colours. The viewport is lit; use
  `model_view`.
- Removing an object and reusing old indices. Later indices shift down by
  one, as the reply says.
