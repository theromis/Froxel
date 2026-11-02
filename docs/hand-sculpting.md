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

1. **Block out.** Build the large forms with `fill_box`,
   `fill_rounded_box`, and `fill_ellipsoid`. Grow limbs, roots, branches,
   tails, and twigs with `fill_tube`. Cut hollows, tunnels, and pits with
   `carve`. Get the proportions and the silhouette right before any detail.
2. **Render and compare.** `render` the document and put the PNG beside the
   reference (concept art, a photo of the real animal). Look at the
   silhouette first, then the proportions, then the colour.
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

`carve` removes the cells inside a `box`, `ellipsoid`, `rounded_box`, or
`tube`: a hollow trunk, a tunnel, a burrow, an eye socket. A carved tube
with a radius just under the trunk's makes a hollow log in one call.

`smooth` softens a blocky region. It fills an empty cell that most of its
26 neighbours surround and removes a solid cell that few of them touch, and
a filled cell takes the colour most of its neighbours hold. One pass with
the defaults cleans pits and spikes and rounds corners; `fillAt` 14 and
`removeAt` 12 round the edges too, and `iterations` repeats it. It removes
anything one cell thick, so smooth before the twigs, thin carved walls, and
fine detail, or keep them outside the box.

```
set_palette          entries=[{index: 3, r: 92, g: 64, b: 40}, {index: 4, r: 60, g: 40, b: 26}]
fill_tube            points=[[0, 0, 4, 4], [14, 2, 5, 3.5], [30, 3, 4, 3]] palettes=[3, 4] pattern="stripes" sectors=10   # fallen trunk with bark grooves
smooth               x0=-5 y0=-5 z0=-1 x1=34 y1=8 z1=9                    # before thin walls and twigs exist
carve                shape="tube" points=[[-1, 0, 4], [12, 1, 4]] radius=2.5      # hollow end
fill_tube            points=[[30, 3, 6, 2], [36, 6, 10, 1.2], [40, 6, 15, 0.5]] curve="smooth" palette=3 mode="empty"   # crown limb
fill_tube            points=[[2, -3, 3, 2], [-2, -7, 0, 1], [-5, -9, -1, 0.5]] palette=4 mode="empty"   # buttress root
preview              path="log.png"
```

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
