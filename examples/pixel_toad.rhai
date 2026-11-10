// A made-up squat toad carved at a small size from detailed views, through
// pixelate_views: the views are drawn at 56 to 64 pixels, many pixels a
// cell, with eyes, pupils, nostrils, spots, and legs three pixels wide. A
// plain carve at 16 cells long averages the pupils and nostrils away and
// drops the thin legs; pixelate_views first turns each view into pixel art
// at the cell grid (a small shared palette, each cell the colour holding
// most of its footprint, small salient features and thin parts kept), and
// its rows carve straight into the hull. Over MCP the views would be crops
// of a views sheet PNG and carve_from_views would take downscale instead.
//
// Parameters
//   length   the toad's length in cells (along y)
//   colors   clusters of the shared palette
//   keep     1 keeps small features and thin parts, 0 shows a plain vote

let length = param_int("length", 16, 10, 48);
let colors = param_int("colors", 4, 2, 8);
let keep = param_int("keep", 1, 0, 1);

set_slot(2, [210, 120, 42]); // skin
set_slot(3, [240, 220, 168]); // eye and belly
set_slot(4, [36, 22, 15]); // pupil and nostril
set_slot(5, [138, 74, 30]); // spot

// Is (x, y) inside the ellipse at (cx, cy) with radii rx and ry?
fn inside(x, y, cx, cy, rx, ry) {
    let dx = (x - cx) / rx;
    let dy = (y - cy) / ry;
    dx * dx + dy * dy <= 1.0
}

// The front: x across (56 pixels), z up (40 pixels, image down).
let front = [];
for y in 0..40 {
    let row = "";
    for x in 0..56 {
        let fx = x.to_float() + 0.5;
        let fy = y.to_float() + 0.5;
        let c = ".";
        let leg = (x >= 9 && x < 12) || (x >= 44 && x < 47);
        if leg && y >= 28 {
            c = "o";
        }
        if inside(fx, fy, 28.0, 24.0, 26.0, 11.0) {
            c = if inside(fx, fy, 28.0, 29.0, 14.0, 5.0) { "c" } else { "o" };
        }
        for ex in [17.0, 39.0] {
            if inside(fx, fy, ex, 12.0, 6.0, 6.0) {
                c = if inside(fx, fy, ex, 12.0, 2.0, 2.0) { "k" } else { "c" };
            }
        }
        if (x == 25 || x == 30) && y == 20 {
            c = "k";
        }
        row += c;
    }
    front.push(row);
}

// The side (left, from +X): y across (64 pixels, the head on the image's
// left), z up.
let side = [];
for y in 0..40 {
    let row = "";
    for x in 0..64 {
        let fx = x.to_float() + 0.5;
        let fy = y.to_float() + 0.5;
        let c = ".";
        if ((x >= 14 && x < 17) || (x >= 46 && x < 49)) && y >= 28 {
            c = "o";
        }
        if inside(fx, fy, 32.0, 24.0, 30.0, 11.0) {
            c = if inside(fx, fy, 20.0, 31.0, 14.0, 4.0) { "c" } else { "o" };
            if inside(fx, fy, 40.0, 20.0, 3.0, 2.5) || inside(fx, fy, 50.0, 26.0, 3.0, 2.5) {
                c = "s";
            }
        }
        if inside(fx, fy, 14.0, 12.0, 6.0, 6.0) {
            c = if inside(fx, fy, 11.0, 12.0, 2.0, 2.0) { "k" } else { "c" };
        }
        row += c;
    }
    side.push(row);
}

// The top: x across, y down the image toward the tail (64 pixels).
let top = [];
for y in 0..64 {
    let row = "";
    for x in 0..56 {
        let fx = x.to_float() + 0.5;
        let fy = y.to_float() + 0.5;
        let c = ".";
        if inside(fx, fy, 28.0, 32.0, 26.0, 30.0) {
            c = "o";
            if inside(fx, fy, 20.0, 36.0, 3.0, 3.0) || inside(fx, fy, 36.0, 44.0, 3.0, 3.0) {
                c = "s";
            }
        }
        for ex in [17.0, 39.0] {
            if inside(fx, fy, ex, 14.0, 6.0, 6.0) {
                c = "c";
            }
        }
        row += c;
    }
    top.push(row);
}

let legend = #{o: "#d2782a", c: "#f0dca8", k: "#24160f", s: "#8a4a1e"};
let views = [
    #{view: "front", rows: front, legend: legend},
    #{view: "left", rows: side, legend: legend},
    #{view: "top", rows: top, legend: legend},
];
let opts = #{size: [(), length, ()], colors: colors};
if keep == 0 {
    opts.features = false;
    opts.thin = false;
}
let px = pixelate_views(views, opts);
carve_from_views(px.views, #{size: px.size, slots: [2, 3, 4, 5]});
if keep == 1 && px.views[0].features.len() == 0 {
    throw "the front kept no feature";
}
