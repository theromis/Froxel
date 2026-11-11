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

If a tool that Froxel's docs or a recent commit mention is missing, the
server is older than the binary: call `reload` (the server re-executes the
rebuilt binary in place, keeps the open document, and sends
`notifications/tools/list_changed`, after which the tool list refreshes).
`reloaded: false` means the binary has not changed (rebuild it first, or
`force: true`); an error means the new build cannot resume and the old
server is still running. A server started with `--watch` reloads by itself.

## Choose an approach

- **Hand sculpting** for one specific shape that must match a reference: a
  creature, a character, a hero prop. This is the default for living things.
- **A Rhai script** (`set_script`, `set_params`, `regenerate`) for a family of
  variants or large regular geometry: buildings, terrain, fences, rocks.
  A curve or polyline is one `param_vec2s` / `param_vec3s` list parameter,
  set as `[[x, y], …]` through `set_params`, not one float per coordinate.
  Scripts have the MCP tools too, by the same names, with optional
  arguments in a trailing map such as `#{mode: "empty"}`: the sculpt tools
  (`fill_tube`, `carve_*`, `recolor`, `surface_stroke`, `shell_plates`, `components`, `deterrace`, `reduce_palette`, `smooth`, `break_end`, `scatter`, `column_relief`, `joints`, `weather`, `bark`, `planks`, `cavity_tint`, `foliage`, `moss`, `algae`, `vines`, `sweep`, `trim_outline`, `loft`, `deform`, `brush`, `carve_from_views` and `optimize_to_views` with views as rows and a legend, `project_paint` and `decal` with the image as rows and a legend, `paint_from_views` with views as rows and a legend, `pixelate_views` (views as rows; returns pixel art rows), `fit_to_views` and `orient_to_views` with views as rows and a legend, and `marker`,
  `collision_box`, `connector`, `collision_from_voxels`),
  `get_voxels`/`set_voxels`, and the object, bone, pose, clip, and palette
  tools (`mirror_object`, `split_object`, `split_parts`, `clear_object`, `add_bone`, `bind_object`, `set_clip_keys`,
  `set_palette`, …). The document tools apply after the script in order;
  guard `add_bone`, `add_clip`, and `mirror_object` with `has_bone`,
  `has_clip`, and `has_object` so a rerun does not fail. Only file-writing
  tools have no script form, and `import_mesh` (scripts read no files):
  `voxelize(#{positions, triangles, colors}, opts)` voxelizes a mesh the
  script builds (`tests/fixtures/examples/faceted_boulder.rhai`). Name things in loops with `"ledge_" + i` or
  `` `ledge_${i}` ``. See `docs/scripting.md`,
  `tests/fixtures/examples/hollow_log.rhai`, `tests/fixtures/examples/barked_trunk.rhai`, and
  `tests/fixtures/examples/plank_dock.rhai`.

- **Markers** for game data: `set_marker` names a ledge point, a collision
  `box`, or a `connector` (a joining end) on an object; scripts call
  `marker`, `collision_box`, and `connector`. They are saved in the `.vox`,
  listed in export replies, and exported as glTF nodes with
  `extras.froxel`, so collision never drifts from the model.
- **Collision from voxels**: `collision_from_voxels` (the same in scripts)
  builds the `box` markers from an object's solid cells, instead of boxes
  written by hand: a deck, a hold, a hatch, and a hole in a hull stay as they
  are. `holeSize` is the smallest opening kept open, and larger means fewer
  boxes; `layer` or `zBands` set where horizontal cuts lie; `minFeature`, `only`,
  and `exclude` (slots) drop vines and rope; `maxBoxes` caps the count. A rerun
  with the same `prefix` replaces its boxes. Read `coveredPercent`,
  `emptyFilledPercent`, and `dropped` in the reply, and look at `preview` with
  `overlay: "boxes"`. Call it last, once the cells are final.
- **Variants**: `export_variants` bakes a scripted document once per
  parameter set and writes `<name>.vox`, `<name>.glb` (with the
  `export_mesh` options), and a manifest of parameters, markers, and bounds.
  A manifest entry's `params` can be passed back unchanged: colours are whole
  0..255, or 0..1 only with a decimal point (`[1, 1, 1]` is near black).
  A later call into the same manifest merges by name: re-export only the
  variant to redo and the others stay listed (`replace: true` starts the
  manifest fresh). `froxel variants` is the command-line form. `tests/fixtures/examples/gorge_wall.rhai`
  shows the stone tools and markers together.

## Coordinates and rules

- `x` and `y` are horizontal, `z` is up. A model faces −Y.
- Voxel tools write the **active object**, in its **local** cells. World cell
  = local cell + the object's `position`. Switch with `set_active_object`.
  Pass `space: "world"` (fills, `carve`, `set_voxels`, `get_voxels`,
  `smooth`, `loft`, `brush`, `carve_from_views`, `project_paint`,
  `paint_from_views`, `decal`, `split_object`) to give world cells instead; parts that moved after a
  save and reopen, or a `mirror_object` copy, need no subtraction then.
- Palette `0` erases. Slots `1..255` are colours; set them with `set_palette`
  (`entries` sets many at once) before you fill. A colour is `color:
  "#rrggbb"` (or `"#rrggbbaa"`) or `r`, `g`, `b`, so a concept's hex codes
  go in as they are.
- An object is at most 256 cells per axis. A cell lies in `[-1024, 1024)`.
- `fill_box`, `fill_rounded_box`, `fill_ellipsoid`, and `fill_tube` write
  **every** cell in the shape, empty ones too. `mode: "empty"` adds without
  overwriting; `mode: "paint"` recolours only existing cells.
- `set_voxels` takes `[x, y, z, palette]` rows, up to 200000 per call. Batch
  detail into few calls.
- Seams between shell plates, markings on a curved body, and grooves:
  `surface_stroke` with a few `points` (they may float above the surface),
  `palette` (0 cuts a groove), `width`, `depth`, `only`. Each sample snaps
  to the nearest surface cell; do not guess a `fill_tube` radius in paint
  mode.
- A shell's plate pattern (central plate, rings of plates, outer band,
  rim, seams): ONE `shell_plates` call on the dome (`palettes` {ring, seam,
  central, band, rim}, `rings` such as [5, 9], `bandCount`, `layout` rings
  or voronoi, `seam` 0 for grooves, `facing` or `zRange` to keep it on
  top). Do not draw each seam with `surface_stroke` or project a pattern
  from the top view, which streaks down the sides.
- Symmetric features (cheeks, eyes, flippers, horns, side stripes): draw one
  side and pass `mirror: true` (x about the object's centre), an axis, or
  `{axis, at}` to `set_voxels`, the fills (not mode intersect), `recolor`,
  `surface_stroke`, `project_paint`, or `decal`. The far side is done on its own
  surface, mirrored, in the same undo step; `mirrored` in the reply counts
  its cells. Do not place the second side by hand.
- To empty an object and start it again, call `clear_object` (index or
  name; default the active one): no box limit, the object, its position,
  and its bone stay. Do not erase with `fill_box` palette 0, which stops at
  2000000 cells.
- To swap palette slots with no shape change (a blockout slot to the shell
  slot, two shades into one, a belly re-tinted), call `recolor` (`from`/`to`
  or `map`, optional box): do not fake it with `weather` or a `mode: "paint"`
  fill.
- `batch` takes a list of `{tool, arguments}` editing calls (`fill_*`, `sweep`,
  `carve`, `set_voxels`, `set_palette`, the surface tools) and applies them
  in order as one undo step. Use it instead of many single calls: it is
  faster and far shorter in context. The first failure stops it, names the
  item, and leaves the document as it was. Tools that write files are
  refused; `preview` and save after it. Scripts need no `batch`: a run is
  already one undo step.
- Call `inspect_document` when you do not know the objects, bounds, or
  palette. Do not guess indices. Its `palette` lists only the slots in use;
  pass `fullPalette: true` to see all 256.

## Hand sculpting loop

1. `new_document`, then `set_palette` with the colours you need.
   From a concept's turnaround sheet, first read it with `measure_sheet`
   (`parts: true`): it gives each view's crop (`fitViews`), the cube pitch
   per view, whether front/side/top agree in percent, and the top view's
   part count (six for a turtle: four flippers, head, tail). Never measure a
   sheet with ad-hoc scripts; fix a sheet whose checks fail. Then start the
   form with `carve_from_views`: the front, side, and top views as PNG crops of the
   sheet (`crop`, `cellsPerPixel` or a box `size`, a background rule) fill
   their visual hull, coloured from the nearest view. Do not copy cells from
   the flat views by hand. When a grey depth pass of the sheet exists (white
   near; an image generator's, or Depth Anything V2 run on the sheet with
   grayscale output), pass it as `depthPath` beside the sheet's `path` and
   crops: the cells in front of each view's depth surface are carved away,
   so heads and shells come out round. Keep `calibrate` at its default
   `hull` (it scales each relative map so its nearest pixel per row meets
   the silhouettes), read each view's `depth.residual` in the reply (under 1
   cell is good), use `depthTolerance` 0.5 to 1 for a noisy map, and
   `back: true` on a view's `depth` when its opposite view has no map. A
   generated map that normalises each part like shading (a head drawn
   nearer than the shell) takes `regions: true` (`edge` 0.03 to 0.1 splits
   parts; read `regionResidual`); Depth Anything's takes `gamma: "auto"`,
   and the generated map as its `detail` gives each part its roundness while
   Depth Anything keeps their order.
   `fit_to_views` takes the same `depthPath` and `depth` as its depth
   target. To round the boxy hull, intersect it: `fill_ellipsoid`
   (or `fill_rounded_box`, `fill_box`, `fill_tube`) with `mode: "intersect"`
   keeps only the cells inside the shape (a dome over a shell), and
   `carve_from_views` `mode: "intersect"` cuts an existing form to the
   views. Do not erase the corners with many carves. The hull is the outer limit: carve the concave
   parts (gaps between legs, under a log) afterwards. Paint a pattern one
   view shows (a shell's plates from the top, face markings from the front)
   with ONE `project_paint` call: `view`, the image crop, `slots`, `depth`
   (1 is the surface), `normalLimit` (e.g. 60, so steep sides get no
   streaks), and `only` (the slots it may repaint). It never adds or removes
   cells; do not paint layer by layer with `carve_from_views` `mode: paint`
   and dummy views. To give a whole form (an image-to-3D import smeared on
   its hidden sides, or a finished blockout) the sheet's colours on EVERY
   side, call `paint_from_views` once with the sheet `path` (it reads the
   layout as `measure_sheet` does) or the four crops, and `newSlots` (about
   8 to 12) or the object's `slots`; pass `objects` for rig parts. Check
   `unseen` (cells no view sees keep their slot) and each view's `missed`. For a SMALL creature (20 to
   60 cells long) pass `downscale: true` to `carve_from_views` and `paint_from_views` (or call
   `pixelate_views` with `size: [null, 20, null]` and `png` first to look): each view becomes pixel art
   at the cell grid, a few shared Oklab colours, crisp edges, eyes, pupils, teeth, nostrils, and spots
   kept at least a cell (`features`), parts thinner than a cell kept inked (`thin`), drawn lines thinner
   than a cell (an eye's ring, a mouth) kept as a line of cells (`lines`, on by default; `outline: true`
   keeps the silhouette's outline too, as a sprite has, off by default so no dark rim lands on the model; `mergeLines`, on by default, folds an outline's
   anti-aliased shades into one outline colour),
   so the eyes do not average into smears and the legs do not vanish. If the sheet's eyes or teeth are
   white on a white background, the background rule keys them out as holes: pass `fillHoles: true`
   (top level, every view) and check the `png` has the whites back. An image-to-3D mesh for a small creature goes in with
   `import_mesh` `coverage: 0.5` (cells kept by their solid share on a finer voxelization, so legs and
   fins do not merge into lumps; `keepThin`, on by default, keeps parts thinner than a cell one cell
   thick) and `minFeature: 2` to list what is still one cell thin. Eyes, a mouth, nostrils, or a badge on a round head or
   body are a `decal` (picture, `point` or `ray`, `size` in cells along the
   surface, `up`, `only`, `mirror: true` for the pair of eyes), not a
   `project_paint`, which smears them on the slopes; check `missed` is 0. To finish a blockout (from `carve_from_views` or
   `import_mesh`) against the approved views sheet, call `fit_to_views`
   with the same crops (front, side, top, back) and `slots`: it clears rays
   that should be background, fills rays the sheet inks, follows a
   `reference` object's depth (an `import_mesh` result kept as its own
   object), and recolours what the views see. Fit each part in turn (other
   visible objects count as cover and stay put), read each view's
   `before`/`after` `iou` and `colorError` in the reply, look at the
   preview, and then refine by hand. Fit a part with a hand-set face (a
   head) with `recolor: false` or its face slots in `keep`: colouring it
   from both the front and the side view duplicates the eyes. A round part
   comes out of three axis views as a box (no view sees its diagonal
   corners): fit it with `roundWeight: 0.3` (`roundAxes` "yz" by default),
   which removes those corners while every silhouette stays; leave it off
   for domes with a flat rim and for plates. For a SMALL creature (20 to 60 cells
   long), finish instead with ONE `optimize_to_views` call (the sheet `path`
   and crops, or `pixelate_views`' rows made at the object's box): it
   pixelates the views at the object's box, then iterates shape (excess
   columns cleared, missing rays given a cell touching the body, within
   `band` cells, connected; `minFeature: 2` keeps legs two cells thick) and
   colours (each surface cell from a facing-weighted vote of the views that
   see it, smoothed into patches, eyes and pupils kept as accents,
   `colorCarve` rounding edges two views paint differently) until the views
   agree. Read each view's `before`/`after` `agreement` (shape and colour
   right over the pixels either covers) and `blocked.unreachable` (where the
   views themselves disagree), then look with `preview`; do not chain
   `paint_from_views` and hand recolouring for it. Pass the hero view first
   (it wins ties) or give it `weight: 2`. `semantic: true` adds a CLIP
   patch-similarity pass (froxel built with `--features clip-metal`, weights
   at `models/clip-vit-base-patch32.safetensors` in Froxel's support folder;
   never download them without the user's yes); read `semantic.before`
   and `after`.
2. **Block out.** For a unique organic asset (a fallen tree, a creature, a
   hero rock) with a mesh sculpted in Blender or generated from the concept
   by an image-to-3D tool, start with `import_mesh` (`voxelSize` 0.08 for
   0.08 m voxels, `anchor: "bottom"`, `newObject: true`, and `slots` to map
   onto your bark, stone, or moss slots; PNG, JPEG, and WebP textures are
   read, and the default `fill` `solid` fills a double-walled or open mesh,
   sealing openings at their rims: check `interior` and `openEdges` in the
   reply) and finish it with the tools below; pass `orient` with the
   sheet's views (as `fit_to_views` takes them, plus `yawStep`) so the
   mesh comes in turned to the sheet (or only the sheet's `path`, read as
   `measure_sheet` does), or call `orient_to_views` (with
   `size`) on the imported object, instead of guessing `rotation` by eye;
   a mesh tilted off level (SF3D, TRELLIS) takes `orient: "upright"` (PCA:
   thinnest axis up, the taller end to −Y; check `orientation.upright`
   `keptUp`/`keptYaw`, a round toad keeps its up) or `{upright: true, path:
   sheet}` to stand it up and then match the sheet's yaw;
   and read `margin` (under 0.01: add a side or back view); first clean it with
   `components`: `list` the parts, `remove_small` (`minCells`) the floaters
   and foam, and `delete` or `split` a rider sitting on the body with a
   `seed` on the rider and an `anchor` on the body (the cut is the thinnest
   neck between them; check `cut` and `selected`); then `deterrace` the
   one-cell contour terraces of its gentle slopes, which read as stripes:
   `stagger` keeps the cell size and breaks them into teeth (check
   `longestRun` after, at most 3), `blocks` with `size` 2 or 3 gives the
   chunky concept-cube look; then `reduce_palette` its noisy texture
   colours (`colors` 6 to 8, `despeckle` 1 or 2; check `slots` after and
   that the eyes are among the accents), or pass the same as `clean` to
   `import_mesh`;
   tubes and boxes alone make such an asset read as pipes. Otherwise block
   out the large forms with `fill_rounded_box` and
   `fill_ellipsoid`. Grow limbs, roots, branches, and twigs with `fill_tube`
   (a radius per point, so it tapers; `curve: "smooth"` bends it). Build
   trunks, roots, and limbs that must not read as pipes with `loft` (an
   elliptical section per point with `roll`, `flatTop`, and a knobbly
   `noise` outline), reshape with `deform` (`bend`, `taper`, `twist`,
   `displace` for burls and lumps, and `scale` to make a small creature's head, eyes, or fins larger
   than life about a `point` with a smooth `falloff`; `minFeature: 2` reports parts one cell thin), and refine the silhouette with `brush`
   (`inflate`, `deflate`, `smooth`, `flatten`, `grab` along points; see
   `tests/fixtures/examples/fallen_tree.rhai`). Cut
   hollows, tunnels, and pits with `carve`, and a slanted side along a
   curved or closed outline with `sweep` and `palette` 0. Narrow a finished
   form from outside, such as a hull toward its keel, with `trim_outline`: a
   closed plan `outline`, `[z, inset]` pairs, and `skin` to seal the cut
   (`tests/fixtures/examples/trimmed_hull.rhai`), not tubes fitted to the surface. Snap wood with `break_end` (a
   ragged face with splinters) instead of cutting it clean, sweep curved
   walls, banks, and ledges with `sweep` (a profile along a smooth path),
   and pile debris
   with `scatter`. Break a cliff face into standing columns with
   `column_relief` (random width, relief, and uneven tops; it finds the face
   itself, curved or not) instead of hand-writing boxes. Cut real joints
   into stone with `joints` (columns,
   running bond courses, or cracks, with dirt at the bottom) instead of
   painting dark lines, then `weather` for chipped edges, pits, and
   subtle colour noise, then `moss` (caps and curtains, or `style:
   "cushions"`: separate cushions where the surface faces up, with drapes
   from overhangs, never one band; `facing: [0, 0, -1]` with `layer:
   "paint"` shades undersides, and a reply `warning` says why nothing grew), `algae` (a
   waterline band), and `vines` (hanging strands with blooms) with your own
   slots and `set_palette` motion. Dress wood by its form, never with
   painted stripes: build it in one slot, `bark` it along the same
   `points` as its `fill_tube`, or the `axis` of a flat deck or plank
   at least 2 cells thick (plates and fissures with dirt at the
   bottom; read `warning` in the reply), then `cavity_tint` to darken fissures, crevices, and root
   crooks (it makes the shades itself) before moss. `cavity_tint` gives
   rock depth the same way. Lay a deck, floor, or dock with `planks`, never
   dozens of `fill_box` and `carve` calls or painted stripes: one call lays
   separate boards with real gaps down to a `dirt` slot, staggered end
   joints, a tone per board (`palettes`, at least two), and optional
   `ragged` ends and `dark` nails and knots; for a curved hull, fill the
   deck in one slot and pass it as `only` (`tests/fixtures/examples/plank_dock.rhai`).
   Build a broadleaf crown with `foliage`, never
   a blob of `fill_ellipsoid` calls with noise or grooves: after the trunk
   and branches, `foliage` over a few ellipsoids (or `from` an existing
   crown's leaf slots) packs separate clumps with sunlit tops, dark
   undersides, and a dark `core` in the gaps (`palettes` darkest first,
   `size`, `gap`, `masses` for lobes; the wood stays whole;
   `tests/fixtures/examples/clustered_tree.rhai`). Give the leaf slots and the core the
   same sway. Pass `exclude` boxes (a ledge top, a
   cave floor, a river bed, with the air above) to these tools and
   `scatter` so walking surfaces stay flat instead of refilling them. Do not compute tubes or carves in
   an outside script. Get the silhouette and proportions right first.
3. **Look.** Use `preview` for a quick lit look at shape and framing after
   every edit: it is the viewport's light, rasterised in well under a second,
   and `object` frames one part. Use `model_view` to compare shape and colour with a reference: it
   is flat palette colour, orthographic, with no lighting. Use `view` (`front`,
   `back`, `left`, `right`, `top`, `bottom`) and its `views` list to write a
   whole turnaround in one call. Use `render` to judge lighting and material.
   `preview` `preset: "game"` is the gameplay camera (a project redefines
   it with `presets`, an object or a JSON file, or the user's
   `camera_presets.json`). For an asset placed turned in a level, pass its
   turn about up as `placementYaw` (a Godot `rotation_degrees.y`) instead
   of working out yaw and pitch by hand. To compare a family,
   give `preview` `variants` (or `inputs`) and a `reference` concept: one
   labelled contact sheet instead of pasting images together. `overlay:
   true` draws collision boxes, points, and connectors over the model, so
   check collision against the stone before exporting. To judge colour,
   give `preview` the game's `lighting` (`old_pond`, or a project preset
   from `lightingPresets`): the viewport light flatters a palette that a
   warm sun and blue ambient turn grey. Its
   `previewTriangles` is the unmerged mesh it draws, often ten times the
   game's; quote `export.triangles`, with `export` set to the game's export
   options (for example `{"colorAtlas": true}`), as the asset's count.
4. **Soften** a blocky region with `smooth` (it keeps colours and removes
   anything one cell thick, so leave twigs out of its box), then **refine**
   with `set_voxels`: cut the snout, place the eyes, add the colour breaks.
5. Repeat steps 3 and 4. A first blockout is never the result. Look after
   every step and read the PNGs; do not report a model you have not viewed.
   Measure it against the concept with `compare_silhouette` (the concept
   PNG and its `crop`, a camera such as an orthographic `view` or `preset:
   "game"`, with `presets` and `placementYaw` as in `preview`): `iou`, `missing`, and `excess` say whether it reads like the
   concept, and the overlay shows where (magenta missing, orange excess).
   Reuse the reply's `align` on later calls so a change in size shows.
   Compare silhouettes, not heights. A hero view with water, splashes, or a
   rider scores nothing until masked: add `flood: true` (water, sky),
   `keepLargest: true` (splashes apart), `erase: [[x, y, w, h]]` (a rider,
   in image pixels), `excludeColors`, or a painted `mask` PNG (white on
   black, same size); check `maskEdits` and the cyan outline in the overlay.
6. `save_file` with a `path` (a missing folder is created). World cells and markers survive reopening exactly,
   including negative coordinates; local cells may rebase to the model box, so
   inspect the object position again before making more local edits. A reopened
   document exports the same glTF frame and marker values. Foreign files without
   Froxel scene-origin metadata still open centred and floored; when positions
   differ from the file's, `open_file` and the exports state `sceneShift`, and
   colliders from a manifest in the file's frame will not line up until the shift
   is subtracted from each object's position.

Build each rig part (head, jaw, body segments, limbs, tail) as its own object
at world `[0, 0, 0]` with `add_object`, so local cells are world cells. A
creature carved in one piece with `carve_from_views` is cut into those parts
with ONE `split_parts` call (a `name` and a seed `point`, `bone`, or
`marker` per part, `planes` to cut a jaw or a tail root): every cell goes to
the part nearest through the solid, and each part comes out at world
`[0, 0, 0]` bound to the bone of its name; check each part's `cells` and
`unreached`. `split_object` (boxes and/or slots, a `name` or `into`) cuts
one part by boxes: the cells keep their world positions, so do not carve
each part again and erase the rest. Sculpt
paired parts once and copy them with `mirror_object`. Keep the rest pose
straight and symmetrical; show poses with bones instead of sculpting them.

## Rig and animate

1. `add_bone` with a `pivot` in fractional world coordinates, so a joint can
   sit between cells. Give children a `parent`.
2. `bind_object` each part to one bone. Skinning is rigid: one bone, weight 1.
   Then `place_joints` (optionally `snap: "half"`) moves every pivot to where
   its part meets its parent's part (contact faces, overlap, or the middle of
   the gap); check `placed[].method` and `gap`, and use `set_bone` with
   `joint: true` for one bone after its part changes. Do not place pivots by eye.
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
- `bake` (with `colorAtlas`) paints light into the atlas by real rays, for a
  game to draw unshaded instead of lighting the mesh: `occlusion`, `distance`
  in cells, `rays`, and a `preset` such as `old_pond` or `sunDirection`,
  `sunColor`, `sunEnergy`, `ambientColor`, `ambientEnergy`, with `sunSoftness`
  in degrees for a soft shadow. `{}` is occlusion alone, which only darkens. The
  material comes out unlit, triangles do not change, and the same input gives
  the same bytes. Look at the file with `preview` and its `glb` argument: the
  ordinary preview draws the document, not the bake.
- `bake_scene` bakes a level's placement of many `.vox` files together, so
  stones shade each other and the ground shades them, into one `.glb` with one
  atlas. `scene` is `{instances: [{source, translation, yaw, scale, lean, sway}],
  occluders: [{mesh, ...} | {plane} | {height}], bake}`, or a JSON file.
  Placements are Y up, in the frame of the exported glb, so a level's transforms
  copy across, and `distance` is in scene units. `lean: {degrees, toward}` tilts
  a placement about its base so its top moves toward the yaw `toward` (0 is −Z,
  like `yaw`), e.g. reeds leaning to water; `sway` (0..4, default 1) multiplies
  how far that instance's moving slots sway, so plant kinds sway apart. Triangles add up across placements, so check the
  reply's `triangles` and `memory`.
- `export_models` writes up to 16 `.glb` files, each from its own `.vox`
  `source`, without touching the open document.

## Window

When the server runs with `--connect`, edits appear in the open editor.
`set_view` moves its camera, `screenshot_viewport` captures it, and
`ui_inspect` with `ui_click` drive the controls. In a headless session these
tools fail; use `preview`, `render`, and `model_view` instead.

## Mistakes to avoid

- Filling over existing detail with a box when you meant to recolour.
- A round `fill_tube` for a trunk or a big root: it reads as a pipe. Use
  `loft` with an egg-shaped, flattened, noisy section, then `deform`
  `displace` and `brush`.
- Writing world coordinates into an object whose `position` is not
  `[0, 0, 0]`.
- Sculpting a curled or posed creature instead of rigging a straight one.
- Using `screenshot_viewport` to check colours. The viewport is lit; use
  `model_view`.
- Removing an object and reusing old indices. Later indices shift down by
  one, as the reply says.
