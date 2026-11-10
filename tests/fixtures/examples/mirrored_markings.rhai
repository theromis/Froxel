// A made-up round critter painted on both sides at once with mirror. Each
// marking is drawn on the left half (x below the centre) and the mirror
// repeats it on the right, on the right half's own surface: cheek stripes
// with surface_stroke, a spot pattern from the front with project_paint, a
// belly patch with fill_box in paint mode, and two horns with fill_tube.
// The plane is the object's centre across x unless mirror names at.
//
// Parameters
//   size    the body's diameter in cells
//   width   the stripes' width in cells

let size = param_int("size", 24, 12, 64);
let width = param_float("width", 1.5, 0.5, 4.0);

set_slot(2, [120, 150, 90]); // body
set_slot(3, [60, 70, 40]); // stripe
set_slot(4, [220, 200, 150]); // belly
set_slot(5, [180, 90, 60]); // spot
set_slot(6, [90, 80, 70]); // horn

let r = size / 2;
fill_ellipsoid(0, 0, 0, size - 1, size - 1, size - 1, 2);

// Two horns: one tube, mirrored.
fill_tube([[r - 3, r, size - 3, 2.0], [r / 3, r, size + 3, 1.0]], 6, #{mirror: true});

// Cheek stripes: three strokes on the left cheek.
for i in 0..3 {
    let z = r - 2 + i * 3;
    let s = surface_stroke([[1, r - 4, z], [3, 1, z + 1]], 3, #{
        width: width,
        curve: "smooth",
        only: [2],
        mirror: "x",
    });
    if s.mirrored == 0 {
        throw "the mirror missed the right cheek";
    }
}

// A belly patch on the left, and its mirror.
fill_box(0, 0, 0, r - 1, r / 2, r - 2, 4, #{mode: "paint", mirror: true});

// Spots from the front, drawn on the image's left only.
let rows = [];
for row in 0..8 {
    let line = "";
    for col in 0..8 {
        line += if col < 3 && (row + col) % 3 == 0 { "s" } else { "." };
    }
    rows.push(line);
}
let spots = project_paint("front", rows, #{legend: #{s: 5}, only: [2], mirror: true});
if spots.mirrored == 0 {
    throw "the spots were not mirrored";
}
