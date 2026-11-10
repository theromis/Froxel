// A beetle: a boxy blockout, then fit_to_views refines it to its front,
// side, and top views and colours it from four slots. Each view pixel is
// one column of cells (orthographic rays): rays that should be background
// are cleared, rays the views ink but the blockout misses are filled with
// cells every other view sees inside, rough single cells are tidied, and
// every cell a view sees takes the slot nearest the colours of the views
// that see it. Over MCP the views are crops of an approved views sheet
// instead of rows, and the blockout comes from carve_from_views or
// import_mesh (docs/hand-sculpting.md).
//
// Parameters
//   iterations   most shape sweeps; 0 only recolours
//   smooth       smoothWeight, the cost of each exposed cell face

let iterations = param_int("iterations", 8, 0, 64);
let smooth = param_float("smooth", 0.1, 0.0, 1.0);

set_slot(1, [128, 128, 128]); // blockout
set_slot(3, [40, 92, 60]); // shell
set_slot(4, [214, 176, 60]); // stripe
set_slot(5, [30, 30, 34]); // head
set_slot(6, [240, 236, 220]); // eye

// The views, one character a cell: 12 cells across (x), 16 long (y), and
// 6 tall (z). The beetle faces -Y: the top view's bottom rows and the side
// view's left columns are the head.
let w = 12;
let l = 16;
let h = 6;
let top = [];
for row in 0..l {
    let line = "";
    for col in 0..w {
        let x = col.to_float() + 0.5 - w.to_float() / 2.0;
        let y = row.to_float() + 0.5 - 7.0;
        let body = row < 14 && (x * x) / 36.0 + (y * y) / 49.0 <= 1.0;
        let head = row >= 14 && col >= 4 && col < 8;
        line += if head { "h" } else if body && (col == 5 || col == 6) { "d" } else if body { "s" } else { "." };
    }
    top.push(line);
}
let front = [];
let side = [];
for row in 0..h {
    // Row 0 is the top: the dome narrows upward.
    let half = 2 + row;
    let line = "";
    for col in 0..w {
        let x = col - w / 2;
        let inside = x >= -half && x < half;
        let face = row >= 3 && col >= 4 && col < 8;
        let eye = row == 3 && (col == 4 || col == 7);
        line += if eye { "e" } else if face { "h" } else if inside { "s" } else { "." };
    }
    front.push(line);
    let line = "";
    for col in 0..l {
        let head = col < 2 && row >= 3;
        let body = col >= 2 && col < 16 && row >= 2 - min(col - 2, 15 - col) / 2;
        line += if head { "h" } else if body { "s" } else { "." };
    }
    side.push(line);
}

// A boxy blockout over the whole extent.
fill_rounded_box(0, 0, 0, w - 1, l - 1, h - 1, 1, 1);

let legend = #{s: 3, d: 4, h: 5, e: 6};
let done = fit_to_views([
    #{view: "front", rows: front, legend: legend},
    #{view: "side", rows: side, legend: legend},
    #{view: "top", rows: top, legend: legend},
], #{slots: [3, 4, 5, 6], iterations: iterations, smoothWeight: smooth});
for view in done.views {
    if view.after.iou < view.before.iou {
        throw `fit_to_views made the ${view.view} view worse`;
    }
}
