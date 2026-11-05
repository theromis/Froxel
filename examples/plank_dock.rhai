// A small dock of separate boards on posts and beams, with a hatch left in
// the deck. planks lays the boards: real gaps down to a dirt cell, end joints
// staggered between neighbouring rows, a tone per board, ragged ends, and a
// few dark nails and knots, so it never reads as painted stripes. Z is up;
// the boards run along +X.
//
// Parameters
//   length   deck length in cells
//   width    deck width in cells
//   ragged   chance a cell across a board's end is cut back, 0..1
//   seed     the boards

let length = param_int("length", 72, 24, 128);
let width = param_int("width", 32, 12, 64);
let ragged = param_float("ragged", 0.3, 0.0, 1.0);
let seed = param_int("seed", 3, 0, 9999);

set_slot(1, [62, 44, 30]); // posts and beams
set_slot(2, [120, 88, 52]); // the hatch
set_slot(3, [138, 100, 62]); // board
set_slot(4, [122, 87, 53]); // board, a darker one
set_slot(5, [152, 113, 72]); // board, a lighter one
set_slot(6, [36, 26, 19]); // dirt in the gaps
set_slot(7, [46, 32, 24]); // nails and knots

let deck = 14; // the top layer of the boards

// Posts, then beams across the boards to carry them, under the dirt layer.
for x in [4, length / 2 - 2, length - 8] {
    for y in [2, width - 6] {
        fill_box(x, y, 0, x + 3, y + 3, deck - 3, 1);
    }
    fill_box(x, 0, deck - 4, x + 3, width - 1, deck - 3, 1);
}

// A hatch set in the deck stays whole: planks leaves its box alone.
let hx = length / 2 + 6;
let hy = width / 2 - 5;
let hatch = [hx, hy, deck - 2, hx + 11, hy + 9, deck];
fill_box(hx, hy, deck - 1, hx + 11, hy + 9, deck, 2);

let laid = planks(0, 0, length - 1, width - 1, deck, "x", [3, 4, 5], #{
    dirt: 6, dark: 7, width: [3, 6], length: [10, 24], thickness: 2,
    ragged: ragged, nails: 0.3, knots: 0.2, exclude: hatch, seed: seed
});
if laid.boards < 2 { throw "no boards"; }
