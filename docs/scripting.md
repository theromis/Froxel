# Scripting

A Froxel script is a [Rhai](https://rhai.rs/) program stored on an object.
It declares parameters, then fills voxels. The editor shows one slider (or a
checkbox, a color, or a text field for a list) per parameter. **Regenerate** runs the script.
Opening a file never runs it: the `.vox` already contains the voxels the
script produced last time, so MagicaVoxel opens it as an ordinary model.

The engine is constructed with `Engine::new_raw()`. Nothing in the standard
library that touches the clock, the filesystem, the network, or a process is
registered. There is no `print`, `eval`, `import`, or `timestamp`, and no
function reads a file: MCP `import_mesh` has no script form, and
`voxelize` takes the mesh inline. Limits:

| Limit | Value |
| --- | --- |
| Call depth | 32 |
| Array length | 4 096 |
| Map size | 256 |
| String length | 16 384 bytes |
| Script source | 65 536 bytes |
| Objects per run | 32 |
| Coordinate | −512 to 512 inclusive |

There is no cap on operations or on voxels per object. Instead a run can be
stopped: the editor runs the script on a worker thread and shows the elapsed
time with a **Cancel** button, and the window stays responsive. MCP
`regenerate`, the bridge, and parameter discovery run with a 300 second
deadline (MCP `timeoutSeconds` changes it; `0` waits as long as it takes).

A script that exceeds a limit, is cancelled, or times out stops. The voxels,
palette, and materials stay as they were, and the error names the line.

The run writes a scratch copy that starts empty on the host object (the
object whose script this is). On success that copy replaces the host, and
any other object the script targeted, as one undo step. Objects the script
did not target are left alone, unless a document call (object, rig, clip,
or palette; see "Document calls") changes them in that same step.

## Parameters

Call these before building voxels. The same name twice is an error. A stored
value from the editor is used when its kind still matches; otherwise the
default is used. The value is clamped to the range.

```rhai
let radius = param_float("radius", 8.0, 3.0, 16.0); // name, default, min, max
let count = param_int("count", 4, 1, 32);
let thick = param_bool("thick", true);
let tint = param_color("tint", [40, 160, 60]); // [r, g, b], each 0..255
```

`param_float` returns a float. `param_int` returns an integer. `param_bool`
returns a bool. `param_color` returns `[r, g, b]`.

### Lists

A list parameter holds numbers or 2D or 3D points, for a polyline, a curve's
control points, or a row of posts:

```rhai
// name, default, min, max, max_len
let heights = param_floats("heights", [1.0, 2.0, 3.0], 0.0, 8.0, 16);
let shore = param_vec2s("shore", [[0.0, 0.0], [12.0, 4.0]], -64.0, 64.0, 256);
let posts = param_vec3s("posts", [], 0.0, 32.0, 8);

for p in shore {
    set(to_int(p[0]), to_int(p[1]), 0, 1);
}
let count = shore.len();
```

`param_floats` returns `[a, b, …]`, `param_vec2s` returns `[[x, y], …]`, and
`param_vec3s` returns `[[x, y, z], …]`, all floats. Integers in the default
are accepted. `min` and `max` bound every number, each coordinate of a point
included. `max_len` is the most entries the list may have; a list may also be
empty. The array-length limit counts numbers, so `max_len` is at most 4 096
for `param_floats`, 2 048 for `param_vec2s`, and 1 365 for `param_vec3s`. A
default that is too long or out of range is an error at that line.

A stored list is used when it has the same kind. It is cut to `max_len` and
every number is clamped to the range, as the scalar kinds are. A list set from
the editor, MCP, or the bridge that is too long or out of range is refused with
an error that names the entry; the stored list does not change.

## Palette

```rhai
set_slot(1, [40, 160, 60]);          // index 1..255, alpha is left as it was
set_motion(1, "sway", 1.5);          // "none", "sway", or "bob"; amount 0..4
```

`set_motion` is the per-slot motion exported as `TEXCOORD_1`. `0` is not a
paintable slot.

`set_palette` is MCP `set_palette`: one entry, or a list of them, each
`#{index, r, g, b, a, motion, motion_amount}` with `a`, `motion`, and
`motion_amount` optional (`a` defaults to 255). It sets alpha, which
`set_slot` leaves alone. It is a document call: it runs after the script
(see "Document calls").

```rhai
set_palette(#{index: 9, r: 120, g: 200, b: 255, a: 96});   // a glassy slot
set_palette([#{index: 5, r: 40, g: 160, b: 60, motion: "sway", motion_amount: 1.5}]);
```

## Voxels

Coordinates are integers from −512 to 512. The box is inclusive, and the two
corners may arrive in either order. Slot `0` erases.

```rhai
set(x, y, z, slot);
fill_box(x0, y0, z0, x1, y1, z1, slot);
let slot = get(x, y, z); // 0 when the cell is empty
clear();                 // empty the object currently being written
```

`get_voxels` and `set_voxels` are MCP `get_voxels` and `set_voxels` on the
object being written:

```rhai
let cells = get_voxels(x0, y0, z0, x1, y1, z1); // [[x, y, z, slot], …], solid cells, sorted
let changed = set_voxels([[0, 0, 0, 1], #{x: 1, y: 0, z: 0, palette: 2}]);
```

`get_voxels` returns the solid cells of the inclusive box, sorted by x, then
y, then z; each is four numbers, so the array limit allows about 1 000.
`set_voxels` takes `[x, y, z, slot]` or `#{x, y, z, palette}` entries (slot
`0` erases) and returns the number of cells that changed.

## Shapes

These are the MCP hand-sculpting tools (`docs/hand-sculpting.md`,
`docs/mcp.md`) as script functions. They run the same argument checks and
the same shape code as the tools, on the object the script is writing, so a
script and an MCP call with the same arguments build the same cells.

Rhai has no keyword arguments. The required arguments come first, in order;
the rest go in an optional trailing map whose keys are the MCP argument
names, `fillAt` and `rimBias` included:

```rhai
fill_ellipsoid(0, 0, 0, 9, 9, 5, 2, #{mode: "empty"});
```

A key the function does not take is an error, so a misspelt option stops
the run instead of being ignored. Numbers may be integers or floats where
the tool takes a number. Every cell a call writes must lie inside the
coordinate range; one outside stops the run. One call covers at most
2 000 000 cells, checked before any cell is written. Each call returns what
the MCP tool replies, without `voxels`.

### Fills

```rhai
fill_box(x0, y0, z0, x1, y1, z1, slot, #{mode: "paint"});
fill_ellipsoid(x0, y0, z0, x1, y1, z1, slot);            // options: mode
fill_rounded_box(x0, y0, z0, x1, y1, z1, radius, slot);  // options: mode
fill_tube(points, slot_or_slots);
fill_tube(points, slot_or_slots, #{radius: 1.5, curve: "smooth"});
```

Each returns the number of cells that changed. `mode` is `"replace"` (the
default: every cell of the shape), `"empty"` (only empty cells, to add
around existing detail), or `"paint"` (only solid cells, to recolour).
Slot `0` erases. `fill_box` with seven arguments is the original fill: no
options, no return value, and no cell cap.

`fill_ellipsoid` fills the ellipsoid inscribed in the inclusive box.
`fill_rounded_box` bevels the box edges by `radius` (clamped to half the
shortest side; `0` is the plain box).

`fill_tube` fills a tube through `points`, each `[x, y, z]` or
`[x, y, z, radius]`; a whole number is the centre of that cell, and a
radius is 0.5 to 128. The second argument is one slot, or a list of slots
laid out by `pattern`. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `radius` | none | Radius of a point that has none. |
| `curve` | `"polyline"` | `"smooth"` passes a Catmull-Rom curve through the points. |
| `aspect` | `1.0` | Thickness over width, 0.05..=1; below 1 flattens the tube. |
| `roll` | `0` | Degrees the flat side turns about the axis. |
| `profile` | `"round"` | `"blade"` tapers the thickness to a wedge. |
| `pattern` | `"rings"` | With a list of slots: `"rings"`, `"stripes"`, or `"random"`. |
| `period` | `2` | Cells per ring. |
| `sectors` | number of slots | Stripes around the tube, 1..=256. |
| `seed` | `0` | Seed of `"random"`, a whole number of at least 0. |
| `mode` | `"replace"` | As above. |

```rhai
// A bark-grooved trunk that thins toward its end.
fill_tube([[0, 0, 4, 4], [14, 2, 5, 3.5], [30, 3, 4, 3]], [3, 4],
    #{curve: "smooth", pattern: "stripes", sectors: 10});
// A flat splinter.
fill_tube([[30, 3, 6, 1.5], [36, 3, 7, 0.5]], 7, #{aspect: 0.4, profile: "blade"});
```

### Carve

```rhai
carve_box(x0, y0, z0, x1, y1, z1);
carve_ellipsoid(x0, y0, z0, x1, y1, z1);
carve_rounded_box(x0, y0, z0, x1, y1, z1, radius);
carve_tube(points);
carve_tube(points, #{radius: 2.5}); // options: radius, curve, aspect, roll, profile
```

Each removes the solid cells inside the shape and returns how many it
removed. The shapes are those of the fills.

```rhai
// Hollow the first 12 cells of the trunk above.
carve_tube([[-1, 0, 4], [12, 1, 4]], #{radius: 2.5});
```

### Smooth

```rhai
let report = smooth(x0, y0, z0, x1, y1, z1);
let report = smooth(x0, y0, z0, x1, y1, z1, #{iterations: 2, fillAt: 14, removeAt: 12});
```

A majority filter over the 26 neighbours of each cell in the box that keeps
colours. An empty cell with at least `fillAt` (1..=27, default 17) solid
neighbours is filled; a solid cell with at most `removeAt` (−1..=25,
default 9) is removed. `iterations` is 1..=8 (default 1). Cells outside the
box are read but not changed. It returns
`#{changed, filled, removed, iterations}`; `iterations` is the passes run,
fewer when a pass changed nothing. Smooth before adding anything one cell
thick, since it removes it.

### Break

```rhai
let report = break_end(point, normal);
let report = break_end(point, normal, #{depth: 4, density: 0.2, bark: 4, sapwood: [5, 6], tips: 7, seed: 3});
```

Breaks the whole object being written across the plane through `point`
(`[x, y, z]`) with `normal` (`[x, y, z]`, out of the wood toward the part
that goes). Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `depth` | `3` | How far pits reach into the wood, 0..=32. |
| `splinters` | none | Number of splinters, at most 512. Pass this or `density`. |
| `density` | `0.15` | Splinters per cell of the new face, 0..=1. |
| `length` | `[4, 12]` | Splinter length range, or one number. |
| `thickness` | `1` | Splinter base radius, 0.5..=4. |
| `taper` | `1` | 0 keeps the base radius to the tip; 1 thins it to one cell. |
| `rimBias` | `0.5` | 0..=1: more and longer splinters near the rim. |
| `bark` | keep | Slot of the rim of the new face. |
| `sapwood` | keep | Slot or list of slots of the new face and splinters. |
| `tips` | sapwood | Slot of the outer part of each splinter. |
| `seed` | `0` | A whole number of at least 0. |

It returns `#{changed, removed, recoloured, splinters}`. Because it acts on
the whole object, add the ground after the break or in another object.

### Scatter

```rhai
let report = scatter(x0, y0, z0, x1, y1, z1, slots);
let report = scatter(x0, y0, z0, x1, y1, z1, [5, 6, 7], #{count: 60, size: [1, 3], seed: 5, floor: 0});
```

Drops small pieces into the box, one after another. Each falls from the top
of the box until it rests on a solid cell of the object being written (or an
earlier piece) or reaches `floor`, so they heap up. `slots` is one slot or a
list. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `count` | `20` | Pieces, at most 2000. |
| `kinds` | all | List of `"shard"`, `"chip"`, `"pebble"`, `"chunk"`. |
| `size` | `[1, 3]` | Piece size range in whole cells, 1..=16, or one number. |
| `seed` | `0` | A whole number of at least 0. |
| `floor` | box bottom | Lowest z a piece may reach. |
| `clump` | `0.5` | 0 spreads pieces over the box; 1 piles them in its middle. |
| `exclude` | none | Boxes no piece may rest in (see [Protected boxes](#protected-boxes)). |

It returns `#{changed, pieces, skipped}`; `skipped` counts pieces that met a
solid cell at the top of the box or would rest in an `exclude` box. Other objects do not stop a falling piece.

### Sweep

```rhai
let report = sweep(points, #{width: 6, height: 24, radius: 2}, slot_or_slots);
let report = sweep(points, [[-6, 0], [6, 0], [4, 5], [0, 9], [-4, 5]], [1, 2], #{curve: "smooth", pattern: "bands", period: 3});
```

MCP `sweep`: a 2D profile filled along `points`, perpendicular to the path,
for curved walls, banks, and ledges. Each point is `[x, y, z]`,
`[x, y, z, twist]`, `[x, y, z, twist, scale]`, or a map with those keys.
The profile is a list of `[u, v]` points (u to the left of the path, v up)
or a map `#{width, height, radius}` for a rounded rectangle standing on the
path. A cell is in when its centre is inside the profile at the nearest
point of the path, so a flat top stays one layer. The third argument is one
slot or a list. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `curve` | `"polyline"` | `"smooth"` passes a curve through the points. |
| `pattern` | `"bands"` | With a list: `"bands"` by height in the profile, `"rings"` along the path, `"random"`. |
| `period` | `2` | Band or ring size in cells. |
| `seed` | `0` | Seed of the random pattern. |
| `mode` | `"replace"` | `"empty"` adds only into empty cells; `"paint"` recolours solid ones. |

Slot `0` erases the solid cells the sweep covers, for a slanted cut along a
closed or curved outline (`"empty"` is refused with it), and adds `removed` to
the report:

```rhai
let report = sweep(outline, [[-2, 0], [10, 0], [10, 30], [4, 30]], 0, #{curve: "smooth"});
```

It returns `#{changed, cells, length}`.

### Loft

```rhai
let report = loft(points, slot_or_slots);
let report = loft([#{x: 0, y: 0, z: 6, rx: 8, ry: 6, flatTop: -0.3}, [30, 0, 4, 4, 3.5]], [3, 4],
    #{curve: "smooth", noise: 0.15, frequency: 6, seed: 2, pattern: "stripes", sectors: 10});
```

MCP `loft`: a solid along `points` whose section changes from point to
point, for a trunk, root, or limb that tapers, bends, and is egg-shaped and
knobbly. Each point is `[x, y, z]`, `[x, y, z, r]`, `[x, y, z, rx, ry]`,
`[x, y, z, rx, ry, roll]`, or a map with `x`, `y`, `z` and any of
`radius`, `rx`, `ry`, `roll`, `flatTop`, `noise`, `frequency`. The second
argument is one slot or a list. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `radius`, `rx`, `ry` | — | Section radii of points that leave them out (0.5..=128); `ry` defaults to `rx`. |
| `roll` | `0` | Section turn about the path, degrees. |
| `flatTop` | `0` | -1..=1: above 0 cuts the top flat (1 halves it), below 0 the bottom. |
| `noise` | `0` | Knobbly outline, a fraction of the radius, 0..=0.9. |
| `frequency` | `5` | About how many lumps around the outline. |
| `curve` | `"polyline"` | `"smooth"` passes a curve through the points. |
| `caps` | `"flat"` | `"round"` rounds the ends. |
| `pattern` | `"rings"` | With a list: `"rings"`, `"stripes"`, or `"random"`. |
| `period`, `sectors` | `2`, slots | Ring length; stripes around. |
| `seed` | `0` | Outline noise and random pattern. |
| `mode` | `"replace"` | `"empty"` or `"paint"`. |

It returns `#{changed, cells, length}`.

### Deform

```rhai
deform("bend", #{axis: "x", angle: 35, direction: "+z", origin: 20});
deform(x0, y0, z0, x1, y1, z1, "displace", #{amplitude: 2, scale: 6, bias: 0.5, seed: 3});
```

MCP `deform` on the object being written, or on the cells of a box when the
six corners come first. `op` is `"bend"`, `"taper"`, `"twist"`, or
`"displace"`. Options: `axis` (`"x"`, `"y"`, `"z"`), `origin`, `length`,
`center` (`[a, b]`), `angle` (degrees, bend and twist), `direction` (bend,
such as `"+z"`), `start` and `end` (taper), and for displace `amplitude`,
`scale`, `octaves`, `seed`, `bias`. Defaults and ranges are the MCP tool's
(`docs/mcp.md`). Slots are kept. It returns
`#{changed, cells, result, added, removed}`.

### Brush

```rhai
brush("inflate", [[10, 0, 8], [16, 0, 8, 4]], #{strength: 0.8, only: [3, 4]});
brush("grab", [[0, 0, 14, 6]], #{vector: [-2, 0, 1]});
```

MCP `brush`: sculpt dabs along a list of points, each `[x, y, z]`,
`[x, y, z, radius]`, `[x, y, z, radius, strength]`, or a map. `op` is
`"inflate"`, `"deflate"` (`"dig"`), `"smooth"`, `"flatten"`, or `"grab"`
(`"drag"`). Options: `radius` (3), `strength` (0.5, grab 1), `falloff`
(`"smooth"`, `"linear"`, `"constant"`), `only` (slots it may change),
`palette` (slot of added cells), `vector` (grab), `spacing` (half the
radius). It returns `#{changed, dabs, added, removed}`.

### Carve from views

```rhai
let report = carve_from_views([
    #{view: "front", rows: ["..bb..", ".bbbb.", "bbbbbb"], legend: #{b: 3}},
    #{view: "side", rows: ["..b...", ".bbb..", "bbbbbb"], legend: #{b: 3}},
]);
let report = carve_from_views(views, #{cellsPerPixel: 1, origin: [0, 0, 0], shell: 2, smooth: 1});
```

MCP `carve_from_views`: the visual hull of 2 or 3 orthographic views. Each
view's silhouette is extruded through a box along its depth axis, and the
cells every view covers are kept: the rough form of a turnaround sheet in
3D. A script reads no files, so each view is a map of `rows` (strings; `.`
and space are empty, any other character is inked) and a `legend` from a
character to a palette index or `"#rrggbb"`. A character missing from the
legend needs the `palette` option. Over MCP a view is a PNG crop instead,
on this same code path. `view` is `front` (seen from −Y, image right is
+X), `back`, `left` or `side` (from +X: image right is +Y, so the model's
front is on the image's left), `right`, `top` (from +Z, image down is −Y),
or `bottom`; `flip: true` mirrors one. A view may also carry its own
`cellsPerPixel` or `size: [across, down]`. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `cellsPerPixel` | none | Cells per row character for every view without its own scale. |
| `size` | the largest view | The box `[x, y, z]` in cells; a view without a scale is stretched to it; `()` in a slot takes the largest view along that axis. |
| `origin` | `[0, 0, 0]` | Local cell of the box's minimum corner. |
| `palette` | none | One slot for every cell instead of the views' colours. |
| `slots` | 1..255 | Slots a `#rrggbb` legend colour quantises to. |
| `coverage` | `0.5` | Share of a cell's footprint that must be inked. |
| `shell` | solid | Keep only cells within this many cells of the outside, 1..64. |
| `smooth` | `0` | Smooth passes over the hull, 0..64. |
| `mode` | `"replace"` | `"empty"` adds only into empty cells; `"paint"` recolours solid ones. |

A view smaller than the box is centred across and stands on the box's
bottom. Each cell takes the colour of the view whose visible surface is
nearest it, so the sides of a stump are bark and its top shows the rings.
It returns `#{changed, cells, bounds: #{min, max}, size}`; `bounds` is `()`
when nothing is carved. `examples/carved_stump.rhai` carves a stump with
roots from front, side, and top rows.

MCP `compare_silhouette` writes a file, so it has no script form; check a
script's result with it over MCP.

### Voxelize

```rhai
let report = voxelize(#{positions: positions, triangles: triangles});
let report = voxelize(#{positions: positions, triangles: triangles, colors: colors},
    #{size: 32, anchor: "bottom", slots: [1, 2]});
```

The voxelizer and palette mapping of MCP `import_mesh` on a mesh the script
builds. Scripts read no files, so `import_mesh` itself is not a script
function (like the tools that write files); import a file with MCP, File >
Import Mesh…, or `froxel import`, or pass its triangles here. The mesh is a
map: `positions` is a list of `[x, y, z]`, `triangles` a list of
`[a, b, c]` indices into it from 0, and `colors` (optional) one sRGB
`[r, g, b]` (0..=255) per position, interpolated across each triangle. The
options are the MCP tool's except `path`, `newObject`, and `name`:

| Option | Default | Meaning |
| --- | --- | --- |
| `size` | `64` | Cells along the longest axis. Or `scale` (cells per unit) or `voxelSize` (units per cell). |
| `upAxis` | `"z"` | `"y"` turns a Y-up mesh to Z-up. Scripts default to z, the cells' own axes; MCP defaults to y. |
| `rotation` | `[0, 0, 0]` | Degrees about x, then y, then z. |
| `position` | `[0, 0, 0]` | Where the anchor lands, in local cells. |
| `anchor` | `"min"` | `"center"` or `"bottom"`. |
| `fill` | `"solid"` | `"shell"` writes only the cells the surface touches. |
| `colors` | `"auto"` | `"vertex"`: the same for a script mesh, which has no texture. |
| `palette` | none | Paint every cell this slot. |
| `slots` | none | Map colours only to these slots. |
| `maxNewSlots` | `32` | New palette slots for colours far from every slot, written after the run like `set_slot`. |
| `tolerance` | `12` | sRGB distance under which a slot is reused. |
| `mode` | `"replace"` | `"empty"` adds only into empty cells; `"paint"` recolours solid ones. |

It returns the MCP reply as a map: `#{changed, cells, shell, interior,
written, slots, newSlots, bounds, scale, triangles, ...}`, where `bounds`
is `#{min, max, size}`. A mesh is at most 4 096 positions and 4 096
triangles (the array limit). `examples/faceted_boulder.rhai` builds a
lumpy sphere, voxelizes it, and weathers and mosses it.

### Column relief

```rhai
let report = column_relief(x0, y0, z0, x1, y1, z1, "-y", 1);
let report = column_relief(x0, y0, z0, x1, y1, z1, [0, -1, 0], 1, #{width: [7, 13], out: [0, 5], crown: 75, top: [-12, 7], lower: false, seed: 4});
```

MCP `column_relief`: the cliff face in the box, facing `"+x"`, `"-x"`,
`"+y"`, `"-y"` (or a vector along one of them), is broken into columns of
`palette` that stand out of it and end at uneven heights. The face is found
in the cells, so it follows a swept, curved wall. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `width` | `[6, 12]` | Column width range along the face, 1..=64. |
| `out` | `[1, 4]` | How far a column stands out of the face, 0..=16. |
| `crown` | highest solid cell | The height the column tops scatter around. |
| `top` | `[-8, 4]` | Column top range relative to the crown, -256..=256. |
| `lower` | `true` | A column ending below the crown cuts the wall above it; `false` only adds cells. |
| `depth` | `3` | How far a column reaches back into the face, and how deep the cut above it goes, 1..=16. |
| `exclude` | none | Boxes left alone (see [Protected boxes](#protected-boxes)). |
| `seed` | `0` | A whole number of at least 0. |

It returns `#{changed, columns, added, removed, crown}`. Run it after
`sweep` and before `joints` and `weather`.

### Joints

```rhai
let report = joints(x0, y0, z0, x1, y1, z1, "columns");
let report = joints(x0, y0, z0, x1, y1, z1, "courses", #{dirt: 2, depth: 2, course: [3, 5], spacing: [6, 11], only: [1, 3], seed: 4});
```

MCP `joints`: real gaps between stones in the solid cells of the box,
decided where each cell meets the surface and cut straight in. The pattern
is `"columns"` (vertical columns of random widths with a bed joint now and
then, for fractured limestone), `"courses"` (a running bond that follows a
curved wall), or `"cracks"` (an irregular crack network). Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `spacing` | by pattern | Stone width range `[min, max]` in cells, 2..=128: column width, block length, crack piece size. |
| `course` | by pattern | Course height range (courses), or the bed joint spacing across a column (columns), 1..=128. |
| `width` | `1` | Gap width, 1..=3. |
| `depth` | `2` | Gap depth under the surface, 1..=16. |
| `dirt` | keep | Slot of the solid cell at the bottom of each gap. |
| `only` | every slot | Cut only these slots. |
| `density` | `0.8` | Cracks: the share of piece boundaries that crack. |
| `exclude` | none | Boxes left uncut (see [Protected boxes](#protected-boxes)). |
| `seed` | `0` | A whole number of at least 0. |

It returns `#{changed, removed, dirtied}`.

### Weather

```rhai
let report = weather(x0, y0, z0, x1, y1, z1);
let report = weather(x0, y0, z0, x1, y1, z1, #{chip: 0.15, pits: 0.02, broken: 3, noise: [4, 5, 6], only: [1, 3], seed: 4});
```

MCP `weather` on the exposed cells of the box: edges and corners lose
small chips, flat faces take pits (real dents), and the faces they uncover
take `broken`; then `noise` slots recolour soft patches. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `chip` | `0.15` | Chance that an edge cell starts a chip, 0..=1. Corners are likelier. |
| `chipSize` | `2` | Most cells one chip takes, 1..=8. |
| `pits` | `0.02` | Chance that a flat face cell starts a pit, 0..=1. |
| `pitDepth` | `1` | Deepest pit, 1..=4. |
| `broken` | keep | Slot of the faces a chip or pit uncovers, about 0.9 x the stone. |
| `noise` | none | Slots of the colour noise. |
| `noiseDensity` | `0.35` | Share of exposed cells the noise recolours. |
| `noiseScale` | `3` | Noise patch size in cells, 1..=32. |
| `only` | every slot | Act only on these slots. |
| `exclude` | none | Boxes left alone (see [Protected boxes](#protected-boxes)). |
| `seed` | `0` | A whole number of at least 0. |

It returns `#{changed, chips, pits, removed, broken, recoloured}`.

### Bark

```rhai
let trunk = [[0, 0, 7, 7], [28, 1, 7, 6], [56, 0, 7, 5]];
fill_tube(trunk, 3, #{curve: "smooth"});
let report = bark(x0, y0, z0, x1, y1, z1, [3, 4, 5], #{points: trunk, curve: "smooth", dirt: 6, only: [3], seed: 5});
let report = bark(x0, y0, z0, x1, y1, z1, [3, 4, 5], #{axis: "z", width: [4, 8], length: [10, 24]});
```

MCP `bark` on the wood in the box: long plates along the grain, split by
fissures cut into the wood with `dirt` at their bottom, plate ends offset
from the next column's, and an occasional crack across a plate. The plate
slots come second; each plate takes one, so the tone changes plate by
plate instead of in stripes. The grain is `axis` or `points`, one of them
required: pass the tube's own points (radius and all) and `curve` so the
plates follow its bend. End grain and the inside of a hollow are left
alone; bark a hollow log after carving it. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `axis` | — | `"x"`, `"y"`, `"z"`, or `[x, y, z]`: a straight grain through the middle of the wood. |
| `points` | — | The wood's path, `[x, y, z]` and any extra numbers (a tube's radius, a sweep's twist and scale). |
| `curve` | `"polyline"` | With `points`: `"smooth"` follows the same curve as a smooth tube. |
| `dirt` | keep | Slot of the cell at the bottom of each fissure. |
| `width` | `[3, 6]` | Plate width range in cells around the wood, 1..=64. |
| `length` | `[8, 20]` | Plate length range in cells along the grain, 2..=256. |
| `depth` | `[1, 2]` | Fissure depth range in cells, 1..=4; plate ends and cracks take the smaller. |
| `cracks` | `0.25` | Share of plates with a crack part way across. |
| `only` | every slot | Act only on these slots, the wood. |
| `exclude` | none | Boxes left alone (see [Protected boxes](#protected-boxes)). |
| `seed` | `0` | A whole number of at least 0. |

It returns `#{changed, plates, removed, dirtied, painted, cracks}`.

### Cavity tint

```rhai
cavity_tint(x0, y0, z0, x1, y1, z1);
cavity_tint(x0, y0, z0, x1, y1, z1, #{radius: 2, dark: 0.1, lighten: true, light: 0.3});
cavity_tint(x0, y0, z0, x1, y1, z1, #{pairs: [[3, 13], [4, 14, 24]]});
```

MCP `cavity_tint` on the exposed cells of the box: cells in concave spots
(fissures, crevices, the crook of a root) take a darker slot and, with
`lighten`, cells on ridges and corners a lighter one, by how many of their
neighbours in a ball of `radius` are solid. Without `pairs` it makes
shades of each slot's own colour in free palette slots, from 255 down,
never one a cell of the document or of this run uses or `set_palette`
named; they are written to the palette with the voxels and found again on
the next run. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `radius` | `2` | Radius of the ball counted around each cell, 1..=4. |
| `dark` | `0.12` | How concave a cell must be to darken, 0 (a flat face) ..= 1 (a closed pocket). |
| `lighten` | `false` | Also lighten convex cells. |
| `light` | `0.25` | How convex a cell must be to lighten, 0 (a flat face) ..= 1 (a lone spike). |
| `pairs` | make shades | `[slot, darker]` or `[slot, darker, lighter]` per slot; 0 leaves that tint out, and slots without a pair stay. |
| `darken` | `0.72` | Without `pairs`: the darker shade as a factor of the colour, 0.05..1. |
| `brighten` | `1.18` | Without `pairs`: the lighter shade as a factor, above 1 up to 4. |
| `only` | every slot | Act only on these slots. |
| `exclude` | none | Boxes left alone. |

It returns `#{changed, darkened, lightened, shades}`, where `shades` lists
`#{slot, from, rgba}` for the shades it made.

### Foliage

```rhai
foliage([3, 4, 5, 6, 7], #{ellipsoids: [[0, 0, 30, 15, 14, 9]], core: 3, masses: 3, seed: 2});
foliage([3, 4, 5], #{from: [3, 4, 5]});  // re-cluster a crown already sculpted
foliage(x0, y0, z0, x1, y1, z1, [3, 4, 5, 6], #{boxes: [[-8, -8, 20, 8, 8, 30]]});
```

MCP `foliage`: a clustered broadleaf crown of separate leaf clumps with lit
tops, dark undersides, and a dark core in the gaps, instead of one blob.
`palettes` are the leaf slots from the darkest to the lightest. The options
name the volume, so there is no form without them; the box form keeps the
writes inside the box. Only empty cells and the crown's own slots are
written, so the wood the script built first stays whole and shows through
the gaps. Slots keep their motion: give the leaves and the core the same
`set_motion` sway. Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `ellipsoids` | none | Crown volume: `[x, y, z, radius]` or `[x, y, z, rx, ry, rz]` each. |
| `boxes` | none | Crown volume: `[x0, y0, z0, x1, y1, z1]` each. |
| `from` | none | Crown volume: the existing cells of these slots; those no clump keeps are removed. |
| `core` | first of `palettes` | Slot of the dark inside seen in the gaps. |
| `size` | `[3, 6]` | Clump radius range in cells, 1..=32. |
| `gap` | `1` | Extra cells between neighbouring clumps, 0..=8. |
| `flatten` | `0.7` | Clump height over width, 0.2..=1.5. |
| `ragged` | `0.4` | Leafy outline noise, 0..=1. |
| `light` | `1` | How far sunlit tops lighten toward the last slot, 0..=2. |
| `dark` | `1` | How far undersides and seams darken toward the first slot, 0..=2. |
| `sun` | `[-0.35, -0.45, 1]` | Toward the sun, Z up. |
| `masses` | `0` | Lobes the clumps are grouped into, 0..=16, split by grooves. |
| `groove` | `2` | How many cells deeper the grooves between masses go, 0..=8. |
| `protect` | none | Slots never written even if they are crown slots. |
| `exclude` | none | Boxes left alone, such as the space in front of the trunk. |
| `seed` | `0` | A whole number of at least 0. |

It returns `#{changed, clumps, masses, added, recoloured, removed, core}`.

### Moss, algae, and vines

```rhai
moss(x0, y0, z0, x1, y1, z1, [10, 11, 12], #{curtains: 0.3, length: [3, 12], only: [1, 3], seed: 2});
algae(x0, y0, z0, x1, y1, z1, [13, 14, 15], #{band: [0, 9], only: [1, 3]});
vines(x0, y0, z0, x1, y1, z1, [16, 17, 18], #{blooms: [19], facing: [0, -1, 0], length: [6, 22]});
```

MCP `moss`, `algae`, and `vines` on the exposed faces of the box. The
plant slots come second, one slot or a list; give vine slots rising sway
with `set_motion` (or `set_palette` with `motion`). Options shared by all
three:

| Option | Default | Meaning |
| --- | --- | --- |
| `amount` | 0.5, 0.9, 0.08 | Moss and algae coverage (algae at the bottom of its band); the share of top edge cells a vine starts from. |
| `noise` | `0.5` | 0 grows in smooth patches, 1 in ragged speckle. |
| `faces` | top and side (every face with `facing`); side for vines | `"top"`, `"side"`, `"bottom"`, `"all"`, or a list. Moss caps take undersides with `"bottom"`. |
| `facing` | any | Only faces pointing within 60 degrees of `[x, y, z]`, such as `[0, 0, -1]` for undersides. A `facing` that no face in `faces` points along is an error. |
| `band` | none | Only cells with z in `[low, high]`; algae thins from its bottom to its top. |
| `only` | every slot | Grow only on cells of these slots. |
| `exclude` | none | Boxes nothing grows from or into (see [Protected boxes](#protected-boxes)). |
| `layer` | grow; paint for algae | `"grow"` adds a layer on the face, `"paint"` recolours it. Vines always grow. |
| `seed` | `0` | A whole number of at least 0. |

Moss also takes `curtains` (0.3, the share of capped top edges a curtain
runs down from), `length` ([2, 8]), and `thickness` (1..=3). With
`style: "cushions"` it grows separate cushions on exposed cells instead,
never one band, and takes `size` ([2, 5], cushion radius), `gap` (2, cells
between cushions), `exposure` (0.35, how far the surface must face up or
`facing`, a cosine), `drapes` (0.25, the share of cushion edges over a drop
that hang a drape), `length` ([2, 6], drape length), and `thickness` (2, at
a cushion's middle, 1..=4); `amount` defaults to 0.45.

```rhai
moss(x0, y0, z0, x1, y1, z1, [7, 8, 9], #{style: "cushions", amount: 0.45, size: [2, 5], gap: 2, exposure: 0.35, seed: 5});
```

Vines take `length` ([4, 16]), `blooms` (slots), and `bloomChance` (0.06).
Each returns `#{changed, added, painted}`, with `curtains` for moss
(`cushions` and `drapes` for cushions) and `strands` and `blooms` for vines,
and a `warning` saying why when nothing grew.

### Protected boxes

```rhai
let floor = [cx0, -4, fz, cx1, cd, fz + 3];          // the cave floor and the air above it
let tops = [[0, 0, 37, 80, 19, 40], floor];
weather(x0, y0, z0, x1, y1, z1, #{chip: 0.16, pits: 0.02, broken: 2, exclude: tops});
moss(x0, y0, z0, x1, y1, z1, [10, 11, 12], #{exclude: tops});
```

`exclude` on `scatter`, `joints`, `weather`, `bark`, `cavity_tint`, `foliage`, `moss`,
`algae`, and `vines` is one inclusive box `[x0, y0, z0, x1, y1, z1]` or a list of up to 64, of
any size. No cell inside a box is removed, recoloured, or written, no plant
grows from a cell inside one, and no piece rests in one. Give a walking
surface's solid top layer and the air above it, and the stone around it
weathers while the top stays flat: no refill after the dressing.

### Example

`examples/hollow_log.rhai` (baked in `examples/hollow_log.vox`) builds a
fallen hollow log with every shape call: a tube trunk with `bark` plates, `smooth`,
buttress roots built with `push`, `break_end` at the far end, a carved
hollow, a ground slab, and `scatter` debris under the break.

`examples/fallen_tree.rhai` (baked in `examples/fallen_tree.vox`) builds a
fallen tree with the organic tools: a `loft` trunk that tapers, sags, and is
egg-shaped and flattened where it rests, flat `loft` roots in a flare at the
torn root plate, a forked limb of three lofts, `deform` `displace` bark
lumps, and `brush` calls that smooth the limb joint, raise a burl, dig a rot
pocket, and grab the root plate back.

`examples/barked_trunk.rhai` (baked in `examples/barked_trunk.vox`) is a
fallen trunk dressed by its form instead of painted stripes: one tube
slot, `bark` along the trunk's own points and along a branch stub,
`cavity_tint` making darker shades in the fissures and the root flare and
lighter ones on the plate ridges, and `moss` cushions on the upper side
with drapes over the edge.

`examples/clustered_tree.rhai` (baked in `examples/clustered_tree.vox`) is
a broadleaf tree whose crown is separate leaf clumps: a tube trunk and
branches, then `foliage` over three ellipsoids in four masses, with lit
tops, dark undersides, a dark core in the gaps, and leaf slots that sway
while the wood stays still.

`examples/swept_cut.rhai` (baked in `examples/swept_cut.vox`) cuts a block
with `sweep` and slot `0`: a slanted side along a curved outline and a round
channel over the top.

`examples/gorge_wall.rhai` (baked in `examples/gorge_wall.vox`) is a gorge
cliff wall built with the stone tools: a profile swept along a curve,
`column_relief` columns, a ledge swept over them and kept flat with
`exclude`, `joints` columns, `weather`, `moss`, `algae` at the waterline,
`vines` with blooms, and markers for the ledge, the top, collision boxes,
and both ends. `examples/gorge_wall_variants.json` is a parameter set list
for `export_variants` and `froxel variants`; exporting a subset of it
again updates the manifest by name and keeps the other entries.

## Markers

```rhai
marker("ledge_top_4m", 12.5, 4.5, 20);
collision_box(0, 0, 0, 39, 7, 29);                // named collision_1, collision_2, …
collision_box("upper_ledge", 0, 0, 30, 15, 7, 33);
connector("east_end", [40, 4, 0], [1, 0, 0]);
```

MCP `set_marker` from a script: a point, a collision box over inclusive
cells, or a connector with the way it faces, on the object being written,
in its local mesh units (cell `(x, y, z)` spans `x..x+1`). The document
keeps them, the `.vox` saves them, and mesh export writes each as a glTF
node with `extras.froxel`, so the game reads the same ledge and collision
the script computed. A rerun replaces the markers of the objects the script
builds; markers placed by hand stay. A later call with the same name
replaces an earlier one. MCP `preview` with `overlay: true` (or `froxel
preview --overlay all`) draws them over the model with their names, so a
collision box can be checked against the stone before export.

## Objects

The script starts on its host object, already cleared. `object` switches the
object that `set`, `get`, `clear`, and every fill, carve, smooth, break, and
scatter write.

```rhai
object(0);          // an existing object, by index
object("legs");     // that name if it exists, otherwise a new object
```

A new name becomes a new object beside the host. The same name on a later
regenerate replaces that object's voxels instead of creating another copy.
An index outside the document is an error.

## Document calls

These are the MCP object, rig, pose, clip, and palette tools. Each takes
the MCP arguments, with the optional ones in a trailing map; an object is an
index or a name.

```rhai
set_object(object, #{name: "arm_r", position: [0, 0, 4], hidden: false});
remove_object(object);                       // not the script's own object
mirror_object(object, "x", "arm_l");         // axis "x", "y", or "z"
mirror_object(object, "x", "arm_l", #{position: [-6, 0, 0], mirrorBones: true});

add_bone(name, pivot);                       // pivot [x, y, z], fractional world cells
add_bone(name, pivot, #{parent: "hip", rotation: [0.0, 0.0, 0.0, 1.0]});
set_bone(name, #{parent: (), pivot: [0, 0, 6], rotation: [0.0, 0.0, 0.0, 1.0], rename: "spine"});
remove_bone(name);
bind_object(object, "arm");                  // () unbinds
let bones = list_bones();                    // [#{name, parent, pivot, rotation, objects}, …]

set_pose(#{arm: [0.0, 0.0, 0.3826834, 0.9238795]});   // or #{arm: #{rotation, scale}}
clear_pose();

add_clip(name);
set_clip_keys(name, bone, [#{time: 0.0, rotation: [0.0, 0.0, 0.0, 1.0]},
                           #{time: 0.5, translation: [0.0, 0.0, 1.0], scale: [1.0, 1.0, 1.0]}]);
let clips = list_clips();                    // [#{name, bones: [#{bone, keys}]}, …]
remove_clip(name);

has_bone(name); has_clip(name); has_object(name);   // true or false
```

They change the document, not the scratch objects, so the run records them
and applies them after the script finishes: first the voxels, `set_slot`,
and `set_motion`, then these calls in the order the script made them,
through the MCP tools themselves, all in the same undo step. So
`mirror_object` copies an object's final voxels, an object made with
`object("name")` can be named in them, and an index means the object at
that index when the call runs, after any earlier `remove_object`. A call
that fails stops the regenerate: the error names the script line and the
function, and the whole document is put back. A run makes at most 4 096 of
these calls.

`list_bones`, `list_clips`, and `has_bone`, `has_clip`, and `has_object`
read the document as it was when the run started; a call recorded earlier
in the same run is not visible to them. Regenerate replays the calls on the
current document, and `add_bone`, `add_clip`, and `mirror_object` fail on a
name that already exists, as the MCP tools do, so guard them for a rerun:

```rhai
object("arm");
fill_box(4, 0, 0, 8, 1, 1, 2);
if !has_bone("arm") {
    add_bone("root", [0.0, 0.0, 0.0]);
    add_bone("arm", [4.0, 0.5, 0.5], #{parent: "root"});
}
bind_object("arm", "arm");
if !has_object("arm_l") {
    mirror_object("arm", "x", "arm_l", #{mirrorBones: true});
}
if !has_clip("wave") {
    add_clip("wave");
}
set_clip_keys("wave", "arm", [#{time: 0.0, rotation: [0.0, 0.0, 0.0, 1.0]},
                              #{time: 1.0, rotation: [0.0, 0.0, 0.3826834, 0.9238795]}]);
```

A rerun that changes nothing adds no undo step.

## Random and noise

There is no clock and no OS random source. `rng` is a deterministic generator:
the same seed and the same calls return the same numbers.

```rhai
let roll = rng(seed);          // seed is an integer
let unit = roll.float(0.0, 1.0); // lo..hi, lo <= hi
let face = roll.int(1, 6);       // inclusive
```

`noise2(x, y, seed)` and `noise3(x, y, z, seed)` return a smooth value in
0..1. `seed` is an integer. The same inputs always return the same value.

## Math

| Function | Result |
| --- | --- |
| `to_float(x)` | A float; a float stays as it is. |
| `to_int(x)` | An integer, truncated toward zero; an integer stays. |
| `abs(x)` | Absolute value, the same type. |
| `floor(x)`, `ceil(x)`, `round(x)` | A float for a float (`round` goes half away from zero); an integer stays. |
| `sqrt(x)` | A float. A negative number is an error. |
| `pow(base, exp)` | An integer when both are integers and `exp` is at least 0 (overflow is an error); a float otherwise. |
| `min(a, b)`, `max(a, b)` | An integer when both are integers; a float otherwise. |
| `clamp(x, lo, hi)` | The same rule. `lo` above `hi` is an error. |
| `sin(x)`, `cos(x)`, `tan(x)` | Radians, a float. |
| `hypot(x, y)` | Float length of `(x, y)`. |
| `atan2(y, x)` | Radians, the same argument order as the C function. |
| `PI`, `PI()` | π. Functions declared with `fn` cannot see `PI`; use `PI()` there. |

```rhai
let r = sqrt(pow(dx, 2) + pow(dy, 2));
let turn = clamp(t, 0.0, 1.0) * 2.0 * PI;
let cells = to_int(ceil(r));
```

Loops, `if`, `let`, `fn`, and array literals such as `[r, g, b]` are the
language. Array indexing, `for item in array`, and `array.len()` work.

## Arrays

| Method | Effect |
| --- | --- |
| `a.push(value)` | Appends. |
| `a.pop()` | Removes and returns the last entry; `()` when empty. |
| `a.insert(index, value)` | Inserts before `index`. A negative index counts from the end; an index past either end clamps to it. |
| `a.remove(index)` | Removes and returns the entry; a negative index counts from the end; `()` when out of range. |

An array holds at most 4 096 numbers, nested arrays included; going past it
stops the run.

```rhai
let points = [];
for i in 0..5 {
    let t = to_float(i) / 4.0;
    points.push([t * 30.0, sin(t * PI) * 2.0, 4.0, 4.0 - t]);
}
fill_tube(points, 3, #{curve: "smooth"});
```

## Strings

```rhai
for i in 0..3 {
    marker("ledge_" + i, to_float(i * 12), 4.0, 30.0);   // ledge_0, ledge_1, ledge_2
    collision_box(`ledge_${i}_box`, i * 12, 0, 0, i * 12 + 8, 8, 29);
}
let note = "height " + 2.5 + " m, lit " + true;      // "height 2.5 m, lit true"
let label = "piece_"; label += 4;                       // "piece_4"
```

`+` joins two strings, and a string with a whole number, a float, or a bool
on either side; `+=` appends one to a string. Backtick strings interpolate
any value with `${...}`. `to_string(x)` (or `x.to_string()`) gives the text
of a number, bool, or string, and `text.len()` its length in characters. A
float prints as interpolation prints it, with a decimal point (`1.0`,
`2.5`). Strings name markers, objects, and parameters; a string is at most
16 384 bytes.

Any function that is not listed on this page is not registered.

## Variants

A script describes a family; MCP `export_variants` (or `froxel variants`,
or the Export panel's **Export variants…**) bakes it once per parameter set
and writes a `.vox`, a `.glb`, and a manifest with each set's parameters,
markers, and bounds. Scripts themselves never write files, and they read none: `carve_from_views` takes rows, not PNGs.

```json
[
  {"name": "cliff_low", "params": {"height": 18, "seed": 3}},
  {"name": "cliff_tall", "params": {"height": 40, "seed": 7}}
]
```

## Storage

Scripts live in an `MVSC` chunk, version 2 (version 1 is still readable). MagicaVoxel skips chunks it does
not know. The chunk holds, for each object whose source is non-empty, the
object name, the source, the auto-regenerate flag, and the current parameter
values, plus the occupied local minimum. A list parameter is kind 4: width
(1, 2, or 3), `min`, `max`, `max_len`, the count of numbers, then the numbers
as little-endian f32. A document without lists is written as before; an older
Froxel refuses a file that has one. Loading restores the generator's
coordinate frame without running the script, so Regenerate after reopening
does not shift the object. Version 1 files lack that origin metadata; existing
generators should be re-authored in their intended local frame and saved once
with the updated editor. A document with no script does not get the chunk.

The voxels are written in the normal `SIZE` / `XYZI` chunks as well. Save
after a regenerate if you want the file to contain both.

## Editor

The Script panel is on the right, for the active object. The code field is a
plain multiline editor. A list is one line of text under its name, count,
and range: numbers split by commas or spaces, and points split by `;`, as in
`0 0; 12 4`. It applies when the field loses focus; text that does not parse,
is too long, or is out of range is kept in the field and the reason is shown
under the button. Sliders appear after a successful run (when the field
loses focus, or when you press Regenerate). **Auto** reruns about a third of
a second after a parameter change. The error line is shown under the button.
A file that carries a script says so in the panel and in the status line
after opening. Neither of those runs the script.

Froxel saves also carry an `MVOR` version 1 scene-origin chunk. It preserves the
chosen world frame for manual and scripted objects alike; it does not run or add
a script. Foreign VOX imports without this chunk retain automatic centring and
floor placement. See the saved scene coordinates section in `mcp.md`.
