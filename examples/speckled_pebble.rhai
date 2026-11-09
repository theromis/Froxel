// A made-up pebble coloured the way an image-to-3D import comes out: a
// dozen slots a shade apart, scattered cell by cell, with a few lone
// specks and a dark spot that should stay. reduce_palette clusters the
// colours into three in Oklab, keeps the dark spot as an accent, and
// despeckles the surface so the lone specks take their neighbours' slot.
//
// Parameters
//   size     the pebble's length in cells
//   colors   clusters the noise is reduced to
//   passes   despeckle passes
//   seed     the noise

let size = param_int("size", 28, 12, 96);
let colors = param_int("colors", 3, 1, 12);
let passes = param_int("passes", 2, 0, 8);
let seed = param_int("seed", 5, 0, 9999);

// Twelve noisy greys and ochres, a dark spot, and a blue speck colour.
for i in 0..6 {
    set_slot(10 + i, [118 + i * 4, 112 + i * 3, 98 + i * 2]);
    set_slot(16 + i, [172 + i * 3, 140 + i * 2, 76 + i]);
}
set_slot(30, [26, 22, 20]);
set_slot(31, [40, 80, 200]);

let w = size;
let d = size * 3 / 4;
let h = size / 2;
fill_ellipsoid(0, 0, 0, w - 1, d - 1, h - 1, 10);
let r = rng(seed);
for x in 0..w {
    for y in 0..d {
        for z in 0..h {
            if get(x, y, z) != 0 {
                // Ochre on the upper half, grey below, each cell a random shade.
                let base = if z > h / 2 { 16 } else { 10 };
                set(x, y, z, base + r.int(0, 5));
            }
        }
    }
}
// A dark spot of 3 x 3 cells on top, and a few lone blue specks.
let cx = w / 2;
let cy = d / 2;
for dx in -1..2 {
    for dy in -1..2 {
        let top = h - 1;
        while top > 0 && get(cx + dx, cy + dy, top) == 0 {
            top -= 1;
        }
        set(cx + dx, cy + dy, top, 30);
    }
}
for i in 0..4 {
    let x = 3 + i * (w / 5);
    let y = d / 4;
    let top = h - 1;
    while top > 0 && get(x, y, top) == 0 {
        top -= 1;
    }
    if top > 0 {
        set(x, y, top, 31);
    }
}

let report = reduce_palette(#{colors: colors, despeckle: passes, minShare: 0.003});
if report.slots.after >= report.slots.before {
    throw "the palette did not shrink";
}
