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
   `fill_rounded_box`, and `fill_ellipsoid`. Get the proportions and the
   silhouette right before any detail.
2. **Render and compare.** `render` the document and put the PNG beside the
   reference (concept art, a photo of the real animal). Look at the
   silhouette first, then the proportions, then the colour.
3. **Refine.** Use `set_voxels` to cut, add, and recolour single cells: the
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

## Tips

**One object per rig part, at world `[0, 0, 0]`.** Build the head, jaw, body
segments, limbs, and tail each as its own object (`add_object` with
`position` `[0, 0, 0]`) and sculpt it in world cells. The local cells are
then the world cells, so `add_bone` pivots are read straight off the cells
and `bind_object` needs no cell moves.

**A fill with a palette fills empty cells too.** `fill_box`,
`fill_rounded_box`, and `fill_ellipsoid` write every cell inside the shape,
whether or not it was solid. To recolour, use a shape that covers only
cells that already exist, or read them with `get_voxels` and write the new
index with `set_voxels`. Palette `0` erases.

**Keep the rest pose straight.** Sculpt the creature straight and
symmetrical, as the bind pose. Show a curl, a bite, or a swim stroke with
bones: `render`'s `pose` poses one still without changing the document, and
`set_pose` previews in the window. A curved model is hard to rig and to
mirror.

**Crop renders when objects are hidden.** `render` frames every object,
hidden ones included. A hidden alternate part (a second jaw, an open eye)
far from the body makes the subject small in the picture. Lower `distance`
(below 1 moves closer), or crop the PNG to the subject before you compare.

**Mirror.** Sculpt one side of a paired part and copy it with
`mirror_object`; `mirrorBones` copies its bones too.
