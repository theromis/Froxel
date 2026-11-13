"""Resample a Hunyuan3D SDF grid so the model is HEIGHT cells tall, write .vox.

usage: sdf_fit.py SDF.npy HEIGHT OUT.vox
"""

import os
import sys

import numpy as np
import torch
import torch.nn.functional as F

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sdf_vox import grid_to_cells, write_vox  # noqa: E402

g = np.load(sys.argv[1])
height = int(sys.argv[2])
n = g.shape[0]
ys = np.nonzero((g > 0).any(axis=(0, 2)))[0]  # glTF Y is up
span = (ys.max() - ys.min() + 1) / n
m = max(2, round(height / span))
t = torch.from_numpy(g)[None, None]
r = F.interpolate(t, size=(m, m, m), mode="trilinear", align_corners=False)[0, 0]
cells, size = grid_to_cells(r.numpy())
write_vox(sys.argv[3], cells, size)
print(f"grid {n} -> {m}: {len(cells)} cells, size {size}")
