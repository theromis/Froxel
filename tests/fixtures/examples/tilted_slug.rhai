// A made-up slug mesh arriving the way an image-to-3D tool leaves one:
// tilted off its axes, turned away, and coloured with texture noise. One
// voxelize call (MCP import_mesh's voxelizer) stands it level with
// orient "upright" (its thinnest principal axis up, signed so it narrows
// upward, and the taller end of its long axis, the head, turned to -Y)
// and cleans its colours with clean (reduce_palette before writing).
//
// Parameters
//   size     cells along the longest axis
//   tilt     degrees the mesh arrives tilted about x
//   turn     degrees it arrives turned about z
//   colors   clusters the noisy colours are reduced to

let size = param_int("size", 40, 16, 96);
let tilt = param_float("tilt", 28.0, -40.0, 40.0);
let turn = param_float("turn", 130.0, -180.0, 180.0);
let colors = param_int("colors", 3, 1, 8);

// A lat-long ellipsoid, long along x, low along z, its +x end (the head)
// standing taller; the head darker, the foot paler, every vertex jittered
// in colour.
let rings = 12;
let segments = 20;
let r = rng(7);
// The colour at a point: the head darker, the foot paler, jittered.
fn shade(x, z, n) {
    let base = if x > 0.55 { [120, 70, 40] } else if z < 0.05 { [220, 190, 120] } else { [196, 128, 52] };
    [base[0] + n, base[1] + n, base[2] + n / 2]
}
fn height(x, polar) {
    let head = if x > 0.4 { 1.0 + (x - 0.4) * 1.6 } else { 1.0 };
    cos(polar) * 0.24 * head + 0.1 * head
}
let positions = [[0.0, 0.0, 0.34]];
let shades = [shade(0.0, 0.34, r.int(-14, 14))];
for i in 1..rings {
    let polar = PI * to_float(i) / to_float(rings);
    for s in 0..segments {
        let around = 2.0 * PI * to_float(s) / to_float(segments);
        let x = sin(polar) * cos(around);
        let z = height(x, polar);
        positions.push([x, sin(polar) * sin(around) * 0.45, z]);
        shades.push(shade(x, z, r.int(-14, 14)));
    }
}
positions.push([0.0, 0.0, -0.14]);
shades.push(shade(0.0, -0.14, r.int(-14, 14)));
let bottom = positions.len() - 1;
fn at(i, s, segments) {
    1 + (i - 1) * segments + (s % segments)
}
let triangles = [];
for s in 0..segments {
    triangles.push([0, at(1, s, segments), at(1, s + 1, segments)]);
    triangles.push([bottom, at(rings - 1, s + 1, segments), at(rings - 1, s, segments)]);
    for i in 1..(rings - 1) {
        let a = at(i, s, segments);
        let b = at(i + 1, s, segments);
        let c = at(i + 1, s + 1, segments);
        let d = at(i, s + 1, segments);
        triangles.push([a, b, c]);
        triangles.push([a, c, d]);
    }
}
// Tilt about x, then turn about z, as the generator left it.
let st = sin(tilt * PI / 180.0);
let ct = cos(tilt * PI / 180.0);
let sz = sin(turn * PI / 180.0);
let cz = cos(turn * PI / 180.0);
let tilted = [];
for p in positions {
    let y1 = p[1] * ct - p[2] * st;
    let z1 = p[1] * st + p[2] * ct;
    tilted.push([p[0] * cz - y1 * sz, p[0] * sz + y1 * cz, z1]);
}

let report = voxelize(#{positions: tilted, triangles: triangles, colors: shades},
    #{size: size, upAxis: "z", anchor: "bottom", orient: "upright",
      clean: #{colors: colors, despeckle: 1}});
if report.orientation.upright.keptUp {
    throw "the slug should stand up";
}
