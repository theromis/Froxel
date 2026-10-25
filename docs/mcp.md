# Froxel MCP

`froxel` speaks the [Model Context Protocol](https://modelcontextprotocol.io)
on stdin/stdout. It is the editor in this repository.

| Server | Binary | Window | What an agent can do |
|---|---|---|---|
| Froxel | `froxel` | optional | Edit voxels, export glTF, and path-trace a PNG with `render`. Screenshot, inspect, and click the live window when connected. |

## Editor

`froxel mcp` is the Rust editor. Build it from the repository root:

```sh
cargo build
```

The binary is `target/debug/froxel` (or the path Cargo prints). Logs and
usage go to stderr. Stdout carries only protocol messages.

### Headless

```sh
froxel mcp
```

The session lives in this process. It starts empty: one object, the default
palette, no file path. No window opens. `screenshot`, `screenshot_viewport`,
`screenshot_region`, `ui_inspect`, and `ui_click` fail and say how to connect.
`froxel mcp --help`
prints usage and exits. Any other argument is an error, so a client that meant
to reach the window does not silently edit a private document.

```json
{
  "mcpServers": {
    "froxel": {
      "command": "/absolute/path/froxel",
      "args": ["mcp"]
    }
  }
}
```

### Docker

The `Dockerfile` at the repository root runs the headless server from a
release build, checked against `SHA256SUMS.txt`, on x86_64 or ARM64 Linux.
`FROXEL_VERSION` picks the release.

```sh
docker build -t froxel-mcp .
docker run -i --rm froxel-mcp
```

Files the server opens or writes are inside the container; mount a folder
with `-v` to reach them.

### Connected to the window

Start the editor with a control port. It listens on `127.0.0.1` only.

```sh
froxel --control 7979
froxel --control 7979 path/to/model.vox
```

`FROXEL_CONTROL_PORT` is the port when `--control` is omitted. A port on the
command line wins.

```sh
froxel mcp --connect 127.0.0.1:7979
```

`--connect` requires `HOST:PORT`. Without it, or with an unknown argument, the
process exits instead of falling back to a headless session. If nothing is
listening, startup fails.

`--launch` opens this same binary with `--control PORT` when the port is
closed, then waits up to 20 seconds. If the editor is already listening, it
connects and does not open a second window.

```json
{
  "mcpServers": {
    "froxel": {
      "command": "/absolute/path/froxel",
      "args": ["mcp", "--connect", "127.0.0.1:7979", "--launch"]
    }
  }
}
```

The editor writes an unguessable token to an owner-only file,
`froxel-control-<port>.token`, under `$XDG_RUNTIME_DIR` or `~/.cache`. The
MCP bridge reads that file itself. A request without the token is refused.
The path is printed on stderr when the port opens; the token is not.

Edits from a connected client appear in the open window. The headless session
and the window are different documents.

### Protocol

Newline-delimited JSON-RPC 2.0: one message per line, 16 MiB limit,
`initialize` first, `ping` before that, tool failures as `isError: true`.
The default revision is `2025-06-18`.
`2026-07-28`, `2025-03-26`, and `2024-11-05` are accepted and echoed back.

### Coordinates

Edits from the voxel tools apply to the **active object**, in that object's
**local** cells. `x` and `y` are horizontal, `z` is up. Add the object's
world `position` (the same three integers as `translation`) to a local cell
to get its world cell. `add_object` and `set_object` take that world
position; moving an object moves its voxels in the world and leaves the local
cells where they are. `position` is the world cell of local `(0, 0, 0)`, in a
new document and in a file just opened. MagicaVoxel stores `nTRN` `_t` at
the model centre, `SIZE / 2` with integer division. A rotation `_r`, and a
parent group's translation, turn that centre. Loading subtracts the rotated
centre offset so the SIZE corner stays local `(0, 0, 0)`. Saving adds the
offset back, so MagicaVoxel still opens the parts in place. Bone pivots are
fractional world coordinates: a joint may sit on a corner, a cell centre, or
anywhere between cells. Palette
index `0` is empty and erases a cell. Indices `1..255` are colors. A cell
must lie in `[-1024, 1024)`. A write that would make the active object longer
than 256 on any axis is refused and changes nothing.

`save_file` writes every non-empty object. A new object is a normal model:
its own `SIZE`/`XYZI`, an `nTRN`/`nSHP` pair under the scene group, `_name`
on the transform, and the model centre in that transform, so MagicaVoxel
opens the parts in place. The reply is `path`, `voxels` (solid cells in the
whole file, not only the active object), and `objects` (models written).
An object with no voxels is omitted. A file that
had a scene graph is written back with that graph: groups, layers, hidden
objects, and animation frames after the first stay. Frame 0 takes the edited
translation, rotation, name, and `_hidden`. Each model is at most 256 cells
on an axis. A rotation read from the file is baked into the cells for editing
and written back as `_r`. Resizing that object keeps the rotation. A new
document, or a file with no scene graph, is saved as one group of objects.
`open_file` loads every object. `nTRN` translation and rotation are applied,
and `_hidden` on a node or an ancestor hides that object in the mesh and the
tracer. A file with no scene graph still loads its first model.

### Tools

| Tool | Required arguments | What it does |
|---|---|---|
| `new_document` | — | Empty document, default palette, cleared path and undo stack |
| `open_file` | `path` | Open a `.vox`, replacing the session |
| `save_file` | — | Write every non-empty object. `path` overwrites the open file when omitted. Replies with `voxels` and `objects` |
| `inspect_document` | — | Path, counts, world bounds, objects (name and world position), palette, and each slot's motion |
| `get_voxel` | `x`, `y`, `z` | Palette index of one **local** cell of the active object. `0` is empty |
| `set_voxel` | `x`, `y`, `z`, `palette` | Write one **local** cell. One undo step |
| `set_voxels` | `cells` | Write up to 200000 **local** cells as one undo step |
| `fill_box` | `x0`, `y0`, `z0`, `x1`, `y1`, `z1`, `palette` | Fill an inclusive box of **local** cells, at most 2000000, one undo step |
| `fill_ellipsoid` | `x0`, `y0`, `z0`, `x1`, `y1`, `z1`, `palette` | Fill the ellipsoid inscribed in that box. The box volume is checked first, even when the solid is smaller. One undo step |
| `fill_rounded_box` | `x0`, `y0`, `z0`, `x1`, `y1`, `z1`, `palette`, `radius` | Same box, with an edge bevel of `radius` voxels. `radius` 0 is the full box. The radius is clamped to half the shortest side |
| `get_voxels` | `x0`, `y0`, `z0`, `x1`, `y1`, `z1` | Non-empty **local** cells inside an inclusive box, at most 100000 |
| `set_palette` | `index`, `r`, `g`, `b` or `entries` | Replace one slot, or many via `entries`, as one undo step. `a` defaults to 255. Optional `motion` (`none`, `sway`, `bob`) and `motion_amount` (0..4) |
| `set_active_object` | `index` | Edit a different object. Later voxel tools use its local cells |
| `add_object` | — | Empty object. Optional `name`, world `position` `[x, y, z]` (default `[0, 0, 0]`), `size` `[x, y, z]` (capacity hint, each axis 1..=256), `make_active` (default true). Returns the index. One undo step |
| `set_object` | `index` | Optional `name`, world `position`, `hidden`. Moving changes the scene-graph translation, so voxels move in the world. One undo step |
| `remove_object` | `index` | Remove the object and its voxels. Its bone stays; the binding goes. Later indices decrease by one. Refuses the last object. One undo step |
| `mirror_object` | `index`, `axis`, `name` | Copy the object mirrored across `x`, `y`, or `z` through the origin. Optional world `position` and `mirrorBones` (default false). One undo step |
| `add_bone` | `name`, `pivot` | Add a bone. `pivot` is a fractional world coordinate `[x, y, z]`. Optional `parent` (omit or null for a root) and `rotation` quaternion `[x, y, z, w]` (default identity `[0, 0, 0, 1]`). One undo step |
| `set_bone` | `name` | Optional `parent` (null makes a root; omit leaves it), world `pivot`, `rotation` `[x, y, z, w]`, `rename`. One undo step |
| `remove_bone` | `name` | Delete a bone. Objects bound to it become unbound. Children keep its parent. One undo step |
| `bind_object` | `index`, `bone` | Bind an object to one bone, or `bone: null` to unbind. Rigid: one bone, weight 1. One undo step |
| `list_bones` | — | Each bone's parent, world pivot, quaternion, and the object indices bound to it |
| `set_pose` | `pose` | Preview pose. Map of bone name to a local quaternion `[x, y, z, w]`, or `{rotation, scale}`, on top of the bind pose. One undo step. Does not write the bind rotation |
| `clear_pose` | — | Remove the preview pose. One undo step |
| `add_clip` | `name` | Empty named clip. One undo step. Saved in the `.vox` `MVAN` chunk |
| `set_clip_keys` | `name`, `bone`, `keys` | Replace one bone's keys. Each key is `time`, optional `rotation`, `translation`, `scale`. One undo step |
| `list_clips` | — | Clips and their bone keys |
| `remove_clip` | `name` | Remove a clip. One undo step |
| `undo` | — | Undo the last edit |
| `redo` | — | Redo the last undone edit |
| `render` | `path` | Path-trace the scene to a PNG. `width` and `height` default to 256. `samples` defaults to 16. `bounces` defaults to 4. `denoise` defaults to true. `yaw` defaults to 30°, `pitch` to 25°. Optional `orthographic`, `views`, `pose`, `showBones` |
| `screenshot` | — | PNG of the whole editor window. Connected mode only |
| `screenshot_viewport` | — | PNG of the 3D viewport, including its mode chips. Connected mode only |
| `screenshot_region` | `x`, `y`, `width`, `height` | PNG of a rectangle in window points. Connected mode only |
| `ui_inspect` | — | Previous frame's controls, tool, mode, active object, voxel count, and status. Connected mode only |
| `ui_click` | `x`, `y` | Click the topmost registered control. A viewport hit stamps the active tool. A slider sets its value from the point. A palette cell selects that slot. Connected mode only |
| `export_mesh` | `path` | Write a glTF 2.0 `.glb` of every object. `merge` defaults to false. `vertexColors` defaults to false. `exportHidden` and `splitObjects` default to false. A slot with motion adds `TEXCOORD_1`. Clips become glTF animations. The reply includes `structure` |
| `export_models` | `models` | Write 1 to 16 `.glb` files. Each item is `{path, source?, merge?, vertexColors?, exportHidden?, splitObjects?}`. `source` is a `.vox` opened in its own session, so the open document stays. Each file's `structure` lists nodes, meshes, skins, animations, and warnings |
| `set_script` | `source`, optional `object` | Store a Rhai script and read its parameters. One undo step. Does not rebuild voxels |
| `get_script` | optional `object` | Source, auto flag, and parameters |
| `set_params` | `values`, optional `object` | Set parameter values by name. One undo step. Does not run the script |
| `list_params` | optional `object` | Parameters from the last successful run |
| `regenerate` | optional `object` | Run the script. One undo step. A failure leaves voxels unchanged and names the script line |

`set_voxels` accepts each cell as `{"x","y","z","palette"}` or as
`[x, y, z, palette]`. A later duplicate of the same coordinate wins. Cells
that already hold the requested index are not an undo step.

`fill_box` corners may arrive in either order. The box is inclusive.
`fill_ellipsoid` and `fill_rounded_box` use that same inclusive box. A cell
is solid when its centre lies inside the shape. The ellipsoid is centred on
the box and touches the middle of each face. `fill_rounded_box` measures
the distance from the cell centre to the box inset by `radius`. Both refuse
a box larger than 2000000 cells before writing, and both are one undo step.

`set_palette` takes either one slot (`index`, `r`, `g`, `b`, optional `a`) or
`entries`: an array of those objects, at most 256. Pass one form or the other,
not both. Optional `motion` is `none`, `sway`, or `bob`. Optional
`motion_amount` is a number from 0 to 4. Omit either field to leave it.
A call that changes nothing does not push undo. Motion is stored on the
slot's material and is one undo step together with the color.

`inspect_document` returns:

- `path`, `activeObject`, `objectCount`, `voxels` (every object)
- `bounds` of every object in world space and `activeBounds` of the object
  being edited (`null` when empty). Each box is `{min: [x,y,z], max: [x,y,z]}`
- `unsynced` (always false; edits are already in the window), `canUndo`, `canRedo`
- `objects`: `index`, `name`, `hidden`, `active`, `model`,
  `voxels`, local `bounds`, world `position` `[x,y,z]`, and `translation`
  (the same three integers as `position`). `bone` is the bound bone name, or
  null. `bounds` is local cells; add `position` for the world box.
- `palette`: 256 strings, `#rrggbbaa`, index 0 first
- `motion`: 256 objects `{kind, amount}`, index 0 first. `kind` is `none`,
  `sway`, or `bob`. `amount` is 0..4 and defaults to 1. `none` is still

A screenshot tool returns a text block with `width`, `height`, and `path`, plus
an MCP `image` block (`image/png`). `path` also writes that file. Omit it to
keep the picture only in the reply. `screenshot_region` is measured in window
points from the top-left of the client area, not the title bar. On a retina
display the PNG is larger than the point size. A rectangle outside the window
is an error.

`ui_inspect` describes the previous frame. `window` is the client area.
`regions` lists the tool buttons, Add/Remove/Paint, the outliner New,
Duplicate, and Delete buttons, the sliders, the palette cells, the
Model/Object and Edit/Render chips, and the viewport. Each region is `id`,
`label`, `x`, `y`, `width`, `height` in window points from the top-left of
the client area. The reply also includes `tool`, `toolMode`, `editorMode`,
`renderMode`, `activeObject`, `voxels`, and `status`.

`ui_click` takes `x` and `y` in those same points and activates the topmost
region that contains the point. A hit on the viewport is a press and release
of the active tool. A hit on a slider sets that slider from how far `x` is
along the track. A hit on a palette cell selects that slot. A point on no
region is an error. Both tools need the connected window; headless they fail
like the screenshots.

`render` path-traces every visible object and writes a PNG. It does not need a
window. `width` and `height` are 1..1024 and default to 256. `samples` is
1..128 and defaults to 16. `bounces` is 0..8 and defaults to 4. `seed`
defaults to 1. The camera frames the scene, then orbits: `yaw` defaults to
30 degrees, `pitch` to 25 degrees (−90..90), and `distance` multiplies that
framing distance (greater than 0, at most 100). Yaw 0 puts the camera on
the +X side of the target, looking toward −X. A model whose face points
along −Y is face-on at yaw −90. `orthographic` defaults to false; true
uses parallel rays whose frame matches the perspective view at the same
distance. `pose` is an optional map of bone name to quaternion
`[x, y, z, w]` or `{rotation, scale}` (see `set_pose`), applied on top of
the bind pose for this still only. The
window's preview pose and the bind rotation are unchanged when the call
returns. `showBones` defaults to false; true draws joint points and parent
links on the PNG after tonemapping. `views` is a list of
`{yaw, pitch, path}`. The required `path` is still written with the
top-level yaw and pitch, and each view is an extra PNG. An empty
`views` list is an error. The file is ACES tonemapped.
`denoise` defaults to true. The default build runs the edge-aware filter;
a build with `--features oidn` runs Open Image Denoise on the finished
frame, using the first-hit albedo and normal. `atmospheric`,
`sky-intensity`, `sun-intensity`, `glass-mode` (`probabilistic` or
`absorption`), `aperture`, and `focus` use the same names as
`froxel render` and change the still only when present. An empty scene
is an error. The call does not move the editor camera. The reply is
`path`, `width`, `height`, and `samples`, plus `views` when that list
was sent.

`export_mesh` writes every solid voxel of every visible object, exposed faces
only. Hidden objects are left out. `merge` defaults to false, one quad per
exposed face. `true` greedy-merges coplanar faces that share a palette slot,
then splits an edge where a corner of a neighbouring face on a different
plane or palette slot lands in the open span, so the seam is not a
T-junction. Slots are never merged together, so different motion stays on
separate faces. `vertexColors` defaults to false:
one material, a 256-wide palette texture, and each face's UVs point at its
palette slot. `true` stores a linear `COLOR_0` and no image. The palette
texture keeps the sRGB bytes. `COLOR_0` is converted with the sRGB transfer
curve. When any palette slot has motion, every vertex also gets
`TEXCOORD_1`: x is the sway amount and y is the bob amount, or 0 for a still
slot. Godot imports that as UV2. A document with no motion omits
`TEXCOORD_1`. The file is Y-up, positions `(x, z, −y)`. File → Export Mesh…
uses the palette texture and always merges. An empty scene is an error.
The reply's `structure` is the JSON chunk: node names, mesh primitive vertex
and index counts, position bounds, skin joint counts, and animation channel
counts. A warning names an empty mesh, a skin with no joints, a missing or
repeated node name, or an animation with no channels.

`export_models` is the same write, for 1 to 16 files. A `source` path loads
that `.vox` into a separate session. The open document is not replaced and is
what a model without `source` exports. One file's error is that model's
`error`; the other files are still written. `ok` is false when any model
failed.

When the document has a skeleton, `export_mesh` writes a skinned mesh
instead of a rigid one. Each bone is a joint node (name, translation,
rotation) parented as in the skeleton. The mesh node carries `skin` 0. That
skin lists every joint and an `inverseBindMatrices` accessor, the inverse of
each bone's bind-pose matrix. Every vertex has `JOINTS_0` (one joint index,
the other three 0) and `WEIGHTS_0` (`[1, 0, 0, 0]`). Binding is rigid: an
object uses one bone, weight 1. An unbound object follows the skeleton root.
A merged quad never shares a vertex between two bones; faces are grouped by
bone before the T-junction split. `TEXCOORD_1` is still written when any slot
has motion.

`exportHidden` defaults to false, so a hidden object stays out of the file.
`true` includes it. `splitObjects` defaults to false, one mesh named
`voxels`. `true` writes one mesh node per object, named with the object
name, still sharing a single skin when the document has a skeleton. A node
name never repeats a bone's or an earlier part's: the clash gets `_mesh`
(then `_mesh2`, ...), so an object `jaw` bound to bone `jaw` is node
`jaw_mesh` and the joint stays `jaw`. Godot makes node names unique and
would otherwise rename the bone. The mesh itself keeps the object name. A game
can then hide one part, such as an alternate face, without a second export.
When the document has a skeleton, clips are written as a glTF `animations` array. Each clip is one animation
whose name is the clip name. Channels target the joint nodes
(scale, linear, plus rotation and translation when any key of that bone sets
them). A bone whose keys never set `rotation` or `translation` has no such
channel, so it stays at its bind rotation or offset. Godot 4 imports that array as
an AnimationLibrary on the AnimationPlayer. A document with no clips omits
`animations`, so the bytes match a rig-only export.

`add_object` appends an empty object and returns its index. `position` is
the world voxel coordinate of local cell `(0, 0, 0)` and defaults to
`[0, 0, 0]`. `size` is an optional local SIZE box, each axis `1..=256`. It
does not stop later edits from growing the object up to the 256-per-axis
limit. `make_active` defaults to true; `false` leaves the previous object
active, so `fill_box` still edits that one. `set_object` changes the name,
the world position, or `hidden`. A new position replaces the translation;
local cells are not rewritten, and the world bounds move by the same offset.
`remove_object` deletes the object and its voxels. The bone it was bound to
stays in the skeleton. Object indices after the removed one decrease by one,
and the reply sets `indicesShifted` with a `note` when that happens. The
last remaining object cannot be removed.

`mirror_object` copies one object. `axis` is `x`, `y`, or `z`. A cell at
world coordinate `c` on that axis lands at `-c - 1` (cell `(2, 0, 0)`
mirrored on X becomes world x −3). The other axes stay. Omit `position`
and the copy's translation is the minimum corner of those mirrored cells;
local cells are relative to it. A `position` is that corner instead, and
the locals are the mirrored world cells minus it. `mirrorBones` defaults
to false, and the copy is unbound. `true` requires a bound object and
copies that bone plus its descendants. A name ending in `_l` becomes
`_r`, and the reverse; any other name gains `_r`. A parent inside the
copied subtree is the mirrored parent. A parent outside it stays, so a
limb remains on the spine. The pivot's component on the axis is negated
(a fractional pivot stays fractional). The local quaternion flips the two
axes that are not the mirror axis. The new bone name must not already
exist. Objects and bones are one undo step.

`set_pose` replaces the window preview. `pose` maps a bone name to a
quaternion `[x, y, z, w]` in voxel space, or to an object with optional
`rotation` (that quaternion) and `scale` `[x, y, z]` (each 0 to 16, default
1). The rotation is multiplied on the right of that bone's bind rotation,
then the parent chain is applied, so a posed parent orbits its children.
The scale is local, about the joint, and the bone's children inherit it,
as in glTF. A scaled part is resampled into the voxel grid, so a grown part
has no holes; a zero axis flattens it, and the tracer leaves it out, the way
a game hides a part with a zero bone scale. A bone omitted from the map
stays at its bind pose. An entry with identity rotation and scale 1 is
dropped. The bind pose in the skeleton is not
written. `clear_pose` removes the preview. Both are undo steps, separate
from `set_bone`. The window draws joint points and parent links whenever
the skeleton is not empty. That overlay is not a tool default.

`add_clip` creates an empty clip. `set_clip_keys` replaces the keys of one
bone in that clip. A key's `time` is seconds, finite and ≥ 0, and times
must increase. `rotation` is the bone's local quaternion. Omit it to keep the bind
rotation, the same way as `translation` below: no rotation channel when no
key of that bone sets one, and the bind rotation on a key that leaves it out
in a track that has some.
`translation` is the full local offset from the parent in voxel units, not
a delta from the bind pose. Omit it to keep the bind offset: a track with no
translation writes no glTF translation channel, and a key without one in a
track that has some repeats the bind offset. `list_clips` shows an omitted
one as `null`. `scale` is local
(default `[1, 1, 1]`). A rest key that should match the bind pose sets neither. `list_clips` returns the clips.
`remove_clip` deletes one. Each of these is one undo step. Save writes
them in an `MVAN` chunk (version 3: a flag byte per key says whether it
has a rotation and a translation; version 2 files, with a translation flag
only, and version 1 files, with both always set, still load) next to `MVRG`. MagicaVoxel skips the chunk.

`add_bone` requires `name` and a world `pivot` `[x, y, z]`. The components
are fractional, so the joint can sit between cells. `parent` omitted
or null is a root. `rotation` is a quaternion `[x, y, z, w]` in voxel space
and defaults to `[0, 0, 0, 1]`. `set_bone` edits one bone; omit a field to
leave it, and pass `parent: null` to make it a root. `remove_bone` unbinds
every object that used that bone and reparents its children to its parent.
`bind_object` takes an object `index` and `bone` (a name, or null to unbind).
One object has one bone. `list_bones` returns `parent`, `pivot`, `rotation`,
and the object indices currently bound to each bone.

A two-part character, headless:

```
new_document
set_voxel            x=0 y=0 z=0 palette=1          # body, local cell of object 0
add_object           name="leg" position=[2, 0, 0]  # becomes active, index 1
fill_box             x0=0 y0=0 z0=0 x1=0 y1=2 z1=0 palette=2
add_bone             name="root" pivot=[0, 0, 0]
add_bone             name="leg" parent="root" pivot=[2, 0, 0]
bind_object          index=0 bone="root"
bind_object          index=1 bone="leg"
export_mesh          path="character.glb"
```

The body voxel is local to object 0. The leg box is local to the new object,
so those cells sit at world x = 2. The glTF has joint nodes `root` and
`leg`, a skin, `inverseBindMatrices`, and `JOINTS_0` / `WEIGHTS_0`.

`set_script` stores `source` on `object` (the active object when `object` is
omitted) and runs it once only to discover parameters. The voxels stay as
they were. A script that does not run is still stored, and the error text
includes the line. `set_params` takes `values`, an object whose keys are
parameter names. Numbers fill float and int parameters, booleans fill bool
parameters, and `[r, g, b]` fills a color. Values outside the declared range
are an error. `list_params` and `get_script` return each parameter as
`{name, kind, value}` plus `min` and `max` for float and int. `kind` is
`float`, `int`, `bool`, or `color`.

`regenerate` runs the stored script in the sandbox and, on success, replaces
every object the script wrote. That is one undo step. A loop that passes the
operation limit, a call-depth limit, or any other runtime error leaves the
voxels, palette, and parameters as they were. The error includes the script
line. Opening a `.vox` never regenerates; call `regenerate` yourself. The
language and the `MVSC` chunk are described in `docs/scripting.md`.
`examples/lily_pad.rhai` is a pad with `radius`, `notch`, `direction`, and
`seed`.

### What this server does not do

Render mode in the window path-traces on the GPU; saving that frame is a
window command. `render` is a separate CPU trace. It does not drive every mouse tool
by name. `ui_click` hits the controls `ui_inspect` lists, including sliders
and palette cells, plus a stamp in the viewport. Voxel changes also go
through `set_voxel`, `set_voxels`, and `fill_box` on the active object.
Screenshots need the connected window.

Batch export checks all destinations before writing. Names that resolve to the
same path, including links, `..` aliases and case-only differences, are rejected
as a malformed batch. Other per-model failures remain independent: a failed
source does not prevent valid sources from exporting.
