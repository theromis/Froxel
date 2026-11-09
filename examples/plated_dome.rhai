// A made-up armoured dome painted with shell_plates: a central plate, two
// rings of plates, an outer band of small plates, and a rim, split by dark
// seams that follow the surface. The plates' places come from the distance
// along the surface from the apex, so the rings keep their width down the
// oval dome's steep sides, with no streaks; symmetric mirrors the layout
// left to right, slots included. Set layout to voronoi for rounder,
// irregular plates, or grooves to cut the seams instead of painting them.
//
// Parameters
//   width, length, height   the dome in cells
//   inner, outer            plates in the two rings
//   band                    plates in the outer band
//   voronoi                 1 for the voronoi layout
//   grooves                 1 to cut the seams one cell deep
//   seed                    the jitter and the slot picks

let width = param_int("width", 30, 12, 96);
let length = param_int("length", 38, 12, 96);
let height = param_int("height", 13, 4, 48);
let inner = param_int("inner", 5, 3, 16);
let outer = param_int("outer", 9, 3, 32);
let band = param_int("band", 16, 0, 48);
let voronoi = param_int("voronoi", 0, 0, 1);
let grooves = param_int("grooves", 0, 0, 1);
let seed = param_int("seed", 3, 0, 9999);

set_slot(2, [92, 74, 42]); // dome
set_slot(3, [122, 104, 58]); // plate
set_slot(4, [138, 118, 66]); // lighter plate
set_slot(5, [108, 90, 50]); // darker plate
set_slot(6, [44, 34, 20]); // seam
set_slot(7, [150, 130, 74]); // central plate
set_slot(8, [70, 56, 32]); // rim

// The dome: the top half of an ellipsoid.
fill_ellipsoid(0, 0, -height, width - 1, length - 1, height, 2);
carve_box(0, 0, -height, width - 1, length - 1, -1);

let plates = shell_plates(#{
    palettes: #{ring: [3, 4, 5], central: 7, band: [4, 5], rim: 8, seam: if grooves == 1 { 0 } else { 6 }},
    layout: if voronoi == 1 { "voronoi" } else { "rings" },
    rings: [inner, outer],
    bandCount: band,
    jitter: 0.3,
    seed: seed,
});
if plates.unreached > 0 {
    throw "part of the dome is not joined to its apex";
}
