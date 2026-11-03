# Scripting

A Froxel script is a [Rhai](https://rhai.rs/) program stored on an object.
It declares parameters, then fills voxels. The editor shows one slider (or a
checkbox, a color, or a text field for a list) per parameter. **Regenerate** runs the script.
Opening a file never runs it: the `.vox` already contains the voxels the
script produced last time, so MagicaVoxel opens it as an ordinary model.

The engine is constructed with `Engine::new_raw()`. Nothing in the standard
library that touches the clock, the filesystem, the network, or a process is
registered. There is no `print`, `eval`, `import`, or `timestamp`. Limits:

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
did not target are left alone.

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

## Voxels

Coordinates are integers from −512 to 512. The box is inclusive, and the two
corners may arrive in either order. Slot `0` erases.

```rhai
set(x, y, z, slot);
fill_box(x0, y0, z0, x1, y1, z1, slot);
let slot = get(x, y, z); // 0 when the cell is empty
clear();                 // empty the object currently being written
```

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

It returns `#{changed, pieces, skipped}`; `skipped` counts pieces that met a
solid cell at the top of the box. Other objects do not stop a falling piece.

### Example

`examples/hollow_log.rhai` (baked in `examples/hollow_log.vox`) builds a
fallen hollow log with every shape call: a striped tube trunk, `smooth`,
buttress roots built with `push`, `break_end` at the far end, a carved
hollow, a ground slab, and `scatter` debris under the break.

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

Any function that is not listed on this page is not registered.

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
