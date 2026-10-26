"""Builds the Froxel icons from examples/frog.vox. Run from the repository root:

    cargo build --release
    uv run --with pillow python packaging/icons/make_icons.py

The frog has its lids and squints hidden. The GPU tracer renders it
with an albedo pass, whose black sky is the cut-out mask. macOS gets the 824 px tile on a 1024
canvas with a shadow (froxel-macos*.png, Froxel.icns); Windows and Linux get the tile edge to
edge (froxel.png, froxel.ico, hicolor/), and iOS the tile with its corners filled
(froxel-ios.png).
"""
import subprocess, sys, tempfile
from pathlib import Path
HERE = Path(__file__).resolve().parent
FROG = HERE.parent.parent / "examples" / "frog.vox"
tmp = tempfile.mkdtemp()
# 1440 is the largest square the GPU tracer binds today.
subprocess.run(["target/release/froxel", "render", "--input", str(FROG),
                "--output", f"{tmp}/frog.png", "--aov-albedo", f"{tmp}/albedo.png",
                "--width", "1440", "--height", "1440", "--samples", "384", "--denoise", "--gpu",
                "--yaw", "-65", "--pitch", "18", "--distance", "0.62"], check=True)
from PIL import Image, ImageDraw, ImageFilter, ImageChops
from background import S, background
render = Image.open(f"{tmp}/frog.png").convert("RGB")
alb = Image.open(f"{tmp}/albedo.png").convert("L")
mask = alb.point(lambda v: 255 if v > 2 else 0)
frog = render.copy(); frog.putalpha(mask)
frog = frog.crop(mask.getbbox())

# macOS template: an 824 tile centred on the 1024 canvas, continuous-corner radius about 185.
tile = 824; off = (S - tile) // 2
grad = background()

shape = Image.new("L", (S, S), 0)
ImageDraw.Draw(shape).rounded_rectangle([off, off, off + tile - 1, off + tile - 1], radius=185, fill=255)
icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
icon.paste(grad, (0, 0), shape)

# Frog, about 640 px tall, standing on a soft shadow.
h = 640
w = round(frog.width * h / frog.height)
frog = frog.resize((w, h), Image.LANCZOS)
fx, fy = (S - w) // 2 + 6, off + tile - h - 52
shadow = Image.new("L", (S, S), 0)
ImageDraw.Draw(shadow).ellipse([fx + 30, fy + h - 60, fx + w - 20, fy + h + 30], fill=120)
shadow = ImageChops.multiply(shadow.filter(ImageFilter.GaussianBlur(22)), shape)
dark = Image.new("RGBA", (S, S), (8, 30, 70, 255)); dark.putalpha(shadow)
icon = Image.alpha_composite(icon, dark)
layer = Image.new("RGBA", (S, S), (0, 0, 0, 0)); layer.paste(frog, (fx, fy), frog)
icon = Image.alpha_composite(icon, layer)
# The macOS drop shadow under the tile.
base = Image.new("RGBA", (S, S), (0, 0, 0, 0))
sh = shape.filter(ImageFilter.GaussianBlur(14)).point(lambda v: v * 0.35)
sh = ImageChops.offset(sh, 0, 10)
blk = Image.new("RGBA", (S, S), (0, 0, 0, 255)); blk.putalpha(sh)
out = Image.alpha_composite(Image.alpha_composite(base, blk), icon)
out.save(f"{tmp}/macos.png")
# Full-bleed tile (no margin) for Windows/Linux, where icons are not padded.
full = icon.crop((off, off, off + tile, off + tile)).resize((S, S), Image.LANCZOS)
full.save(f"{tmp}/tile.png")

mac = out
tile = full
mac.save(HERE / "froxel-macos.png", optimize=True)
mac.resize((256, 256), Image.LANCZOS).save(HERE / "froxel-macos-256.png", optimize=True)
tile.save(HERE / "froxel.png", optimize=True)
tile.save(HERE / "froxel.ico", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
for s in (16, 32, 48, 64, 128, 256, 512):
    tile.resize((s, s), Image.LANCZOS).save(HERE / "hicolor" / f"{s}x{s}.png", optimize=True)
iconset = Path(tmp) / "Froxel.iconset"
iconset.mkdir()
for s in (16, 32, 128, 256, 512):
    mac.resize((s, s), Image.LANCZOS).save(iconset / f"icon_{s}x{s}.png")
    mac.resize((s * 2, s * 2), Image.LANCZOS).save(iconset / f"icon_{s}x{s}@2x.png")
subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(HERE / "Froxel.icns")], check=True)
subprocess.run([sys.executable, str(HERE / "make_ios_icon.py")], check=True)
