# Scripting

A Froxel script is a [Rhai](https://rhai.rs/) program stored on an object.
It declares parameters, then fills voxels. The editor shows one slider (or a
checkbox, or a color) per parameter. **Regenerate** runs the script.
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

## Objects

The script starts on its host object, already cleared. `object` switches the
object that `set`, `fill_box`, `get`, and `clear` write.

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

`to_float(int)`, `to_int(float)`, `abs`, `sin`, `cos`, `floor`, `hypot(x, y)`,
and `atan2(y, x)` (radians, the same argument order as the C function).

Loops, `if`, `let`, and array literals such as `[r, g, b]` are the language.
Array indexing works. Methods that grow an array, and any function that is
not listed here, are not registered.

## Storage

Scripts live in an `MVSC` chunk, version 2 (version 1 is still readable). MagicaVoxel skips chunks it does
not know. The chunk holds, for each object whose source is non-empty, the
object name, the source, the auto-regenerate flag, and the current parameter
values, plus the occupied local minimum. Loading restores the generator's
coordinate frame without running the script, so Regenerate after reopening
does not shift the object. Version 1 files lack that origin metadata; existing
generators should be re-authored in their intended local frame and saved once
with the updated editor. A document with no script does not get the chunk.

The voxels are written in the normal `SIZE` / `XYZI` chunks as well. Save
after a regenerate if you want the file to contain both.

## Editor

The Script panel is on the right, for the active object. The code field is a
plain multiline editor. Sliders appear after a successful run (when the field
loses focus, or when you press Regenerate). **Auto** reruns about a third of
a second after a parameter change. The error line is shown under the button.
A file that carries a script says so in the panel and in the status line
after opening. Neither of those runs the script.
