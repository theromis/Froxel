// A gorge cliff wall: a fractured limestone face swept along a curve, with
// real joints, chipped edges and pits, moss caps and curtains, algae at the
// waterline, and vines with yellow blooms. Markers give the game a ledge,
// the wall top, collision boxes, and the two joining ends. Z is up; the
// wall runs along +X and its cliff face looks toward +Y.
//
// Parameters
//   length   wall length in cells
//   height   wall height in cells
//   water    waterline height; algae grows below it
//   seed     joints, weathering, and plants

let length = param_int("length", 72, 32, 160);
let height = param_int("height", 32, 14, 64);
let water = param_int("water", 6, 0, 20);
let seed = param_int("seed", 5, 0, 9999);

set_slot(1, [176, 148, 118]); // stone
set_slot(2, [158, 133, 106]); // broken stone, 0.9 x
set_slot(3, [66, 52, 40]); // joint dirt
set_slot(4, [181, 151, 119]); // stone noise, warmer
set_slot(5, [171, 146, 120]); // stone noise, cooler
set_slot(6, [173, 145, 113]); // stone noise, darker
set_slot(7, [92, 84, 70]); // gorge floor gravel
set_slot(10, [88, 122, 52]); // moss
set_slot(11, [104, 140, 60]);
set_slot(12, [120, 156, 70]);
set_slot(13, [88, 126, 54]); // algae, bottom to top
set_slot(14, [98, 140, 60]);
set_slot(15, [128, 170, 90]);
set_slot(16, [70, 110, 40]); // vine, anchor to tip
set_slot(17, [82, 124, 46]);
set_slot(18, [96, 138, 54]);
set_slot(19, [230, 200, 60]); // bloom
// Vines sway from the anchor to the tip; stone, moss, and algae stay still.
set_motion(16, "sway", 0.15);
set_motion(17, "sway", 0.5);
set_motion(18, "sway", 1.0);
set_motion(19, "sway", 0.3);

let l = length;
let h = to_float(height);
let mid = to_int(h * 0.55);

// The wall's path bends gently, and its profile has a batter, a ledge at
// mid height, and a narrower crown. u points to the cliff side (+Y).
let path = [[0, 0, 0], [l / 3, 8, 0], [2 * l / 3, -4, 0], [l, 4, 0]];
let profile = [
    [-6.0, 0.0], [7.0, 0.0], [6.0, h * 0.55], [9.0, h * 0.55],
    [9.0, h * 0.55 + 2.0], [5.0, h * 0.55 + 2.0], [3.5, h], [-4.0, h]
];
sweep(path, profile, 1, #{curve: "smooth"});

// Work on a box a cell larger than the wall, so plants can grow on it.
let x0 = -12; let y0 = -16; let x1 = l + 12; let y1 = 24;
joints(x0, y0, 0, x1, y1, height + 1, "columns",
    #{spacing: [6, 13], course: [12, 28], dirt: 3, depth: 2, only: [1], seed: seed});
weather(x0, y0, 0, x1, y1, height + 1,
    #{chip: 0.12, pits: 0.015, broken: 2, noise: [4, 5, 6], noiseDensity: 0.3, only: [1], seed: seed});
// The gorge floor, after the stone tools so it is not jointed; strands stop on it.
fill_box(x0, y0, -2, x1, y1, -1, 7, #{mode: "empty"});
let stone = [1, 2, 4, 5, 6];
moss(x0, y0, -1, x1, y1, height + 2, [10, 11, 12],
    #{only: stone, amount: 0.45, curtains: 0.3, length: [3, 10], seed: seed});
algae(x0, y0, 0, x1, y1, height, [13, 14, 15], #{band: [0, water], only: stone, seed: seed});
vines(x0, y0, 0, x1, y1, height + 2, [16, 17, 18],
    #{blooms: [19], amount: 0.14, bloomChance: 0.1, facing: [0, 1, 0], length: [6, height / 2], seed: seed});

// Game markers, in mesh units.
marker("ledge_mid", to_float(l / 3), 16.5, to_float(mid + 2));
marker("top_west", 0.5, 0.5, h);
marker("top_east", to_float(l) + 0.5, 4.5, h);
let spans = 4;
for i in 0..spans {
    let a = i * l / spans;
    let b = (i + 1) * l / spans - 1;
    collision_box(a, -8, 0, b, 17, height - 1);
}
connector("west_end", [0.0, 0.5, 0.0], [-1.0, 0.0, 0.0]);
connector("east_end", [to_float(l) + 1.0, 4.5, 0.0], [1.0, 0.0, 0.0]);
