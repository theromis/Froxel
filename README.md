<p align="center"><img src="packaging/icons/hicolor/256x256.png" width="128" alt="Froxel"></p>

# Froxel

A voxel editor in the spirit of MagicaVoxel, written in Rust: an egui window,
a wgpu viewport, a path tracer, and an MCP server for automation. It runs on
macOS, Windows, Linux, and FreeBSD.

## Build

```sh
cargo run
cargo run -- path/to/model.vox
```

The binary is `froxel`. On macOS the File and Edit commands live on
the application menu bar. Elsewhere, or with `FROXEL_NO_NATIVE_MENU` set,
they are in a menu strip at the top of the window.

## What it does

- Loads and saves MagicaVoxel `.vox` files. A file that has a scene graph is
  written back with its groups, layers, hidden objects, and animation frames.
  Frame 0 keeps the edited translation, rotation, name, and `_hidden`.
  Materials (`_diffuse`, `_metal`, `_glass`, `_emit`) round-trip.
- Edits voxels with a brush, line, box, face fill, generators, and a marquee.
  Object mode moves, rotates, and flips whole objects. Strokes undo as one step.
- Draws the scene as a mesh. Coplanar faces of one palette color merge, and
  the export then splits edges so a neighbour's corner is not a T-junction.
- Switches to a path-traced Render mode. The frame accumulates on the GPU
  through wgpu. A scene that does not fit one grid falls back to the CPU
  tracer. Save Render writes the frame that is on screen.

## Controls

- **Right-drag** orbits. **Scroll** zooms. **Space-drag** or middle-drag pans.
- **Left-drag** uses the active tool.
- **V** brush, **L** line, **B** box, **F** face, **U** mutator, **M** marquee.
- **T** add, **R** remove, **G** paint. These three apply in Model mode.
- **Tab** switches Model and Object mode.
- In Object mode, **X / Y / Z** rotates the active object. **Shift** flips it.
- **⌘Z / ⇧⌘Z** undo and redo. **⌘C / ⌘X / ⌘V** copy, cut, and paste.
- **⌘N / ⌘O / ⌘S / ⇧⌘S** new, open, save, and save as.
- **⇧⌘E** exports a glTF `.glb` with a palette texture and merged faces.

## Command line

`froxel render` path-traces a `.vox` file to a PNG and exits. It opens no
window and starts no MCP server. The CPU tracer is the default, so one seed
always writes the same PNG. `--gpu` uses the wgpu tracer and exits with
`no GPU adapter` when the machine has none.

```sh
froxel render --input model.vox --output frame.png \
  --width 512 --height 512 --samples 64 --bounces 5 --seed 1
```

`--help` lists the defaults. `--denoise` uses Open Image Denoise
when the binary is built with `--features oidn`, and the edge-aware filter
otherwise. The feature needs the Open Image Denoise library on the machine
that builds it.

`froxel voxrt --input model.vox` loads the file, saves it, loads that save,
and compares objects, cells, palette, materials, the scene graph, names,
translations, rotations, hidden flags, layers, frames, bones, scripts, and
motion. It also checks that a second save is byte-identical and that every
`_r` value round-trips. `--dump` prints the chunk tree and skips the checks.
Exit code 0 is a match. Exit code 1 lists the differences.

## MCP

`froxel mcp` edits a document over stdin/stdout. `render` path-traces
a PNG in that process. Connected to a window it can also screenshot and
click. Details are in [docs/mcp.md](docs/mcp.md).

```sh
cargo build
froxel --control 7979
froxel mcp --connect 127.0.0.1:7979 --launch
```

## Releases

The Release workflow runs only on request:

```sh
gh workflow run release.yml                      # version from Cargo.toml
gh workflow run release.yml -f version=0.1.0-rc.1
gh workflow run release.yml -f draft-release=false  # artifacts only
```

It builds macOS universal (`.dmg`, `.zip`), Windows x64, x86 and arm64
(`.zip`), Linux x86_64, aarch64 and riscv64 (`.tar.gz`, `.deb`) and FreeBSD
x86_64 (`.tar.gz`), then creates or updates the draft release `v<version>`
with a `SHA256SUMS.txt`.

The macOS app (`com.lonje.froxel`) and its dmg are signed with Developer ID,
notarized, and stapled when these repository secrets are set. Without them the
app has an ad-hoc signature and Gatekeeper blocks the first launch. The other
platforms are unsigned.

| Secret | Value |
|---|---|
| `MACOS_CERTIFICATE` | The Developer ID Application certificate and its private key as a `.p12`, base64: `base64 -i froxel.p12 \| pbcopy` |
| `MACOS_CERTIFICATE_PASSWORD` | The password the `.p12` was exported with |
| `APPLE_API_KEY` | The text of the App Store Connect API key, `AuthKey_<id>.p8` |
| `APPLE_API_KEY_ID` | That key's ID |
| `APPLE_API_ISSUER_ID` | The Issuer ID shown above the key list |

The certificate comes from Certificates, Identifiers & Profiles in the Apple
Developer account (Developer ID Application, created by the Account Holder).
The API key comes from App Store Connect, Users and Access, Integrations, Team
Keys, with the Developer role.

## Thanks

Inspired by [MacVoxel](https://tangled.org/pa.ulst.ar/MacVoxel).

## License

MIT. See `LICENSE`.
