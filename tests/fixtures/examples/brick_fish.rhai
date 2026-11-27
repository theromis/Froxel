// A fish body built from irregular bricks: the cells give the shape, and
// the skin covers it with bricks of uneven length in staggered rows along
// the body, each with its own relief and the colour of the cells under it,
// so it reads like the Frog World concepts instead of grid steps. The
// fins are thin, so they keep their cells; the eyes are a bare slot.
// A rerun reshapes the skin with the body.
//
// Parameters
//   length  body length, in cells
//   brick   longest brick, in cells
//   relief  random height of each brick, in cells
//   seed    the brick layout

let length = param_int("length", 48, 24, 120);
let brick = param_float("brick", 4.0, 1.0, 12.0);
let relief = param_float("relief", 0.3, 0.0, 1.5);
let seed = param_int("seed", 0, 0, 9999);

set_slot(1, [84, 92, 44]);   // back
set_slot(2, [128, 104, 58]); // flank
set_slot(3, [210, 196, 160]); // belly
set_slot(4, [182, 92, 48]);  // fins
set_slot(5, [230, 170, 40]); // eye

let half = length / 2;
let tall = length / 5;
let wide = length / 6;
// Back, flank, and belly bands of one long body.
fill_ellipsoid(-half, -wide, -tall, half, wide, tall, 2);
recolor(2, 1, #{x0: -half, y0: -wide, z0: tall / 3, x1: half, y1: wide, z1: tall});
recolor(2, 3, #{x0: -half, y0: -wide, z0: -tall, x1: half, y1: wide, z1: -tall / 3});
// Thin fins: a dorsal blade and a tail fan, two cells thick.
fill_box(-half / 4, 0, tall - 1, half / 4, 1, tall + tall / 2, 4);
fill_box(half - 2, 0, -tall / 2, half + tall / 2, 1, tall / 2, 4);
// Eyes on both sides of the head.
let ex = -half + length / 8;
fill_box(ex, -wide - 1, tall / 4, ex + 1, wide + 1, tall / 4 + 1, 5);

skin(#{axis: "x", length: [brick / 2, brick], relief: relief, bare: [5], seed: seed});
