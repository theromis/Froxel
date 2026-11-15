// A made-up fluted pillar for radial_relief: a plinth, a shaft, and a
// capital. The shaft gets flutes, shallow grooves spaced evenly along its
// outline: a square shaft gets as many per face, a round one evenly by
// angle. Their ends cup round over fade cells instead of stopping square,
// and the floors take a darker shade (made from the stone's own slot) so
// the slope reads where cells can only step.
//
// Parameters
//   side      the shaft's width in cells (square) or diameter (round)
//   height    the shaft's height in cells
//   flutes    flutes round the shaft (2 (side - 1) alternate evenly)
//   round     1 for a round shaft
//   depth     the flutes' depth in cells
//   profile   0 u, 1 v, 2 flat

let side = param_int("side", 8, 6, 40);
let height = param_int("height", 66, 12, 200);
let flutes = param_int("flutes", 14, 4, 64);
let round = param_int("round", 0, 0, 1);
let depth = param_float("depth", 1.0, 0.5, 4.0);
let profile = param_int("profile", 0, 0, 2);

set_slot(2, [196, 188, 170]); // stone
set_slot(3, [168, 160, 142]); // plinth and capital

let pad = 2;
let lo = -pad;
let hi = side - 1 + pad;
let z0 = 4;
let z1 = z0 + height - 1;
// The shaft, rounded by a tall ellipsoid in mode intersect when round.
fill_box(0, 0, z0, side - 1, side - 1, z1, 2);
if round == 1 {
    fill_ellipsoid(0, 0, z0 - 4 * height, side - 1, side - 1, z1 + 4 * height, 0, #{mode: "intersect"});
}
// Plinth and capital.
fill_box(lo, lo, 0, hi, hi, z0 - 1, 3);
fill_box(lo, lo, z1 + 1, hi, hi, z1 + 4, 3);

// Flutes are spaced along the ring of surface cells: a square shaft of side
// s has 4 (s - 1) of them, so 2 (s - 1) flutes (14 on side 8) alternate
// flute and ridge all the way round. Other counts give an uneven rhythm,
// and a flute too narrow to cut a cell is carried by its shade alone.
let fluted = radial_relief("flutes", flutes, #{
    from: z0 + 2,
    to: z1 - 2,
    depth: depth,
    profile: ["u", "v", "flat"][profile],
    only: [2],
});
if fluted.recesses < flutes {
    throw `only ${fluted.recesses} of ${flutes} flutes cut`;
}
