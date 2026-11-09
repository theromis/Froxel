#!/usr/bin/env python3
"""Writes the mesh import fixtures beside this script.

cube.obj + cube.mtl     a unit cube, red Kd, quads
quad.gltf + quad.bin    a 1 x 1 quad on the ground with an external 2x2
  + checker.png         PNG: red, green / blue, white quadrants
quad_embedded.gltf      the same quad, buffer and image as data URIs
quad_jpeg.obj + .mtl    the same quad as OBJ with a 16x16 JPEG map_Kd
  + checker.jpg           (made with macOS sips)
quad_webp.glb           the same quad as a .glb whose texture is a lossless
                        WebP checker in a bufferView, through
                        EXT_texture_webp only (made with cwebp)
parts.glb               two primitives (red factor; blue PNG in a
                        bufferView) under a scaled root node and a child
                        node with a matrix: 6 x 3 x 2 metres
"""

import base64
import json
import os
import struct
import subprocess
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))


def png(width, height, rows):
    raw = b"".join(b"\0" + bytes(sum(row, ())) for row in rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def write(name, data, mode="wb"):
    with open(os.path.join(HERE, name), mode) as f:
        f.write(data)


CHECKER = png(2, 2, [[(255, 0, 0), (0, 255, 0)], [(0, 0, 255), (255, 255, 255)]])
write("checker.png", CHECKER)

write("cube.obj", """# unit cube
mtllib cube.mtl
o cube
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 0 1
v 1 0 1
v 1 1 1
v 0 1 1
usemtl red
f 1 4 3 2
f 5 6 7 8
f 1 2 6 5
f 2 3 7 6
f 3 4 8 7
f 4 1 5 8
""", "w")
write("cube.mtl", "newmtl red\nKd 0.8 0.1 0.1\n", "w")

# The quad: glTF y up, lying on y = 0 from (0, 0, 0) to (1, 0, 1). glTF
# uv (0, 0) is the top-left texel (red).
quad_positions = [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)]
quad_uvs = [(0, 0), (1, 0), (1, 1), (0, 1)]
quad_indices = [0, 2, 1, 0, 3, 2]
quad_bin = (b"".join(struct.pack("<3f", *p) for p in quad_positions)
            + b"".join(struct.pack("<2f", *t) for t in quad_uvs)
            + struct.pack("<6H", *quad_indices))


def quad_doc(buffer_uri, image_uri):
    return {
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{
            "attributes": {"POSITION": 0, "TEXCOORD_0": 1},
            "indices": 2, "material": 0}]}],
        "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9728, "wrapS": 33071, "wrapT": 33071}],
        "images": [{"uri": image_uri}],
        "buffers": [{"uri": buffer_uri, "byteLength": len(quad_bin)}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 48},
            {"buffer": 0, "byteOffset": 48, "byteLength": 32},
            {"buffer": 0, "byteOffset": 80, "byteLength": 12},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
             "min": [0, 0, 0], "max": [1, 0, 1]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2"},
            {"bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR"},
        ],
    }


write("quad.bin", quad_bin)
write("quad.gltf", json.dumps(quad_doc("quad.bin", "checker.png"), indent=1), "w")
write("quad_embedded.gltf", json.dumps(quad_doc(
    "data:application/octet-stream;base64," + base64.b64encode(quad_bin).decode(),
    "data:image/png;base64," + base64.b64encode(CHECKER).decode()), indent=1), "w")

# The OBJ quad, Y-up as Blender writes it; vt has its origin at the bottom.
# JPEG halves chroma, so its checker is 16 x 16 with 8 x 8 quadrants.
big = png(16, 16, [[(255, 0, 0)] * 8 + [(0, 255, 0)] * 8] * 8
          + [[(0, 0, 255)] * 8 + [(255, 255, 255)] * 8] * 8)
scratch = os.path.join(HERE, "checker16.png")
write("checker16.png", big)
subprocess.run(["sips", "-s", "format", "jpeg", "-s", "formatOptions", "100", scratch,
                "--out", os.path.join(HERE, "checker.jpg")], check=True, capture_output=True)
os.remove(scratch)
write("quad_jpeg.obj", """mtllib quad_jpeg.mtl
v 0 0 0
v 1 0 0
v 1 0 1
v 0 0 1
vt 0 1
vt 1 1
vt 1 0
vt 0 0
usemtl checker
f 1/1 3/3 2/2
f 1/1 4/4 3/3
""", "w")
write("quad_jpeg.mtl", "newmtl checker\nKd 0.5 0.5 0.5\nmap_Kd -o 0 0 checker.jpg\n", "w")

# parts.glb: unit cubes at x 0..1 (primitive 0, red factor, white normalized
# u8 COLOR_0, u16 indices) and x 2..3 (primitive 1, a 1x1 blue PNG in a
# bufferView, u32 indices). The child node's matrix stretches z by 1.5; the
# root scales by 2.
cube = [(x, y, z) for z in (0, 1) for y in (0, 1) for x in (0, 1)]
cube_tris = [0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4,
             2, 6, 7, 2, 7, 3, 0, 4, 6, 0, 6, 2, 1, 3, 7, 1, 7, 5]
blue = png(1, 1, [[(0, 0, 255)]])
parts = bytearray()
views = []


def view(data):
    while len(parts) % 4:
        parts.append(0)
    views.append({"buffer": 0, "byteOffset": len(parts), "byteLength": len(data)})
    parts.extend(data)
    return len(views) - 1


p0 = view(b"".join(struct.pack("<3f", *p) for p in cube))
c0 = view(bytes([255, 255, 255, 255] * 8))
i0 = view(struct.pack("<36H", *cube_tris))
p1 = view(b"".join(struct.pack("<3f", x + 2, y, z) for x, y, z in cube))
i1 = view(struct.pack("<36I", *cube_tris))
im = view(blue)
doc = {
    "asset": {"version": "2.0"},
    "scene": 0,
    "scenes": [{"nodes": [0]}],
    "nodes": [
        {"name": "root", "translation": [10, 0, 0], "scale": [2, 2, 2], "children": [1]},
        {"name": "part", "mesh": 0,
         "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1.5, 0, 0, 0, 0, 1]},
    ],
    "meshes": [{"primitives": [
        {"attributes": {"POSITION": 0, "COLOR_0": 1}, "indices": 2, "material": 0},
        {"attributes": {"POSITION": 3}, "indices": 4, "material": 1},
    ]}],
    "materials": [
        {"pbrMetallicRoughness": {"baseColorFactor": [1, 0, 0, 1]}},
        {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}},
    ],
    "textures": [{"source": 0}],
    "images": [{"bufferView": im, "mimeType": "image/png"}],
    "accessors": [
        {"bufferView": p0, "componentType": 5126, "count": 8, "type": "VEC3"},
        {"bufferView": c0, "componentType": 5121, "normalized": True, "count": 8, "type": "VEC4"},
        {"bufferView": i0, "componentType": 5123, "count": 36, "type": "SCALAR"},
        {"bufferView": p1, "componentType": 5126, "count": 8, "type": "VEC3"},
        {"bufferView": i1, "componentType": 5125, "count": 36, "type": "SCALAR"},
    ],
    "bufferViews": views,
    "buffers": [{"byteLength": len(parts)}],
}
while len(parts) % 4:
    parts.append(0)
doc["buffers"][0]["byteLength"] = len(parts)
text = json.dumps(doc).encode()
text += b" " * (-len(text) % 4)
glb = (struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(text) + 8 + len(parts))
       + struct.pack("<II", len(text), 0x4E4F534A) + text
       + struct.pack("<II", len(parts), 0x004E4942) + bytes(parts))
write("parts.glb", glb)

# quad_webp.glb: the quad with a lossless WebP checker, as image-to-3D tools
# write it: the texture has no plain source, only EXT_texture_webp.
scratch = os.path.join(HERE, "checker_webp.png")
write("checker_webp.png", CHECKER)
webp_path = os.path.join(HERE, "checker_webp.webp")
subprocess.run(["cwebp", "-quiet", "-lossless", scratch, "-o", webp_path], check=True,
               capture_output=True)
with open(webp_path, "rb") as f:
    webp = f.read()
os.remove(scratch)
os.remove(webp_path)
blob = bytearray(quad_bin)
while len(blob) % 4:
    blob.append(0)
image_offset = len(blob)
blob.extend(webp)
while len(blob) % 4:
    blob.append(0)
doc = quad_doc(None, None)
del doc["buffers"][0]["uri"]
doc["buffers"][0]["byteLength"] = len(blob)
doc["bufferViews"].append({"buffer": 0, "byteOffset": image_offset, "byteLength": len(webp)})
doc["images"] = [{"bufferView": 3, "mimeType": "image/webp"}]
doc["textures"] = [{"sampler": 0, "extensions": {"EXT_texture_webp": {"source": 0}}}]
doc["extensionsUsed"] = ["EXT_texture_webp"]
doc["extensionsRequired"] = ["EXT_texture_webp"]
text = json.dumps(doc).encode()
text += b" " * (-len(text) % 4)
glb = (struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(text) + 8 + len(blob))
       + struct.pack("<II", len(text), 0x4E4F534A) + text
       + struct.pack("<II", len(blob), 0x004E4942) + bytes(blob))
write("quad_webp.glb", glb)
