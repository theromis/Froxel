// A blockout cleaned up the way an image-to-3D mesh needs after
// import_mesh: a mossy boulder that came with a snail sitting on its top
// and loose foam bits around its foot. components removes the floaters,
// then cuts the snail off at its thinnest neck between a seed in the snail
// and an anchor in the boulder, and moves it into its own object. On a
// rerun the snail object exists: clear_object empties it and the cut goes
// into it (into) instead of making another.
//
// Parameters
//   radius   the boulder's radius in cells
//   height   the boulder's height in cells
//   foam     how many loose foam bits lie around its foot

let radius = param_int("radius", 12, 8, 32);
let height = param_int("height", 9, 5, 24);
let foam = param_int("foam", 9, 0, 40);

set_slot(2, [112, 108, 96]); // stone
set_slot(3, [86, 112, 52]); // moss
set_slot(4, [150, 112, 70]); // snail shell
set_slot(5, [236, 240, 236]); // foam

// The generator's mesh, all in one object: a dome of stone with moss on
// top, a snail shell resting on the crown, and foam bits around it.
let r = radius;
fill_ellipsoid(-r, -r, -height, r, r, height, 2);
carve_box(-r - 1, -r - 1, -height - 1, r + 1, r + 1, -1);
fill_ellipsoid(-r, -r, height - 3, r, r, height + 1, 3, #{mode: "paint"});
let top = height + 3;
fill_ellipsoid(-3, -2, top - 3, 3, 3, top + 3, 4);
let bits = [];
let roll = rng(7);
for i in 0..foam {
    let a = roll.float(0.0, 2.0 * PI);
    let d = to_float(r) + roll.float(3.0, 6.0);
    let x = to_int(floor(d * cos(a) + 0.5));
    let y = to_int(floor(d * sin(a) + 0.5));
    bits.push([x, y, 0, 5]);
    if i % 3 == 0 {
        bits.push([x + 1, y, 0, 5]);
    }
}
set_voxels(bits);

// The cleanup.
let found = components("list");
components("remove_small", #{minCells: 12});
let rider = #{seed: [0, 0, top + 1], anchor: [0, 0, 2]};
if has_object("snail") {
    clear_object("snail");
    rider.into = "snail";
} else {
    rider.name = "snail";
}
components("split", rider);
