// A tree stump carved from three views, the way a concept's turnaround
// sheet gives them: the front and side outlines flare into roots at the
// bottom, and the top view is the cut face with its growth rings and the
// roots spreading out. carve_from_views keeps the cells every view covers,
// and each cell takes the colour of the view whose surface is nearest it,
// so the sides come out bark and the top shows the rings. Over MCP the
// views are crops of a concept PNG instead of rows (docs/hand-sculpting.md).
//
// Parameters
//   height   trunk height in cells above the root flare
//   hollow   wall thickness of a hollow stump; 0 is solid
//   smooth   smooth passes over the carved form

let height = param_int("height", 8, 2, 24);
let hollow = param_int("hollow", 0, 0, 4);
let smooth = param_int("smooth", 1, 0, 3);

set_slot(3, [92, 64, 40]); // bark
set_slot(5, [196, 160, 104]); // sapwood
set_slot(6, [150, 112, 70]); // growth ring

// Front and side: the trunk, then four rows of root flare.
let outline = [];
for row in 0..height {
    outline.push("...bbbbbbbbbb...");
}
outline.push("..bbbbbbbbbbbb..");
outline.push(".bbbbbbbbbbbbbb.");
outline.push("bbbbbbbbbbbbbbbb");
outline.push("bbbbbbbbbbbbbbbb");

let top = [
    "......bbbb......",
    "......bbbb......",
    "......bbbb......",
    ".....bbbbbb.....",
    "....bbwwwwbb....",
    "...bbwrwwrwbb...",
    "bbbbwrwwwwrwbbbb",
    "bbbbwwwrrwwwbbbb",
    "bbbbwwwrrwwwbbbb",
    "bbbbwrwwwwrwbbbb",
    "...bbwrwwrwbb...",
    "....bbwwwwbb....",
    ".....bbbbbb.....",
    "......bbbb......",
    "......bbbb......",
    "......bbbb......",
];

let legend = #{b: 3, w: 5, r: 6};
let opts = #{cellsPerPixel: 1, smooth: smooth};
if hollow > 0 {
    opts.shell = hollow;
}
let done = carve_from_views([
    #{view: "front", rows: outline, legend: legend},
    #{view: "side", rows: outline, legend: legend},
    #{view: "top", rows: top, legend: legend},
], opts);

// A hollow stump is open at the top, inside the trunk's bark wall (the
// trunk is 3 cells in from the roots' reach on each side).
if hollow > 0 {
    let b = done.bounds;
    let inset = 3 + hollow;
    carve_box(b.min[0] + inset, b.min[1] + inset, b.max[2] - 1,
              b.max[0] - inset, b.max[1] - inset, b.max[2]);
}
