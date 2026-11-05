# Fast preview

`preview` (MCP) and `froxel preview` (command line) draw a model the way the
edit viewport shows it, without the path tracer and without a window. The
implementation is `src/preview.rs`; both entry points call it.

- **Mesh.** The viewport's own exposed-face mesh (`mesh::mesh_cells`, the
  same faces `mesh_chunk` uploads per chunk), moved by each object's
  position and the preview pose.
- **Light.** The viewport shader: palette colour in linear light times
  `0.30 + key·max(n·sun, 0) + 0.08·n.z`, clamped to 0.12..1.35, where `key`
  grows with the document's sun intensity and `sun` is its sun direction.
  No shadows, sky, bounces, materials, or tonemap. `lighting` replaces it
  with a game's sun and ambient, below.
- **Raster.** A CPU triangle rasteriser with a depth buffer. 2×2
  supersampling (resolved in linear light) is on by default up to
  2048×2048 output pixels. No GPU is needed, so it runs headless, in CI,
  and in Docker.
- **Speed.** A 140 000-voxel scene at 1600×800 takes about 0.2 s in a
  release build, opening the `.vox` included.

## Camera and framing

The camera orbits at `yaw` and `pitch` in degrees, with the conventions of
`render`: yaw 0 sits on +X looking toward −X, a model facing −Y is face-on
at yaw −90, and pitch 90 looks straight down. `view` (`front`, `back`,
`left`, `right`, `top`, `bottom`) names the same angles as in `model_view`.

The camera does not use the editor's framing distance. It fits the posed
vertices themselves inside the image, leaving `margin` (a fraction of the
image on each side, default 0.06):

- **Perspective** (default) uses the viewport's 50° vertical field of view,
  or `fov`. Each side plane of the frustum, shrunk by the margin, touches
  the most extreme vertex toward it, and the silhouette is then centred.
  The tighter axis touches the margin; the other has room to spare.
- **Orthographic** fits the projected silhouette exactly and replies
  `pixelsPerVoxel`.

`object` (an index or a name) frames one object. Other visible objects are
still drawn, and may fall outside the image, unless `isolate` is set. An
object named this way is drawn even when hidden. Without `object`, every
visible object is drawn and framed.

`preset` names a camera. `game` is the gameplay camera: frog-world's
(`scripts/player/pond_camera.gd`), orthographic, yaw −135, pitch 32. `default`
is the perspective yaw 30, pitch 25. A project defines its own or overrides
these with camera presets, name to `{yaw, pitch, orthographic, fov, margin}`.
They come from, later over earlier and field by field:

1. The built-in ones.
2. The user's `camera_presets.json` in Froxel's support folder
   (`~/Library/Application Support/Froxel` on macOS, else
   `$XDG_DATA_HOME/froxel` or `~/.local/share/froxel`), or the file
   `FROXEL_CAMERA_PRESETS` names.
3. `presets` (MCP, an object or the path of a JSON file), `--presets
   presets.json` (command line), or the Camera presets… file in the Camera
   section. A file holds the object itself or has it under `presets`, so one
   project file can hold `presets` and `lighting` together.

A name found nowhere below is a new preset over `default`. `yaw`, `pitch`,
`view`, `orthographic`, `fov`, and `margin` given beside a preset win over it.
`set_view` and `compare_silhouette` take the same `preset`, `presets`, and
`placementYaw`, and the Camera section's Game view button is `set_view` with
`game`.

```json
{"presets": {"game": {"godotLook": [0.5997, -0.5299, -0.5997], "orthographic": true},
             "low": {"pitch": 8, "orthographic": true}}}
```

`godotLook` is a Godot camera's look direction (its `-basis.z`, Y up), in place
of `yaw` and `pitch`. A Godot camera's yaw about up, measured as the rotation
of its offset from the target, is Froxel's yaw plus 90, because Froxel's yaw is
from +X in Z-up cells and a `.glb` goes in at `(x, z, -y)`.

`placementYaw` (`--placement-yaw`; Placement turn in the Camera section) is
how far the asset is turned about up where it is placed in a level, positive
counter-clockwise from above: the same number as a Godot node's
`rotation_degrees.y`. The image is the asset as it stands in the level, from
the same camera. The camera turns the other way (the reply's `yaw` is the
angle used, yaw minus the turn), and so does the sun of a `lighting` that gives
one; the viewport light stays. It applies to `yaw` and `view` as well as to a
preset, and `set_view` refuses it with no angle to turn from.

```json
{"name": "preview", "arguments": {"path": "out/boat.png", "preset": "game",
  "placementYaw": 45, "lighting": "old_pond"}}
```

`background` is `#rrggbb`, `#rrggbbaa`, or `[r, g, b, a?]`; it defaults to
the viewport grey `#16181c`. `#00000000` gives a transparent PNG whose
edges are blended into alpha.

## MCP

```json
{"name": "preview", "arguments": {
  "path": "out/tree.png", "width": 1600, "height": 800,
  "yaw": -35, "pitch": 30, "margin": 0.05}}
```

| Argument | Default | |
|---|---|---|
| `path` | required | PNG to write; its folder is created |
| `width`, `height` | 1024 | 1..4096 |
| `yaw`, `pitch` | 30, 25 | degrees; or `view` |
| `orthographic` | false | |
| `fov` | 50 | perspective, 10..120 degrees |
| `margin` | 0.06 | 0..0.45 per side |
| `object` | — | index or name to frame |
| `isolate` | false | draw only `object` |
| `background` | `#16181c` | |
| `antialias` | true | 2×2 up to 2048×2048 |
| `pose` | — | temporary pose, as in `render` |
| `preset`, `presets` | — | named camera, see above |
| `inputs`, `variants`, `reference`, `columns`, `labels` | — | contact sheet, below |
| `overlay`, `overlayLabels` | off, true | markers over the model, below |
| `lighting`, `lightingPresets` | `viewport` | a game's light, below |
| `export` | `export_mesh` defaults | export options the counts use, below |
| `countsLabel` | false | write both triangle counts in the corner |

The reply is `path`, `width`, `height`, `yaw`, `pitch`, `orthographic`,
`previewTriangles`, `export`, `milliseconds`, `lighting` (the values
used) when given, `object` (`index` and `name`) when one was framed, and
`pixelsPerVoxel` when orthographic. It is not an edit and
does not move the editor camera. Connected to a window, the window process
draws it.

## Lighting presets

The viewport light is for modelling, not for judging colour: a stone that
reads right under it can turn pale grey under a game's warm sun and blue
ambient. `lighting` lights the preview the way Godot lights the exported
glb instead:

- **Sun.** One directional light: `sunDirection` `[x, y, z]` toward the
  sun, Froxel Z up, or `godotSunBasis`, the numbers of a
  `DirectionalLight3D`'s `Transform3D` as the `.tscn` writes them (9 basis
  numbers, or all 12). A Godot light shines along its −Z, so the sun is
  its +Z axis, the third basis column, and Godot `(X, Y, Z)` is Froxel
  `(X, −Z, Y)` (the glb maps Froxel `(x, y, z)` to `(x, z, −y)`). With
  neither, the document's sun direction. `sunColor` (`[r, g, b]` as
  Godot stores it, sRGB 0..16, or `#rrggbb`) and `sunEnergy`.
- **Ambient.** A flat colour ambient (Godot's ambient source Color):
  `ambientColor` and `ambientEnergy`.
- **Exposure**, then a clamp to 0..1 (Godot's linear tonemapper, its
  default), then sRGB.
- **lut**, optional: a 3D LUT PNG, as Godot's Environment colour
  correction takes it, with the slices side by side (`N·N × N`, red across
  a slice, green down, blue from slice to slice) or stacked (`N × N·N`).
  It grades the finished sRGB image, the background too, trilinearly.
- **background**, optional: the clear colour when the call gives none.

A face is `albedo × (sunColor × sunEnergy × max(n·l, 0) + ambientColor ×
ambientEnergy) × exposure` in linear light, with every colour converted
from sRGB first. That is Godot's Lambert term on a rough non-metal (the
glb's material: metallic 0, roughness 1) with a light of no specular, as
frog-world's sun is. There are no shadows, so use it with an engine light
that casts none on the asset.

Presets are named sets of these fields. `lighting` takes a name, or an
object of fields over the preset its `preset` names (or over a white sun
of energy 1 and a white ambient of 0.3). The names come from, first match
first:

1. `lightingPresets` (MCP), `--lighting-presets` (command line), or the
   Light presets… file in the Render section: a JSON object of name to
   fields, or one with it under `lighting`, or the path of such a file. A
   relative `lut` is relative to the file.
2. The user's `lighting_presets.json` in Froxel's support folder
   (`~/Library/Application Support/Froxel` on macOS, else
   `$XDG_DATA_HOME/froxel` or `~/.local/share/froxel`), or the file
   `FROXEL_LIGHTING_PRESETS` names.
3. The built-in ones: `viewport` (the editor light, the default) and
   `old_pond`.

A preset with the name of one further down overrides it field by field;
one with `preset` starts from that one instead.

`old_pond` is frog-world's Old Pond region: the `SunNoShadow` light of
`scenes/regions/old_pond/old_pond_region.tscn` (colour `(1, 0.76, 0.48)`,
energy 1.45, about 40° up, toward Froxel `(−0.30, 0.70, 0.64)`) and the
Environment of `themes/world/level_01_environment.tres`: ambient
`(0.62, 0.72, 0.9)` at 1.1 (a cool blue, which is what greys a stone face
the sun misses), exposure 1.05, background `#0a3f57`. The level's grade,
`themes/world/level_grade_lut.png` (33 slices), is the game's file, so a
frog-world presets file adds it:

```json
{"lighting": {"old_pond_graded": {"preset": "old_pond",
  "lut": "../themes/world/level_grade_lut.png"}}}
```

```sh
froxel preview --input gorge_crest.vox --out crest_pond.png --preset game \
  --lighting old_pond_graded --lighting-presets froxel/lighting.json
```

What the preview leaves out, so what can still differ from the game: the
direction is in the asset's own axes, so an instance rotated in the level
sees the sun from elsewhere; Godot's default diffuse is Burley, which
equals Lambert facing the light and camera and departs from it, by up to
about a fifth, toward grazing angles; the ambient's faint specular, fog,
glow, SSAO, shadows from other objects, and vertex AO the shader may
multiply in are not drawn.

## Two triangle counts

The preview draws the viewport mesh: every exposed cell face, unmerged.
That is not what a game loads. A gorge crest drew about 256 000
triangles in the preview, and its colour-atlas glb has about 32 000. So the reply names the
two apart:

- `previewTriangles` is the unmerged viewport mesh that touched the image.
  It changes with the framing and with `object` and `isolate`.
- `export` is what `export_mesh` or `export_models` writes with the options
  in the `export` argument: `merge`, `colorAtlas`, `vertexColors`,
  `exportHidden`, `splitObjects`, `aoStrength`, `aoRadius`, `aoTarget`,
  with `export_mesh`'s defaults (all off) for what is not given. It replies
  `triangles` (every triangle the glb's index buffers hold), `quads`
  (`export_mesh`'s own `quads`: index count / 6; with the colour atlas a
  face is an ear-clipped polygon, so `triangles` is the exact figure), and
  the options used. It counts the whole document, whatever `object` frames,
  because the export writes the whole document. If that export would fail
  (the colour atlas with a skeleton, say), `export` has `error` instead.
  `export: false` leaves it out.

Unmerged counting only counts faces and adds little. `merge` builds the
merged mesh with its junction cuts, so it costs what the export costs
(about five seconds on the crest in a release build); the colour atlas is quicker.
`export_mesh` and `export_models` reply `triangles` too, so the numbers
can be checked against each other.

```json
{"name": "preview", "arguments": {"path": "out/crest.png", "preset": "game",
  "export": {"colorAtlas": true}, "countsLabel": true}}
```

`countsLabel` writes the two in the bottom-left corner, as
`preview 256,412 tris (unmerged)` over `export 32,236 tris 16,118 quads
(atlas)`. Contact sheet tiles take it too, and each tile's reply has its
own `previewTriangles` and `export`.

The Render section's Counts on preview check box writes the label on Save
preview and the contact sheets; they count with the Export panel's options
(what File > Export writes), and the status line gives both numbers.

## Overlays

`overlay` draws the document's markers (`set_marker`, Rhai `marker`,
`collision_box`, `connector`) over the model, so a collision box can be
checked against the stone before export: `true` or `"all"` (`"markers"`
is the same) for every kind, or `"boxes"`, `"points"`, `"connectors"`, or
a list of them. Collision boxes are orange outlines, points cyan dots, and
connectors magenta arrows four cells long toward their direction. Each is
named beside it on a dark plate unless `overlayLabels` is false. A line or
dot the model hides is drawn faded, and hidden lines dashed, from the
preview's own depth buffer. Only the markers of drawn objects are drawn
(with `isolate`, the object's own). The reply adds `overlay`: each drawn
marker's `name`, `kind`, `object`, the `pixel` of its anchor (a box's top
centre), and whether it is `hidden`. Contact sheet tiles take the overlay
too.

```json
{"name": "preview", "arguments": {"path": "out/wall_collision.png",
  "preset": "game", "overlay": ["boxes", "connectors"]}}
```

The Render section's Markers on preview check box adds the overlay to Save
preview and the contact sheets.

## Contact sheets

`inputs`, `variants`, or `reference` turn `preview` into a contact sheet:
one tile per model, in a grid, with the same camera for all.

- `inputs` lists `.vox` paths, or `{path?, label?, params?}`. Without
  `path` the tile is the open document; `params` are `set_params` values
  applied first, so a scripted file is baked with them.
- `variants` is the open scripted document baked once per parameter set,
  `[{name?, params}]` as `export_variants` takes, or the path of a
  variants JSON file. Each set runs in a scratch run, so the document and
  its undo history are left as they were.
- `reference` is a PNG (a concept, a turnaround) or `{path, label?}`,
  scaled into the first tile with its aspect kept. Alone, it sits beside
  the open document.

`width` and `height` are each tile's size (default 512×512). `columns`
defaults to about square. Under each tile, unless `labels` is false, is its
name (the file stem, the variant name, or `label`) and its parameters as
`key=value`. The sheet is at most 8192 pixels a side and holds at most 64
models and a reference; the text is a fixed 5×7 bitmap font, so the sheet
is the same bytes every time. The reply adds `columns`, `rows`, and
`tiles`, each with `label`, `x`, `y`, `width`, `height`, `triangles`,
`voxels`, `previewTriangles`, `export`, and `input` or `params` when it
has them.

```json
{"name": "preview", "arguments": {
  "path": "out/gorge_sheet.png", "preset": "game", "width": 420, "height": 300,
  "variants": "examples/gorge_wall_variants.json",
  "reference": {"path": "concepts/gorge_cliffs_v01.png", "label": "concept"}}}
```

![The gorge wall example's three variants at the game preset, with markers](images/preview-contact-sheet.png)

`froxel preview --input examples/gorge_wall.vox --variants
examples/gorge_wall_variants.json --preset game --overlay all --width 480
--height 340 --columns 3` draws the sheet above.

The Render section's Sheet of files… and Sheet of variants… buttons make
the same sheet from the window's camera angle; Reference… sets the PNG for
the first tile.

## Command line

```sh
froxel preview --input model.vox --out preview.png --yaw -35 --pitch 30 --width 1600 --height 800
```

The flags mirror the MCP arguments: `--width`, `--height`, `--yaw`,
`--pitch`, `--view`, `--ortho`, `--fov`, `--margin`, `--object NAME|INDEX`,
`--isolate`, `--background`, `--no-antialias`, `--preset`, `--presets
FILE`, `--placement-yaw DEG`, `--lighting NAME`, and `--lighting-presets FILE`. `--output` is an alias of `--out`, which must end in `.png`. It
prints the time taken and both triangle counts on stderr. The export
counts take the `froxel export` flags: `--merge`, `--color-atlas`,
`--vertex-colors`, `--export-hidden`, `--split-objects`, `--ao`,
`--ao-radius`, and `--ao-target`; `--counts-label` writes them on the
image and `--no-export-counts` skips them. `froxel preview --help` lists
them.

```sh
froxel preview --input gorge_crest.vox --out crest.png --preset game --color-atlas
# preview: 256412 triangles (the unmerged viewport mesh it draws)
# export: 32236 triangles, 16118 quads (merge false, colorAtlas true, as froxel export writes it)
```

Several `--input` files make a contact sheet, as do `--variants
sets.json` (baked from the one `--input`) and `--reference concept.png`
(with `--reference-label`); `--columns N` and `--no-labels` lay it out.
`--overlay KIND` (repeatable) and `--no-overlay-labels` draw the markers.

```sh
froxel preview --input examples/gorge_wall.vox --variants examples/gorge_wall_variants.json \
  --reference concept.png --reference-label concept --preset game \
  --width 420 --height 300 --out gorge_sheet.png
```

## Which picture to use

| Tool | For | Cost |
|---|---|---|
| `preview` | Shape, silhouette, framing, quick looks after each edit | well under a second |
| `model_view` | Palette-exact colours against a turnaround sheet | fast, flat, no light |
| `render` | Lighting, materials, glass, final stills | seconds to minutes |
| `compare_silhouette` | How closely the silhouette from one camera matches a concept view, as IoU, missing, and excess, with an overlay | a preview plus the fit |
