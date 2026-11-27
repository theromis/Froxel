# Froxel agent guide

The MCP server's `initialize` instructions are kept short, because clients cut
long ones (Claude Code keeps the first 4096 characters). This is the rest:
which tool to reach for, workflow by workflow. The server serves this file as
the MCP resource `froxel://guide`, so an agent can read it with
`resources/read` without the repository. Every tool's own options are in
[mcp.md](mcp.md); the hand-sculpting walk-through is
[hand-sculpting.md](hand-sculpting.md) (also served, as
`froxel://hand-sculpting`).

## Cells, batch, and files

Edits apply to the active object, in that object's local cells; `space: "world"`
on the fills, carve, set_voxels, get_voxels, smooth, loft, blend_shapes,
brush, carve_from_views, project_paint, decal, and split_object takes world
cells instead and subtracts the object's position, so parts moved by a save
and reopen or a mirror_object copy need no recomputed boxes.

Every tool that takes a box (the fills, carve, generate, recolor, smooth,
deform, the stone and wood tools, set_marker, and the rest) takes it as the
inclusive corners x0, y0, z0, x1, y1, z1 or as `min: [x, y, z]` and
`max: [x, y, z]` with the same meaning, in batch items too; give one form, not
both.

`batch` applies a list of editing tools in order as one undo step, stopping at
the first error with the document as it was: use it instead of dozens of
single fill_box and carve calls, and keep render and export calls outside it.

Every tool that writes a file (save_file, render, model_view, preview,
compare_silhouette, export_mesh, export_models, export_variants, bake_scene,
screenshots) creates the missing folders of its path first.

`set_palette` takes a slot's color as `#rrggbb` or `#rrggbbaa` (or r, g, b), so
a concept's hex codes go in as they are.

If a tool the docs mention is missing, this server is older than the froxel
binary on disk: `reload` re-executes the rebuilt binary in place, keeping the
connection and the open document, and sends
`notifications/tools/list_changed` (`--watch` does it by itself).

## Looking at the model

`preview` writes a fast lit PNG (the viewport mesh and light, rasterised, well
under a second) framed on the model or one object; use it to look after every
step. Its preset `game` is the gameplay camera (a project redefines it with
presets, an object or a JSON file, or the user's `camera_presets.json`),
`placementYaw` sees an asset turned that much about up in a level from that
camera, `lighting` (`old_pond`, or a project's `lightingPresets`) shows colour
under a game's sun and ambient instead of the viewport light, `overlay` draws
collision boxes, points, and connectors over the model, and `inputs`,
`variants`, and `reference` make one labelled contact sheet of several models
beside a concept image. Its `previewTriangles` is the unmerged mesh it draws,
not the game's: its `export` counts (with `export` set to the game's options,
e.g. `colorAtlas`) are what export_mesh writes. `preview` with `glb` looks at
the written file.

`render` path-traces a PNG in this process; the window path-traces in Render
mode. `model_view` writes the flat model view (palette colours, no light) for
comparing with a turnaround sheet. `set_view` moves the window camera before a
`screenshot` with `region: "viewport"`. screenshot (the window, the viewport,
or a rectangle), ui_inspect, and ui_click need an editor started with `--control` and this
server started with `--connect`.

## Export and baking

Export's `bevel` (0..0.4 cells) writes every cell bevelled: `bevelMode` `cubes`
rounds each cube with grooves between neighbours (the toy-block look of a
low-resolution concept), `edges` rounds only the shape's outer edges. Look at
it first with `preview` and `export: {bevel, bevelMode}`; faces are not merged.

Export's `bake` option (with `colorAtlas`) paints ray-traced occlusion and a sun
with shadows into the atlas, for a game to draw unshaded. `bake_scene` bakes a
placement of many .vox files together, shaded by each other and by ground,
into one merged glb and atlas; an instance's `lean` tilts it about its base, and
its `sway` multiplies how far its moving slots sway.

## Hand sculpting

Hand sculpting ([hand-sculpting.md](hand-sculpting.md)) models a creature or a
unique asset without a script: block out with fill_box, fill_rounded_box, and
fill_ellipsoid, grow limbs and roots with fill_tube (`caps: "flat"` cuts its
ends square, for pipes, discs, rims, and wheels), fill a cylinder, cone,
pyramid, torus, helix, Perlin noise block, or stand-in tree or conifer fitted
to a box with `generate` (the editor's Generator tool), cut hollows with carve,
soften with smooth, round a boxy blockout with a fill in mode `intersect` (only
the cells inside the shape stay, so a hull becomes a dome), refine with
set_voxels, draw seams, markings, and grooves that follow a curved surface
with surface_stroke (points snap to the nearest surface cells), paint a whole
shell's plates (a central plate, rings of plates, an outer band, and a rim,
with painted or grooved seams that follow the dome, mirrored left to right)
with one shell_plates call, swap one palette slot for another (in a box or
everywhere, no shape change) with recolor, and look with preview after every
step (render for the final still).

For a symmetric creature draw one side: `mirror` (true for x about the
object's centre, an axis, or `{axis, at}`) on set_voxels, the fills, recolor,
surface_stroke, project_paint, and decal repeats the call across the plane on
the far side's own surface, mirrored, in the same undo step.

`clear_object` empties an object of any size in one step (fill_box with
palette 0 stops at 2000000 cells), to start a part again.

## A mesh from Blender or an image-to-3D tool

For a unique organic asset with a mesh sculpted in Blender or made by an
image-to-3D tool, `import_mesh` voxelizes the .glb, .gltf, or .obj (PNG, JPEG,
or WebP textures and vertex colours quantized into the palette; `fill: "solid"`
fills a double-walled or open mesh, sealing its openings at their rims, so
check `interior` and `openEdges` in the reply) as the blockout.

- Its `orient` option, or `orient_to_views` afterwards, turns it to the views
  sheet's turn by silhouette IoU over the 24 axis-aligned turns and fine yaw,
  and scales it, so rotation and size are not guessed by eye; `orient:
  "upright"` stands a mesh tilted off level from its shape alone, thinnest
  principal axis up and the taller end of its long axis to -Y, and
  `{upright: true, path: sheet}` then searches only the yaw.
- `components` then removes what the generator added (`keep_largest`,
  `remove_small` floaters, or a rider cut off the body at its thinnest neck
  between seed and anchor and deleted or split out).
- For a small creature its `coverage` option keeps cells by their solid share
  on a finer voxelization (legs and fins do not merge into lumps) with
  `keepThin` keeping parts thinner than a cell one cell thick.
- `reduce_palette` cuts its noisy colours to a few clusters in Oklab (keeping
  small accents such as eyes) or snaps them to a palette, and despeckles the
  surface with a majority filter (`import_mesh` `clean` does both at import).
- `deterrace` breaks the one-cell contour terraces a smooth mesh leaves
  (`blocks` re-quantizes into 2- or 3-cell cubes for the chunky concept-cube
  look; `stagger` staggers the step edges in seeded teeth), and the tools
  below finish it.

## Organic forms

Statues, creatures, topiary, and other organic forms built from a body, a
head, eyes, and limbs should not read as separate balls and bricks:
`blend_shapes` fills spheres, ellipsoids, rounded boxes, tapered capsules, and
tori blended as signed distance fields with a smooth minimum (`k` cells of
fillet, so a head melts into a body like clay), subtracts shapes softly (a
mouth groove, eye sockets), colours each cell from the shape it is deepest
inside, and takes mirror (`tests/fixtures/examples/blob_snowman.rhai`).

Organic forms (trunks, roots, limbs, rocks) should not read as pipes or boxes:
`loft` builds a trunk or limb whose elliptical, flattened, knobbly section
changes along a curve; `deform` bends, tapers, twists, or displaces (lumps,
burls) an object or a box, and scales a part about a point with a smooth
falloff (a small creature's head, eyes, or fins made larger than life so they
read at 20 to 60 cells; `minFeature` reports parts thinner than N cells);
`brush` inflates, deflates, smooths, flattens, or grabs the surface along a
path of points to refine the silhouette
(`tests/fixtures/examples/fallen_tree.rhai`).

Thin bent parts (sedge and reed leaves, water weeds, barbels, fin rays) and
detail finer than a cell should not step on the grid: `add_strand` lays a
chain of cubes along a curve on an object, each turned along the path, with
`width`, `thickness`, and `step` down to 0.125 cells, a `twist` and `scale`
per point, and `palettes` as a base-to-tip `gradient`. A strand stays
editable (`edit_strand`, `list_strands`, `remove_strand`), moves with its
object, bone, and pose, and `preview`, `render`, `model_view`, and
`export_mesh` all draw the same turned boxes; `bake_strands` writes a grid
copy for MagicaVoxel (`tests/fixtures/examples/sedge_tuft.rhai`).

A creature body that should read as irregular bricks instead of grid steps
(the Frog World look) gets `skin` after it is sculpted: bricks of uneven
`length` in staggered rows along `axis` (the body's length, the same on
every rig part), with `relief`, and `bare` for the eye and teeth slots. The
cells stay the shape and the skin follows later edits; `preview` and the
glb show the bricks (`tests/fixtures/examples/brick_fish.rhai`).

## Surfaces

Stone surfaces: `column_relief` breaks a cliff face into columns of uneven
width, relief, and height, `joints` cuts real joint gaps (columns, courses,
cracks), `weather` chips edges, dents pits, and adds colour noise, and `moss`,
`algae`, and `vines` grow plants on exposed faces (the caller gives the slots
and their motion); moss style `cushions` grows separate cushions where the
surface faces up, with drapes from overhangs, never one band.

Wood surfaces: build the wood in one slot, then `bark` cuts plates along its
grain (the same points as its fill_tube, or an axis on a flat deck or plank)
split by fissures with dirt at the bottom, instead of painted stripes, and
`cavity_tint` darkens fissures, crevices, and root crooks (and lightens ridges)
by neighbour occupancy, making the shades itself; it gives rock depth too.
`tests/fixtures/examples/barked_trunk.rhai` uses them.

Architecture and turned pieces: `radial_relief` cuts regular shallow recesses
repeated evenly round an axis, flutes along a column, pilaster, vase, urn, or
barrel side (round or square, following the cells' outer surface, ends cupped
or tapered over fade cells) or gores dished between the proud ribs of a dome
(fading at the crown and the foot), with a u, v, or flat section 0.5 to 4
cells deep, and carries the slope in colour (`shade` tones the floors, `light`
the slopes by the ridges, `ribs` paints and `ribHeight` raises the lines
between them), instead of surface_stroke one flat groove at a time
(`tests/fixtures/examples/fluted_column.rhai`, `gored_dome.rhai`).

Decks, floors, and docks: `planks` lays separate boards over a rectangle at a
height, with real gaps down to a dirt cell, end joints staggered between
neighbouring rows, a tone per board, and optional ragged ends, nails, and
knots, instead of dozens of fill_box and carve calls or painted stripes
(`tests/fixtures/examples/plank_dock.rhai`).

Tree crowns: `foliage` packs many separate leaf clumps over ellipsoids (or
re-clusters an existing crown's slots) with lit tops, dark undersides, and a
dark core in the gaps, leaving the wood alone, instead of one blob of
fill_ellipsoid calls (`tests/fixtures/examples/clustered_tree.rhai`).

Their `exclude` boxes (and scatter's, bark's, planks', and cavity_tint's) keep
walking surfaces whole, with no refill afterwards.

`sweep` fills a cross-section along a curve for curved walls, banks, and
ledges, and with palette 0 erases one, for a slanted cut along a curved
outline. `trim_outline` reshapes a finished form from outside: state a closed
plan outline and `[z, inset]` pairs once, and every solid cell outside that
surface goes, with a new skin along the cut if asked, so a hull narrows toward
its keel with no tubes fitted by hand
(`tests/fixtures/examples/trimmed_hull.rhai`).

## From a concept's turnaround sheet

For a unique organic asset from a concept's turnaround sheet:

1. `measure_sheet` first reads the sheet (each view's crop as `fitViews`, the
   cube pitch per view, front/side/top agreement in percent, and with `parts`
   the top view's flippers, head, and tail) and writes no file, so never
   measure a sheet with scripts.
2. `carve_from_views` fills the visual hull of its front, side, and top
   silhouettes (PNG crops) as the rough form, and with a depth map per view
   (`depthPath`: a grey depth pass of the sheet, white near, from an image
   generator or Depth Anything V2, calibrated to the hull by default) carves
   away the cells in front of each view's surface, so heads and domes come out
   round.
3. `project_paint` then recolours the surface one view sees from a single
   image (a shell's plates from the top view, markings from the front) without
   adding or removing cells; `paint_from_views` recolours every surface cell
   from the whole sheet in one call (each cell from the views that see it, by
   the view its normal faces most, blended at seams, quantised into the
   object's slots or a few new ones with eyes and mouths kept), which gives an
   image-to-3D import smeared on its hidden sides the approved colours all
   round.
4. For a small creature (20 to 60 cells long) `pixelate_views` turns the views
   into pixel art at the cell grid first (a few shared Oklab colours, the
   majority colour per cell, eyes, pupils, teeth, nostrils, and spots kept at
   least a cell, parts thinner than a cell kept inked; rows and legend per
   view, and a png to check) and `downscale` on carve_from_views and
   paint_from_views does the same in one call, so eyes do not average into
   smears and thin legs do not vanish.
5. `optimize_to_views` then iterates such a small creature's shape (excess
   columns cleared, missing rays given a cell touching the body, within
   `band` cells, connected, never thinner than `minFeature`) and surface
   colours (a vote of the views that see each cell weighted by how squarely
   they face it, smoothed into patches, eyes kept as accents, edge cells two
   views paint differently carved off) until its views agree with the pixel
   art (the views' crops pixelated at the object's box, or pixelate_views'
   rows), replying with each view's agreement before and after (`semantic`
   adds a CLIP patch-similarity term on a froxel built with the `clip-metal`
   feature).
6. `decal` lays an eye, a mouth, or a badge onto a curved part around a point
   (or a ray from the camera) so it keeps its size and shape where a
   projection smears it on the slopes (mirror for the pair of eyes).
7. `fit_to_views` refines a blockout (from carve_from_views or import_mesh) so
   its orthographic views match the sheet's views (excess and missing rays, an
   optional reference object's depth, and colours from the object's slots),
   replying with each view's IoU and colour error before and after.
8. `compare_silhouette` scores the model against a concept view from a preview
   camera (IoU, missing, excess) and writes an overlay PNG; check it after
   every step instead of comparing heights.

A hero view with water, splashes, or a rider needs its background masked first
(`excludeColors`, `flood`, `mask`, `erase`, `keepLargest`, on
compare_silhouette and every view), or its IoU means nothing; white eyes or
teeth on a white sheet are keyed out as background and come back with
`fillHoles`.

## Rig parts, markers, and colliders

`split_parts` then cuts that one blockout into all its rig parts in one call
(head, jaw, body, four limbs, tail): each cell goes to the part whose named
seed point, bone, or marker is nearest through the solid (geodesic), with
optional cutting planes, and each part becomes its own object at world
`[0, 0, 0]` ready for bind_object; `split_object` moves the cells inside boxes,
or of some slots, into a new or existing object, keeping their world
positions. After bind_object, `place_joints` puts each bone's pivot where its
part meets its parent's part (contact faces, overlap, or the middle of the
gap), so pivots are never placed by eye.

`set_marker` names points, collision boxes, and connectors that the .vox keeps
and the glb carries as nodes; `collision_from_voxels` builds the collision
boxes from an object's solid cells within a tolerance (`holeSize`, `layer` or
`zBands`, `minFeature`, `only`, `exclude`, `maxBoxes`), keeps hollows open,
replaces its own earlier boxes, and replies with how much solid is covered and
how much empty is wrongly filled, so colliders never drift from the model
(check them with preview `overlay` boxes).

`export_variants` writes a .vox and .glb per parameter set and merges them by
name into a manifest of parameters, markers, and bounds (`replace: true`
starts it fresh); its reply, like export_mesh's and export_models', counts the
markers by kind instead of listing them (`reply: "full"` for every one, or read
the manifest file or list_markers). `tests/fixtures/examples/gorge_wall.rhai`
uses them all.

## Scripts

A Rhai script (`set_script`) can call the same sculpt, voxel, object, rig,
clip, and palette tools, with the MCP options in a trailing map, for a family
of organic pieces ([scripting.md](scripting.md),
`tests/fixtures/examples/hollow_log.rhai`).
