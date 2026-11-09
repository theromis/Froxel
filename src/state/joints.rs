//! Joint pivots: MCP `place_joints` and `set_bone` `joint`. A bone's pivot
//! goes where its part meets its parent's part, from the cells of the
//! objects bound to each, in world cells:
//!
//! - **overlap**: the centroid of the cells both parts hold (a copy split);
//! - **contact**: else the centroid of the faces where a cell of one part
//!   touches a cell of the other;
//! - **nearest**: else the midpoints between each surface cell of the child
//!   part and the parent's nearest surface cell, for the child cells within
//!   `band` cells of the closest gap, averaged.
//!
//! The parent part is the cells bound to the nearest ancestor bone that has
//! any, so a bone with no object of its own (a hip) is skipped over. A cell
//! `(x, y, z)` spans `x..x + 1`, so a face between two cells sits on an
//! integer coordinate across it and a cell centre on `n + 0.5`.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use super::{Cell, Session};

/// Most cells one part may hold for a joint.
const MAX_PART_CELLS: usize = 8_000_000;
/// Side of a bucket of the nearest-cell search.
const BUCKET: i32 = 8;

/// How a pivot is rounded onto the cell grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum JointSnap {
    /// As measured.
    #[default]
    None,
    /// The nearest cell corner (integer coordinates).
    Corner,
    /// The nearest cell centre (`n + 0.5`).
    Center,
    /// The nearest half cell.
    Half,
}

impl JointSnap {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "none" => Ok(Self::None),
            "corner" => Ok(Self::Corner),
            "center" | "centre" => Ok(Self::Center),
            "half" => Ok(Self::Half),
            other => Err(format!(
                "snap must be none, corner, center, or half ({other})"
            )),
        }
    }

    pub fn from_index(index: usize) -> Self {
        [Self::None, Self::Corner, Self::Center, Self::Half][index.min(3)]
    }

    fn apply(self, value: f32) -> f32 {
        match self {
            Self::None => value,
            Self::Corner => value.round(),
            Self::Center => (value - 0.5).round() + 0.5,
            Self::Half => (value * 2.0).round() * 0.5,
        }
    }
}

/// Options of a joint measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointOptions {
    pub snap: JointSnap,
    /// Cells past the closest gap that still count for `nearest`.
    pub band: f32,
}

impl Default for JointOptions {
    fn default() -> Self {
        Self {
            snap: JointSnap::None,
            band: 0.5,
        }
    }
}

/// Where two parts meet.
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub pivot: [f32; 3],
    /// `overlap`, `contact`, or `nearest`.
    pub method: &'static str,
    /// Overlap cells, contact faces, or nearest pairs averaged.
    pub samples: usize,
    /// Empty cells between the parts at their closest (0 when they touch).
    pub gap: f32,
}

/// The joint between a child part and its parent part, before snapping.
/// `None` when either is empty.
pub fn joint_between(child: &HashSet<Cell>, parent: &HashSet<Cell>, band: f32) -> Option<Joint> {
    if child.is_empty() || parent.is_empty() {
        return None;
    }
    let (small, large) = if child.len() <= parent.len() {
        (child, parent)
    } else {
        (parent, child)
    };
    let overlap: Vec<Cell> = small
        .iter()
        .filter(|cell| large.contains(cell))
        .copied()
        .collect();
    if !overlap.is_empty() {
        let mut sum = [0.0f64; 3];
        for cell in &overlap {
            sum[0] += cell.0 as f64 + 0.5;
            sum[1] += cell.1 as f64 + 0.5;
            sum[2] += cell.2 as f64 + 0.5;
        }
        return Some(Joint {
            pivot: mean(sum, overlap.len()),
            method: "overlap",
            samples: overlap.len(),
            gap: 0.0,
        });
    }
    let mut sum = [0.0f64; 3];
    let mut faces = 0usize;
    for cell in small {
        for step in STEPS {
            let next = (cell.0 + step.0, cell.1 + step.1, cell.2 + step.2);
            if large.contains(&next) {
                sum[0] += (cell.0 + next.0) as f64 * 0.5 + 0.5;
                sum[1] += (cell.1 + next.1) as f64 * 0.5 + 0.5;
                sum[2] += (cell.2 + next.2) as f64 * 0.5 + 0.5;
                faces += 1;
            }
        }
    }
    if faces > 0 {
        return Some(Joint {
            pivot: mean(sum, faces),
            method: "contact",
            samples: faces,
            gap: 0.0,
        });
    }
    let child_surface = surface(child);
    let parent_surface = surface(parent);
    let grid = Buckets::new(&parent_surface);
    let nearest: Vec<(Cell, Cell, f32)> = child_surface
        .iter()
        .filter_map(|cell| grid.nearest(*cell).map(|(other, d)| (*cell, other, d)))
        .collect();
    let closest = nearest
        .iter()
        .map(|pair| pair.2)
        .fold(f32::INFINITY, f32::min);
    let reach = closest + band.max(0.0);
    let mut sum = [0.0f64; 3];
    let mut pairs = 0usize;
    for (a, b, d) in &nearest {
        if *d <= reach + 1e-4 {
            sum[0] += (a.0 + b.0) as f64 * 0.5 + 0.5;
            sum[1] += (a.1 + b.1) as f64 * 0.5 + 0.5;
            sum[2] += (a.2 + b.2) as f64 * 0.5 + 0.5;
            pairs += 1;
        }
    }
    Some(Joint {
        pivot: mean(sum, pairs.max(1)),
        method: "nearest",
        samples: pairs,
        gap: (closest - 1.0).max(0.0),
    })
}

const STEPS: [Cell; 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

fn mean(sum: [f64; 3], count: usize) -> [f32; 3] {
    let n = count as f64;
    [
        (sum[0] / n) as f32,
        (sum[1] / n) as f32,
        (sum[2] / n) as f32,
    ]
}

/// Cells with an empty face neighbour.
fn surface(cells: &HashSet<Cell>) -> Vec<Cell> {
    cells
        .iter()
        .filter(|cell| {
            STEPS
                .iter()
                .any(|s| !cells.contains(&(cell.0 + s.0, cell.1 + s.1, cell.2 + s.2)))
        })
        .copied()
        .collect()
}

/// Cells in cubes of `BUCKET` cells, for the nearest-cell search.
struct Buckets {
    cells: HashMap<Cell, Vec<Cell>>,
    min: Cell,
    max: Cell,
}

impl Buckets {
    fn new(cells: &[Cell]) -> Self {
        let mut map: HashMap<Cell, Vec<Cell>> = HashMap::new();
        let mut min = (i32::MAX, i32::MAX, i32::MAX);
        let mut max = (i32::MIN, i32::MIN, i32::MIN);
        for cell in cells {
            let key = Self::key(*cell);
            min = (min.0.min(key.0), min.1.min(key.1), min.2.min(key.2));
            max = (max.0.max(key.0), max.1.max(key.1), max.2.max(key.2));
            map.entry(key).or_default().push(*cell);
        }
        Self {
            cells: map,
            min,
            max,
        }
    }

    fn key(cell: Cell) -> Cell {
        (
            cell.0.div_euclid(BUCKET),
            cell.1.div_euclid(BUCKET),
            cell.2.div_euclid(BUCKET),
        )
    }

    /// The nearest cell and its centre distance.
    fn nearest(&self, from: Cell) -> Option<(Cell, f32)> {
        if self.cells.is_empty() {
            return None;
        }
        let home = Self::key(from);
        let far = [
            (home.0 - self.min.0).abs().max((self.max.0 - home.0).abs()),
            (home.1 - self.min.1).abs().max((self.max.1 - home.1).abs()),
            (home.2 - self.min.2).abs().max((self.max.2 - home.2).abs()),
        ]
        .into_iter()
        .max()
        .unwrap_or(0);
        let mut best: Option<(Cell, i64)> = None;
        for ring in 0..=far {
            for dx in -ring..=ring {
                for dy in -ring..=ring {
                    for dz in -ring..=ring {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != ring {
                            continue;
                        }
                        let key = (home.0 + dx, home.1 + dy, home.2 + dz);
                        let Some(list) = self.cells.get(&key) else {
                            continue;
                        };
                        for cell in list {
                            let d = [cell.0 - from.0, cell.1 - from.1, cell.2 - from.2];
                            let d2 = d.iter().map(|v| (*v as i64) * (*v as i64)).sum::<i64>();
                            if best.is_none_or(|(_, b)| d2 < b) {
                                best = Some((*cell, d2));
                            }
                        }
                    }
                }
            }
            // Every unsearched bucket is at least `ring * BUCKET` cells away.
            if let Some((_, d2)) = best {
                let reach = (ring as i64) * (BUCKET as i64);
                if d2 <= reach * reach {
                    break;
                }
            }
        }
        best.map(|(cell, d2)| (cell, (d2 as f32).sqrt()))
    }
}

/// One bone's measured joint.
#[derive(Clone, Debug, PartialEq)]
pub struct JointReport {
    pub bone: String,
    /// The ancestor whose part it meets.
    pub parent: String,
    pub previous: [f32; 3],
    pub joint: Joint,
}

impl JointReport {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.bone,
            "parent": self.parent,
            "pivot": self.joint.pivot,
            "previous": self.previous,
            "method": self.joint.method,
            "samples": self.joint.samples,
            "gap": self.joint.gap,
        })
    }
}

impl Session {
    /// World cells of the objects bound to `bone`.
    fn bone_cells(&self, bone: &str) -> Result<HashSet<Cell>, String> {
        let mut out = HashSet::new();
        for object in &self.objects {
            if object.bone.as_deref() != Some(bone) {
                continue;
            }
            let t = object.translation;
            out.extend(
                object
                    .cells
                    .keys()
                    .map(|c| (c.0 + t.0, c.1 + t.1, c.2 + t.2)),
            );
            if out.len() > MAX_PART_CELLS {
                return Err(format!(
                    "bone `{bone}` holds more than {MAX_PART_CELLS} cells"
                ));
            }
        }
        Ok(out)
    }

    /// The joint of `bone` with its parent part. `parent` overrides the
    /// bone's parent (`set_bone` with both `parent` and `joint`).
    pub fn measure_joint(
        &self,
        bone: &str,
        parent: Option<Option<String>>,
        options: JointOptions,
    ) -> Result<JointReport, String> {
        let index = self
            .skeleton
            .index_of(bone)
            .ok_or_else(|| format!("unknown bone `{bone}`"))?;
        let child = self.bone_cells(bone)?;
        if child.is_empty() {
            return Err(format!(
                "bone `{bone}` has no bound object with cells (bind_object first)"
            ));
        }
        let mut up = match parent {
            Some(parent) => parent,
            None => self.skeleton.bones[index].parent.clone(),
        };
        let mut seen = 0;
        while let Some(name) = up {
            let cells = self.bone_cells(&name)?;
            if !cells.is_empty() {
                let mut joint = joint_between(&child, &cells, options.band)
                    .ok_or_else(|| format!("bone `{bone}` meets no part"))?;
                for axis in &mut joint.pivot {
                    *axis = options.snap.apply(*axis);
                }
                return Ok(JointReport {
                    bone: bone.to_string(),
                    parent: name,
                    previous: self.skeleton.bones[index].pivot,
                    joint,
                });
            }
            seen += 1;
            if seen > self.skeleton.bones.len() {
                break;
            }
            up = self
                .skeleton
                .index_of(&name)
                .and_then(|i| self.skeleton.bones[i].parent.clone());
        }
        Err(format!(
            "bone `{bone}` has no parent bone with a bound object to meet"
        ))
    }

    /// Put the pivots of `bones` (every bone when `None`) at their joints,
    /// as one undo step. A bone with no part, or no ancestor with one, is
    /// skipped with the reason when the list was not given, and an error
    /// when it was named.
    pub fn place_joints(
        &mut self,
        bones: Option<&[String]>,
        options: JointOptions,
    ) -> Result<(Vec<JointReport>, Vec<(String, String)>), String> {
        let named = bones.is_some();
        let names: Vec<String> = match bones {
            Some(list) => list.to_vec(),
            None => self.skeleton.bones.iter().map(|b| b.name.clone()).collect(),
        };
        let mut placed = Vec::new();
        let mut skipped = Vec::new();
        for name in &names {
            match self.measure_joint(name, None, options) {
                Ok(report) => placed.push(report),
                Err(err) if !named => skipped.push((name.clone(), err)),
                Err(err) => return Err(err),
            }
        }
        if placed.is_empty() {
            return Err(match skipped.first() {
                Some((_, reason)) => format!("no joint to place: {reason}"),
                None => "the document has no bones".into(),
            });
        }
        let mut next = self.skeleton.clone();
        for report in &placed {
            next.set(&report.bone, None, Some(report.joint.pivot), None, None)?;
        }
        if next != self.skeleton {
            self.checkpoint();
            self.skeleton = next;
            self.commit();
        }
        Ok((placed, skipped))
    }

    /// The Bones panel's Place joints button.
    pub fn place_joints_from_panel(&mut self) {
        let options = JointOptions {
            snap: JointSnap::from_index(self.edit.joint_snap),
            ..JointOptions::default()
        };
        self.status = match self.place_joints(None, options) {
            Ok((placed, skipped)) => {
                let mut text = format!("Placed {} joint pivots", placed.len());
                if !skipped.is_empty() {
                    let names: Vec<&str> = skipped.iter().map(|(n, _)| n.as_str()).collect();
                    text.push_str(&format!(" (skipped {})", names.join(", ")));
                }
                text
            }
            Err(err) => err,
        };
    }

    /// The Bone inspector's At joint button.
    pub fn joint_pivot_from_panel(&mut self, bone: &str) {
        let options = JointOptions {
            snap: JointSnap::from_index(self.edit.joint_snap),
            ..JointOptions::default()
        };
        let names = [bone.to_string()];
        if let Err(err) = self.place_joints(Some(&names), options) {
            self.status = err;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/joints.rs"]
mod tests;
