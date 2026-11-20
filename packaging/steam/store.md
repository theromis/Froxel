# Froxel: Steam store page

The text for the Steamworks store page (Store Admin), taken from README.md.
Paste each section into the field it names. Steam's description editor takes
BBCode, so the long description is written in it. Keep the page in step with
README.md when a feature lands. The itch.io page can reuse the same text.

## Name

Froxel

## Short description

Steam allows 300 characters.

> A voxel editor with a path tracer and an MCP server. Open and save
> MagicaVoxel .vox files, sculpt with brushes, shapes, and selections, rig
> characters with bones and clips, path-trace them on the GPU, and export
> glTF for Godot, Blender, or any engine. An AI agent can drive all of it.

## Long description ("About This Software")

```bbcode
[h2]Build voxel models, light them, and ship them to your engine[/h2]
Froxel opens and saves MagicaVoxel [b].vox[/b] files, edits them with brushes, shapes, and selections, path-traces them on the GPU, and exports glTF with bones and animation clips. It is free and open source (MIT).

[h2]Features[/h2]
[list]
[*][b].vox in and out.[/b] Keeps groups, layers, hidden objects, animation frames, and materials (diffuse, metal, glass, emit). Files saved by Froxel open in MagicaVoxel.
[*][b]Editing.[/b] Brush, line, circle, square, box, face fill, generators, mutators, marquee, box select, and flood select. Add, remove, and paint modes. Object mode moves, rotates, mirrors, and flips whole objects. Each stroke undoes in one step.
[*][b]Characters.[/b] Bones, rigid binding of objects to bones, poses, and animation clips, saved in the .vox file and exported to glTF.
[*][b]Render mode.[/b] A path tracer with an atmospheric sky, sun, metal, glass, and emission. It accumulates on the GPU (Metal, Vulkan, DX12) and denoises the frame.
[*][b]glTF export.[/b] .glb with merged coplanar faces, no T-junctions, a palette texture or vertex colours, and optional baked ambient occlusion.
[*][b]Mesh import.[/b] A .glb, .gltf, or .obj sculpted in Blender or made by an image-to-3D tool, voxelized solid or as a shell, its colours quantized into the palette.
[*][b]Scripts.[/b] Objects can carry a sandboxed Rhai script with sliders for its parameters.
[*][b]Godot and Blender bridges.[/b] A Godot node or the Blender sidebar drives those sliders, imports the mesh live, and bakes a standalone scene.
[*][b]Agents and automation.[/b] A built-in Model Context Protocol (MCP) server lets an AI agent create objects, place voxels, add bones, render, and export, and a command line does batch renders and exports.
[/list]

[h2]For game developers[/h2]
Export game-ready .glb meshes with bones and clips, bake lighting into a texture atlas for unlit games, and generate prop variants from one scripted model. The Godot plugin and Blender add-on come with every release.
```

## Tags

Steam lets the developer suggest up to 20 tags; the first ones weigh most.

1. Design & Illustration
2. Animation & Modeling
3. Game Development
4. Voxel
5. 3D
6. Utilities
7. Software
8. Free to Play

Genre (Basic Info): Animation & Modeling, Design &
Illustration, Game Development, Utilities. Type: Software. Price: free.

## Languages

English (interface and full audio: none).

## System requirements

### Windows

| | Minimum | Recommended |
|---|---|---|
| OS | Windows 10 64-bit | Windows 11 64-bit |
| Processor | x86-64 dual core | Quad core or better |
| Memory | 4 GB RAM | 8 GB RAM |
| Graphics | A DirectX 12 or Vulkan 1.1 GPU | A recent discrete GPU for Render mode |
| Storage | 200 MB | 200 MB |
| Notes | The Visual C++ 2015–2022 runtime. Without a GPU, Render mode falls back to the CPU. | |

### macOS

| | Minimum | Recommended |
|---|---|---|
| OS | macOS 11 Big Sur | macOS 14 or newer |
| Processor | Apple silicon or Intel (universal app) | Apple silicon |
| Memory | 4 GB RAM | 8 GB RAM |
| Graphics | A Metal GPU | Apple silicon GPU |
| Storage | 200 MB | 200 MB |

### SteamOS and Linux

| | Minimum | Recommended |
|---|---|---|
| OS | 64-bit Linux with glibc 2.28 or newer (Ubuntu 20.04+, Debian 10+, Fedora, SteamOS 3) | Ubuntu 24.04 or SteamOS 3 |
| Processor | x86-64 dual core | Quad core or better |
| Memory | 4 GB RAM | 8 GB RAM |
| Graphics | A Vulkan 1.1 GPU and driver (Mesa or the vendor's) | A recent discrete GPU for Render mode |
| Storage | 200 MB | 200 MB |
| Notes | Wayland or X11, and libxkbcommon. | |

## Screenshots and capsules

Screenshots: `docs/images/editor.jpg`, `docs/images/godot-bridge.jpg`,
`docs/images/blender-bridge.jpg`, and frames of `docs/images/turntable.gif`
(Steam wants at least five at 1920x1080 or larger, so new captures are
needed). The capsule images (header 920x430, small 462x174, main 1232x706,
vertical 748x896, library 600x900, library hero 3840x1240, logo) are not
made yet.
