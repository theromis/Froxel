// A turtle shell: a dome, then its plate pattern painted from ONE top
// view with project_paint. The pattern is the top view of a concept: plates
// with dark seams between them and a row of lighter scutes down the spine.
// Each ray from above recolours the first cell it meets (depth more cells
// below it), and normalLimit leaves the steep rim alone, so the sides keep
// the dome's own colour instead of streaks. No cell is added or removed.
// Over MCP the pattern is a crop of the concept sheet instead of rows
// (docs/hand-sculpting.md).
//
// Parameters
//   width, length   the shell in cells
//   height          the dome's height in cells
//   plate           plate size in cells
//   limit           normalLimit in degrees; 90 paints the rim too
//   depth           cells painted into the surface

let width = param_int("width", 22, 8, 64);
let length = param_int("length", 28, 8, 64);
let height = param_int("height", 8, 3, 32);
let plate = param_int("plate", 6, 3, 16);
let limit = param_float("limit", 60.0, 0.0, 90.0);
let depth = param_int("depth", 1, 1, 4);

set_slot(2, [70, 58, 36]); // rim
set_slot(3, [118, 104, 54]); // plate
set_slot(4, [46, 38, 24]); // seam
set_slot(5, [150, 132, 70]); // spine scute

// The dome: the top half of an ellipsoid.
fill_ellipsoid(0, 0, -height, width - 1, length - 1, height, 2);
carve_box(0, 0, -height, width - 1, length - 1, -1);

// The top view, one character a cell: image down is -Y, so row 0 is the
// head end (the largest y).
let spine = width / 2;
let rows = [];
for row in 0..length {
    let line = "";
    for col in 0..width {
        let shift = if (col / plate) % 2 == 0 { 0 } else { plate / 2 };
        let seam = col % plate == 0 || (row + shift) % plate == 0;
        let scute = col >= spine - plate / 3 && col <= spine + plate / 3;
        line += if seam { "d" } else if scute { "v" } else { "p" };
    }
    rows.push(line);
}

let painted = project_paint("top", rows, #{
    legend: #{p: 3, d: 4, v: 5},
    normalLimit: limit,
    depth: depth,
});
if painted.cells == 0 {
    throw "the pattern missed the dome";
}
