// A made-up four-legged critter blocked out in one piece, then cut into
// its rig parts with ONE split_parts call instead of a split_object box per
// part: each cell goes to the part whose seed is nearest through the solid,
// so a leg ends where it meets the body and the jaw stops at the mouth.
// The jaw keeps the side of a plane below the mouth. Every part becomes its
// own object at world [0, 0, 0], ready for add_bone and bind_object; the
// body keeps this object's name, so the script stays on it.
//
// Parameters
//   length   the body's length in cells
//   legs     the legs' length in cells

let length = param_int("length", 26, 16, 64);
let legs = param_int("legs", 7, 3, 20);

set_slot(2, [96, 140, 60]); // skin
set_slot(3, [200, 190, 120]); // belly
set_slot(4, [30, 26, 20]); // eye

let w = 12;
let top = legs + 10;
// Body, head with a jaw under a mouth slit, tail, and four legs.
fill_ellipsoid(0, 0, legs, w - 1, length - 1, top, 2);
fill_ellipsoid(1, -9, legs + 4, w - 2, 2, top + 3, 2);
fill_box(2, -8, legs + 2, w - 3, -1, legs + 5, 3);
carve_box(1, -10, legs + 6, w - 2, -5, legs + 6);
fill_tube([[6, length - 2, legs + 4, 3.0], [6, length + 8, legs + 2, 1.5]], 2);
for corner in [[2, 4], [w - 3, 4], [2, length - 6], [w - 3, length - 6]] {
    fill_box(corner[0] - 1, corner[1] - 1, 0, corner[0] + 1, corner[1] + 1, legs + 1, 2);
}
set(3, -9, top, 4);
set(w - 4, -9, top, 4);

split_parts("critter", [
    #{name: "critter", point: [6, length / 2, top - 2]},
    #{name: "head", point: [6, -5, top]},
    #{name: "jaw", point: [6, -6, legs + 3], planes: [#{normal: [0, 0, -1], at: [0, 0, legs + 5]}]},
    #{name: "tail", point: [6, length + 6, legs + 2]},
    #{name: "leg_fl", point: [2, 4, 0]},
    #{name: "leg_fr", point: [w - 3, 4, 0]},
    #{name: "leg_bl", point: [2, length - 6, 0]},
    #{name: "leg_br", point: [w - 3, length - 6, 0]},
]);
