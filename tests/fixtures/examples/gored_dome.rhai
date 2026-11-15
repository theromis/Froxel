// A made-up gored dome for radial_relief: a half ellipsoid on a drum. Each of
// the gores, the segments between two meridians, dishes toward its middle
// while the ribs between them stay proud and take their own slot; the
// gores fade out at the crown and over fade cells at the foot. The floors
// take a darker shade and the slopes by the ribs a lighter one, both made
// from the roof's own slot, so the dish reads sloped in a preview.
//
// Parameters
//   radius    the dome's foot radius in cells
//   rise      the dome's height in cells
//   gores     gores round the dome
//   depth     the gores' depth in cells
//   raised    cells the ribs stand out

let radius = param_int("radius", 24, 8, 96);
let rise = param_int("rise", 18, 4, 96);
let gores = param_int("gores", 12, 3, 64);
let depth = param_float("depth", 2.0, 0.5, 4.0);
let raised = param_int("raised", 1, 0, 4);

set_slot(2, [92, 124, 132]); // roof
set_slot(3, [208, 196, 168]); // ribs
set_slot(4, [118, 108, 96]); // drum

// A drum: a box rounded by a flat ellipsoid in mode intersect.
let drum = radius + 1;
let base = 6;
fill_box(-drum, -drum, 0, drum, drum, base - 1, 4);
fill_ellipsoid(-drum, -drum, -12, drum, drum, base + 11, 0, #{mode: "intersect"});
// The dome: the top half of an ellipsoid of the radius and the rise.
fill_ellipsoid(-radius, -radius, base - rise, radius, radius, base + rise, 2, #{mode: "empty"});
carve_box(-drum - 1, -drum - 1, base - rise - 1, drum + 1, drum + 1, -1);

let dome = radial_relief("gores", gores, #{
    from: base,
    center: [0, 0],
    depth: depth,
    ribs: 3,
    ribHeight: raised,
    light: true,
    only: [2],
});
if dome.recesses < gores {
    throw `only ${dome.recesses} of ${gores} gores cut`;
}
