"""Write a MagicaVoxel .vox from a Hunyuan3D SDF grid (positive inside).

The grid is indexed [x, y, z] in glTF Y-up; Froxel is Z-up with the model
facing -Y, so a cell (x, y, z) goes to (x, N-1-z, y). The occupied cells
are cropped to their bounds and written in palette slot 1.

usage: sdf_vox.py SDF.npy OUT.vox
"""

import struct

import numpy as np


def grid_to_cells(g, level=0.0):
    occ = g > level
    occ = np.flip(occ.transpose(0, 2, 1), axis=1)  # (x, z, y) -> Y = N-1-z
    xs, ys, zs = np.nonzero(occ)
    if len(xs) == 0:
        return np.zeros((0, 3), int), (1, 1, 1)
    lo = np.array([xs.min(), ys.min(), zs.min()])
    cells = np.stack([xs, ys, zs], 1) - lo
    return cells, tuple(int(v) for v in cells.max(0) + 1)


def chunk(tag, content, children=b""):
    return tag + struct.pack("<ii", len(content), len(children)) + content + children


def write_vox(path, cells, size):
    assert max(size) <= 256, size
    sz = chunk(b"SIZE", struct.pack("<iii", *size))
    body = struct.pack("<i", len(cells)) + b"".join(
        struct.pack("<BBBB", x, y, z, 1) for x, y, z in cells)
    xyzi = chunk(b"XYZI", body)
    with open(path, "wb") as f:
        f.write(b"VOX " + struct.pack("<i", 150) + chunk(b"MAIN", b"", sz + xyzi))


if __name__ == "__main__":
    import sys
    g = np.load(sys.argv[1])
    cells, size = grid_to_cells(g)
    write_vox(sys.argv[2], cells, size)
    print(f"{len(cells)} cells, size {size}")
