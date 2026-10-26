"""Builds froxel-ios.png, the App Store icon, from froxel.png. Run from the repository root:

    uv run --with pillow python packaging/icons/make_ios_icon.py

iOS wants an opaque square and rounds the corners itself, so the tile's transparent
corners are filled with its background. make_icons.py runs this after it writes froxel.png.
"""
from pathlib import Path
from PIL import Image, ImageFilter
from background import S, OFF, TILE, background

HERE = Path(__file__).resolve().parent
tile = Image.open(HERE / "froxel.png").convert("RGBA")
bg = background().crop((OFF, OFF, OFF + TILE, OFF + TILE)).resize((S, S), Image.LANCZOS)
# The resized tile's edge is part transparent and darker; keep only its opaque inside.
inside = tile.getchannel("A").point(lambda v: 255 if v == 255 else 0).filter(ImageFilter.MinFilter(5))
Image.composite(tile.convert("RGB"), bg, inside).save(HERE / "froxel-ios.png", optimize=True)
