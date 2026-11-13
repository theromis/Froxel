"""Run Hunyuan3D-2mv shape generation on MPS and dump the SDF grid as .vox.

usage: HY3D_REPO=/path/to/Hunyuan3D-2 hy_mv.py OUT_DIR front=PATH
       [left=PATH] [back=PATH] [right=PATH]
       [--steps N] [--res 64] [--mesh-res 0] [--seed S]
       [--subfolder hunyuan3d-dit-v2-mv | hunyuan3d-dit-v2-mv-turbo]

Writes sdf_<res>.npy (the decoder's logits at cell centres, positive inside)
and hy_<res>.vox for each resolution, and mesh.glb when --mesh-res is set.
"""

import argparse
import os
import sys
import time

os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")

import numpy as np
import torch
from PIL import Image

sys.path.insert(0, os.environ["HY3D_REPO"])
from hy3dgen.shapegen import Hunyuan3DDiTFlowMatchingPipeline  # noqa: E402

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sdf_vox import grid_to_cells, write_vox  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument("out")
ap.add_argument("views", nargs="+")
ap.add_argument("--steps", type=int, default=30)
ap.add_argument("--res", default="64")
ap.add_argument("--mesh-res", type=int, default=0)
ap.add_argument("--subfolder", default="hunyuan3d-dit-v2-mv")
ap.add_argument("--seed", type=int, default=1234)
ap.add_argument("--device", default="mps")
a = ap.parse_args()
os.makedirs(a.out, exist_ok=True)

images = {}
for v in a.views:
    k, p = v.split("=", 1)
    images[k] = Image.open(p).convert("RGBA")

t = time.time()
pipe = Hunyuan3DDiTFlowMatchingPipeline.from_pretrained(
    "tencent/Hunyuan3D-2mv",
    subfolder=a.subfolder,
    variant="fp16",
    device=a.device,
    dtype=torch.float16,
)
print(f"load {time.time() - t:.1f}s", flush=True)

t = time.time()
latents = pipe(
    image=images,
    num_inference_steps=a.steps,
    generator=torch.Generator("cpu").manual_seed(a.seed),
    output_type="latent",
)
print(f"diffusion {time.time() - t:.1f}s", flush=True)

vae = pipe.vae
with torch.no_grad():
    dec = vae(1.0 / vae.scale_factor * latents)
    for r in [int(x) for x in a.res.split(",") if x]:
        t = time.time()
        # Sample at cell centres of an r^3 grid over [-1.01, 1.01]^3.
        h = 1.01 * (1 - 1 / r)
        grid = vae.volume_decoder(
            dec, vae.geo_decoder, bounds=[-h] * 3 + [h] * 3,
            octree_resolution=r - 1, num_chunks=20000, enable_pbar=False,
        )
        g = grid[0].cpu().numpy().astype(np.float32)
        np.save(os.path.join(a.out, f"sdf_{r}.npy"), g)
        cells, size = grid_to_cells(g)
        write_vox(os.path.join(a.out, f"hy_{r}.vox"), cells, size)
        print(f"grid {r}: {g.shape} inside={(g > 0).sum()} {time.time() - t:.1f}s",
              flush=True)
    if a.mesh_res:
        t = time.time()
        mesh = pipe._export(latents, output_type="trimesh",
                            octree_resolution=a.mesh_res, mc_algo="mc",
                            enable_pbar=False)[0]
        mesh.export(os.path.join(a.out, "mesh.glb"))
        print(f"mesh {len(mesh.faces)} faces {time.time() - t:.1f}s", flush=True)
