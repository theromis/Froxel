"""Intersect a Hunyuan3D SDF with a carve_from_views hull .vox.

usage: hull_and.py SDF.npy HULL.vox OUT_PREFIX [--level L]

The SDF's occupied bounds are stretched onto the hull's box (axis by axis),
then three files are written: OUT_PREFIX_box.vox (Hunyuan alone in the box),
OUT_PREFIX_and.vox (hull AND Hunyuan), and OUT_PREFIX_and_d1.vox (hull AND
Hunyuan grown by one cell). The hull is read from the first SIZE and XYZI
chunks, so it should be a single-object file such as `froxel carve` writes.
"""

import os
import struct
import sys

import numpy as np
import torch
import torch.nn.functional as F

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sdf_vox import write_vox  # noqa: E402


def read_vox(path):
    d = open(path, "rb").read()
    i = d.find(b"SIZE")
    size = struct.unpack("<iii", d[i + 12:i + 24])
    j = d.find(b"XYZI")
    n = struct.unpack("<i", d[j + 12:j + 16])[0]
    xyzi = np.frombuffer(d[j + 16:j + 16 + 4 * n], np.uint8).reshape(n, 4)
    occ = np.zeros(size, bool)
    occ[xyzi[:, 0], xyzi[:, 1], xyzi[:, 2]] = True
    return occ


sdf_path, hull_path, prefix = sys.argv[1:4]
level = float(sys.argv[sys.argv.index("--level") + 1]) if "--level" in sys.argv else 0.0

g = np.load(sdf_path)
# glTF (x, y, z) -> Froxel (x, -z, y)
g = np.flip(g.transpose(0, 2, 1), axis=1).copy()
occ = g > level
idx = [np.nonzero(occ.any(axis=tuple(a for a in range(3) if a != k)))[0] for k in range(3)]
lo = [int(i.min()) for i in idx]
hi = [int(i.max()) + 1 for i in idx]
crop = g[lo[0]:hi[0], lo[1]:hi[1], lo[2]:hi[2]]

hull = read_vox(hull_path)
box = hull.shape
t = torch.from_numpy(crop)[None, None]
hy = F.interpolate(t, size=box, mode="trilinear", align_corners=False)[0, 0].numpy() > level

grown = F.max_pool3d(torch.from_numpy(hy)[None, None].float(), 3, 1, 1)[0, 0].numpy() > 0


def save(name, o):
    cells = np.argwhere(o)
    write_vox(f"{prefix}_{name}.vox", cells, box)
    return len(cells)


print({
    "box": box,
    "hull": int(hull.sum()),
    "hunyuan": save("box", hy),
    "and": save("and", hull & hy),
    "and_d1": save("and_d1", hull & grown),
})
