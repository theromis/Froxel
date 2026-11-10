use std::collections::HashMap;

use super::*;
use crate::state::Cell;

fn boxed(min: Cell, max: Cell, slot: u8, cells: &mut HashMap<Cell, u8>) {
    for z in min.2..=max.2 {
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                cells.insert((x, y, z), slot);
            }
        }
    }
}

#[test]
fn a_fin_and_a_leg_are_thin_and_the_body_is_not() {
    let mut cells = HashMap::new();
    boxed((0, 0, 4), (9, 9, 9), 1, &mut cells); // body
    boxed((4, 10, 5), (4, 15, 8), 2, &mut cells); // fin, one cell thick
    boxed((1, 1, 0), (1, 1, 3), 3, &mut cells); // leg, one cell wide
    let report = thin_parts(&cells, 2).unwrap();
    assert_eq!(report.count, 2);
    assert_eq!(report.cells, 24 + 4);
    assert_eq!(report.parts[0].cells, 24);
    assert_eq!(report.parts[0].thickness, 1);
    assert_eq!(report.parts[0].min, (4, 10, 5));
    // At 3 cells the leg and fin are still the only thin parts.
    assert_eq!(thin_parts(&cells, 3).unwrap().count, 2);
    assert!(thin_parts(&cells, 1).is_err());
}
