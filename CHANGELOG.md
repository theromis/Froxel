# Changelog

## Unreleased

### Fixed

- The MCP server's `initialize` instructions were about 15,000 characters,
  and Claude Code keeps only the first 4096, so agents never saw most of
  them. They are now a short summary (under 1,900 characters); the rest is
  `docs/agent-guide.md`, served as the MCP resource `froxel://guide`
  (`docs/hand-sculpting.md` is `froxel://hand-sculpting`).
- `carve_from_views`' tool description was over the same 4096-character
  limit; its depth-map options are now described under `views`.
- Moving an object with `set_object` (or the Position fields) left its cells
  drawn at the old place in the viewport until another edit touched them.
- A control-port request the window does not take within 5 seconds, because
  it draws no frames (hidden, minimised, or covered), is withdrawn with
  `state` `not-run` instead of waiting a minute and running whenever the
  window next draws.
- Every number argument whose description states a range now has it in the
  schema (218 of them), so a value outside it is refused the same way by
  every frontend instead of by some handlers and clamped by others.
- A number argument outside the range its tool's schema states
  (`minimum`/`maximum`) is refused by the argument check of MCP, the control
  port, `froxel run`, the panels, and Rhai, naming the argument; before,
  only tools whose handler checked it refused it, and the others clamped it.
- With **One frame** on, Render mode traces a new frame once an orbit, a
  slider drag, or an edit has held still for a moment; before, it stayed on
  the edit view until **Render frame** was pressed.
- The viewport **Motion** checkbox has a tooltip and is disabled in Render
  mode: it previews palette slot sway and bob in the edit view only.
- Scripts can loop over `a..=b`; it stopped with "For loop expects iterable
  type" at the top level and inside a `fn`.

### Added

- Strands: chains of cubes along a curve, each cube turned along the path,
  for leaves, reeds, weeds, barbels, and fin rays that bend instead of
  stepping, and detail finer than a cell. A strand is stored as its
  definition on its object (the `.vox` keeps it in an `MVST` chunk), moves
  with the object, its bone, and the pose, and is drawn, traced (CPU and
  GPU), and exported as the same turned boxes. MCP and Rhai `add_strand`,
  `edit_strand`, `remove_strand`, `list_strands`, and `bake_strands`; the
  Strand tool (**H**) places, selects, and drags them in the window, and the
  Strands section lists, removes, and bakes them. `mirror_object` mirrors
  strands and `clear_object` clears them.
- MCP `set_palette` (and Rhai `set_palette`) sets a slot's Render material:
  `material` (`diffuse`, `metal`, `emissive`, `glass`), `roughness`,
  `metallic`, `transparency`, `ior`, `emission`, and `power`, with the colour
  optional. The Material section draws its sliders from those arguments.
- MCP `set_tool` picks the window's tool, mode, active slot, brush size and
  shape, and the Mutator's seed colour and density, so a `ui_click` on the
  viewport stamps with a known tool; the tool options row draws Size, Seed
  color, and Density from it.
- MCP `ui_click` takes a control's `id` from `ui_inspect` instead of a
  point (a slider with `value`), so a redraw that moved the control cannot
  send the click astray; picking an item of an open drop-down closes it.
- MCP `render` (and `froxel run render`) takes the Render panel's light:
  `sunElevation`, `sunAzimuth`, `sunDisc`, `sunColor`, `horizon`, `nee`,
  `ris`, and `indirectClamp`; `set_view` takes the window's `oneFrame` and
  `temporalSmooth`. The panel draws those rows from the same schema.
- The phone's Generator sheet shows the kind's options, as the desktop does.
- Every tool has a key: **C** circle, **Q** square, **N** generator,
  **P** tube, **K** box select, and **W** flood select join **V** **L** **B**
  **F** **S** **U** **M**, and each tool strip tooltip names its key.
- MCP `generate` (also Rhai `generate(x0, y0, z0, x1, y1, z1, kind, #{...})`
  and `froxel run generate`) is the Generator tool: an ellipsoid, rounded
  box, cylinder, cone, pyramid, torus, Perlin noise block, tree, helix, or
  conifer fitted to an inclusive box, with each kind's options (axis, apex,
  seed, depth, leaf, tiers, needle, ...), `mode`, `space`, `mirror`, and
  `batch`. The Generator panel draws those options from the tool's schema and
  a drag runs through the same argument check and generator.
- `export_mesh`, `export_models`, `export_variants`, and `preview`'s `export`
  take `gameView` `{yaw, pitch, spread?, occlusion?, solid?}`: the faces a
  game's fixed camera never sees are left out of the mesh and the colour
  atlas (back faces, undersides, and faces the model's own cells or the
  `solid` boxes of its neighbours hide from every direction within the
  spread). A wall kit for a fixed-camera game lost about half its triangles
  and two thirds of its atlas texels with the picture unchanged.
- `fill_tube` (MCP, Rhai, and `carve` `tube`) takes `caps`: `round` (the
  default, as before) or `flat`, which cuts the tube square at its first and
  last points, so two points make an exact cylinder and a flat tube hollowed
  by a thinner one is a pipe with square ends. Flat caps refuse points that
  are all in one place. The Tube tool has **Flat ends**.
  `tests/fixtures/examples/flanged_pipe.rhai` is a pipe with a flange and a
  valve wheel.
- Scripts have `range(a, b)`, `range(a, b, step)`, `atan(x)`, `atan(y, x)`,
  `asin`, and `acos`; `atan2` takes integers too.

### Changed

- MCP `screenshot` takes `region: "viewport"` or a rectangle (`x`, `y`,
  `width`, `height`), and `screenshot_viewport` and `screenshot_region` are
  no longer listed (their names still run it), so `tools/list` is shorter.
- MCP tool and argument descriptions say the same in fewer words: tool
  descriptions are a quarter shorter and `tools/list` about a tenth smaller
  (318 KB to 287 KB).
- MCP `fill_tube` and `carve` list a `title` for the Tube tool's options and
  `minimum`/`maximum` for `radius`, `aspect`, and `sectors`; `froxel run
  fill_tube --help` prints each option's short hint. The Tube tool's labels,
  slider ranges, and tooltips come from the same schema, so its tooltips now
  name the MCP argument they set.
- The Tube tool keeps its options as `fill_tube` arguments and runs them
  through the same argument check and parser as MCP `fill_tube`.
- Every tool panel (Foliage, Stone, Organic, Silhouette, Import, the edit
  sections, Break, Scatter, Smooth, and the Sculpt brush) takes its labels,
  tooltips, and slider ranges from the tool schema; the tooltips end with the
  MCP argument they set. Rhai functions take every option their MCP tool
  lists, from the same schema.
- The Export options and the Render, Camera, Path tracer, and Sky rows that
  `render` takes have tooltips from `export_mesh` and `render`; so do the
  Bake light sliders, colours, and preset, from `export_mesh` `bake`.
- MCP `ui_click` presses the Tube tool's toggles (Blade, Flat ends, Smooth)
  and its colour buttons; `ui_inspect` lists them as `op.fill_tube.*`.
- MCP `ui_click` on a point with no listed control sends the window a real
  pointer press and release, so any widget there (a checkbox, a combo box,
  a button) is clicked; before, it was an error.
- Rhai `set_palette(#{...})` refuses a key `set_palette` does not take when
  the script runs, naming it, instead of when the run is replayed.

## 0.4.0 – 2026-11-25

### Removed

- The tool-shaped subcommands are gone; `froxel run <tool>` does each with
  the tool's own arguments: `render` (`froxel run render`), `preview`,
  `export` (`export_mesh`), `bake-scene` (`bake_scene`), `carve`
  (`carve_from_views`), `paint` (`project_paint`, `decal`, `shell_plates`,
  `paint_from_views`), `fit` (`fit_to_views`), `optimize`
  (`optimize_to_views`), `compare` (`compare_silhouette`), `measure`
  (`measure_sheet`), `pixelate` (`pixelate_views`), `import`
  (`import_mesh`), `orient` (`orient_to_views`), `deterrace`, `deform`,
  `blend` (`blend_shapes`), `relief` (`radial_relief`), `split-parts`
  (`split_parts`), `reduce-palette` (`reduce_palette`), `components`, and
  `variants` (`export_variants`). The file a tool writes is `--path` (not
  `--out` or `--output`), flags are the MCP arguments (`--color-atlas true`,
  `--ao-strength 0.8`, `--glb file.glb`), and the reply prints as JSON. An
  old name exits with the command that replaces it. `froxel voxrt`,
  `froxel mcp`, and `froxel bridge` stay.

### Added

- Tool schemas carry JSON Schema `default` values (smooth, deterrace,
  reduce_palette, split_parts, orient_to_views, brush, deform, loft,
  blend_shapes, column_relief, bark, planks, cavity_tint, radial_relief,
  foliage). A left-out argument takes it from every caller (MCP, the
  control port, `froxel run`, Rhai, the panels), and the panels start from
  it instead of numbers of their own. Three panels start differently as a
  result, now as MCP always did: Reduce palette's despeckle at 0 (was 1),
  Orient's yaw step at 0 (was 5), and Loft's noise at 0 (was 0.15).

- MCP `render` (and `froxel run render`) takes what `froxel render` did:
  `gpu` for the wgpu tracer, `albedoPath` and `normalPath` for the guides,
  `aces`, `stochasticGi`, and up to 8192 samples. `glassMode`,
  `skyIntensity`, and `sunIntensity` are its camelCase names; the hyphenated
  ones still work. `export_variants` takes `variants` as the path of a JSON
  file too, as `preview` does. In an `--args` file, `froxel run` reads paths
  relative to the file.

- `froxel run <tool>` runs any MCP tool on a `.vox` from the command line,
  with `--input`, `--object`, `--out`, and its arguments from `--args`,
  `--json`, or `--<argument> <value>` flags, checked against the tool's
  schema. `froxel run --list` and `froxel run <tool> --help` are generated
  from the same schemas as MCP `tools/list`.

- Render panel **One frame**, on by default: Render mode traces one frame of
  the view as it was when Render was entered or **Render frame** was
  pressed, instead of starting over on every orbit and edit. A changed view
  shows the edit view with a note until the next Render frame. Off is the
  live render as before.

### Changed

- Debug builds optimize dependencies fully and Froxel's own code at
  opt-level 1, so `cargo run` and the tests are many times faster (the unit
  tests went from about 58 s to 5 s). Release builds use thin LTO and one
  codegen unit.
- `eframe` and `winit` are pinned to the exact versions in `vendor/`, and a
  test fails if `Cargo.lock` ever takes them from crates.io, which would
  drop the iOS and App Store fixes those copies carry.
- Editing is lighter on large models. The viewport remesh gathers each
  chunk's cells once per edit instead of scanning the whole object for every
  chunk; a brush stroke copies the object once per pointer move instead of
  twice; a move drag no longer copies its base each step; and a transaction
  copies the document once. Render mode's CPU fallback keeps its world while
  the scene is unchanged instead of rebuilding it every frame, and
  `froxel render`, MCP `render`, and Save Render trace rows on every core
  (the same image as before).
- An object's cells are shared until written: undo history, transactions,
  and stroke and drag bases no longer copy objects an edit did not touch,
  so history on a many-object document takes a fraction of the memory, and
  a stroke's base is no longer copied when it starts.
- Mesh export's T-junction split looks up only the corners on each quad's
  own edges instead of every corner on its plane, so large flat terrain and
  walls export much faster, with the same triangles.

- A `.vox` with an `MVRG`, `MVAN`, `MVSC`, or `MVMK` chunk newer than this
  Froxel reads opens with that chunk left out, instead of refusing the whole
  file. The status line and MCP `open_file` `warnings` say what was left out
  and that a save drops it. A newer `MVOR` still refuses the file, since the
  objects would land in the wrong place without it.

- MCP `add_object` `makeActive` and `set_palette` `motionAmount` (and in
  Rhai `set_palette`) are spelt in camelCase like every other option. The
  older `make_active` and `motion_amount` still work; passing both names is
  an error.

- MCP `tools/call` of a tool that is not listed, or with `params` or
  `arguments` that are not objects, is a JSON-RPC error `-32602`, as MCP
  asks, instead of a tool result with `isError`.

- An argument a tool's schema does not list is refused for every caller,
  not only MCP: the `--control` port, the command line, Rhai document
  calls, and the panels go through the same check. `control.methods` lists
  every method with its tool, arguments, and description, taken from the
  tool schemas instead of a separate table.

### Fixed

- MCP `pixelate_views` lists `despeckle` in its schema. The tool read it,
  the docs and Rhai named it, but MCP refused it as unknown. A test now
  checks that every option a Rhai function takes is an argument of its MCP
  tool.
- A damaged or hostile `.vox` no longer hangs or exhausts memory: a scene
  graph that shares groups or nests deeper than 256 nodes is refused, and
  counts read from the file reserve only what its bytes can hold.
- A glTF accessor whose `count` is beyond what the importer reads is refused
  before anything is allocated, and buffer view and stride sums are checked.
- glTF buffers and images and OBJ `mtllib` and `map_Kd` must be paths
  relative to the mesh; an absolute path, a drive letter, or a URI scheme is
  refused (a warning for OBJ).
- The `--control` port reads at most 16 MiB a line without buffering past
  it, gives a connection 5 s in all to authenticate, closes it on a wrong
  token, serves at most 16 at once, and compares tokens in constant time.
  `froxel mcp --connect` sends its token only to a loopback address. The
  token file is created anew, owner-only, instead of rewritten in place.
- The Godot/Blender bridge serves up to 8 clients at once, so a connected
  Godot no longer locks Blender out, and the 5 s to authenticate is a
  deadline rather than a per-read timeout. A wrong protocol version and a
  wrong token have their own errors, and the session directory name no
  longer contains part of the token.
- `froxel mcp --connect` warns on stderr when the window is another Froxel
  version, whose tools may differ from the schema it checks against.
- On Android, an error that stops the app from starting is logged instead
  of dropped.
- A bridge reply too long for the clients' 64 KiB frame, such as the schema
  of very many parameters, is an error that says so instead of a frame the
  client drops and disconnects on. `describe` replies name the `server`
  version.
- A panic inside a tool no longer kills the MCP server, the control port,
  or the window: the call returns an internal error and the document is put
  back as it was before the call.
- MCP `reload` hands its state over in a new owner-only directory with a
  random name, in files it creates owner-only, not at a predictable path in
  the shared temporary folder.

## 0.3.5 – 2026-11-21

### Changed

- The Mac App Store build has no network entitlements, as App Review asked:
  `--control`, `froxel mcp --connect` and `--launch`, and the Godot/Blender
  bridge say to use the dmg or Homebrew Froxel. Headless `froxel mcp` works
  as before.

### Fixed

- The macOS window no longer calls the private
  `CGSSetWindowBackgroundBlurRadius` (from winit), which App Review rejects.

## 0.3.4 – 2026-11-20

### Added

- Linux channels: a signed apt repository on GitHub Pages, COPR (Fedora,
  RHEL, EPEL), the openSUSE Build Service, the AUR (`froxel-bin`), the Snap
  Store, Flathub (`com.lonje.Froxel`), and a Homebrew tap (macOS cask, Linux
  formula), filled from a published release by `linux-repos.yml`; ebuild,
  Void template, FreeBSD port, and nixpkgs recipes as an artifact; and a Nix
  flake (`nix run github:theromis/Froxel`). Linux packages now carry AppStream
  metadata (`com.lonje.Froxel.metainfo.xml`) and the release tarball the
  hicolor icons. Not run yet: every channel needs its account and secret
  (docs/releasing.md, "Linux channels").
- A Mac App Store build: the `Mac App Store` job of `release.yml` makes a
  sandboxed `Froxel.app` (`packaging/macos/appstore.sh`, App Sandbox
  entitlements in `packaging/macos/Froxel.entitlements`) signed for the store
  and its installer `.pkg`, the `store-<version>-macos` artifact; with
  `upload-stores` it goes to App Store Connect beside the iOS build, and with
  `submit-review` to review (`tools/asc_release.py --platform MAC_OS`). In the
  sandbox, **File > Grant Folder Access…** gives MCP tools a folder for good
  (security-scoped bookmarks, also kept for Open Recent), and a path outside
  the container and the granted folders fails with a note saying so. Not run
  yet: the store certificates, profile, and the macOS platform of the App
  Store Connect record are still to be set up.
- Android stores: store metadata for F-Droid in
  `fastlane/metadata/android/en-US` and its build recipe
  (`packaging/android/fdroid`), and `upload-stores` jobs for the Amazon
  Appstore and the Samsung Galaxy Store. The APK now has the same
  versionCode as the Play bundle (`packaging/android/apk.sh`), so a
  release updates its prereleases.
- Windows channels: a Microsoft Store MSIX for x64, x86, and arm64 and an
  `.msixbundle` (`packaging/windows`; `froxel` on the PATH through an
  execution alias, `.vox` files open in it), built unsigned for the Store
  to sign and sent to Partner Center by `upload-stores`; and, when a
  release is published, a winget manifest (`Lonje.Froxel`), the Scoop
  bucket in this repository (`bucket/froxel.json`), and a portable
  Chocolatey package, all from the release zips and their checksums
  (`.github/workflows/windows-repos.yml`).
- Registry listings for the add-ons and the MCP server (`docs/registries.md`).
  The repository is a Claude Code plugin marketplace: `claude plugin
  marketplace add theromis/Froxel`, then `claude plugin install froxel@froxel`
  installs the `froxel mcp` server and the agent skill together. The Blender
  add-on is now an extension (`blender_manifest.toml`, Blender 4.2+, network
  and files permissions), built and validated with Blender 4.2 LTS by the
  `addons` job, ready for extensions.blender.org. Releases also carry
  `froxel-<version>-godot-assetlib.zip` for the Godot Asset Library, and
  `tools/godot_assetlib.py` posts a new version to it. A published release
  pushes the `ghcr.io/theromis/froxel-mcp` image and publishes `server.json`
  (`io.github.theromis/froxel`) to the official MCP Registry
  (`mcp-registry.yml`, GitHub OIDC); `glama.json` lets the owner claim the
  Glama listing. `tools/sync_versions.py` keeps all of these at the
  `Cargo.toml` version, and the release fails when one is behind. Not done
  yet: the first Blender, Godot, and Glama submissions, which need accounts,
  and the first registry run, which needs a release.
- itch.io and Steam uploads: the `game-stores.yml` workflow pushes a
  published release to itch.io with butler, one channel per platform
  (`osx-universal`, `windows-x64`, `windows-x86`, `windows-arm64`,
  `linux-x86_64`, `linux-aarch64`, `android`) with an `.itch.toml` launch
  manifest, and uploads the Windows x64, macOS, and Linux x86_64 depots to
  Steam with steamcmd, live on a beta branch only. Prereleases skip itch.io.
  The Steam store text is `packaging/steam/store.md`. Not run yet: the
  itch.io project and the Steamworks app are still to be set up.
- Linux `.rpm` packages for Fedora, RHEL, CentOS Stream, Rocky, Alma, and
  openSUSE (x86_64, aarch64, riscv64). The x86_64 and aarch64 Linux builds
  now need glibc 2.28 instead of 2.35, so they also run on RHEL 8 and its
  rebuilds, Debian 10 and 11, and Ubuntu 20.04.
- Blended shapes: spheres, ellipsoids, rounded boxes, capsules (tapered, or
  bent through points), and tori as signed distance fields joined by a
  smooth minimum, so they flow into each other like clay (MCP
  `blend_shapes`, Rhai the same, `froxel blend`, and the Blend shapes
  section). Each shape is a union, a soft subtract (a groove its palette
  lines), or an intersect, with its own fillet `k`, slot, and turn; cells
  take the slot of the shape they are deepest inside, or of the last listed
  shape that holds them, with an optional dithered seam. It takes `grow`, a
  box, `mode`, `mirror`, and `space` world. Example: `blob_snowman.rhai`.
- Radial relief: regular, shallow, sloped recesses repeated evenly round an
  axis (MCP `radial_relief`, Rhai the same, `froxel relief`, and the
  Radial relief section). Flutes groove a column, vase, urn, or barrel
  side, spaced along each slice's ring of surface cells and following the
  outer surface, round or square, with cupped or tapered ends; gores dish
  a dome's segments between proud, optionally raised ribs, fading at the
  crown and the foot. A `u`, `v`, or `flat` section 0.5 to 4 cells deep,
  with the slope carried by shade and light tones made from each cell's
  own slot. Examples: `fluted_column.rhai` and `gored_dome.rhai`.
- Edit a posed part where it is drawn. With a preview pose (the Pose section
  or `set_pose`), the brush, line, box, face, sculpt, and selection tools,
  the hover cursor, the selection shell, and the object outlines follow
  each object's bone, so a fin or a seat sculpted flat on its own grid can
  be shaped at its angle on the body without baking it. Object mode drags a
  posed part with the pointer.
- A bone's rest turn: the saved angle its parts sit at, on top of its bind
  rotation (`set_bone` and `add_bone` `rest`, `set_bone` `restFromPose`,
  Rhai the same, the Bone section's Rest, and Keep as rest in the Pose
  section). The window, render, preview, and model view draw the part
  turned and its tools edit it there while its cells stay flat on their own
  grid. The `.vox` keeps it (`MVRG` version 2, only when a bone has one),
  `list_bones` reports it, and the glTF joint node rests at it, so a fin
  sculpted flat sits at its angle in a game with no clip.

### Changed

- `bake_scene` and `froxel bake-scene` take up to 32768 instances, not 4096,
  so a zone of many small path stones bakes as one file.
- MCP `export_variants` replies with a summary instead of the whole manifest:
  per set exported now its files, bounds, voxels, quads, and markers counted
  by kind, plus the manifest path and the names added, replaced, and kept.
  Six curve walls with collision boxes went from about 115,000 characters
  to 5,500. `reply: "full"` returns the manifest as before; the manifest
  file is unchanged. `export_mesh` and `export_models` count the markers by
  kind the same way, with `reply: "full"` for every marker.

### Fixed

- The marquee and the Project line hit a moved object's cells where they
  are drawn, not as if the object sat at the origin.

## 0.3.3 – 2026-11-12

### Fixed

- iOS: the app launches on iOS 27. It now adopts the UIScene lifecycle
  (a scene delegate in a patched winit and a scene manifest in `Info.plist`);
  built with the iOS 26 SDK, it used to stop at launch.

### Added

- Pixel-art lines: `pixelate_views` (and `downscale` on `carve_from_views`,
  `paint_from_views`, and `optimize_to_views`; Rhai `pixelate_views`, the
  Pixelate views section's Lines, `froxel pixelate`) keeps a drawn line
  thinner than a cell (an eye's ring, a mouth, a belly's edge) as a line of
  cells in its colour (`lines`, on by default), where the vote used to give
  every cell to the fill around it. `outline` keeps the silhouette's outline
  too; without it the outline's pixels vote only where nothing else does,
  so no stray dark cell is left on the rim. `mergeLines` (default 0.2)
  merges the stroke colours near the commonest one, such as an outline
  blended into a shadow, so a view has one outline colour.
- Fill holes: the mask edit `fillHoles` (every views tool and
  `compare_silhouette`, the Compare section's and Pixelate views' Fill
  holes, `froxel pixelate --fill-holes`) inks again, in the image's colour,
  the background regions a view encloses, so white eyes and teeth on a
  white sheet are no longer keyed out as background.

## 0.3.2 – 2026-11-10

### Fixed

- MCP `carve_from_views` takes the top-level `tolerance` and `alphaThreshold`
  its description promises, and `orient_to_views` and `optimize_to_views`
  the top-level `alphaThreshold`: the handlers already applied them to every
  view without its own, but the schemas left them out, so the calls were
  refused as unknown arguments.
- A colour parameter reads one way everywhere (`set_params`, `export_variants`,
  `preview`, `froxel variants`, `froxel bridge`): whole numbers 0..255, or 0..1
  when every channel is at most 1 and one has a decimal point. `[1, 1, 1]` is
  near black and `[1.0, 1.0, 1.0]` white. The bridge used to take only 0..1 and
  `set_params` only whole numbers, so a client sending whole 0..1 values such as
  `[1, 0, 0]` must now write `[1.0, 0.0, 0.0]`; Godot and the addon already do.
- `objectName/param` keys now resolve, as documented, so a variants manifest's
  `params` can be passed back unchanged.

### Added

- Coverage import: `import_mesh` `coverage` (Rhai `voxelize`, the Import
  mesh section's Coverage, `froxel import --coverage`) keeps a cell only
  when that share of it is solid on a voxelization 2 to 4 times finer,
  instead of every cell the surface touches, so a small creature's legs and
  fins do not merge into lumps; `keepThin` (default true) keeps a part
  thinner than a cell as the one cell its midline crosses, as a voxel
  remesher keeps thin sheets. `minFeature` reports the imported parts
  thinner than N cells.
- Deform scale: MCP `deform` `op: "scale"` (Rhai `deform("scale", ...)`, the
  Deform section's Scale, `froxel deform`) enlarges or shrinks a part about
  `point` by `factor` (or per axis): the cells within `radius` as one rigid
  piece, easing back to none over `falloff` cells, so a small creature's
  head, eyes, or fins get the cells they need without moving the rest. A
  falloff that would fold cells is refused. `minFeature` N on any deform
  reports the parts of the result thinner than N cells (`thin`: cells,
  count, and the largest parts with bounds and thickness).
- Pixelate views: MCP `pixelate_views`, Rhai `pixelate_views(views, opts)`,
  the Pixelate views section, and `froxel pixelate` turn an orthographic
  views sheet into pixel art at the cell grid before it is carved or
  painted, for creatures 20 to 60 cells long: one small palette shared by
  every view (k-means in Oklab with the accent guard), each cell the colour
  that holds most of a centre-weighted kernel over its footprint (never a
  mean), small salient features (eyes, pupils, teeth, nostrils, spots) kept
  at least `minCells` cells, cells on the midline of parts thinner than a
  cell kept inked (`thin`), and a `despeckle` pass. The reply's views
  (rows and legend) go straight into `carve_from_views` and
  `paint_from_views`, which also take the options as `downscale`; `png`
  writes the pixel art for checking.
- Upright import: `import_mesh` `orient: "upright"` (Rhai `voxelize`, the
  Import mesh panel's Orient, `froxel import --orient upright`) stands a
  tilted image-to-3D mesh level from its shape before voxelizing it: the
  thinnest principal axis up within `maxTilt` (default 40 degrees), signed
  so the form narrows upward, and the taller end of its long axis toward
  −Y; round shapes keep their up or yaw. `{upright: true, ...views}` stands
  it up and then searches only the yaw against the sheet, and the views
  search of `orient` and `orient_to_views` takes a lone sheet `path` (its
  `measure_sheet` layout).
- Split parts: MCP `split_parts`, Rhai `split_parts(object, parts, opts)`,
  the Split parts section, and `froxel split-parts` cut one blockout into
  its rig parts in one undo step: every cell goes to the part whose seed
  (a point, a bone's pivot, or a point marker) is nearest through the solid
  (geodesic, Euclidean as the other metric and the fallback), on the kept
  side of the part's cutting planes, and each part becomes its own object
  at world [0, 0, 0], bound to the bone of its name.
- Reduce palette: MCP `reduce_palette`, Rhai `reduce_palette(opts)`, the
  Reduce palette section, `froxel reduce-palette`, and `import_mesh` `clean`
  (Rhai `voxelize`, the Import mesh panel's Clean, `froxel import --clean`)
  clean up the noisy colours of a voxelized mesh in one undo step: `colors`
  clusters them in Oklab with an accent guard that keeps eyes and mouths
  (clusters take over their own slots in place), or `palette` snaps them to
  slots or `#rrggbb` colours, then `despeckle` passes of a surface majority
  filter recolour lone specks; `protect` keeps slots as they are.
- Paint from views: MCP `paint_from_views`, Rhai `paint_from_views(views,
  opts)`, the Paint from views section, and `froxel paint --tool
  paint_from_views` recolour every surface cell of an object (or of several
  objects together) from a whole views sheet in one undo step: each cell
  from the views that see it (the first solid cell of its column), by the
  view its normal faces most, blended within a seam angle or softly, mixed
  in Oklab and quantised into the object's slots, given slots, or a few new
  slots clustered from the sheet with an accent guard for eyes and mouths.
  A lone sheet path reads the `measure_sheet` layout; a missing right view
  is the left one mirrored.
- De-terrace: MCP `deterrace`, Rhai `deterrace(opts)`, the De-terrace
  section, and `froxel deterrace` take the one-cell contour terraces out of a
  voxelized smooth mesh, in one undo step: `blocks` re-quantizes the object
  into cubes of 2 to 4 cells within a tolerance of its surface, and `stagger`
  breaks the step edges of its top surface into seeded teeth, reporting the
  one-cell steps and the longest straight run before and after. It runs in
  `batch`.
- Orient to views: MCP `orient_to_views`, Rhai `orient_to_views(views,
  opts)`, the Orient to views section, and `froxel orient` turn an object to
  the axis-aligned turn (24, or 4 upright) and fine yaw (or tilt) steps whose
  silhouettes best match a views sheet by IoU, report each view's IoU before
  and after and the runner-up's margin, and scale it to a size, in one undo
  step. `import_mesh` (and Rhai `voxelize`, the Import mesh section, and
  `froxel import --orient`) take the same search as `orient` and voxelize
  the mesh already turned. Quarter-turn `rotation`s are now exact, so faces
  on cell boundaries stay there.
- Components: MCP `components`, Rhai `components(op, opts)`, and the
  Components section list an object's connected parts (6, 18, or 26
  neighbours), keep the largest, remove floaters under `minCells`, and
  delete, move, or split out the part holding a `seed`, first cut at its
  thinnest neck from an `anchor` (a minimum cut by max flow), in one undo
  step: a rider, moss tufts, and foam bits off an image-to-3D blockout.
- Hex palette colours: MCP `set_palette` (and its `entries`, and Rhai
  `set_palette`) takes `color` as `"#rrggbb"` or `"#rrggbbaa"`, or
  `[r, g, b, a?]`, besides `r`, `g`, `b`, as `background` and legends take
  colours. Rhai `set_slot(index, "#rrggbb")` and a hex field beside the
  palette dock's active colour do the same.
- Surface stroke: MCP `surface_stroke`, Rhai `surface_stroke(points, slot,
  opts)`, and the Surface stroke section paint a stroke through points that
  snaps to the object's nearest surface cells, with a `width`, a `depth`,
  `only` slots, and a smooth curve, or cut a groove with slot 0, in one undo
  step: seams between shell plates and markings on curved bodies, without a
  tube radius guessed to reach the surface. It takes `space` and runs in
  `batch`.
- Intersect mode: `mode: "intersect"` on MCP `fill_box`, `fill_ellipsoid`,
  `fill_rounded_box`, `fill_tube`, and `carve_from_views` (and their Rhai
  options) keeps only the object's solid cells inside the shape and erases
  the rest, as they are, so a boxy visual hull becomes an ellipsoid dome in
  one call. The Intersect section cuts the active object to a box, an
  ellipsoid, or a rounded box over the selection, and Carve from views has
  Keep inside only. `palette` is no longer required by the box fills in that
  mode; other tools refuse it.
- Clear object: MCP `clear_object`, Rhai `clear_object()` and
  `clear_object(object)`, and the Clear object section empty an object of
  every cell in one undo step, with no 2000000-cell box limit; its name,
  position, bone, script, and markers stay. It runs in `batch`.
- Recolor: MCP `recolor`, Rhai `recolor(from, to, opts)` and
  `recolor(pairs, opts)`, and the Recolor section replace one palette slot
  with another, or a map of several (a swap included), in a box or the whole
  object, in one undo step and with no shape change. It runs in `batch`.
- World cells: `space: "world"` on MCP `get_voxel`, `set_voxel`,
  `set_voxels`, `get_voxels`, `fill_box`, `fill_ellipsoid`,
  `fill_rounded_box`, `fill_tube`, `carve`, `smooth`, `loft`, `brush`,
  `carve_from_views`, `project_paint`, and `split_object` (and the same key
  in their Rhai options) takes world cells and subtracts the object's
  position first; the reply's coordinates come back in world cells. The
  Split object, Carve from views, and Paint from view sections have a World
  switch. `mirror_object` replies with the copy's `position`.
- Split object: MCP `split_object`, Rhai `split_object(object, opts)`, and
  the Split object section move (or copy) the cells of an object inside one
  or more boxes, or of some slots, into a new or an existing object, each
  keeping its world position, in one undo step: one `carve_from_views`
  blockout is cut into its rig parts. `invert` takes the rest.
- Baked light: the export option `bake` (MCP `export_mesh`, `export_models`,
  `export_variants`; `--bake` on `froxel export`, `variants`, `preview`, and
  `bridge`; Bake light in the Export panel) paints occlusion from real rays
  and a sun with shadows into the colour atlas, one texel per cell face. The
  material is `KHR_materials_unlit`, off by default, and deterministic. MCP
  `preview` `glb` and `froxel preview --input file.glb` draw a written file
  unlit. `examples/stacked_blocks.rhai` is a model to try it on.
- Fit to views: MCP `fit_to_views`, Rhai `fit_to_views(views, opts)`,
  `froxel fit`, and the Fit to views section refine the active object so its
  orthographic views match an approved views sheet (the crops of
  `carve_from_views`), with colours from a palette, in one undo step:
  excess rays are cleared, missing rays filled with cells every other view
  sees inside, a `reference` object's depth followed, rough cells tidied,
  and every seen cell recoloured to the nearest slot. Other visible objects
  count as cover and never change. `normalLimit`, a view's `color: false`,
  and `keep` decide which views colour which cells; a part with a hand-set
  face is fitted with `recolor: false`. `roundWeight` rounds the diagonal
  corners three axis views cannot see (a round head that the hull leaves a
  box) with superellipses in each section, or a `reference` object, while
  every silhouette stays. The reply gives each view's IoU,
  missing, excess, and colour error before and after. Inspired by Voxify3D
  (CVPR 2026), as a deterministic CPU search with no neural network.
  `examples/fitted_beetle.rhai` fits a boxy blockout to a beetle's views.
- Projected paint: MCP `project_paint`, Rhai `project_paint(view, rows,
  opts)`, `froxel paint`, and the Paint from view section recolour the
  active object's existing surface from one image projected along a view
  (the six sheet views, or `yaw` and `pitch`), in one call and one undo
  step: `depth` cells below the surface, `normalLimit` to leave steep sides,
  `only` to keep other slots, and a box. No cell is added or removed.
  `examples/painted_shell.rhai` paints a shell's plates onto a dome.
- Scene bake: MCP `bake_scene`, `froxel bake-scene`, and Bake scene… in the
  Export panel bake one placement of many `.vox` files together, at any yaw and
  scale, with mesh, plane, or height field occluders, into one merged `.glb`
  with one atlas (`examples/stacked_blocks_scene.json`).

- Silhouette tools: MCP `carve_from_views`, Rhai `carve_from_views(views,
  opts)`, `froxel carve`, and the Carve from views section fill the visual
  hull of 2 or 3 orthographic views (PNG crops with a background rule, or
  rows in a script) into the active object, coloured from the nearest
  view, optionally hollow and smoothed. MCP `compare_silhouette`, `froxel
  compare`, and the Compare silhouette section score the model's preview
  silhouette from a camera against a concept view (IoU, missing, excess,
  with a fitted or given uniform scale and offset) and write an overlay;
  the editor shows it over the viewport. `examples/carved_stump.rhai`
  carves a stump from three views.

- Mesh import: MCP `import_mesh`, File > Import Mesh… with the Import mesh
  section, and `froxel import` voxelize a `.glb`, `.gltf` (embedded or
  external buffers and textures), or `.obj` (with `.mtl` and textures) into
  the active object or a new one: every primitive under its node
  transforms, sized by cells, scale, or metres per voxel, rotated and
  anchored, solid (a vote of ray parity on three axes, so small holes do not
  empty it) or shell, coloured from the base-colour texture, factor, and
  vertex colours, and quantized into the palette (close slots reused, at
  most `maxNewSlots` added, or mapped to given `slots`). One undo step. Rhai
  `voxelize` runs the same voxelizer on a mesh the script builds
  (`examples/faceted_boulder.rhai`); scripts read no files.

- Preview overlays: MCP `preview` `overlay` (and `froxel preview
  --overlay`) draws collision boxes, points, and connectors over the model
  with their names, faded and dashed where the model hides them, and
  replies with each marker's pixel. The Render section's Markers on preview
  adds it to Save preview and contact sheets.

- Contact sheets and camera presets: MCP `preview` and `froxel preview`
  take `inputs` (several `.vox` files, or the open document with
  parameters), `variants` (parameter sets of the scripted document), and a
  `reference` concept PNG, and lay them out in one labelled PNG grid.
  `preset` names a camera (`game`: orthographic, yaw −60, pitch 25), and
  `presets` / `--presets` define or override presets; `set_view` takes
  them too. The Render section has Sheet of files…, Sheet of variants…, and
  Reference…, and the Camera section a Game view button.

- Rhai strings: `"ledge_" + 4`, a number or bool on either side of `+`
  with a string, `+=`, `to_string`, and `len`, so loops name markers and
  objects; backtick interpolation (`` `ledge_${i}` ``) works too.

- Column relief: MCP `column_relief`, Rhai `column_relief(box, facing,
  palette, opts)`, and the Relief section break a cliff face into columns
  of random width that stand out of it and end at uneven heights around a
  crown, cutting the crown above short ones unless `lower` is false. The
  face is found in the cells, so swept, curved walls work.
  `examples/gorge_wall.rhai` uses it and keeps its ledge flat with
  `exclude`.

- Protected boxes: `exclude` on MCP and Rhai `scatter`, `joints`,
  `weather`, `moss`, `algae`, and `vines` (one inclusive box or a list of
  up to 64) keeps walking surfaces whole: no cell inside is cut, chipped,
  recoloured, or grown on, and no debris rests in one. The Protect section
  holds the boxes for the Scatter, Joints, Weather, and Growth sections.

- `examples/gorge_wall.rhai`: a fractured, mossy gorge wall swept along a
  curve, with algae, vines, and markers, and
  `examples/gorge_wall_variants.json` for variant export.

- Variant export: MCP `export_variants`, `froxel variants`, and the Export
  panel's Export variants… bake a scripted document once per parameter set
  and write a `.vox` and a `.glb` per set with the `export_models` mesh
  options, plus a manifest of each set's parameters, markers, and bounds.
  The document and its undo history stay as they were.

- Markers: MCP `set_marker`, `remove_marker`, and `list_markers`, Rhai
  `marker`, `collision_box`, and `connector`, and the Markers section name
  points, collision boxes, and connectors on an object. The `.vox` keeps
  them (`MVMK`), the export replies list them, and mesh export writes each as
  a glTF node with `extras.froxel` and the scene's `extras.froxelMarkers`.

- Sweep: MCP `sweep`, Rhai `sweep(points, profile, slots, opts)`, and the
  Sweep section fill a polygon or rounded-rectangle cross-section along a
  polyline or smooth curve, perpendicular to it, with twist and scale per
  point and strata bands, for curved walls, banks, and ledges.

- Growth: MCP `moss`, `algae`, and `vines`, the Rhai functions of the same
  names, and the Growth section grow plants on exposed faces filtered by
  face, facing, and a height band: moss cushions and ragged curtains, an
  algae band that thins upward in clumps, and vine strands graded to the tip
  with blooms. The caller gives the slots and their motion.

- Weather: MCP `weather`, Rhai `weather(x0, y0, z0, x1, y1, z1, opts)`, and
  the Weather section chip edges and corners, dent pits into flat faces,
  and add subtle colour noise on exposed stone; uncovered faces take a
  `broken` slot.

- Joints: MCP `joints`, Rhai `joints(x0, y0, z0, x1, y1, z1, pattern, opts)`,
  and the Joints section cut real joint gaps into stone: vertical columns of
  random widths, running bond courses, or a crack network, `width` wide and
  `depth` deep under the surface, with a `dirt` slot at the bottom.

- Script document tools: `get_voxels`, `set_voxels`, `set_palette` (alpha
  and motion), `set_object`, `remove_object`, `mirror_object`, `add_bone`,
  `set_bone`, `remove_bone`, `bind_object`, `list_bones`, `set_pose`,
  `clear_pose`, `add_clip`, `set_clip_keys`, `list_clips`, and
  `remove_clip`, plus `has_bone`, `has_clip`, and `has_object`. Object, rig,
  clip, and palette calls are recorded and replayed through the MCP tools
  after the script, in order, in the regenerate's undo step; a failing call
  names its line and puts the document back.

- Script shape tools: Rhai scripts have `fill_ellipsoid`, `fill_rounded_box`,
  `fill_tube`, `fill_box` with a mode, `carve_box`, `carve_ellipsoid`,
  `carve_rounded_box`, `carve_tube`, `smooth`, `break_end`, and `scatter`.
  They run the MCP tools' argument checks and shape code, with the MCP
  options in a trailing map such as `#{mode: "empty", aspect: 0.5}`. Scripts
  also gain `sqrt`, `pow`, `PI`, `min`, `max`, `clamp`, `ceil`, `round`,
  `tan`, and `push`, `pop`, `insert`, and `remove` on arrays.
  `examples/hollow_log.rhai` is a hollow log with roots and a snapped end.

- Colour atlas mesh export: MCP `export_mesh` and `export_models` take
  `colorAtlas`, the Export panel has **Colour atlas**, and `froxel bridge`
  takes `--color-atlas`. Coplanar faces merge whatever their palette slot, and
  each cell's colour goes into a per-mesh texture with nearest filtering.
  Faces of different motion or opacity still stay apart. Where a corner of
  another face lands on an edge, it becomes a vertex of the merged face's
  outline instead of cutting the face, so seams stay closed without
  multiplying quads. On noisy palettes this roughly halves the triangles
  (a pond bed 79.5k to 34.2k). One rigid mesh only: not with a skeleton,
  `splitObjects`, or `vertexColors`.

- Script list parameters: `param_floats`, `param_vec2s`, and `param_vec3s`
  declare a list of numbers or of 2D or 3D points with a range and a longest
  length, for a polyline or a curve. The `.vox` keeps them with the other
  parameters (`MVSC` kind 4). The Script panel edits a list as one line of
  text. MCP `set_params`, the bridge `evaluate`, Godot `set_parameter`, and
  `variants.json` take a JSON list and refuse one that is too long or out of
  range. The Godot inspector shows them as packed arrays and Blender as a
  JSON text field. Scripts can loop over an array with `for` and call `len()`.
- Hand sculpting is a named workflow, described in `docs/hand-sculpting.md`:
  block out with fills, refine with `set_voxels`, render after every step.
  The MCP server instructions point to it.
- The release workflow builds an Android arm64 APK, an iOS ipa, and an iOS
  simulator app. They are signed when the signing secrets in
  `docs/releasing.md` are set.
- Store builds: the release workflow makes a Google Play App Bundle and, with
  an App Store profile, an App Store ipa, and with `upload-stores` sends them
  to Google Play's internal track and TestFlight. The iOS app carries a
  privacy manifest. Both stores get the build number from
  `packaging/build_number.sh`. See `docs/releasing.md`.
- The release workflow's `submit-review` input sends the build for App Store
  review and Google Play production review, with release notes from this
  changelog.
- The Godot **FroxelBridge** node has Bake settings: **Bake Voxel Size**
  (metres per voxel), **Bake Origin** (Source, Center or Bottom Center of the
  geometry), **Bake Apply Transform** (keep the node's own transform) and
  **Bake Collision** (none, a box or an upright capsule around the geometry).
  With any of them set, the baked scene is ready to place: an unscaled root
  with a `Visual` child and a `Collision` StaticBody3D. The preview shows the
  voxel size and origin too. The headless `bake.gd` takes `--voxel-size`,
  `--origin` and `--collision`. The defaults bake the raw import as before.
- `tests/bridge_godot.py` bakes through the Godot node with `GODOT_PATH` and
  checks the scenes with the bridge stopped.

### Changed

- The Android package is `com.lonje.froxel`, as on iOS and macOS, and
  targets Android 16 (API 36). An APK from an earlier release installs beside
  it instead of updating.

### Fixed

- A failed Godot bake leaves the previous scene in place. The scene is written
  beside the target first and moved over it only when it is complete.
- The Godot plugin embeds the palette texture when it imports a model. In the
  editor it used to extract the texture as a file beside the glb, which failed
  outside `res://`, so the preview and **Bake standalone scene** came out
  untextured.
- The Godot inspector shows a script's parameters under one object heading
  instead of the same heading twice.
- `examples/lily_pad.rhai` lays the lily pad flat. It used to stand on its
  edge, because `.vox` has Z up.
- `froxel render --gpu` renders large frames such as 3840x1646 and 5244x2950.
  It used to panic, because the headless device kept wgpu's default buffer
  limits. It now asks for the adapter's limits, and a frame that still does
  not fit exits with an error that suggests the CPU tracer. Render mode
  falls back to the CPU tracer for such a frame instead of panicking.

## 0.2.0

Froxel models can now live inside Godot and Blender. Open a scripted `.vox`
with `froxel bridge`, and the Godot plugin or the Blender add-on shows its
parameters as sliders. Move one and the model is rebuilt and reimported, bones
and animation clips included. When it looks right, bake it into an ordinary
scene that no longer needs Froxel.

### Added

- **Godot and Blender bridge.** `froxel bridge --input model.vox` serves a
  document on `127.0.0.1`, and `froxel --bridge PORT` serves the open editor,
  so edits in the window show up in the next import. Each change is one undo
  step, and a rejected value or a failing script leaves the model as it was.
  See [docs/bridges.md](https://github.com/theromis/Froxel/blob/v0.2.0/docs/bridges.md).
- **Godot plugin**, `froxel-0.2.0-godot-addon.zip`. It adds a FroxelBridge node
  with live sliders and **Bake standalone scene**. A headless script bakes one
  `.tscn` per entry of a `variants.json`. Tested with Godot 4.5; needs 4.3+.
- **Blender add-on**, `froxel-0.2.0-blender-addon.zip`. It adds a Froxel tab in
  the 3D View sidebar, with sliders clamped to each parameter's range and
  **Bake into scene**, which keeps the rig working. Tested with Blender 5.2;
  needs 4.2+.
- `examples/lily_pad.vox`, a scripted lily pad with four parameters to try the
  bridge on.

### Fixed

- A click with the brush adds or removes one voxel. Held for a few frames, it
  used to stack a column toward the camera in Add mode, or dig a hole in
  Remove mode. A drag now paints one layer over the model as it was when the
  stroke began.
- The `--control` port now starts on Windows. Its token used to come from
  `/dev/urandom`, which Windows does not have.

## 0.1.0 – 2026-10-24

First release.

- Opens and saves MagicaVoxel `.vox`, including groups, layers, hidden objects,
  animation frames, and materials.
- Brush, shape, face, generator, mutator, and selection tools, plus object mode.
- Bones, poses, and animation clips, exported to glTF.
- Path-traced Render mode on the GPU (wgpu), with a CPU fallback.
  `froxel render` renders headless.
- glTF export with merged faces and a palette texture or vertex colours.
- Rhai generator scripts with sliders for their parameters.
- MCP server for AI agents, headless or connected to the window.
- Builds for macOS (universal, signed and notarized), Windows (x64, x86, ARM64),
  Linux (x86_64, aarch64, riscv64), and FreeBSD (x86_64).
