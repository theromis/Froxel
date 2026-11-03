// A faceted boulder: a low-poly mesh built in the script, voxelized with
// `voxelize` (the voxelizer of MCP import_mesh), then finished with the
// stone tools. A sculpted or generated mesh comes in the same way through
// MCP import_mesh, File > Import Mesh…, or `froxel import`; scripts read no
// files, so they pass the triangles inline.
//
// Parameters
//   size     cells along the longest axis
//   rings    facets from top to bottom
//   lumps    how far each vertex moves in or out, as a share of the radius
//   seed     the lumps, the chips, and the moss

let size = param_int("size", 28, 8, 96);
let rings = param_int("rings", 6, 3, 12);
let lumps = param_float("lumps", 0.22, 0.0, 0.5);
let seed = param_int("seed", 3, 0, 9999);

set_slot(1, [112, 108, 98]);
set_slot(2, [86, 84, 78]);
set_slot(3, [58, 92, 44]);
set_slot(4, [84, 120, 58]);

// The index of vertex `s` of ring `r` (1 is the ring under the top pole).
fn at(r, s, segments) {
    1 + (r - 1) * segments + (s % segments)
}

// A UV sphere, squashed and lumpy: a pole, rings of `segments` vertices,
// a pole. Lower vertices are darker, so the colour sampling shows.
let segments = rings * 2;
let positions = [];
let colors = [];
positions.push([0.0, 0.0, 0.62]);
colors.push([150, 144, 130]);
for r in 1..rings {
    let polar = PI * to_float(r) / to_float(rings);
    for s in 0..segments {
        let around = 2.0 * PI * to_float(s) / to_float(segments);
        let bump = 1.0 + (noise2(to_float(r) * 1.7, to_float(s) * 1.3, seed) - 0.5) * 2.0 * lumps;
        let ring = sin(polar) * bump;
        positions.push([ring * cos(around), ring * sin(around) * 0.8, cos(polar) * 0.62 * bump]);
        let shade = 150 - to_int(70.0 * to_float(r) / to_float(rings));
        colors.push([shade, shade - 6, shade - 18]);
    }
}
positions.push([0.0, 0.0, -0.5]);
colors.push([70, 66, 60]);

let bottom = positions.len() - 1;
let triangles = [];
for s in 0..segments {
    triangles.push([0, at(1, s, segments), at(1, s + 1, segments)]);
    triangles.push([bottom, at(rings - 1, s + 1, segments), at(rings - 1, s, segments)]);
    for r in 1..(rings - 1) {
        let a = at(r, s, segments);
        let b = at(r + 1, s, segments);
        let c = at(r + 1, s + 1, segments);
        let d = at(r, s + 1, segments);
        triangles.push([a, b, c]);
        triangles.push([a, c, d]);
    }
}

// Colours map to the two stone slots above; with no slots they would be
// quantized into new palette slots instead.
let rock = voxelize(#{positions: positions, triangles: triangles, colors: colors},
    #{size: size, anchor: "bottom", slots: [1, 2]});
let b = rock.bounds;
weather(b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2],
    #{chip: 0.25, pits: 0.04, noise: [1, 2], seed: seed});
moss(b.min[0] - 1, b.min[1] - 1, b.min[2], b.max[0] + 1, b.max[1] + 1, b.max[2] + 1, [3, 4],
    #{amount: 0.5, curtains: 0.3, seed: seed});
