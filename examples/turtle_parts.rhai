// A pond turtle blocked out in one piece, then cut into its rig parts.
// The body, head, flippers, and tail are filled together, as a
// carve_from_views blockout would come out, and the boxy shell is cut to a
// dome with mode intersect. recolor gives the underside its own slot with
// no shape change, surface_stroke draws the plate seams onto the curved
// shell, and split_object moves the cells inside boxes into one object per
// part. Each cell keeps its world position, so nothing is carved twice. On
// a rerun the parts exist: clear_object empties each one and the cut goes
// into it (into) instead of making more.
//
// Parameters
//   length   the shell's length in cells (along +Y, the head end)
//   width    the shell's width in cells
//   height   the shell's height in cells

let length = param_int("length", 28, 16, 64);
let width = param_int("width", 22, 12, 48);
let height = param_int("height", 9, 5, 24);

set_slot(2, [92, 104, 56]); // skin
set_slot(3, [104, 92, 50]); // shell
set_slot(4, [168, 150, 92]); // plastron
set_slot(5, "#3a321c"); // seam, as a concept sheet's hex code

let neck = length / 5;
let flipper = width / 3;
let x1 = width - 1;
let y1 = length - 1;

// The blockout: shell, head, four flippers, and a tail, all in one object.
// The shell starts as a box, as three flat views give it, and intersect
// keeps only its cells inside a dome sunk to the box's floor.
fill_box(0, 0, 0, x1, y1, height - 1, 3);
fill_ellipsoid(0, 0, -height, x1, y1, height - 1, 0, #{mode: "intersect"});
fill_ellipsoid(width / 2 - 3, y1 - 2, 1, width / 2 + 3, y1 + neck, 6, 2);
for side in [-1, 1] {
    let x = if side < 0 { -flipper } else { x1 };
    fill_ellipsoid(x, y1 - 9, 1, x + flipper, y1 - 3, 3, 2);
    fill_ellipsoid(x, 2, 1, x + flipper, 7, 3, 2);
}
fill_ellipsoid(width / 2 - 1, -4, 1, width / 2 + 1, 1, 3, 2);

// The plastron: the shell's lowest two layers take a paler slot, no shape change.
recolor(3, 4, #{x0: 0, y0: 0, z0: 0, x1: x1, y1: y1, z1: 1});

// Seams between the shell's plates: strokes drawn above the dome snap onto
// its surface, only on shell cells.
surface_stroke([[x1 / 2, 0, height], [x1 / 2, y1, height]], 5, #{only: [3]});
for row in 1..4 {
    let y = row * y1 / 4;
    surface_stroke([[0, y, height / 2], [x1 / 2, y, height + 2], [x1, y, height / 2]], 5,
        #{curve: "smooth", only: [3]});
}

// The cuts: everything past the shell's ends and sides, by part.
let parts = [
    ["head", [[0, y1 + 1, 0, x1, y1 + neck, height]]],
    ["flipper_l", [[-flipper, 0, 0, -1, y1, height]]],
    ["flipper_r", [[x1 + 1, 0, 0, x1 + flipper, y1, height]]],
    ["tail", [[0, -4, 0, x1, -1, height]]],
];
for part in parts {
    let name = part[0];
    if has_object(name) {
        // Empty the old part first, so a smaller turtle leaves nothing behind.
        clear_object(name);
        split_object(0, #{boxes: part[1], into: name});
    } else {
        split_object(0, #{boxes: part[1], name: name});
    }
}
