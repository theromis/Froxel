// A fallen trunk with bark that follows its form: plates along the grain
// split by fissures with dirt at their bottom, depth shaded into the
// fissures and the root flare, and moss in separate cushions on the upper
// side with drapes hanging over the edge. No stripes are painted: bark
// cuts the plates, cavity_tint shades them, and moss grows in cushions.
// Z is up; the trunk lies along +X on the ground.
//
// Parameters
//   length   trunk length in cells
//   radius   trunk radius at the root end
//   branch   add a branch stub on the upper side
//   moss     moss coverage of the exposed upper side, 0..1
//   seed     bark, moss, and root seed

let length = param_int("length", 56, 24, 96);
let radius = param_float("radius", 7.0, 3.0, 10.0);
let branch = param_bool("branch", true);
let moss_cover = param_float("moss", 0.45, 0.0, 1.0);
let seed = param_int("seed", 5, 0, 9999);

set_slot(3, [92, 66, 44]); // bark
set_slot(4, [80, 57, 38]); // bark, a darker plate
set_slot(5, [101, 74, 49]); // bark, a lighter plate
set_slot(6, [40, 28, 20]); // fissure dirt
set_slot(7, [74, 102, 44]); // moss
set_slot(8, [92, 122, 52]); // moss, sunlit
set_slot(9, [60, 84, 38]); // moss, shaded

let len = to_float(length);
let reach = to_int(ceil(radius)) + 3;

// The trunk sags a little sideways and thins toward the far end. The same
// points drive fill_tube and bark, so the plates follow the bend.
let trunk = [];
for i in 0..5 {
    let t = to_float(i) / 4.0;
    trunk.push([t * len, sin(t * PI) * 1.5, radius, radius * (1.0 - 0.3 * t)]);
}
fill_tube(trunk, 3, #{curve: "smooth"});

// A root flare at the near end, smoothed into the trunk.
let roll = rng(seed);
for i in 0..4 {
    let angle = PI * (0.2 + 0.6 * to_float(i) / 3.0) + roll.float(-0.2, 0.2);
    let side = if i % 2 == 0 { 1.0 } else { -1.0 };
    let dy = cos(angle) * side * radius * roll.float(1.4, 2.0);
    fill_tube([[2.0, 0.0, radius, radius * 0.5], [-1.0, dy, 0.0, 1.0]], 3, #{
        curve: "smooth", mode: "empty"
    });
}
smooth(-reach, -reach - 4, 0, length + reach, reach + 4, 2 * reach, #{iterations: 1});

// Bark along the trunk's own curve, then along the branch.
bark(-reach, -reach - 4, 0, length + reach, reach + 4, 2 * reach, [3, 4, 5], #{
    points: trunk, curve: "smooth", dirt: 6, width: [3, 6], length: [8, 18],
    depth: [1, 2], cracks: 0.3, only: [3], seed: seed
});
if branch {
    let at = len * 0.45;
    let stub = [[at, 0.0, radius * 1.6, 2.5], [at + 6.0, 2.0, radius * 2.0 + 6.0, 1.8]];
    fill_tube(stub, 3, #{curve: "polyline", mode: "empty"});
    bark(to_int(at) - 4, -4, to_int(radius), to_int(at) + 10, 6, to_int(radius * 2.0) + 10, [3, 4, 5], #{
        points: stub, dirt: 6, width: [2, 4], length: [5, 10], depth: 1, only: [3], seed: seed + 1
    });
}

// Depth by shading: fissures, the crook of the roots, and the branch join
// darken; the plate ridges lighten a little.
cavity_tint(-reach, -reach - 4, 0, length + reach, reach + 4, 2 * reach + 12, #{
    radius: 2, dark: 0.1, lighten: true, light: 0.3, darken: 0.7, brighten: 1.12
});

// Moss in cushions on the upper side, draping over the edge.
if moss_cover > 0.0 {
    moss(-reach, -reach - 4, 0, length + reach, reach + 4, 2 * reach + 14, [7, 8, 9], #{
        style: "cushions", amount: moss_cover, size: [2, 5], gap: 2, exposure: 0.35,
        drapes: 0.35, length: [2, 6], thickness: 2, seed: seed
    });
}
