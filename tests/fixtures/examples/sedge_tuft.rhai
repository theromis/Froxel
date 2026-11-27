// A sedge tuft on a clump of mud: leaves are strands, chains of cubes
// turned along a curve, so they bend instead of stepping on the grid.
// Each leaf fans out from the base, arches over, twists, and tapers, dark
// at the base and pale at the tip. A rerun replaces the leaves.
//
// Parameters
//   leaves  how many leaves
//   height  tallest leaf, in cells
//   lean    how far the tips reach out, in cells
//   seed    the scatter

let leaves = param_int("leaves", 11, 3, 40);
let height = param_float("height", 14.0, 4.0, 40.0);
let lean = param_float("lean", 6.0, 0.0, 20.0);
let seed = param_int("seed", 4, 0, 9999);

set_slot(1, [86, 60, 38]);
set_slot(2, [52, 92, 34]);
set_slot(3, [104, 146, 48]);
set_slot(4, [196, 190, 96]);

// The mud clump the leaves stand in.
fill_ellipsoid(-3, -3, 0, 3, 3, 2, 1);

let r = rng(seed);
let tau = 6.283185307179586;
for i in 0..leaves {
    let a = tau * i / leaves + r.float(-0.25, 0.25);
    let tall = height * r.float(0.6, 1.0);
    let out = lean * r.float(0.5, 1.0);
    let bend = r.float(15.0, 45.0);
    add_strand([
        [0.4 * cos(a), 0.4 * sin(a), 2.0, 0.0, 1.0],
        [out * 0.25 * cos(a), out * 0.25 * sin(a), 2.0 + tall * 0.6, bend * 0.5, 0.85],
        [out * cos(a), out * sin(a), 2.0 + tall, bend, 0.35],
    ], #{width: 1.0, thickness: 0.3, step: 0.8, palettes: [2, 3, 4]});
}
