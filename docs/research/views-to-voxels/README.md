# Views to voxels with 3D generative models

Research from 2026-11-13: can an image-to-3D model turn a multi-view sheet
into a voxel model better than `carve_from_views`, and is it worth putting
into Froxel?

**Outcome: no.** For frog-world's small hard-surface assets (about 30 cells
tall) the existing workflow, a views sheet built into a model with Froxel's
tools, is cleaner than anything the models produced. A model would need to
be retrained on clean voxel art, and the training data does not exist at the
needed scale. Nothing was integrated.

## The three models

All three were read from their code (shallow clones on 2026-11-13).

| | SPAR3D (Stability AI) | TRELLIS (Microsoft) | Hunyuan3D-2mv (Tencent) |
|---|---|---|---|
| Pipeline | Point diffusion (512 coloured points) → transformer → triplanes → DMTet mesh | Rectified flow in a 16³ latent → **64³ occupancy**; second flow → per-voxel features → Gaussians, radiance field, or mesh | Flow DiT in a ShapeVAE vecset latent (3072 tokens) → SDF on any grid → marching cubes |
| Several views | The code has a view axis (`Nv`, camera AdaNorm on DINOv2), but the weights are single-view and `run_image` fixes one camera | Training-free: the sampler alternates images per step (`stochastic`) or averages predictions (`multidiffusion`); the views have **no poses** | **Trained multi-view**: four slots `front`, `left`, `back`, `right`, each with a view embedding; no `top` |
| Colour | Albedo, roughness, metallic | From the Gaussians or radiance field | None in the shape model (a separate, heavy Paint model) |
| Voxel path | Query the triplanes on an N³ grid | The 64³ occupancy is voxels already | Decode the SDF at cell centres of any N³ |
| Mac | MPS, experimental | Effectively no: spconv, flash-attn/xformers, kaolin, nvdiffrast | Yes (MPS); the shape part is plain PyTorch |
| Training code | None (inference only) | Yes (`train.py`, `dataset_toolkits`) | Not in Hunyuan3D-2; Hunyuan3D-2.1 publishes training code |
| Licence | Stability Community: free under $1M revenue | **MIT** (but nvdiffrast and the Gaussian rasterizer are non-commercial; the voxel path does not need them) | Tencent Community: **not valid in the EU, UK, South Korea**; 1M MAU cap; distillation counts as a derivative |

TRELLIS is the closest in kind (a voxel stage, MIT, training code) but needs
a fine-tune to learn view poses and a CUDA GPU. Hunyuan3D-2mv is the only one
trained on several views and runs on a Mac, so it was the one tested.

## Running Hunyuan3D-2mv on an M4 with 16 GB

- Python 3.12 venv with torch 2.15 (MPS), `transformers>=4.46,<5` (version
  5 renames the DINOv2 keys and the checkpoint no longer loads), diffusers
  0.35, plus einops, omegaconf, trimesh, pymeshlab, scikit-image, opencv.
  The texture generator and its CUDA rasterizer are not needed.
- Weights from `tencent/Hunyuan3D-2mv`: `hunyuan3d-dit-v2-mv` (30–50 steps)
  or `hunyuan3d-dit-v2-mv-turbo` (5 steps), 4.9 GB each in fp16.
- Skip the mesh: `scripts/hy_mv.py` asks the pipeline for latents, decodes
  the SDF straight at cell centres, and writes `.vox`.

| | Full model, 30 steps | Turbo, 5 steps |
|---|---|---|
| Load | 34 s | 38–61 s |
| Diffusion | 778 s | 39–42 s |
| SDF 64³ | 49 s | 4–14 s |
| Mesh 256³ (skipped later) | 474 s | — |
| Peak memory | 11 GB | 11 GB |

The full model was swapping at 16 GB; turbo is the usable one.

## Tests

**Repo example (girl figure, front, left, back, 64 cells tall).** Hunyuan
gave a real volume: a round head, the pigtails as separate tufts, arms apart
from the body. `carve_from_views` made the pigtails a disc through the whole
depth and the body a box. Here the model clearly won.

**frog-world slide** (`views-slide-v01.png`, row 1, a 15 x 31 x 30 box;
compared with `assets/voxel/park_playground/park_slide_v01.vox`, 15 x 32 x 31).
Every variant was painted with `paint_from_views` from the four views.

| Variant | Result |
|---|---|
| `carve_from_views` (top, front, left, back) | Slide a wedge across the whole width, deck and roof solid |
| Hunyuan turbo (front, left, back) | Open frame, separate posts, narrow slide, ladder; but the slide is stairs (it read the pixel steps as a staircase), the roof is flat, posts break up at 2x downsampling |
| Hull AND Hunyuan grown one cell | Almost the hull again; growing brings the wedge back |
| Hunyuan + `fit_to_views` | Views IoU 0.95–0.99, but floating debris |
| **Hull AND Hunyuan + `fit_to_views` + `components keep_largest`** | Best: IoU 0.96–0.99, open frame, narrow slide, roof closer to a pyramid |
| `park_slide_v01.vox` | Still clearly the cleanest: straight 2 x 2 posts, a clean slide, a readable ladder |

The best combination still had ragged posts, a stepped slide, white cells no
view sees (under the roof, inside the deck), and a lost ladder. It is a draft
to clean by hand, not an asset.

## Why, and what it would take

The models were trained on smooth 3D objects and know nothing of voxel-art
rules: a post is a straight 2 x 2 column, a slope is an even staircase, a
one-cell detail is kept, not smeared. At 30 cells every cell shows. Fitting
to the views and intersecting with the hull fix silhouettes, not that.

Clean voxel output needs a model trained on clean voxels:

| Option | Cost | Worth it when |
|---|---|---|
| Fine-tune TRELLIS stage 1 with view poses (embeddings per view, as Hunyuan3D-2mv does) | Weeks of work, hundreds of GPU hours, plus a dataset | Thousands of clean voxel models are available |
| A small own model, views + visual hull → corrections to the hull, voxel loss only | Cheap to train (one GPU, days), small enough to run inside Froxel | The same dataset problem |
| Nothing | — | Assets are needed by the dozen, not the thousand |

Views for training are easy (`model_view` renders them from any `.vox`); the
models are the bottleneck. frog-world has dozens; community voxel models vary
in style and licence; voxelized Objaverse is stepped smooth geometry, which is
what Hunyuan already does. Revisit this if generation becomes a Froxel
feature for users, or if a large clean voxel corpus with a usable licence
appears; TRELLIS is then the starting point. Large (60+ cells) or organic
shapes are the one place a model already beat the hull.

## Scripts

`scripts/` holds the pipeline as it was run. The comparison images were in a
temporary folder and were not kept.

- `hy_mv.py`: Hunyuan3D-2mv on MPS (`HY3D_REPO` points at a Hunyuan3D-2
  checkout), writes `sdf_<res>.npy` and `hy_<res>.vox`, optionally a mesh.
- `sdf_vox.py`: SDF grid to `.vox` (glTF Y-up to Froxel's Z-up, facing −Y).
- `sdf_fit.py`: resample the SDF so the model is a given number of cells tall.
- `hull_and.py`: stretch the SDF onto a `froxel carve` box and intersect.
- `views_json.py`: tight alpha crops and a box size for `froxel carve` and
  `froxel paint --tool paint_from_views`.

The slide run, for reference:

```sh
HY3D_REPO=../Hunyuan3D-2 python hy_mv.py out front=front.png left=left.png back=back.png \
  --subfolder hunyuan3d-dit-v2-mv-turbo --steps 5 --res 96
froxel carve --views carve.json --out carve.vox
python hull_and.py out/sdf_96.npy carve.vox mix
froxel fit --args fit.json --input mix_and.vox --out mix_and_fit.vox
froxel paint --tool paint_from_views --args paint.json --input mix_and_fit.vox --out painted.vox
froxel components --input painted.vox --out best.vox --op keep_largest --connectivity 6
```

`fit.json` held the four views with `recolor: false`; `paint.json` the same
views with `newSlots: 16` (without it the colours fall onto the existing
palette and nothing changes).
