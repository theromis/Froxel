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
  No shadows, sky, bounces, materials, or tonemap.
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

The reply is `path`, `width`, `height`, `yaw`, `pitch`, `orthographic`,
`triangles` (drawn), `milliseconds`, `object` (`index` and `name`) when one
was framed, and `pixelsPerVoxel` when orthographic. It is not an edit and
does not move the editor camera. Connected to a window, the window process
draws it.

## Command line

```sh
froxel preview --input model.vox --out preview.png --yaw -35 --pitch 30 --width 1600 --height 800
```

The flags mirror the MCP arguments: `--width`, `--height`, `--yaw`,
`--pitch`, `--view`, `--ortho`, `--fov`, `--margin`, `--object NAME|INDEX`,
`--isolate`, `--background`, and `--no-antialias`. `--output` is an alias
of `--out`, which must end in `.png`. It prints the triangle count and the
time taken on stderr. `froxel preview --help` lists them.

## Which picture to use

| Tool | For | Cost |
|---|---|---|
| `preview` | Shape, silhouette, framing, quick looks after each edit | well under a second |
| `model_view` | Palette-exact colours against a turnaround sheet | fast, flat, no light |
| `render` | Lighting, materials, glass, final stills | seconds to minutes |
