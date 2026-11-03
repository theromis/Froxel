// A fallen tree trunk with a root flare and a forked limb, shaped with
// loft, displace, and brush instead of round tubes. Z is up; the trunk lies
// along +X on the ground, the torn root plate at x 0.
//
// Parameters
//   length   trunk length in cells
//   radius   trunk half-width at the root end
//   roots    number of flare roots
//   lumps    bark lump height in cells (displace amplitude)
//   ground   add a ground slab under the trunk
//   seed     noise, roots, and lumps seed

let length = param_int("length", 64, 32, 128);
let radius = param_float("radius", 7.0, 4.0, 12.0);
let roots = param_int("roots", 6, 0, 10);
let lumps = param_float("lumps", 1.5, 0.5, 4.0);
let ground = param_bool("ground", true);
let seed = param_int("seed", 5, 0, 9999);

set_slot(3, [92, 64, 40]); // bark
set_slot(4, [66, 46, 30]); // dark bark
set_slot(5, [120, 92, 60]); // root bark
set_slot(8, [70, 92, 44]); // ground

let len = to_float(length);
let r = radius;

// The trunk: egg-shaped (wider than tall), its bottom flattened where it
// rests, tapering and sagging toward the far end, with a knobbly outline.
let trunk = loft([
    #{x: 0.0, y: 0.0, z: r * 0.85, rx: r * 1.15, ry: r * 0.95, flatTop: -0.3},
    #{x: len * 0.3, y: 1.5, z: r * 0.8, rx: r, ry: r * 0.85, flatTop: -0.25},
    #{x: len * 0.65, y: -1.0, z: r * 0.7, rx: r * 0.75, ry: r * 0.65, flatTop: -0.2},
    #{x: len, y: 1.0, z: r * 0.55, rx: r * 0.5, ry: r * 0.45, noise: 0.08},
], [3, 4], #{curve: "smooth", noise: 0.14, frequency: 7, seed: seed, pattern: "stripes", sectors: 12});

// The root flare: flat, tapering roots that spread from the root end in the
// plane of the torn root plate and curl back along the trunk.
let roll = rng(seed);
for i in 0..roots {
    let angle = 2.0 * PI * (to_float(i) + roll.float(-0.25, 0.25)) / to_float(max(roots, 1));
    let dy = cos(angle);
    let dz = sin(angle);
    let spread = r * roll.float(2.2, 2.9);
    let z0 = r * 0.85;
    // Roots under the ground line are torn short.
    let reach = if dz < -0.3 { 0.6 } else { 1.0 };
    let tip_z = max(z0 + dz * spread * reach, 0.0);
    loft([
        #{x: 3.0, y: 0.0, z: z0, rx: r * 0.65, ry: r * 0.4},
        #{x: 0.0, y: dy * spread * 0.55 * reach, z: max(z0 + dz * spread * 0.55 * reach, 0.0), rx: r * 0.35, ry: r * 0.22},
        #{x: -1.0 - roll.float(0.0, 3.0), y: dy * spread * reach, z: tip_z, rx: 0.9, ry: 0.6},
    ], 5, #{curve: "smooth", roll: angle * 180.0 / PI + 90.0, noise: 0.2, frequency: 4, seed: seed + i, mode: "empty"});
}

// The forked limb: one bough rising from the trunk two thirds along, that
// splits into two branches.
let base = [len * 0.55, 0.0, r * 1.2];
let fork = [len * 0.7, r * 1.6, r * 2.6];
loft([
    #{x: base[0], y: base[1], z: base[2], rx: r * 0.5, ry: r * 0.45},
    #{x: (base[0] + fork[0]) / 2.0, y: fork[1] * 0.45, z: (base[2] + fork[2]) / 2.0 + 1.0, rx: r * 0.4, ry: r * 0.36},
    #{x: fork[0], y: fork[1], z: fork[2], rx: r * 0.33, ry: r * 0.3},
], [3, 4], #{curve: "smooth", noise: 0.12, frequency: 5, seed: seed + 20, pattern: "stripes", sectors: 8, mode: "empty"});
loft([
    #{x: fork[0], y: fork[1], z: fork[2], rx: r * 0.3, ry: r * 0.28},
    #{x: fork[0] + r * 1.6, y: fork[1] + r * 0.9, z: fork[2] + r * 1.4, rx: r * 0.2, ry: r * 0.2},
    #{x: fork[0] + r * 2.4, y: fork[1] + r * 1.1, z: fork[2] + r * 2.6, rx: 0.8, ry: 0.8},
], [3, 4], #{curve: "smooth", noise: 0.1, seed: seed + 21, mode: "empty"});
loft([
    #{x: fork[0], y: fork[1], z: fork[2], rx: r * 0.26, ry: r * 0.24},
    #{x: fork[0] + r * 1.8, y: fork[1] + r * 1.9, z: fork[2] + r * 0.3, rx: r * 0.17, ry: r * 0.17},
    #{x: fork[0] + r * 3.0, y: fork[1] + r * 2.6, z: fork[2] - r * 0.1, rx: 0.7, ry: 0.7},
], [3, 4], #{curve: "smooth", noise: 0.1, seed: seed + 22, mode: "empty"});

// Bark lumps and burls over the trunk and the root plate: bulges only, so
// the trunk never thins to a hole.
let reach = to_int(ceil(r * 3.0)) + 2;
deform(-reach, -reach, 0, length + 2, reach, reach * 2, "displace", #{
    amplitude: lumps, scale: r * 1.2, octaves: 2, bias: 0.6, seed: seed
});

// Refine the silhouette: melt the joint where the limb leaves the trunk,
// raise a burl on the flank, dig a rot pocket near the far end, and pull
// the root plate's top edge back a little so it leans like a torn plate.
brush("smooth", [[base[0] - 2.0, base[1], base[2] - 1.0], [base[0] + 3.0, base[1] + 1.5, base[2] + 1.0]], #{radius: r * 0.6, strength: 0.8});
brush("inflate", [[len * 0.4, -r, r * 0.9], [len * 0.44, -r, r * 1.0]], #{radius: r * 0.55, strength: 0.9, only: [3, 4]});
brush("deflate", [[len * 0.82, r * 0.5, r * 0.9]], #{radius: r * 0.45, strength: 1.0, only: [3, 4]});
brush("grab", [[0.0, 0.0, r * 2.4, r * 1.2]], #{vector: [-2.0, 0.0, 1.0]});

if ground {
    fill_box(-reach, -reach - 4, -1, length + 8, reach + 4, -1, 8);
}
