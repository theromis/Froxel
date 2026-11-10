// A lily pad: a wobbly disc with a notch, two layers, a light centre,
// veins, and a dark rim.
//
// Parameters
//   radius     disc radius, before the wobble
//   notch      half-width of the cut, in radians
//   direction  direction of the notch, in degrees, 0 toward +X
//   seed       wobble seed

let radius = param_float("radius", 8.0, 3.0, 16.0);
let notch = param_float("notch", 0.45, 0.0, 1.4);
let direction = param_float("direction", 25.0, 0.0, 360.0);
let seed = param_int("seed", 1, 0, 9999);

set_slot(1, [18, 72, 28]);
set_slot(2, [52, 148, 56]);
set_slot(3, [196, 220, 130]);
set_slot(4, [28, 96, 42]);

let extent = to_int(radius) + 3;
let aim = direction * 3.141592653589793 / 180.0;
let pi = 3.141592653589793;
let tau = 6.283185307179586;

for iz in -extent..(extent + 1) {
    for ix in -extent..(extent + 1) {
        let x = to_float(ix);
        let z = to_float(iz);
        let dist = hypot(x, z);
        let wobble = (noise2(x * 0.35, z * 0.35, seed) - 0.5) * 2.4;
        let edge = radius + wobble;
        if dist > edge {
            continue;
        }
        let ang = atan2(z, x);
        let delta = ang - aim;
        if delta > pi { delta = delta - tau; }
        if delta < -pi { delta = delta + tau; }
        if delta > pi { delta = delta - tau; }
        if delta < -pi { delta = delta + tau; }
        if abs(delta) < notch && dist > radius * 0.18 {
            continue;
        }
        let slot = if dist > edge - 1.15 {
            1
        } else if dist < radius * 0.26 {
            3
        } else if abs(sin(ang * 5.0)) > 0.92 && dist > radius * 0.35 {
            4
        } else {
            2
        };
        // Z is up in .vox, so the disc lies in the XY plane, two voxels thick.
        set(ix, iz, 0, slot);
        set(ix, iz, 1, slot);
    }
}
