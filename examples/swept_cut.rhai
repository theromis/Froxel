// A block cut by sweeps with slot 0: one slopes its near side along a curved
// outline, one runs a round channel over its top. Z is up.
//
// Parameters
//   length   block length in cells
//   slope    how far the near side leans in over the block's height
//   channel  width of the channel on top, 0 for none

let length = param_int("length", 40, 16, 96);
let slope = param_float("slope", 6.0, 0.0, 12.0);
let channel = param_int("channel", 6, 0, 12);

let width = 24;
let height = 16;
set_slot(1, [168, 128, 84]);
fill_box(0, 0, 0, length - 1, width - 1, height - 1, 1);

// u runs to the left of the path, so walking along +X the profile's +u side
// is the block's inside. Everything between the slanted edge and the outside
// goes; the path's bow is the curved outline of the cut.
let l = to_float(length);
let h = to_float(height);
let outline = [[-2.0, 2.0, 0.0], [l / 2.0, 5.0, 0.0], [l + 2.0, 2.0, 0.0]];
let side = [[-12.0, -1.0], [0.0, -1.0], [slope, h + 1.0], [-12.0, h + 1.0]];
sweep(outline, side, 0, #{curve: "smooth"});

if channel > 0 {
    let w = to_float(width);
    let across = to_float(channel);
    let top = [[-2.0, w * 0.6, h - 2.0], [l / 2.0, w * 0.5, h - 2.0], [l + 2.0, w * 0.6, h - 2.0]];
    sweep(top, #{width: across, height: across, radius: across / 2.0 - 0.5}, 0, #{curve: "smooth"});
}
