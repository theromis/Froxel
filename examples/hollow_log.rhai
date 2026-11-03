// A fallen hollow log: a tapering trunk with bark grooves, hollow from the
// root end, buttress roots, a snapped far end with splinters, and debris
// heaped under the break. Z is up; the log lies along +X on the ground.
//
// Parameters
//   length   trunk length in cells
//   radius   trunk radius at the root end
//   wall     wall thickness of the hollow
//   roots    number of buttress roots
//   ground   add a ground slab under the log, after the break
//   seed     roots, break, and debris seed

let length = param_int("length", 40, 16, 96);
let radius = param_float("radius", 5.0, 2.5, 10.0);
let wall = param_float("wall", 1.5, 1.0, 4.0);
let roots = param_int("roots", 4, 0, 8);
let ground = param_bool("ground", true);
let seed = param_int("seed", 3, 0, 9999);

set_slot(3, [92, 64, 40]); // bark
set_slot(4, [60, 40, 26]); // dark bark
set_slot(5, [196, 160, 104]); // sapwood
set_slot(6, [214, 184, 128]); // light sapwood
set_slot(7, [238, 218, 170]); // splinter tips
set_slot(8, [70, 92, 44]); // ground

let len = to_float(length);
let reach = to_int(ceil(radius)) + 2;

// The trunk axis sags a little sideways and thins toward the far end.
fn axis_at(t, len, radius) {
    [t * len, sin(t * PI()) * 1.5, radius, radius * (1.0 - 0.25 * t)]
}

let trunk = [];
for i in 0..5 {
    trunk.push(axis_at(to_float(i) / 4.0, len, radius));
}
fill_tube(trunk, [3, 4], #{curve: "smooth", pattern: "stripes", sectors: 10});

// Smooth while the trunk is still solid: smooth removes thin walls.
smooth(-reach, -reach - 2, 0, length + reach, reach + 2, 2 * reach, #{iterations: 1});

// Buttress roots: smooth tubes that spread from the root end and dig down.
let roll = rng(seed);
for i in 0..roots {
    let angle = PI * (0.15 + 0.7 * to_float(i) / max(to_float(roots - 1), 1.0)) + roll.float(-0.2, 0.2);
    let side = if i % 2 == 0 { 1.0 } else { -1.0 };
    let dy = cos(angle) * side;
    let spread = radius * roll.float(1.6, 2.4);
    let points = [];
    points.push([1.5, 0.0, radius, radius * 0.5]);
    points.push([-0.5, dy * spread * 0.6, max(radius * 0.4, 0.0), radius * 0.3]);
    points.push([-2.0 - roll.float(0.0, 2.0), dy * spread, 0.0, 0.5]);
    fill_tube(points, 4, #{curve: "smooth", mode: "empty"});
}

// Snap the far end. Normal points out of the wood, a little upward.
let tip = axis_at(0.82, len, radius);
break_end([tip[0], tip[1], tip[2]], [1.0, 0.0, 0.15], #{
    depth: 3, density: 0.2, length: [4, 12], bark: 4, sapwood: [5, 6], tips: 7, seed: seed
});

// Hollow it from the root end to just short of the break. This comes
// after the roots so it cuts them out of the opening too.
let mid = axis_at(0.6, len, radius);
carve_tube([[-2.0, 0.0, radius], [mid[0], mid[1], mid[2]]], #{
    radius: max(radius - wall, 0.5), curve: "polyline"
});

// The ground goes in after the break, which acts on the whole object, so
// it stays whole; the debris then settles on it.
if ground {
    fill_box(-reach - 6, -reach - 10, -1, length + 14, reach + 10, -1, 8);
}

// Debris under the break settles on the log, the ground, or z 0.
scatter(to_int(tip[0]) - 4, -reach - 6, 0, length + 10, reach + 6, 2 * reach + 6, [5, 6, 7, 3], #{
    count: 40, size: [1, 3], seed: seed, floor: 0, clump: 0.6
});
