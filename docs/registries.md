# Registries

Where Froxel's add-ons and MCP server are listed, what the repository holds for
each, and what a new release needs. The files are kept at the `Cargo.toml`
version by `tools/sync_versions.py`, and the `addons` job of `release.yml`
fails when one is behind (see [releasing.md](releasing.md)).

| Registry | In the repo | Per release | Status |
|---|---|---|---|
| Claude Code plugin marketplace | `.claude-plugin/marketplace.json`, `.claude-plugin/plugin.json` | Nothing: users get the version on `main` | Live once pushed |
| Official MCP Registry | `server.json`, `Dockerfile`, `.github/workflows/mcp-registry.yml` | Automatic on a published release | Not yet run; needs a first release with the workflow and a public GHCR package |
| Glama | `glama.json`, `Dockerfile` | Automatic (Glama reads the repository) | Listed and claimed by `theromis` (2026-11-20) |
| mcp.so | — | Nothing | Not yet done: web form, paid |
| Smithery | — | — | Not applicable: closed to new servers |
| Blender Extensions | `blender/froxel_bridge/blender_manifest.toml` | None | Not listed: the site takes add-ons only under GPL-3.0-or-later, and the add-on stays MIT (owner's decision, 2026-11-20); users install the release's `blender-addon.zip` |
| Godot Asset Library | `godot/addons/froxel_bridge`, `tools/godot_assetlib.py` | Run `tools/godot_assetlib.py` | Submitted 2026-11-20 (edit 25040), waiting for moderation |
| Godot Asset Store (beta) | the same zip | Upload the zip | Not yet done: optional, needs an account |

## Claude Code plugin marketplace

The repository is a marketplace named `froxel` with one plugin, also
`froxel`, whose root is the repository root. The plugin is the
`skills/froxel` skill (found by the default `skills/` scan) and the MCP
server `froxel` (`froxel mcp`, declared in `plugin.json`, so opening the
repository in Claude Code does not start it as a project server). The plugin
does not install the binary: `froxel` has to be on `PATH`.

```sh
claude plugin marketplace add theromis/Froxel
claude plugin install froxel@froxel
```

In a session: `/plugin marketplace add theromis/Froxel`, then
`/plugin install froxel@froxel`, or both at once with
`/plugin install froxel --marketplace theromis/Froxel`.

`plugin.json` carries `version`, so installed copies update when the version
changes, not on every commit. Check both manifests with

```sh
claude plugin validate .
```

It passes with one warning: `CLAUDE.md` at the plugin root is not loaded as
plugin context, which is intended (it is the repository's own guide). Do not
add top-level folders named `commands`, `agents`, `hooks`, `bin`, `monitors`,
`themes`, `workflows`, or `output-styles`: in a plugin whose root is the
repository they would become plugin components.

Listing it in Anthropic's own plugin directory is a separate submission and is
not done.

## Official MCP Registry

`server.json` names the server `io.github.theromis/froxel` and one package:
the OCI image `ghcr.io/theromis/froxel-mcp:<version>`, run over stdio. The
image is the repository's `Dockerfile`: Debian with that release's Linux
build, `ENTRYPOINT ["froxel", "mcp"]`, and the label
`io.modelcontextprotocol.server.name`, which the registry checks against the
name. A client starts it as `docker run -i --rm ghcr.io/theromis/froxel-mcp:<version>`;
it is the headless server, so no window, `--connect`, screenshots, or files
outside the container.

`.github/workflows/mcp-registry.yml` runs when a release is published (not a
pre-release) or by hand with a tag:

1. builds the image for `linux/amd64` and `linux/arm64` from that release,
   pushes `<version>` and `latest` to GHCR, and checks that it answers
   `initialize`;
2. sets `version` and the image tag in `server.json` from the tag, runs
   `mcp-publisher validate`, logs in with `mcp-publisher login github-oidc`
   (the job's `id-token: write`), and runs `mcp-publisher publish`.

```sh
gh workflow run mcp-registry.yml -f tag=v0.3.3
```

To be done once, by hand:

- After the first run, make the `froxel-mcp` package public: GitHub →
  Packages → froxel-mcp → Package settings → Change visibility. A private
  image cannot be pulled by clients.
- The first publish creates the entry; check it at
  `https://registry.modelcontextprotocol.io/v0/servers?search=io.github.theromis/froxel`.

Locally, `mcp-publisher validate server.json` (Homebrew: `brew install
mcp-publisher`) checks the file. To bump mcp-publisher, change
`MCP_PUBLISHER_VERSION` and `MCP_PUBLISHER_SHA256` in the workflow together;
the hash is the `mcp-publisher_linux_amd64.tar.gz` line of that release's
`registry_<version>_checksums.txt`.

## Glama

Glama builds the `Dockerfile` to list the server's tools. `glama.json` names
the maintainers (`theromis`) so that account can claim the listing. Not yet
done:

1. Sign in at https://glama.ai with GitHub (`theromis`).
2. https://glama.ai/mcp/servers → **Add Server**, repository
   `https://github.com/theromis/Froxel`.
3. When it is listed, **Claim** it; the claim is checked against `glama.json`.

The `Dockerfile`'s default `FROXEL_VERSION` is the current release, since
Glama builds it without build arguments.

## mcp.so

A web form, not a file. Not yet done. https://mcp.so/submit?type=server asks
for:

- **Repository URL**: `https://github.com/theromis/Froxel`
- **Name**: `Froxel`

At the time of writing the form takes a one-time $39 fee for a listing that
is published without review, and offers no free path. Whether to pay is the
owner's call.

## Smithery

Smithery stopped its connection services and CLI on 2026-10-31 and keeps only
a read-only public registry, so there is no `smithery.yaml`. Its listing
draws on other registries; the official MCP Registry entry is the one to keep
current.

## Listing text

For the web forms (Glama, mcp.so, directories that ask for a description):

- **Name**: Froxel
- **Short description** (≤ 100 characters, as in `server.json`): Voxel
  editor: sculpt, script, paint, rig, preview, and export .vox models to glTF
- **Description**: Froxel is a voxel editor whose MCP server lets an agent
  build `.vox` models: place and fill voxels, sculpt tubes, lofts, and blended
  shapes, run Rhai scripts with parameters, paint and reduce the palette,
  carve or paint from reference views, add bones and animation clips, preview
  and path-trace the result, and export glTF for Godot, Blender, or a game.
  It runs headless (`froxel mcp`) or drives an open editor window
  (`froxel mcp --connect 127.0.0.1:7979 --launch`).
- **Category**: 3D, design, game development
- **License**: MIT
- **Install**: `froxel mcp` from a release
  (https://github.com/theromis/Froxel/releases), or the image
  `ghcr.io/theromis/froxel-mcp`

## Blender Extensions

`blender/froxel_bridge` is an extension: `blender_manifest.toml` has the id
`froxel_bridge`, `blender_version_min = "4.2.0"`, the license `SPDX:MIT`, the
tags `Import-Export`, `Modeling`, `Object`, and two permissions, `network`
(the bridge on `127.0.0.1`) and `files` (the session file and the glTF). The
`addons` job of `release.yml` installs Blender 4.2 LTS, sets the manifest's
version to the release version, copies `LICENSE` in, and makes
`froxel-<version>-blender-addon.zip` with `blender --command extension build`,
validating the folder and the zip with `blender --command extension validate`.

Locally:

```sh
blender --command extension validate blender/froxel_bridge
blender --command extension build --source-dir blender/froxel_bridge --output-dir dist
```

Not listed there. extensions.blender.org refuses an add-on whose `license`
is not GPL-3.0-or-later ("license for add-ons must be GPL v3.0 or later"),
and the add-on stays MIT, so it is installed from the release's zip (Blender,
Preferences, Get Extensions, Install from Disk). If that changes, the upload
went like this:

1. Sign in at https://extensions.blender.org with a Blender ID.
2. **Upload Extension**, and choose the release's
   `froxel-<version>-blender-addon.zip`. The site reads the name, id, tagline,
   tags, license, and permissions from the manifest.
3. Fill in what the manifest does not carry:
   - **Description**: Froxel Bridge connects Blender to Froxel, a voxel
     editor. Start `froxel bridge --input model.vox` (or `froxel --bridge
     29771 model.vox` for the open editor), choose the printed session file
     in the 3D View sidebar's Froxel tab, and the model is imported as glTF,
     bones and clips included. Each script parameter of the model becomes a
     slider; moving one reruns the script in Froxel and reimports. Bake into
     scene keeps a copy that no longer needs Froxel. It talks only to
     127.0.0.1, with the token from the session file.
   - **Permissions**, as the manifest states them: network, to connect to the
     Froxel bridge on 127.0.0.1; files, to read the session file and import
     the glTF it writes.
   - **Images**: `docs/images/blender-bridge.jpg`; the icon
     `packaging/icons/hicolor/256x256.png`.
   - **Website**: https://github.com/theromis/Froxel/blob/main/docs/bridges.md#blender
   - **Report issues**: https://github.com/theromis/Froxel/issues
4. Submit for review. The reviewers may ask about `network`; the answer is
   that it is loopback only and Blender's Allow Online Access setting is
   about the internet.

Each later release: open the extension's page → **Upload New Version**,
choose the new `blender-addon.zip`, and add the release notes. The version
comes from the manifest, so it is the Froxel version.

## Godot Asset Library

The library downloads one zip per version and installs what it contains. The
repository has the plugin at `godot/addons/froxel_bridge`, not at
`addons/` in the root, so the library's GitHub provider (an archive of the
whole repository at a commit) would install it as `res://godot/addons/…` with
the rest of the repository beside it. Instead the listing uses the **Custom**
provider, whose download is a URL: the release's
`froxel-<version>-godot-assetlib.zip`, which `release.yml` builds as
`froxel_bridge/addons/froxel_bridge/…`, with `LICENSE` inside the addon
folder so it never replaces the project's own. The editor's installer
treats the single root folder as the asset root and drops it ("Ignore Asset
Root"), as it does for a GitHub archive, so the plugin lands in
`res://addons/froxel_bridge`. The release's `godot-addon.zip` (root
`addons/`) stays for unzipping by hand.

A repository-wide `.gitattributes` `export-ignore` would only matter to the
GitHub provider, and it would also strip the "Source code" archives of every
GitHub release, which packagers read, so there is none.

First submission, not yet done (https://godotengine.org/asset-library/asset/submit,
signed in):

| Field | Value |
|---|---|
| Asset Name | Froxel Bridge |
| Category | 3D Tools |
| License | MIT |
| Repository host | Custom |
| Repository URL | https://github.com/theromis/Froxel |
| Issues URL | https://github.com/theromis/Froxel/issues |
| Minimum Godot version | 4.3 |
| Asset Version | the release, e.g. `0.3.3` |
| Download Commit/URL | `https://github.com/theromis/Froxel/releases/download/v0.3.3/froxel-0.3.3-godot-assetlib.zip` |
| Icon URL | `https://raw.githubusercontent.com/theromis/Froxel/main/packaging/icons/hicolor/256x256.png` |
| Preview 1 (image) | `https://raw.githubusercontent.com/theromis/Froxel/main/docs/images/godot-bridge.jpg` |

Description:

> Froxel Bridge connects the Godot editor to Froxel, a voxel editor. Start
> `froxel bridge --input model.vox` (or `froxel --bridge 29771 model.vox` to
> serve the open editor), add a FroxelBridge node, and set its Session File to
> the printed path. The model is imported as glTF, bones and clips included,
> and each script parameter of the model becomes an inspector property:
> changing one reruns the script in Froxel and reimports. Bake writes a plain
> scene that no longer needs Froxel, and the plugin's bake.gd bakes a list
> of variants with headless Godot. The glb's markers (collision boxes, connectors) come along as
> nodes. It talks only to 127.0.0.1, with the token from the session file.
> Needs Froxel 0.2.0 or later: https://github.com/theromis/Froxel/releases.
> Guide: https://github.com/theromis/Froxel/blob/main/docs/bridges.md#godot

A moderator reviews it, which takes a few days. Note the asset's number from
its URL (`/asset-library/asset/<id>`).

Each later release, once the GitHub release is published:

```sh
GODOT_ASSETLIB_USER=… GODOT_ASSETLIB_PASSWORD=… \
  python3 tools/godot_assetlib.py --asset-id <id> --version 0.3.4
```

It checks the zip is on the release and posts an edit with the new version
string and download URL through the library's API (`POST /login`, then
`POST /asset/<id>`); the other fields stay. `--dry-run` prints the request.
A moderator accepts the edit. By hand it is the same: the asset's page →
**Edit**, change Asset Version and Download Commit/URL, submit.

## Godot Asset Store (beta)

The Godot Foundation's newer store (https://store.godotengine.org) takes an
uploaded zip that follows `addons/<asset_name>/` and carries the license:
`froxel-<version>-godot-addon.zip` does. It also wants a 16:9 thumbnail
(`docs/images/godot-bridge.jpg` cropped to 16:9) and tags that are not the
asset's name (for example `voxel`, `import`, `gltf`, `editor-plugin`). Not yet
done and optional while the store is in beta.
