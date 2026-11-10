// A made-up frog blocked out at 20 cells long, then exaggerated for its
// size: deform scale grows the head and eyes about the head's centre (the
// cells within radius as one piece, easing back to none over falloff), so
// the face reads at a gameplay distance, and minFeature reports the legs
// that are still one cell thin. Low-resolution caricature: simplify the
// form, then enlarge what makes it read.
//
// Parameters
//   head     the head's scale factor in percent (100 leaves it)
//   radius   cells around the head's centre scaled as one piece
//   falloff  cells over which the scale eases back to none

let head = param_int("head", 160, 100, 250);
let radius = param_int("radius", 5, 2, 8);
let falloff = param_int("falloff", 3, 1, 8);

set_slot(2, [96, 150, 60]); // skin
set_slot(3, [230, 220, 150]); // belly
set_slot(4, [20, 18, 14]); // eyes

// Body behind, head in front (the frog faces -Y), belly underneath.
fill_ellipsoid(2, 7, 2, 13, 19, 9, 2);
fill_ellipsoid(4, 1, 4, 11, 8, 10, 2);
fill_ellipsoid(4, 9, 2, 11, 17, 4, 3);
// Eyes on top of the head, one cell each.
fill_box(5, 3, 10, 5, 3, 10, 4);
fill_box(10, 3, 10, 10, 3, 10, 4);
// Legs one cell thin.
for x in [3, 12] {
    fill_box(x, 8, 0, x, 8, 2, 2);
    fill_box(x, 17, 0, x, 17, 2, 2);
}

let report = deform(0, 0, 3, 15, 9, 12, "scale", #{
    point: [7.5, 4.5, 7.5],
    factor: head.to_float() / 100.0,
    radius: radius,
    falloff: falloff,
    minFeature: 2,
});
if report.thin.count < 4 {
    throw "the four legs should be reported thin";
}
