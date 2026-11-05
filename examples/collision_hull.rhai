// A small wrecked hull whose colliders come from its cells: a walkable deck
// with a hatch, a hollow hold under it, a hole through one side, a mast, and
// vines the collision ignores. Rerun it after any change to the hull and the
// boxes follow. Z is up; the keel runs along +X.
//
// Parameters
//   length     hull length in cells
//   hatch      hatch width in cells
//   hole_size  the smallest opening the boxes keep open
//   max_boxes  most boxes; 0 for no limit

let length = param_int("length", 48, 28, 96);
let hatch = param_int("hatch", 8, 4, 12);
let hole_size = param_int("hole_size", 3, 1, 8);
let max_boxes = param_int("max_boxes", 0, 0, 256);

set_slot(1, [106, 72, 44]); // hull planks
set_slot(2, [140, 104, 66]); // deck
set_slot(3, [84, 60, 38]); // mast
set_slot(4, [84, 130, 52]); // vines
set_motion(4, "sway", 0.6);

let half = 10;
let top = 16;
let l = length - 1;

// The hull narrows toward the keel, so its outside steps in.
for z in 0..top {
    let narrow = (top - z) / 4;
    fill_box(narrow, narrow - half, z, l - narrow, half - 1 - narrow, z, 1);
}
// The hold is two cells inside the hull all round.
for z in 2..top {
    let narrow = (top - z) / 4 + 2;
    carve_box(narrow, narrow - half, z, l - narrow, half - 1 - narrow, z);
}
// The deck, one cell thick, with the hatch through it.
fill_box(2, 2 - half, top - 2, l - 2, half - 3, top - 2, 2);
let mid = length / 2;
carve_box(mid - hatch / 2, -hatch / 2, top - 2, mid + hatch / 2 - 1, hatch / 2 - 1, top - 2);
// A hole through the side, above the waterline.
carve_box(8, half - 3, 7, 13, half - 1, 11);
// The mast stands on the deck.
fill_box(6, -1, top - 1, 8, 1, top + 19, 3);
// Vines hang over the side; they are one cell thick.
for i in 0..4 {
    fill_box(14 + i * 7, -half - 1, 3 + i, 14 + i * 7, -half - 1, top, 4);
}

// The last step, once the cells are final: boxes for everything but the vines.
let opts = #{prefix: "hull", holeSize: hole_size, exclude: [4]};
if max_boxes > 0 {
    opts.maxBoxes = max_boxes;
}
let cover = collision_from_voxels(opts);
marker("hull_covered_percent", cover.coveredPercent, 0, 0);
