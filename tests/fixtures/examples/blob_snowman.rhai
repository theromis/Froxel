// A made-up snowman for blend_shapes: three snow balls, coal eyes and
// buttons, a carrot nose, stick arms, a hat, and a softly carved smile.
// The shapes are signed distance fields joined by a smooth minimum, so the
// balls melt into each other like packed snow instead of stacking as three
// separate spheres, and the hat brim is a torus fused to its crown.
//
// Parameters
//   size     the bottom ball's radius in cells
//   soft     the fillet between the balls, k in cells (0 is a hard stack)
//   arms     1 for stick arms

let size = param_float("size", 12.0, 7.0, 24.0);
let soft = param_float("soft", 4.0, 0.0, 12.0);
let arms = param_int("arms", 1, 0, 1);

set_slot(1, [236, 240, 246]); // snow
set_slot(2, [34, 32, 36]);    // coal and hat
set_slot(3, [232, 124, 40]);  // carrot
set_slot(4, [118, 80, 50]);   // sticks
set_slot(5, [176, 40, 44]);   // hat band

let r1 = size;
let r2 = size * 0.72;
let r3 = size * 0.52;
let z1 = r1 - 2.0; // sunk 2 cells, then cut flat at z 0
let z2 = z1 + r1 + r2 * 0.55;
let z3 = z2 + r2 + r3 * 0.6;

// The smile's arc, on the face below the nose, a little proud of the
// sphere so it still cuts where the fillet to the body bulges.
let smile = [];
for i in 0..7 {
    let a = (i - 3) * 0.4;
    let x = r3 * 0.5 * a.sin();
    let z = z3 - r3 * 0.5 + r3 * 0.25 * (1.0 - a.cos());
    let dz = z - z3;
    smile.push([x, -sqrt(r3 * r3 - x * x - dz * dz) - 0.3, z]);
}
let brim = z3 + r3 * 0.95;

let shapes = [
    #{shape: "sphere", center: [0, 0, z1], radius: r1, palette: 1},
    #{shape: "sphere", center: [0, 0, z2], radius: r2, palette: 1},
    #{shape: "sphere", center: [0, 0, z3], radius: r3, palette: 1},
    // Flatten the bottom where it sits on the ground.
    #{shape: "box", center: [0, 0, z3], size: [4.0 * r1, 4.0 * r1, 2.0 * z3 + 1.0],
      op: "intersect", k: 0},
    // Coal eyes and buttons sink into the snow, listed after it so
    // colors last keeps them.
    #{shape: "sphere", center: [-r3 * 0.38, -r3 * 0.85, z3 + r3 * 0.25], radius: 1.3, palette: 2, k: 0},
    #{shape: "sphere", center: [r3 * 0.38, -r3 * 0.85, z3 + r3 * 0.25], radius: 1.3, palette: 2, k: 0},
    #{shape: "sphere", center: [0, -r2 * 0.97, z2 + r2 * 0.3], radius: 1.2, palette: 2, k: 0},
    #{shape: "sphere", center: [0, -r2, z2 - r2 * 0.1], radius: 1.2, palette: 2, k: 0},
    // A carrot: a capsule tapering to a point.
    #{shape: "capsule", from: [0, -r3 * 0.8, z3], to: [0, -r3 * 1.9, z3 - 0.5],
      radii: [1.6, 0.5], palette: 3, k: 1},
    // A smile carved softly into the head and lined with coal: a subtract
    // along an arc just under the face.
    #{shape: "capsule", points: smile, radius: 0.9, op: "subtract", k: 1, palette: 2},
    // A hat: a brim (a flat torus and a disc) and a crown with a band.
    #{shape: "box", center: [0, 0, brim], size: [r3 * 2.2, r3 * 2.2, 1.6], round: 0.8,
      palette: 2, k: 1},
    #{shape: "torus", center: [0, 0, brim], radius: r3 * 1.1, thickness: 0.9,
      palette: 2, k: 1},
    #{shape: "box", center: [0, 0, brim + 4.0], size: [r3 * 1.3, r3 * 1.3, 7.0],
      round: 1.5, palette: 2, k: 1},
    #{shape: "box", center: [0, 0, brim + 1.6], size: [r3 * 1.36, r3 * 1.36, 1.4],
      round: 1.0, palette: 5, k: 0},
];
if arms == 1 {
    // A bent stick arm on one side; mirror below repeats the whole snowman,
    // so the arm is drawn once and blended again on the other side.
    shapes.push(#{shape: "capsule", points: [
        [r2 * 0.8, 0, z2 + r2 * 0.2, 0.8],
        [r2 * 1.7, 0, z2 + r2 * 0.6, 0.7],
        [r2 * 2.1, -1, z2 + r2 * 1.1, 0.5],
    ], palette: 4, k: 0.5});
}

// The balls take k soft; each shape that states its own k keeps it.
let done = blend_shapes(shapes, #{k: soft, colors: "last", mirror: #{axis: "x", at: 0.5}});
if done.cells == 0 {
    throw "the snowman filled no cells";
}
