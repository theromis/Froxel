// A made-up small fish voxelized from an inline mesh at 24 cells long: a
// box body and a tail, with a dorsal fin and two pectoral fins a third of
// a cell thick. Every cell the surface touches (the default) keeps the fins
// but makes them two cells thick and fattens the body; coverage keeps a
// cell only when half of it is solid, measured three or four times finer,
// so the body is as tight as the mesh, and keepThin keeps each fin as the
// one cell its midline crosses instead of losing it under half a cell.
//
// Parameters
//   length   the fish's length in cells
//   mode     0 every touched cell, 1 coverage alone, 2 coverage and keepThin

let length = param_int("length", 24, 12, 64);
let mode = param_int("mode", 2, 0, 2);

set_slot(2, [120, 140, 70]); // olive body
set_slot(3, [190, 90, 60]); // red-brown fins

let olive = [120, 140, 70];
let fin = [190, 90, 60];
// Axis-aligned boxes [lo, hi, colour]: the body along y (the fish faces
// -Y), 24 units long, so 1 cell a unit at length 24; then the tail, the
// dorsal fin, and the pectoral fins, a third of a cell thick.
let boxes = [
    [[-2.5, -12.0, 0.0], [2.5, 6.0, 5.0], olive],
    [[-1.0, 6.0, 1.0], [1.0, 9.0, 4.0], olive],
    [[-0.15, 9.0, -1.0], [0.15, 12.0, 6.0], fin],
    [[-0.15, -4.0, 5.0], [0.15, 3.0, 8.0], fin],
    [[2.5, -6.0, 1.35], [5.5, -3.0, 1.65], fin],
    [[-5.5, -6.0, 1.35], [-2.5, -3.0, 1.65], fin],
];
let positions = [];
let triangles = [];
let colors = [];
for b in boxes {
    let base = positions.len();
    for i in 0..8 {
        let x = if i % 2 == 0 { b[0][0] } else { b[1][0] };
        let y = if (i / 2) % 2 == 0 { b[0][1] } else { b[1][1] };
        let z = if (i / 4) % 2 == 0 { b[0][2] } else { b[1][2] };
        positions.push([x, y, z]);
        colors.push(b[2]);
    }
    for f in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
        triangles.push([base + f[0], base + f[1], base + f[2]]);
        triangles.push([base + f[0], base + f[2], base + f[3]]);
    }
}

let opts = #{size: length, anchor: "bottom", slots: [2, 3], minFeature: 2};
if mode >= 1 {
    opts.coverage = 0.5;
    opts.keepThin = mode == 2;
}
let report = voxelize(#{positions: positions, triangles: triangles, colors: colors}, opts);
if mode == 2 && report.coverage.thin == 0 {
    throw "keepThin kept no fin cell";
}
