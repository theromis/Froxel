// A round creature head carved from three views WITH a depth map each, the
// way a views sheet and its depth pass give them: the front, side, and top
// silhouettes alone leave a box with rounded edges (three axis views cannot
// see the diagonal corners), and each view's depth map carves away the cells
// in front of its surface, so the head comes out round. back mirrors each
// map for the far side the three views do not see. The maps here are
// computed, as an image generator's depth pass or Depth Anything V2 would
// give them: grey 1 nearest, falling to 0.3 at the rim, relative, so the
// default calibrate "hull" scales them to the silhouettes. Over MCP the maps
// are crops of a depth PNG instead of values (depthPath, docs/mcp.md).
//
// Parameters
//   radius   the head's radius in cells
//   depth    1 carves with the depth maps, 0 keeps the silhouette hull
//   squash   height over width, in percent

let radius = param_int("radius", 8, 3, 20);
let use_depth = param_int("depth", 1, 0, 1);
let squash = param_int("squash", 80, 50, 100);

set_slot(3, [96, 140, 70]); // skin
set_slot(4, [24, 24, 20]); // eye
set_slot(5, [200, 210, 150]); // chin

let n = 2 * radius;
let height = n * squash / 100;
let rz = height / 2.0;
let r = radius;

// One view of the head: its rows (skin, with the front's eyes and the
// chin seen from below) and its depth map, both n wide.
fn view(name, n, down, r, rd, eyes, use_depth) {
    let rows = [];
    let values = [];
    for row in 0..down {
        let line = "";
        let grey = [];
        for col in 0..n {
            let a = (col + 0.5 - n / 2.0) / r;
            let b = (row + 0.5 - down / 2.0) / rd;
            let q = a * a + b * b;
            if q >= 1.0 {
                line += ".";
                grey.push(());
                continue;
            }
            let eye = eyes && row == down / 3 && (col == n / 3 || col == n - 1 - n / 3);
            line += if eye { "e" } else if eyes && row > down * 3 / 4 { "c" } else { "s" };
            // Nearest at the middle, 0.3 at the rim: affine in true depth.
            grey.push(0.3 + 0.7 * sqrt(1.0 - q));
        }
        rows.push(line);
        values.push(grey);
    }
    let out = #{view: name, rows: rows, legend: #{s: 3, e: 4, c: 5}};
    if use_depth == 1 {
        out.depth = #{values: values, back: true};
    }
    out
}

let views = [
    view("front", n, height, r, rz, true, use_depth),
    view("left", n, height, r, rz, false, use_depth),
    view("top", n, n, r, r, false, use_depth),
];
carve_from_views(views, #{cellsPerPixel: 1, slots: [3, 4, 5], depthTolerance: 0.25});
