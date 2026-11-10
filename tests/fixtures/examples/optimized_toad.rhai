// A made-up squat toad 20 cells long, carved from its pixel-art views and
// then optimised to them with optimize_to_views. The carve is the views'
// visual hull: a box where no view sees it, its colours from one view per
// cell, so the edges the front and top views share streak. The solver
// stretches the pixel-art views over the object's box, one pixel a column
// of cells, and iterates the shape (excess columns cleared, missing rays filled, within
// a band of the surface, connected, legs kept two cells thick) and the
// surface colours (each cell from the views that see it, weighted by how
// squarely they face it, smoothed into patches, the pupils kept as
// accents) until the views agree. Over MCP the views would be crops of a
// views sheet PNG, pixelated at the object's box by the same call.
//
// Parameters
//   length      the toad's length in cells (along y)
//   optimize    1 optimises the carve, 0 leaves the carve as it is
//   min_feature legs and toes are never cut thinner than this

let length = param_int("length", 20, 12, 48);
let optimize = param_int("optimize", 1, 0, 1);
let min_feature = param_int("min_feature", 2, 1, 4);

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

// The top: x across, image down toward the head (-Y), so the eyes are near
// the bottom of the image.
let top = [];
for y in 0..64 {
    let row = "";
    for x in 0..56 {
        let fx = x.to_float() + 0.5;
        let fy = y.to_float() + 0.5;
        let c = ".";
        if inside(fx, fy, 28.0, 32.0, 26.0, 30.0) {
            c = "o";
            if inside(fx, fy, 20.0, 28.0, 3.0, 3.0) || inside(fx, fy, 36.0, 20.0, 3.0, 3.0) {
                c = "s";
            }
        }
        for ex in [17.0, 39.0] {
            if inside(fx, fy, ex, 50.0, 6.0, 6.0) {
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
let px = pixelate_views(views, #{size: [(), length, ()], colors: 4});
carve_from_views(px.views, #{size: px.size});
if optimize == 1 {
    let r = optimize_to_views(px.views, #{minFeature: min_feature});
    if r.agreement.after < r.agreement.before {
        throw "the views agree less after optimising";
    }
}
