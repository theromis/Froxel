// Three rough blocks stacked off-centre, to look at baked light: crevices
// between them, overhangs, and a lit and a shaded side.
//
//   froxel export --input examples/stacked_blocks.vox --output blocks.glb \
//     --color-atlas --bake '{"preset": "old_pond", "distance": 12}'
//   froxel preview --input blocks.glb --out blocks.png
//
// Parameters
//   seed     the chips and the colour noise

let seed = param_int("seed", 3, 0, 9999);

set_slot(1, [142, 136, 124]);
set_slot(2, [128, 122, 112]);
set_slot(3, [156, 150, 138]);

fill_rounded_box(-14, -10, 0, 13, 9, 9, 3, 1);
fill_rounded_box(-8, -7, 10, 9, 8, 18, 3, 2);
fill_rounded_box(-11, -4, 19, 2, 5, 26, 2, 3);
weather(-16, -12, 0, 15, 11, 27, #{chip: 0.3, chipSize: 2, pits: 0.04, noise: [1, 2, 3], noiseDensity: 0.25, seed: seed});
