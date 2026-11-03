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
   when there is one (see "Start from a mesh"). Otherwise build the large
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
   cells and is one undo step.
4. **Repeat** steps 2 and 3. Render after every step and compare again. A
   first blockout is never the result.
5. **Save.** `save_file`.

```
new_document
set_palette          entries=[{index: 1, r: 74, g: 92, b: 58}, {index: 2, r: 196, g: 184, b: 120}]
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
   cell takes the colour of the nearest view, quantised to `slots`.
2. **Measure it.** `compare_silhouette` draws the model from a camera and
   scores it against one concept view: `iou`, the share of the concept
   `missing`, and the model's `excess`, with an overlay PNG that shows
   where. Compare each turnaround view orthographically (`view` `front`,
   `left`, `top`) and the hero view from `preset` `game`.
3. **Shape it** with the steps below: the hull fills concave parts (the
   space under an arch, between roots), so `carve` them, grow the thin
   parts with `fill_tube`, and `smooth` it. Compare again after each step,
   with the first reply's `align` passed back so the placement holds still.

```
carve_from_views     views=[{view: "front", path: "sheet.png", crop: [40, 60, 520, 300]},
                            {view: "side",  path: "sheet.png", crop: [620, 60, 180, 300]},
                            {view: "top",   path: "sheet.png", crop: [40, 420, 520, 180]}]
                     size=[96, 34, 56] slots=[3, 4, 5, 6] smooth=1
compare_silhouette   path="front-vs.png" concept="sheet.png" crop=[40, 60, 520, 300] view="front" orthographic=true
compare_silhouette   path="hero-vs.png" concept="hero.png" preset="game"
```

`examples/carved_stump.rhai` does the same in a script, with the views as
rows. In the editor, the Carve from views and Compare silhouette sections
hold the same fields, and the comparison shows over the viewport.

## Start from a mesh

A unique organic asset (a fallen tree, a creature, a hero rock) built only
from tubes and boxes tends to read as pipes. When the form can be sculpted
in Blender, or generated from the approved concept by an image-to-3D tool,
bring that mesh in with `import_mesh` and finish it with the voxel tools:

```
import_mesh          path="fallen_tree.glb" voxelSize=0.08 anchor="bottom" newObject=true name="fallen_tree"
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
Read the reply's `bounds` for the boxes of the next tools. `fill` `shell`
keeps only the surface, for a hollow log to carve into. `mode` `paint`
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
`faces`, `facing`, and a height `band`. Moss caps tops and hangs curtains
with a ragged end down the faces below; algae is a band at the waterline,
dense low and thinning upward in clumps; vines hang from top edges, graded
through their slots to the tip, with occasional blooms. Give the vine slots
`set_palette` `motion` `sway` rising from the anchor to the tip and leave
moss and algae still. Pass `only` with the stone slots so plants do not
grow on plants.

Pass `exclude` boxes to `joints`, `weather`, the plants, and `scatter` to
keep walking surfaces whole: a ledge top, a cave floor, or a river bed (its
top layer and the air above it) stays flat while the stone around it is
cut, chipped, and grown on, so it needs no refill. The editor's Protect
section holds the boxes; Add selection appends the selected box.

`set_marker` names a ledge top, a collision box, or a connector on the
piece, so the game reads them from the exported glb (nodes with
`extras.froxel`) instead of measuring the model again. `preview` with
`overlay: true` draws them over the model, so check a collision box
against the stone before export.

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
set_palette          entries=[{index: 3, r: 92, g: 64, b: 40}, {index: 4, r: 60, g: 40, b: 26}, {index: 5, r: 196, g: 160, b: 104}, {index: 6, r: 214, g: 184, b: 128}, {index: 7, r: 238, g: 218, b: 170}]
fill_tube            points=[[0, 0, 4, 4], [14, 2, 5, 3.5], [30, 3, 4, 3]] palettes=[3, 4] pattern="stripes" sectors=10   # fallen trunk with bark grooves
smooth               x0=-5 y0=-5 z0=-1 x1=34 y1=8 z1=9                    # before thin walls and twigs exist
break_end            point=[25, 3, 4] normal=[1, 0, 0.15] depth=4 density=0.2 length=[4, 14] bark=4 sapwood=[5, 6] tips=7 seed=3   # snapped end
fill_box             x0=-8 y0=-12 z0=-1 x1=40 y1=14 z1=-1 palette=4       # ground, after the break so it stays whole
scatter              x0=24 y0=-6 z0=0 x1=38 y1=12 z1=16 palettes=[5, 6, 7, 3] count=60 size=[1, 3] seed=5   # debris under the break
carve                shape="tube" points=[[-1, 0, 4], [12, 1, 4]] radius=2.5      # hollow end
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
fill_tube(trunk, [3, 4], #{curve: "smooth", pattern: "stripes", sectors: 10});
smooth(-reach, -reach - 2, 0, length + reach, reach + 2, 2 * reach, #{iterations: 1});
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

**A fill with a palette fills empty cells too.** `fill_box`,
`fill_rounded_box`, `fill_ellipsoid`, and `fill_tube` write every cell
inside the shape, whether or not it was solid. Pass `mode` `empty` to add
around existing detail without overwriting it, and `mode` `paint` to
recolour only cells that already exist. Palette `0` erases; `carve` erases
a shape.

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
