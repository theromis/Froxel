// A blockout cleaned up the way an image-to-3D mesh needs after
// import_mesh: a mossy boulder that came in lying on its side, with a snail
// sitting on its crown and loose foam bits around its foot.
// orient_to_views turns it upright by matching its silhouettes against a
// views sheet (drawn here in rows), then components removes the floaters,
// cuts the snail off at its thinnest neck between a seed in the snail and
// an anchor in the boulder, and moves it into its own object. On a rerun
// the snail object exists: clear_object empties it and the cut goes into
// it (into) instead of making another.
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

// The generator's mesh, all in one object and lying on its side: up is +Y.
// A dome of stone with moss on top, a snail shell resting on the crown, and
// foam bits around it.
let r = radius;
fill_ellipsoid(-r, -height, -r, r, height, r, 2);
carve_box(-r - 1, -height - 1, -r - 1, r + 1, -1, r + 1);
fill_ellipsoid(-r, height - 3, -r, r, height + 1, r, 3, #{mode: "paint"});
let top = height + 3;
fill_ellipsoid(-3, top - 3, -2, 3, top + 3, 3, 4);
let bits = [];
let roll = rng(7);
for i in 0..foam {
    let a = roll.float(0.0, 2.0 * PI);
    let d = to_float(r) + roll.float(3.0, 6.0);
    let x = to_int(floor(d * cos(a) + 0.5));
    let z = to_int(floor(d * sin(a) + 0.5));
    bits.push([x, 0, z, 5]);
    if i % 3 == 0 {
        bits.push([x + 1, 0, z, 5]);
    }
}
set_voxels(bits);

// The views sheet of the boulder alone: a half ellipse from the front and
// the side, a circle from the top, one character per cell.
let dome = [];
for z in 0..height + 1 {
    let row = "";
    let k = to_float(height - z) / (to_float(height) + 0.5);
    for x in -r..r + 1 {
        let u = to_float(x) / (to_float(r) + 0.5);
        row += if u * u + k * k <= 1.0 { "#" } else { "." };
    }
    dome.push(row);
}
let disc = [];
for y in -r..r + 1 {
    let row = "";
    for x in -r..r + 1 {
        row += if x * x + y * y <= r * r { "#" } else { "." };
    }
    disc.push(row);
}
let turned = orient_to_views([
    #{view: "front", rows: dome},
    #{view: "left", rows: dome},
    #{view: "top", rows: disc},
]);

// The cleanup, on the boulder standing up.
components("remove_small", #{minCells: 12});
let lo = turned.bounds.min;
let hi = turned.bounds.max;
let cx = (lo[0] + hi[0]) / 2;
let cy = (lo[1] + hi[1]) / 2;
let rider = #{seed: [cx, cy, hi[2] - 2], anchor: [cx, cy, lo[2] + 2]};
if has_object("snail") {
    clear_object("snail");
    rider.into = "snail";
} else {
    rider.name = "snail";
}
components("split", rider);
