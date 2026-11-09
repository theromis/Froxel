// A made-up round head with a sleepy face laid on by decal. A face drawn
// from the front and projected would smear sideways on the cheeks; a decal
// unrolls the surface around its point instead, so the eye and the smile
// keep their size and shape seen from any side. The left eye is placed on
// the head's -X cheek and mirror lays the mirrored eye on the +X cheek in
// the same call; the smile is centred, so it needs no mirror.
//
// Parameters
//   width, depth, height   the head's box in cells
//   eye                    the eye's width in cells along the surface
//   spread                 degrees from the front to each eye

let width = param_int("width", 28, 16, 64);
let depth = param_int("depth", 30, 16, 64);
let height = param_int("height", 24, 12, 64);
let eye = param_float("eye", 9.0, 4.0, 20.0);
let spread = param_float("spread", 32.0, 10.0, 60.0);

set_slot(2, [150, 180, 110]); // skin
set_slot(3, [30, 34, 28]); // lid
set_slot(4, [235, 235, 220]); // eye white
set_slot(5, [40, 40, 60]); // pupil
set_slot(6, [120, 40, 50]); // mouth

fill_ellipsoid(0, 0, 0, width - 1, depth - 1, height - 1, 2);

// A sleepy eye: the lid's line over a half-closed eye, the pupil low.
let eye_rows = [
    "..llllll..",
    ".l......l.",
    "l.wwwwww.l",
    "..wwppww..",
];
// A ray from outside, aimed at the head's middle from its front-left, a
// little above: the decal centres where it meets the skin.
let cx = (width - 1) / 2.0;
let cy = (depth - 1) / 2.0;
let cz = (height - 1) / 2.0;
let a = spread * PI / 180.0;
let dir = [-sin(a), -cos(a), 0.25];
let eyes = decal(eye_rows, #{
    legend: #{l: 3, w: 4, p: 5},
    ray: #{
        origin: [cx + dir[0] * 100.0, cy + dir[1] * 100.0, cz + dir[2] * 100.0],
        direction: [-dir[0], -dir[1], -dir[2]],
    },
    size: eye,
    only: [2],
    mirror: true,
});
if eyes.missed > 0 || eyes.mirrored == 0 {
    throw "the eyes ran off the head";
}

// A smile below them, on the front.
let smile = decal([
    "m.........m",
    ".m.......m.",
    "..mm...mm..",
    "....mmm....",
], #{
    legend: #{m: 6},
    point: [cx, 0.0, cz - height / 6.0],
    size: eye * 1.4,
    only: [2],
});
if smile.missed > 0 {
    throw "the smile ran off the head";
}
