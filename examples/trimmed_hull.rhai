// A hollow boat hull with straight sides, narrowed toward its keel in one call:
// the plan outline and a [z, inset] table say where the surface stands, and
// trim_outline removes the rest and seals the cut with a new skin. Z is up.
//
// Parameters
//   length   hull length in cells
//   beam     hull width in cells
//   tumble   how far the sides move in from the rim to the keel
//   skin     skin thickness in cells

let length = param_int("length", 60, 24, 120);
let beam = param_int("beam", 24, 12, 48);
let tumble = param_float("tumble", 7.0, 0.0, 10.0);
let skin = param_int("skin", 1, 1, 4);

let height = 16;
set_slot(1, [132, 92, 58]); // the old planks, and the block's inside
set_slot(2, [92, 62, 40]); // the new skin
set_slot(3, [214, 196, 160]); // the deck

// A block, hollowed from above: straight sides two cells thick and a floor.
fill_box(0, 0, 0, length - 1, beam - 1, height - 1, 1);
carve_box(2, 2, 2, length - 3, beam - 3, height - 1);
fill_box(2, 2, height - 4, length - 3, beam - 3, height - 4, 3);

// The plan: a pointed bow and stern. Whole numbers are cell centres.
let l = to_float(length - 1);
let b = to_float(beam - 1);
let outline = [
    [0.0, b / 2.0],
    [l * 0.25, 0.0],
    [l * 0.75, 0.0],
    [l, b / 2.0],
    [l * 0.75, b],
    [l * 0.25, b],
];

// The sides stand at the outline from the rim down to z 9, then come in to
// the keel. The deck stays in its own slot and is cut with the rest.
let cut = trim_outline(outline, [[0, tumble], [9, 0]], #{curve: "smooth", skin: 2, thickness: skin});
if cut.removed == 0 { throw "the trim removed nothing"; }
