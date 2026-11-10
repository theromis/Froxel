// A made-up rounded crate painted from three views in one paint_from_views
// call: the front shows a hatch, the side a band of planks, the top a lid
// with a cross brace. Every surface cell takes its colour from the views
// that see it, by the view its normal faces most, blended at the rounded
// edges; a missing right view is the left one mirrored. Over MCP the views
// would be crops of a views sheet PNG; a script passes rows.
//
// Parameters
//   size     the crate's width and depth in cells
//   height   its height in cells
//   seam     degrees around a view boundary where two views blend
//   fresh    1 to quantise into 5 new slots instead of the slots below

let size = param_int("size", 24, 12, 64);
let height = param_int("height", 18, 8, 64);
let seam = param_int("seam", 10, 0, 45);
let fresh = param_int("fresh", 0, 0, 1);

set_slot(2, [150, 104, 60]); // wood
set_slot(3, [104, 70, 40]); // dark plank
set_slot(4, [60, 40, 24]); // hatch and brace
set_slot(5, [196, 160, 96]); // lid

fill_rounded_box(0, 0, 0, size - 1, size - 1, height - 1, 3, 2);

// 12 x 9 pictures, stretched over the crate as each view sees it.
let w = "wwwwwwwwwwww";
let front = [w, w, "wwwhhhhhhwww", "wwwhwwwwhwww", "wwwhwwwwhwww", "wwwhwwwwhwww", "wwwhhhhhhwww", w, w];
let side = [w, "dddddddddddd", w, w, "dddddddddddd", w, w, "dddddddddddd", w];
let top = ["llllllllllll", "bllllllllllb", "lbllllllllbl", "llbllllllbll", "lllbllllblll", "llllbllbllll", "lllbllllblll", "llbllllllbll", "lbllllllllbl", "bllllllllllb", "llllllllllll"];
let legend = #{w: "#966838", d: "#684628", h: "#3c2818", l: "#c4a060", b: "#3c2818"};
let views = [
    #{view: "front", rows: front, legend: legend},
    #{view: "left", rows: side, legend: legend},
    #{view: "top", rows: top, legend: legend},
];
let opts = if fresh == 1 { #{newSlots: 5, seam: seam} } else { #{slots: [2, 3, 4, 5], seam: seam} };
let report = paint_from_views(views, opts);
// The back and the bottom have no view: they keep the wood slot.
if report.unseen * 2 > report.surface {
    throw "most of the crate is seen by no view";
}
