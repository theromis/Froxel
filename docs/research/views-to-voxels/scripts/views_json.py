"""Write carve_from_views / paint_from_views arguments for RGBA views.

usage: views_json.py OUT.json HEIGHT front=PATH left=PATH [back=PATH] ...

Each view gets its tight alpha crop; the box is HEIGHT cells tall and as
wide and deep as the front and side crops say.
"""

import json
import os
import sys

import numpy as np
from PIL import Image

out, height = sys.argv[1], int(sys.argv[2])
views, crops = [], {}
for arg in sys.argv[3:]:
    name, path = arg.split("=", 1)
    a = np.asarray(Image.open(path).convert("RGBA"))[..., 3] > 128
    ys, xs = np.nonzero(a)
    crop = [int(xs.min()), int(ys.min()), int(np.ptp(xs) + 1), int(np.ptp(ys) + 1)]
    crops[name] = crop
    views.append({"view": name, "path": os.path.abspath(path), "crop": crop,
                  "background": "alpha"})


def aspect(name):
    c = crops[name]
    return c[2] / c[3]


side = next(n for n in ("left", "right", "side") if n in crops)
size = [round(height * aspect("front")), round(height * aspect(side)), height]
json.dump({"views": views, "size": size}, open(out, "w"), indent=1)
print(json.dumps({"size": size, "crops": crops}))
