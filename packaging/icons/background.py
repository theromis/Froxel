"""The icon tile's background: a blue gradient, a soft light behind the frog, and a faint
voxel grid. make_icons.py draws the frog on it; make_ios_icon.py fills the tile's corners
with it."""
from PIL import Image, ImageDraw, ImageFilter

S = 1024
# The macOS template: an 824 tile centred on the 1024 canvas.
TILE = 824
OFF = (S - TILE) // 2


def background():
    top, bottom = (70, 170, 240), (24, 70, 160)
    grad = Image.new("RGB", (S, S))
    d = ImageDraw.Draw(grad)
    for y in range(S):
        t = y / (S - 1)
        d.line([(0, y), (S, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))
    # A soft light behind the frog.
    glow = Image.new("L", (S, S), 0)
    ImageDraw.Draw(glow).ellipse([212, 150, 812, 750], fill=110)
    glow = glow.filter(ImageFilter.GaussianBlur(120))
    grad = Image.composite(Image.new("RGB", (S, S), (180, 230, 255)), grad, glow)
    # Faint voxel grid.
    grid = Image.new("L", (S, S), 0); g = ImageDraw.Draw(grid)
    step = 64
    for i in range(OFF, OFF + TILE + 1, step):
        g.line([(i, 0), (i, S)], fill=26, width=3); g.line([(0, i), (S, i)], fill=26, width=3)
    return Image.composite(Image.new("RGB", (S, S), (255, 255, 255)), grad, grid)
