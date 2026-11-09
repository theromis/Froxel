// A pond turtle blocked out in one piece, then cut into its rig parts.
// The body, head, flippers, and tail are filled together, as a
// carve_from_views blockout would come out, and split_object moves the
// cells inside boxes into one object per part. Each cell keeps its world
// position, so nothing is carved twice. On a rerun the parts exist, and the
// cut goes into them (into) instead of making more.
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

let neck = length / 5;
let flipper = width / 3;
let x1 = width - 1;
let y1 = length - 1;

// The blockout: shell, head, four flippers, and a tail, all in one object.
fill_ellipsoid(0, 0, 0, x1, y1, height - 1, 3);
fill_ellipsoid(width / 2 - 3, y1 - 2, 1, width / 2 + 3, y1 + neck, 6, 2);
for side in [-1, 1] {
    let x = if side < 0 { -flipper } else { x1 };
    fill_ellipsoid(x, y1 - 9, 1, x + flipper, y1 - 3, 3, 2);
    fill_ellipsoid(x, 2, 1, x + flipper, 7, 3, 2);
}
fill_ellipsoid(width / 2 - 1, -4, 1, width / 2 + 1, 1, 3, 2);

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
        split_object(0, #{boxes: part[1], into: name});
    } else {
        split_object(0, #{boxes: part[1], name: name});
    }
}
