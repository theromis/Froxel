// A broadleaf tree whose crown is many separate leaf clumps, as pixel-art
// trees draw them: sunlit tops, darker undersides, and dark gaps between
// the clumps, grouped into a few larger masses. The trunk and branches are
// tubes; foliage packs the clumps over three ellipsoids around them and
// leaves the wood alone, so branches show through the gaps.
// Z is up; the tree stands on the origin.
//
// Parameters
//   height   trunk height in cells, to the crown's middle
//   spread   crown radius in cells
//   size     largest clump radius in cells
//   masses   lobes the clumps are grouped into (0 for none)
//   seed     clump, branch, and tone seed

let height = param_int("height", 30, 16, 48);
let spread = param_float("spread", 15.0, 8.0, 24.0);
let size = param_float("size", 5.0, 3.0, 8.0);
let masses = param_int("masses", 4, 0, 8);
let seed = param_int("seed", 3, 0, 9999);

set_slot(1, [86, 60, 40]); // bark
set_slot(2, [62, 43, 30]); // bark, shaded
set_slot(3, [22, 48, 30]); // leaves: core and deep shade
set_slot(4, [34, 74, 38]); // leaves: shade
set_slot(5, [56, 108, 44]); // leaves: mid
set_slot(6, [96, 150, 56]); // leaves: light
set_slot(7, [156, 196, 84]); // leaves: sunlit tips
// The leaves sway together; the wood stays still.
for slot in 3..8 {
    set_motion(slot, "sway", 1.0);
}

let h = to_float(height);
let roll = rng(seed);

// A trunk that leans a little, and branches that reach into the crown.
fill_tube([[0.0, 0.0, 0.0, 3.0], [0.5, 0.0, h * 0.5, 2.2], [1.0, -0.5, h, 1.6]], 1, #{
    curve: "smooth"
});
for i in 0..4 {
    let angle = PI * 0.5 * to_float(i) + roll.float(-0.4, 0.4);
    let reach = spread * roll.float(0.5, 0.75);
    let from_z = h * roll.float(0.55, 0.75);
    fill_tube([
        [0.5, 0.0, from_z, 1.4],
        [cos(angle) * reach, sin(angle) * reach, from_z + reach * 0.6, 0.8]
    ], 2, #{curve: "polyline", mode: "empty"});
}

// The crown: a broad middle and two lobes, wider than tall.
let crown = [
    [1.0, 0.0, h, spread, spread * 0.9, spread * 0.6],
    [-spread * 0.45, spread * 0.25, h + spread * 0.3, spread * 0.6, spread * 0.6, spread * 0.45],
    [spread * 0.45, -spread * 0.3, h + spread * 0.25, spread * 0.6, spread * 0.55, spread * 0.45]
];
let leaves = foliage([3, 4, 5, 6, 7], #{
    ellipsoids: crown, core: 3, size: [size * 0.6, size], gap: 1.0, flatten: 0.7,
    ragged: 0.4, light: 1.0, dark: 1.0, masses: masses, groove: 2, seed: seed
});
if leaves.clumps == 0 { throw "no clumps"; }
