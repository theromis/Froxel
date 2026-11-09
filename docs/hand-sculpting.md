# Hand sculpting

Hand sculpting is modelling by hand: large primitive fills for the forms,
then single-cell edits for the detail, with a render after every step. There
is no stored Rhai script. The voxels themselves are the source, and the
`.vox` is the file you keep and edit.

The other way to make a model is a [script](scripting.md): a Rhai program
that fills the voxels from parameters. A script suits families with
variants and large, regular geometry. Hand sculpting suits one specific
shape that has to match a reference: a creature, a character, a hero prop.
It is how agents should model living things through MCP.

## Steps

1. **Block out.** Import a sculpted or generated mesh with `import_mesh`
   when there is one (see "Start from a mesh"), and clean it with
   `components`: `remove_small` erases floaters, and `delete` or `split`
   with a `seed` on a rider and an `anchor` on the body cuts the rider off
   at its thinnest neck. Otherwise build the large
   forms with `fill_box`,
   `fill_rounded_box`, and `fill_ellipsoid`. Grow limbs, roots, branches,
   tails, and twigs with `fill_tube`. Cut hollows, tunnels, and pits with
   `carve`. Get the proportions and the silhouette right before any detail.
2. **Render and compare.** `render` the document and put the PNG beside the
   reference (concept art, a photo of the real animal). Look at the
   silhouette first, then the proportions, then the colour. With a concept
   image, `compare_silhouette` puts a number on the silhouette (see
   [From a concept sheet](#from-a-concept-sheet)).
3. **Soften and refine.** `smooth` a blocky region to fill single-cell pits
   and knock off single-cell spikes. Then use `set_voxels` to cut, add, and
   recolour single cells: the
   face, eyes, fins, scales, colour breaks. One `set_voxels` call takes many
   cells and is one undo step. Seams between shell plates and markings on a
   curved body are `surface_stroke`: the path through a few points snaps
   to the nearest surface cells, `width` cells across and `depth` cells
   deep (`palette` `0` cuts a groove), so no tube radius is guessed. To change a whole slot (the blockout's slot
   to the shell's, the belly to a paler one), call `recolor` with `from` and
   `to` or a `map`, in a box or everywhere: the shape stays as it is. A
   symmetric creature is painted once: `mirror: true` on `set_voxels`, the
   fills, `recolor`, `surface_stroke`, and `project_paint` repeats the call
   across the object's centre plane (or `{axis, at}`), on the far side's own
   surface, with the picture mirrored, so the two cheeks, eyes, or flippers
   match without placing each by hand.
4. **Repeat** steps 2 and 3. Render after every step and compare again. A
   first blockout is never the result.
5. **Save.** `save_file`. Froxel saves retain world voxel and marker positions
   when reopened, including negative cells and an off-centre chosen origin.
   Local cells may rebase to the model box: read the object position again before
   further local-cell edits, or send the edits with `space: "world"`, which
   subtracts each object's position for you. Foreign files without Froxel
   origin metadata still open centred and floored.

```
new_document
set_palette          entries=[{index: 1, r: 74, g: 92, b: 58}, {index: 2, r: 196, g: 184, b: 120}]
set_palette          entries=[{index: 3, color: "#5c6838"}]   # or a hex colour
fill_rounded_box     x0=0 y0=0 z0=0 x1=11 y1=5 z1=4 palette=1 radius=2    # body
fill_ellipsoid       x0=2 y0=1 z0=0 x1=9 y1=4 z1=1 palette=2              # belly, see the tip below
render               path="step1.png" yaw=-90 pitch=10
set_voxels           cells=[[11, 2, 3, 0], [11, 3, 3, 0], [10, 1, 3, 5]]   # cut the snout, place an eye
render               path="step2.png" yaw=-90 pitch=10
save_file            path="creature.vox"
```

## From a concept sheet

A unique asset with an approved turnaround sheet (a hero view plus front,
side, and top views) starts from its silhouettes, not from cells copied
off the flat views.

1. **Carve the rough form.** `carve_from_views` takes the front, side, and
   top views as crops of the sheet and fills their visual hull: the cells
   every view covers. Give the cells per pixel, or the box `size` in cells
   at the game's scale (0.08 m voxels: a 4 m log is 50 cells long). Each
   cell takes the colour of the nearest view, quantised to `slots`. Three
   flat views give a boxy hull; round it with `fill_ellipsoid` (or
   `fill_rounded_box`) in `mode` `intersect`, which keeps only the cells
   inside the shape, so a turtle's shell becomes a dome in one call.
   Better, give each view a depth map (`depthPath`: a grey depth pass of
   the same sheet, white near, from the image generator or from Depth
   Anything V2 run on the sheet): the cells in front of each view's depth
   surface go, so a head is round and a shell domed from the sheet itself.
   The maps are relative, and the default `calibrate: "hull"` scales each
   one so its nearest pixel per row meets the silhouettes; check the reply's
   `residual` and see [mcp.md](mcp.md#depth-maps). A generated map that
   draws each part with its own range takes `regions: true`; Depth
   Anything's takes `gamma: "auto"`, and the generated map as its `detail`
   for the roundness inside each part. Front, side, and top
   cannot see the back-right-bottom corner: add `back: true` to their
   depth, or draw back views too.
   `carve_from_views` in `mode` `intersect` does the reverse: it cuts an
   existing form to the views.
2. **Measure it.** `compare_silhouette` draws the model from a camera and
   scores it against one concept view: `iou`, the share of the concept
   `missing`, and the model's `excess`, with an overlay PNG that shows
   where. Compare each turnaround view orthographically (`view` `front`,
   `left`, `top`) and the hero view from `preset` `game`. A hero view with
   water, splashes, or a rider in it must be masked first, or its IoU
   measures the scene: `flood: true` for the water, `keepLargest: true` for
   splashes apart from the body, `erase` rectangles over a rider, or a
   painted `mask` PNG (see "Masking a concept" in mcp.md).
3. **Shape it** with the steps below: the hull fills concave parts (the
   space under an arch, between roots), so `carve` them, grow the thin
   parts with `fill_tube`, and `smooth` it. Compare again after each step,
   with the first reply's `align` passed back so the placement holds still.
4. **Paint the patterns.** `project_paint` takes one view (a shell's plates
   from the top, face markings from the front) and recolours the cells that
   view sees under the image, in one call: no cells are added or removed.
   `depth` paints that many cells into the surface, `normalLimit` leaves the
   steep sides alone so they get no streaks, and `only` keeps other slots
   (the eyes, the belly) as they are. With the `origin` and
   `cellsPerPixel` that carved the form, the same crop lands on the same
   cells.
5. **Fit it to the views.** `fit_to_views` takes the same crops (front,
   side, top, back) and refines the blockout, whether it came from
   `carve_from_views` or from `import_mesh`, so that its orthographic views
   match the sheet: each view pixel is one column of cells, rays that
   should be background are cleared, rays the sheet inks but the model
   misses are filled with cells every other view sees inside, and rough
   single cells are tidied (`smoothWeight`). Then every cell a view sees
   takes the slot (from `slots`, by default the object's own) nearest the
   sheet's colours. With a mesh imported into a separate object, pass it as
   `reference` so the rays' front follows its depth. Other visible objects
   (the head, the flippers) count as cover but never change, so each part
   is fitted in turn. The reply gives each view's `iou`, `missing`,
   `excess`, and `colorError` before and after, the cells added, removed,
   and recoloured, and the palette in use; pass its `align` back to hold
   the placement. A part with a hand-set face is fitted with
   `recolor: false` (or its face slots in `keep`): the front and side views
   draw the same eyes at places that do not meet on a boxy head, and
   colouring from both gives four. A round part (a head) comes out of the
   visual hull as a box, since no axis view sees its diagonal corners:
   fit it with `roundWeight: 0.3` and they go while every view stays the
   same, or pass the views' depth maps (`depthPath`, `depth`), which say
   where the surface is instead of guessing. Then refine by hand: the fit matches
   views, not the sides no view sees.

```
carve_from_views     views=[{view: "front", path: "sheet.png", crop: [40, 60, 520, 300]},
                            {view: "side",  path: "sheet.png", crop: [620, 60, 180, 300]},
                            {view: "top",   path: "sheet.png", crop: [40, 420, 520, 180]}]
                     size=[96, 34, 56] slots=[3, 4, 5, 6] smooth=1
                     depthPath="sheet-depth.png" depthTolerance=0.5
compare_silhouette   path="front-vs.png" concept="sheet.png" crop=[40, 60, 520, 300] view="front" orthographic=true
compare_silhouette   path="hero-vs.png" concept="hero.png" preset="game"
                     flood=true keepLargest=true erase=[[412, 96, 80, 120]]
project_paint        view="top" path="sheet.png" crop=[40, 420, 520, 180] size=[96, 34, 56]
                     slots=[3, 4, 5] only=[3] normalLimit=60
fit_to_views         views=[{view: "front", path: "sheet.png", crop: [40, 60, 520, 300]},
                            {view: "side",  path: "sheet.png", crop: [620, 60, 180, 300]},
                            {view: "top",   path: "sheet.png", crop: [40, 420, 520, 180]}]
                     slots=[3, 4, 5, 6] reference="mesh"
```

`preset="game"` is the gameplay camera; a project redefines it with `presets`
(an object or a JSON file), and `placementYaw` checks an asset placed turned in
a level from that same camera, in `preview` too ([preview.md](preview.md)).

`examples/carved_stump.rhai` does the same in a script, with the views as
rows, `examples/painted_shell.rhai` paints a shell's plates from a top
view, `examples/fitted_beetle.rhai` fits a boxy blockout to its views,
and `examples/depth_head.rhai` carves a round head from three views with a
depth map each (its `depth` parameter at 0 shows the boxy hull without).
In the editor, the Carve from views, Paint from view, Fit to views (on the
Carve from views section's views), and Compare silhouette sections hold the
same fields, and the comparison shows over the viewport.

## Start from a mesh

A unique organic asset (a fallen tree, a creature, a hero rock) built only
from tubes and boxes tends to read as pipes. When the form can be sculpted
in Blender, or generated from the approved concept by an image-to-3D tool,
bring that mesh in with `import_mesh` and finish it with the voxel tools:

```
import_mesh          path="fallen_tree.glb" voxelSize=0.08 anchor="bottom" newObject=true name="fallen_tree"
components           op="remove_small" minCells=40                  # floaters and foam bits
preview              path="tree_import.png"
model_view           path="tree_side.png" view="left"                # compare with the concept
smooth               x0=-40 y0=-12 z0=0 x1=40 y1=12 z1=20 iterations=1
weather              x0=-40 y0=-12 z0=0 x1=40 y1=12 z1=20 chip=0.2 noise=[3, 4]
moss                 x0=-41 y0=-13 z0=0 x1=41 y1=13 z1=21 palettes=[6, 7]
preview              path="tree_finished.png"
```

`voxelSize` keeps the mesh's real size at the game's voxel size; `size`
fits its longest axis to a number of cells instead. The colours of the
base-colour texture go into the palette (reusing close slots, adding at
most `maxNewSlots`); pass `slots` to map onto a palette you have already
chosen, such as bark and moss slots, so the finishing tools find them.
Read the reply's `bounds` for the boxes of the next tools. The default
`fill` `solid` fills a generated mesh even when it is double-walled or not
watertight (the reply's `openEdges` counts its open edges): openings are
sealed at their rims, so a shell open underneath comes out solid; check
`interior` in the reply. Textures may be PNG, JPEG, or WebP (TRELLIS writes
WebP). `fill` `shell` keeps only the surface, for a hollow log to carve
into. `mode` `paint`
projects a textured mesh's colours onto a form already in the object.
`docs/mcp.md` describes the options; `froxel import` does the same from
the command line.

## Organic shapes

`fill_tube` fills a round tube through a list of points, each with its own
radius, so one call makes a tapering limb, root, or branch. Its joints and
ends are round. `curve` `smooth` bends it through the points instead of
joining them straight. `palettes` with a `pattern` colours it: `stripes`
runs grooves along it (bark), `rings` bands it along its length (a tail),
and `random` speckles it.
`aspect` flattens it and `profile` `blade` makes it a wedge, so a tube
that tapers from radius 1.5 to 0.5 with `aspect` 0.4 and `profile` `blade`
is a flat splinter, and a short wide one is a broken plank end or a shard.
`roll` turns the flat side about the axis.

A round tube reads as a pipe. For a trunk, a root, or a limb, `loft`
gives each point its own section: an ellipse with two radii (`rx` across,
`ry` up), a `roll`, a flat side (`flatTop`; negative flattens the bottom
where a log rests on the ground), and a knobbly outline (`noise` and
`frequency`). One smooth loft from a wide, egg-shaped root end to a thin
round tip tapers, sags, and lumps in one call.

`deform` then reshapes what is there, on the whole object or a box of it:
`bend` curls a limb toward a direction by an angle over a length, `taper`
narrows it, `twist` turns it, and `displace` pushes the surface in and out
with noise for burls and lumps (`bias` 1 only bulges, -1 only dents). Slots
are kept.

`brush` refines the silhouette like Blender's clay and grab brushes: dabs
along a path of points `inflate` a burl, `deflate` (dig) a rot pocket,
`smooth` the joint where a limb leaves the trunk, `flatten` a face, or
`grab` a stub out along a vector. `only` keeps it off other slots. In the
editor the Sculpt tool (**S**) is the same brush by drag.

`carve` removes the cells inside a `box`, `ellipsoid`, `rounded_box`, or
`tube`: a hollow trunk, a tunnel, a burrow, an eye socket. A carved tube
with a radius just under the trunk's makes a hollow log in one call.
A tube has at most 2000000 cells and 512000000 cells in the boxes around
its segments, and a closed path counts like an open one. For a slanted cut
along a closed or curved outline, sweep a polygon with `palette` `0` rather
than fitting round tubes to the surface. To remove everything outside a
surface, such as a boat's side, call `trim_outline`.

`break_end` snaps a trunk, branch, plank, or post across a plane: the part
beyond goes, the new face is ragged with pits torn along the grain, its rim
keeps the `bark` colour, the inside turns `sapwood`, and tapered splinters
stick out along the normal with lighter `tips`, longer near the rim with
`rimBias`. Break the two halves of a fallen tree with opposite normals and
different seeds so the faces do not match exactly.

`scatter` drops small pieces into a box: shards, chips, pebbles, and
chunks. Each falls until it rests on a cell of the active object or an
earlier piece, so they pile into a heap at a break or a stump. Sculpt the
ground in the same object, or pass `floor`, since other objects do not stop
a falling piece. `clump` 1 piles them in the middle of the box.

`sweep` fills a cross-section along a curve: a wall, a bank, a ledge, or a
kerb that bends with no corners. Give a rounded rectangle (`width`,
`height`, `radius`) or a polygon, `curve: "smooth"`, and a twist or scale
per point to lean or taper it. `palettes` with `pattern: "bands"` lays
horizontal strata.
`palette` `0` erases the solid cells the same shape covers: the sweep is the
cut, and `examples/swept_cut.rhai` slopes a block's side and cuts a channel.

`trim_outline` reshapes a finished form from outside, and it is the way to
narrow a boat hull toward the water: give the closed plan `outline` and `[z,
inset]` pairs once, and every solid cell outside that surface goes. `skin` and
`thickness` lay a new skin along the cut and seal a hollow it opened, leaving
the rim and any port alone, so there is no second `sweep` to rebuild it.
`examples/trimmed_hull.rhai` does this to a hollow block in one call.

`examples/gorge_wall.rhai` puts the stone tools together in order: sweep,
column relief, the ledge, joints, weather, moss, algae, vines, then markers.

`column_relief` breaks a cliff face into columns of random width that
stand out of it by a random amount and end at uneven heights around the
crown, cutting the crown above a short one unless `lower` is false. It
finds the face in the cells, so a swept, curved wall works; give it the
face direction and a box a few cells larger than the wall.

`joints` cuts real gaps between stones, one cell wide and `depth` deep,
with a `dirt` slot at the bottom of each gap: `columns` for fractured
limestone, `courses` for masonry in a running bond, `cracks` for a crack
network. Cut joints before moss and plants, or pass `only` with the stone
slots.

`weather` chips edges and corners, dents pits into flat faces, and adds
subtle colour noise; the faces a chip or pit uncovers take `broken`, a
slightly darker stone. Run it after `joints`, so the new block edges chip
too, and before the plants.

`moss`, `algae`, and `vines` grow plants on exposed faces filtered by
`faces`, `facing`, and a height `band`. Moss caps tops (and undersides with
`faces` `bottom`) and hangs curtains with a ragged end down the faces below; algae is a band at the waterline,
dense low and thinning upward in clumps; vines hang from top edges, graded
through their slots to the tip, with occasional blooms. Give the vine slots
`set_palette` `motion` `sway` rising from the anchor to the tip and leave
moss and algae still. Pass `only` with the stone slots so plants do not
grow on plants.

Wood is dressed by its form, never with painted stripes, which read as a
pipe with a stripe. Build the trunk, branch, or root with one slot, then:

- `bark` cuts long plates along the grain, the same `points` and `curve`
  as the `fill_tube` (or `axis` for a straight one), split by fissures 1 to
  2 cells deep with a `dirt` slot at their bottom, plate ends offset from
  the next column's and an occasional crack across. `palettes` are a few
  bark tones; each plate takes one. On a flat deck or plank pass the `axis`
  the boards run along, at least 2 cells thick: the columns go across its
  width, and the back layer stays. Read `warning` in the reply.
- `cavity_tint` darkens the cells in concave spots, the fissures, the
  crook of a root, a branch join, and with `lighten` lightens the plate
  ridges, by how many of their neighbours are solid. Without `pairs` it
  makes the shades in free palette slots. It works on rock and roots too.
- `moss` with `style: "cushions"` grows separate cushions where the
  surface faces up (or `facing`), `gap` cells apart so it never forms a
  band, with drapes hanging over the edge. `facing: [0, 0, -1]` with
  `layer: "paint"` shades the underside of a crown or overhang; with
  `facing` and no `faces` every face is considered. A call that grows
  nothing replies with a `warning` saying why.

`examples/barked_trunk.rhai` puts them together on a fallen trunk.

A deck, floor, or dock is not 50 `fill_box` and `carve` calls, and not painted
stripes. `planks` lays separate boards over a rectangle at a height: real gaps
down to a `dirt` slot, end joints staggered between neighbouring rows, a tone per
board from `palettes` (at least two), and with `ragged` and `dark` worn ends and a
few nails and knots. For a curved hull, fill the deck to its outline in one slot
and pass that slot as `only`; an `exclude` box keeps a hatch whole.
`examples/plank_dock.rhai` is a whole dock.

```
planks               x0=0 y0=0 x1=71 y1=31 z=14 axis=x palettes=[3, 4, 5] dirt=6 dark=7 length=[10, 24] ragged=0.3 seed=3   # boards, not stripes
```

A broadleaf crown is not a blob of `fill_ellipsoid` calls with noise or
grooves on top: that reads as one uniform mass. Build the trunk and
branches first, then `foliage` over a few ellipsoids (a broad middle and
two or three lobes, wider than tall): it packs separate leaf clumps over
the surface, each shaded as one piece with a sunlit top and a dark
underside, and fills the inside with a dark `core` that shows in the gaps.
`palettes` run from the darkest leaf slot to the sunlit tips; `size` is the
clump radius, `gap` how much core shows between them, and `masses` groups
the clumps into a few lobes split by deeper grooves. The wood is never
written, so branches show through the gaps; an `exclude` box keeps the
space in front of the trunk open. To re-cluster a crown that is already
sculpted, pass its leaf slots as `from`. Give the leaf slots and the core
the same sway. `examples/clustered_tree.rhai` is a whole tree.

```
foliage              palettes=[3, 4, 5, 6, 7] core=3 ellipsoids=[[1, 0, 30, 15, 13.5, 9], [-7, 4, 34, 9, 9, 7], [7, -4, 34, 9, 8, 7]] size=[3, 5] masses=4 seed=3   # clumps, not a blob
```

Pass `exclude` boxes to `joints`, `weather`, `bark`, `planks`, `cavity_tint`, `foliage`, the
plants, and `scatter` to
keep walking surfaces whole: a ledge top, a cave floor, or a river bed (its
top layer and the air above it) stays flat while the stone around it is
cut, chipped, and grown on, so it needs no refill. The editor's Protect
section holds the boxes; Add selection appends the selected box.

`set_marker` names a ledge top, a collision box, or a connector on the
piece, so the game reads them from the exported glb (nodes with
`extras.froxel`) instead of measuring the model again. `preview` with
`overlay: true` draws them over the model, so check a collision box
against the stone before export.

Do not write collision boxes for a sculpted hull by hand. When the shape is
final, `collision_from_voxels` builds them from the cells, and running it again
after an edit replaces them. Keep vines and moss out with `exclude` slots or
`minFeature`, and set `holeSize` a little under the smallest opening the player
uses:

```text
collision_from_voxels prefix="hull" holeSize=12 minFeature=2 exclude=[14, 15, 16] maxBoxes=46
preview               overlay="boxes" path=hull.png                 # hatch and hold still open?
```

Read `coveredPercent`, `emptyFilledPercent`, and `dropped` in the reply. With
`dropped` above 0 the budget was too small for that `holeSize`: raise one.

When a scripted piece needs several sizes, `export_variants` writes a
`.vox` and `.glb` per parameter set with a manifest of the parameters,
markers, and bounds. Exporting again into the same folder updates that
manifest by name, so redoing one variant keeps the others.

`smooth` softens a blocky region. It fills an empty cell that most of its
26 neighbours surround and removes a solid cell that few of them touch, and
a filled cell takes the colour most of its neighbours hold. One pass with
the defaults cleans pits and spikes and rounds corners; `fillAt` 14 and
`removeAt` 12 round the edges too, and `iterations` repeats it. It removes
anything one cell thick, so smooth before the twigs, thin carved walls, and
fine detail, or keep them outside the box.

```
set_palette          entries=[{index: 3, r: 92, g: 64, b: 40}, {index: 4, r: 60, g: 40, b: 26}, {index: 5, r: 196, g: 160, b: 104}, {index: 6, r: 214, g: 184, b: 128}, {index: 7, r: 238, g: 218, b: 170}, {index: 8, r: 70, g: 92, b: 44}, {index: 9, r: 82, g: 60, b: 38}, {index: 10, r: 40, g: 28, b: 20}]
fill_tube            points=[[0, 0, 4, 4], [14, 2, 5, 3.5], [30, 3, 4, 3]] palette=3   # fallen trunk, one slot
smooth               x0=-5 y0=-5 z0=-1 x1=34 y1=8 z1=9                    # before thin walls and twigs exist
bark                 x0=-5 y0=-5 z0=-1 x1=34 y1=8 z1=9 points=[[0, 0, 4, 4], [14, 2, 5, 3.5], [30, 3, 4, 3]] palettes=[3, 9] dirt=10 width=[3, 5] length=[6, 14] only=[3] seed=3   # plates along the grain, not stripes
break_end            point=[25, 3, 4] normal=[1, 0, 0.15] depth=4 density=0.2 length=[4, 14] bark=4 sapwood=[5, 6] tips=7 seed=3   # snapped end
fill_box             x0=-8 y0=-12 z0=-1 x1=40 y1=14 z1=-1 palette=4       # ground, after the break so it stays whole
scatter              x0=24 y0=-6 z0=0 x1=38 y1=12 z1=16 palettes=[5, 6, 7, 3] count=60 size=[1, 3] seed=5   # debris under the break
carve                shape="tube" points=[[-1, 0, 4], [12, 1, 4]] radius=2.5      # hollow end
cavity_tint          x0=-5 y0=-5 z0=0 x1=34 y1=8 z1=9 only=[3, 9, 10] lighten=true   # darker fissures, lighter plate ridges
moss                 x0=-5 y0=-5 z0=0 x1=34 y1=8 z1=12 palettes=[8] style="cushions" only=[3, 9] seed=3   # cushions on the upper side
fill_tube            points=[[14, 4, 7, 2], [17, 8, 11, 1.2], [18, 9, 16, 0.5]] curve="smooth" palette=3 mode="empty"   # side branch
fill_tube            points=[[2, -3, 3, 2], [-2, -7, 0, 1], [-5, -9, -1, 0.5]] palette=4 mode="empty"   # buttress root
preview              path="log.png"
```

### In a script

Every tool above is also a Rhai function (`docs/scripting.md`, "Shapes"),
with the MCP options in a trailing map, so a family of organic pieces can
be a script with parameters instead of a list of calls. This is the core of
`examples/hollow_log.rhai`, the log above with parameters for its length,
radius, wall, roots, and seed:

```rhai
let trunk = [];
for i in 0..5 {
    let t = to_float(i) / 4.0;
    trunk.push([t * len, sin(t * PI) * 1.5, radius, radius * (1.0 - 0.25 * t)]);
}
fill_tube(trunk, 3, #{curve: "smooth"});
smooth(-reach, -reach - 2, 0, length + reach, reach + 2, 2 * reach, #{iterations: 1});
bark(-reach, -reach - 2, 0, length + reach, reach + 2, 2 * reach, [3, 9], #{
    points: trunk, curve: "smooth", dirt: 4, width: [3, 5], length: [6, 14], depth: 1, only: [3], seed: seed
});
// ... roots: fill_tube(points, 4, #{curve: "smooth", mode: "empty"}) ...
break_end([tip[0], tip[1], tip[2]], [1.0, 0.0, 0.15], #{
    depth: 3, density: 0.2, length: [4, 12], bark: 4, sapwood: [5, 6], tips: 7, seed: seed
});
carve_tube([[-2.0, 0.0, radius], [mid[0], mid[1], mid[2]]], #{radius: max(radius - wall, 0.5)});
fill_box(-reach - 6, -reach - 10, -1, length + 14, reach + 10, -1, 8);
scatter(to_int(tip[0]) - 4, -reach - 6, 0, length + 10, reach + 6, 2 * reach + 6, [5, 6, 7, 3],
    #{count: 40, size: [1, 3], seed: seed, floor: 0, clump: 0.6});
```

The order matters as it does for the tools: smooth while the trunk is
solid, add the roots before carving so the hollow cuts them out of the
opening, and add the ground after the break.

`examples/fallen_tree.rhai` is a fallen tree that does not read as a pipe:

```rhai
loft([
    #{x: 0.0, y: 0.0, z: r * 0.85, rx: r * 1.15, ry: r * 0.95, flatTop: -0.3},
    #{x: len * 0.3, y: 1.5, z: r * 0.8, rx: r, ry: r * 0.85, flatTop: -0.25},
    #{x: len * 0.65, y: -1.0, z: r * 0.7, rx: r * 0.75, ry: r * 0.65, flatTop: -0.2},
    #{x: len, y: 1.0, z: r * 0.55, rx: r * 0.5, ry: r * 0.45, noise: 0.08},
], [3, 4], #{curve: "smooth", noise: 0.14, frequency: 7, seed: seed, pattern: "stripes", sectors: 12});
// ... flat roots in a flare, a bough and its two fork branches: loft(..., #{mode: "empty"}) ...
deform(-reach, -reach, 0, length + 2, reach, reach * 2, "displace", #{
    amplitude: lumps, scale: r * 1.2, octaves: 2, bias: 0.6, seed: seed
});
brush("smooth", [[base[0] - 2.0, base[1], base[2] - 1.0], [base[0] + 3.0, base[1] + 1.5, base[2] + 1.0]], #{radius: r * 0.6, strength: 0.8});
brush("inflate", [[len * 0.4, -r, r * 0.9], [len * 0.44, -r, r * 1.0]], #{radius: r * 0.55, strength: 0.9, only: [3, 4]});
brush("grab", [[0.0, 0.0, r * 2.4, r * 1.2]], #{vector: [-2.0, 0.0, 1.0]});
```

Loft the main forms first, displace them while they are thick, and brush
last, on the forms as they will stay.

## Tips

**One object per rig part, at world `[0, 0, 0]`.** Build the head, jaw, body
segments, limbs, and tail each as its own object (`add_object` with
`position` `[0, 0, 0]`) and sculpt it in world cells. The local cells are
then the world cells, so `add_bone` pivots are read straight off the cells
and `bind_object` needs no cell moves.

**Aim in world cells.** Once the parts have positions of their own (after a
`split_object` into an existing object, a `mirror_object`, or a save and
reopen), pass `space: "world"` to the fills, `carve`, `set_voxels`,
`carve_from_views`, and `project_paint`: the same world box hits the same
place in whichever part is active, with no position subtracted by hand.

**Send many small edits as one `batch`.** A run of `fill_box`, `carve`,
`sweep`, `set_palette`, and surface tools such as `planks` goes in one call, one undo step:

```
batch operations=[{tool: "fill_box", arguments: {x0: 0, y0: 0, z0: 0, x1: 9, y1: 2, z1: 0, palette: 3}}, {tool: "carve", arguments: {shape: "box", x0: 4, y0: 0, z0: 0, x1: 4, y1: 2, z1: 0}}]
```

The first item that fails stops it, the error names the item, and the
document stays as it was. Render and export outside it: tools that write
files are refused.

**A fill with a palette fills empty cells too.** `fill_box`,
`fill_rounded_box`, `fill_ellipsoid`, and `fill_tube` write every cell
inside the shape, whether or not it was solid. Pass `mode` `empty` to add
around existing detail without overwriting it, and `mode` `paint` to
recolour only cells that already exist. Palette `0` erases, in `sweep` too;
`carve` erases a shape.

**Keep the rest pose straight.** Sculpt the creature straight and
symmetrical, as the bind pose. Show a curl, a bite, or a swim stroke with
bones: `render`'s `pose` poses one still without changing the document, and
`set_pose` previews in the window. A curved model is hard to rig and to
mirror.

**Frame the part you are working on.** `render` and `model_view` fit every
visible cell in the picture; hidden objects are left out. To look at one
part, pass `target` (an object index, `active`, or a `{min, max}` box of
world cells), or `zoom` to magnify the fitted frame. For a long model, give
only `width` (up to 4096) and the height follows its shape.

**Mirror.** Sculpt one side of a paired part and copy it with
`mirror_object`; `mirrorBones` copies its bones too.

**Start a part again.** `clear_object` empties an object (the active one,
or one by index or name) of every cell, however large: a `fill_box` with
palette `0` stops at 2000000 cells. The object keeps its name, position,
bone, and markers, so a new `carve_from_views` lands where the old one was.

**Split into rig parts.** A creature carved in one piece from its views
(`carve_from_views`) is cut into its parts with `split_object`: the cells
inside boxes, or of some slots, move into a new object (or `into` an
existing one) and keep their world positions, so nothing needs carving
twice. Cut the head, the flippers, and the tail off the body, then bind
each part to its bone.

```
split_object   index="turtle" boxes=[[0, 30, 6, 23, 41, 20]] name="head"
split_object   index="turtle" boxes=[[16, -6, 2, 23, 0, 8]] name="tail"
```

**Joints.** After the parts are bound to their bones, `place_joints` puts
every pivot where its part meets its parent's part: the centre of the faces
they share, the middle of an overlap, or the middle of the gap when they do
not touch. Do not place pivots by eye. `snap: "half"` keeps them on half
cells; `set_bone` with `joint: true` redoes one bone after its part changes.

```
add_bone       name="body" pivot=[12, 20, 10]
add_bone       name="head" parent="body" pivot=[0, 0, 0]
bind_object    index="head" bone="head"
place_joints   snap="half"
```
