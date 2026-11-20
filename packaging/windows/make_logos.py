"""Builds the MSIX logos in packaging/windows/Assets from packaging/icons/froxel.png (the
full-bleed tile make_icons.py writes). Run from the repository root after make_icons.py:

    uv run --with pillow python packaging/windows/make_logos.py

Square logos are the tile edge to edge at each scale; the app list and taskbar logo also gets
the unplated target sizes Windows asks for. The wide tile puts the square tile in the middle
of the tile's own background. makepri (msix.ps1) indexes the qualified names, so the manifest
names only the base file, as in Assets/Square150x150Logo.png.
"""
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
ICONS = HERE.parent / "icons"
sys.path.insert(0, str(ICONS))
from background import background  # noqa: E402

OUT = HERE / "Assets"
SCALES = (100, 150, 200)
# Base logo sizes at scale 100.
SQUARE = {
    "Square44x44Logo": 44,
    "Square71x71Logo": 71,
    "Square150x150Logo": 150,
    "Square310x310Logo": 310,
    "StoreLogo": 50,
}
TARGETS = (16, 24, 32, 48, 256)

tile = Image.open(ICONS / "froxel.png").convert("RGBA")


def square(px):
    return tile.resize((px, px), Image.LANCZOS)


def wide(w, h):
    canvas = background().convert("RGBA").resize((w, w), Image.LANCZOS)
    top = (w - h) // 2
    canvas = canvas.crop((0, top, w, top + h))
    side = round(h * 0.84)
    canvas.alpha_composite(square(side), ((w - side) // 2, (h - side) // 2))
    return canvas


OUT.mkdir(exist_ok=True)
for old in OUT.glob("*.png"):
    old.unlink()
for name, base in SQUARE.items():
    for scale in SCALES:
        square(round(base * scale / 100)).save(OUT / f"{name}.scale-{scale}.png", optimize=True)
for scale in SCALES:
    wide(round(310 * scale / 100), round(150 * scale / 100)).save(
        OUT / f"Wide310x150Logo.scale-{scale}.png", optimize=True
    )
for px in TARGETS:
    img = square(px)
    img.save(OUT / f"Square44x44Logo.targetsize-{px}.png", optimize=True)
    img.save(OUT / f"Square44x44Logo.targetsize-{px}_altform-unplated.png", optimize=True)
print(f"{len(list(OUT.glob('*.png')))} logos in {OUT}")
