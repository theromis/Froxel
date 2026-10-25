#!/usr/bin/env python3
"""Zip blender/froxel_bridge for Blender's Install from Disk."""

import pathlib
import zipfile

root = pathlib.Path(__file__).resolve().parents[1]
source = root / "blender" / "froxel_bridge"
dest = root / "dist" / "froxel_bridge.zip"
dest.parent.mkdir(exist_ok=True)
with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(source.rglob("*")):
        if not path.is_file() or "__pycache__" in path.parts or path.suffix == ".pyc":
            continue
        archive.write(path, "froxel_bridge/" + path.relative_to(source).as_posix())
print(dest)
