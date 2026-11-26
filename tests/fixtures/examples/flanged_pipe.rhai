// A standing iron pipe with a flange at its mouth and a valve wheel on its
// side, built from flat-capped tubes: a flat tube is an exact cylinder from
// its first point to its last, and the same tube in slot 0 with a smaller
// radius hollows it to a clean wall with square ends. Z is up.
//
// Parameters
//   height   pipe height in cells
//   radius   outer pipe radius
//   wall     wall thickness
//   flange   how far the flange stands out past the pipe

let height = param_int("height", 16, 4, 64);
let radius = param_float("radius", 6.0, 2.0, 20.0);
let wall = param_float("wall", 1.0, 1.0, 4.0);
let flange = param_float("flange", 2.0, 0.0, 6.0);

set_slot(3, [88, 92, 96]); // iron
set_slot(4, [60, 64, 68]); // dark iron
set_slot(5, [150, 40, 32]); // valve red

let h = to_float(height);
// The pipe, then hollowed through its whole length.
fill_tube([[0, 0, 0], [0, 0, h]], 3, #{radius: radius, caps: "flat"});
// The flange: a wider ring two cells tall at the mouth.
if flange > 0.0 {
    fill_tube([[0, 0, h - 1.0], [0, 0, h]], 4, #{radius: radius + flange, caps: "flat"});
}
fill_tube([[0, 0, 0], [0, 0, h]], 0, #{radius: radius - wall, caps: "flat"});

// The valve wheel: a stem, a one-cell disc rim hollowed to a ring, and two
// spokes across it.
let x = radius + 3.0;
let z = h / 2.0;
fill_tube([[radius - 0.5, 0, z], [x, 0, z]], 4, #{radius: 0.5, caps: "flat"});
fill_tube([[x, 0, z], [x + 0.5, 0, z]], 5, #{radius: 3.5, caps: "flat"});
fill_tube([[x, 0, z], [x + 0.5, 0, z]], 0, #{radius: 2.5, caps: "flat"});
fill_tube([[x, -3, z], [x, 3, z]], 5, #{radius: 0.5, caps: "flat"});
fill_tube([[x, 0, z - 3.0], [x, 0, z + 3.0]], 5, #{radius: 0.5, caps: "flat"});
