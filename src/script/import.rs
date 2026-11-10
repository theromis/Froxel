//! Rhai `voxelize`: the voxelizer of MCP `import_mesh` on a mesh the script
//! gives inline.
//!
//! Scripts read no files, so `import_mesh` itself is not a script function.
//! `voxelize(mesh, opts)` takes `#{positions, triangles, colors}` and the
//! MCP options except `path`, `newObject`, and `name`, and runs
//! `remote::import_plan` on the object being written. Its up axis defaults
//! to z, the cells' own. New palette slots are written after the run, like
//! `set_slot`.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, Map};
use serde_json::Value;

use super::document::dynamic;
use super::shapes::{options, Args, Fallible};
use super::{number, runtime, Scratch};
use crate::import::{Mesh, Up};
use crate::remote;

const VOXELIZE: &[&str] = &[
    "size",
    "scale",
    "voxelSize",
    "upAxis",
    "rotation",
    "position",
    "anchor",
    "fill",
    "colors",
    "palette",
    "slots",
    "maxNewSlots",
    "tolerance",
    "mode",
    "orient",
    "clean",
    "coverage",
    "keepThin",
    "minFeature",
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |mesh: &Map, opts: &Map| -> Fallible<Map> {
            let args = options(opts, VOXELIZE, "voxelize")?;
            let mesh = mesh_of(mesh).map_err(runtime)?;
            state.borrow_mut().voxelize(&mesh, args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn("voxelize", move |mesh: Map, opts: Map| run(&mesh, &opts));
        } else {
            engine.register_fn("voxelize", move |mesh: Map| run(&mesh, &Map::new()));
        }
    }
}

fn rows<const N: usize>(value: &Dynamic, what: &str) -> Result<Vec<[f64; N]>, String> {
    let list = value
        .read_lock::<Array>()
        .ok_or_else(|| format!("voxelize mesh {what} must be a list"))?;
    list.iter()
        .enumerate()
        .map(|(i, row)| {
            let bad = || format!("voxelize mesh {what}[{i}] must be {N} numbers");
            let row = row.read_lock::<Array>().ok_or_else(bad)?;
            if row.len() != N {
                return Err(bad());
            }
            let mut out = [0.0; N];
            for (slot, item) in out.iter_mut().zip(row.iter()) {
                *slot = number(item).filter(|v| v.is_finite()).ok_or_else(bad)?;
            }
            Ok(out)
        })
        .collect()
}

/// `#{positions: [[x, y, z]], triangles: [[a, b, c]], colors: [[r, g, b]]}`:
/// corners index `positions` from 0; `colors` (sRGB 0..=255) is optional,
/// one per position.
fn mesh_of(map: &Map) -> Result<Mesh, String> {
    for key in map.keys() {
        if !["positions", "triangles", "colors"].contains(&key.as_str()) {
            return Err(format!(
                "voxelize mesh has no key `{key}`; it takes positions, triangles, and colors"
            ));
        }
    }
    let positions = rows::<3>(
        map.get("positions")
            .ok_or("voxelize mesh needs positions")?,
        "positions",
    )?;
    let triangles = rows::<3>(
        map.get("triangles")
            .ok_or("voxelize mesh needs triangles")?,
        "triangles",
    )?;
    let colors = match map.get("colors") {
        Some(value) => rows::<3>(value, "colors")?,
        None => Vec::new(),
    };
    if !colors.is_empty() && colors.len() != positions.len() {
        return Err(format!(
            "voxelize mesh has {} colors for {} positions",
            colors.len(),
            positions.len()
        ));
    }
    let mut mesh = Mesh::default();
    mesh.materials.push(Default::default());
    for (i, position) in positions.iter().enumerate() {
        let color = match colors.get(i) {
            Some(rgb) => {
                let mut linear = [1.0; 4];
                for c in 0..3 {
                    if !(0.0..=255.0).contains(&rgb[c]) {
                        return Err(format!("voxelize mesh colors[{i}] must be 0..=255"));
                    }
                    linear[c] = crate::import::to_linear(rgb[c].round() as u8);
                }
                linear
            }
            None => [1.0; 4],
        };
        mesh.push_vertex(*position, [0.0, 0.0], color);
    }
    for (i, corners) in triangles.iter().enumerate() {
        let mut v = [0u32; 3];
        for (slot, corner) in v.iter_mut().zip(corners) {
            if corner.fract() != 0.0 || *corner < 0.0 || *corner as usize >= positions.len() {
                return Err(format!(
                    "voxelize mesh triangles[{i}] must index positions 0..{}",
                    positions.len()
                ));
            }
            *slot = *corner as u32;
        }
        mesh.push_triangle(v, 0)?;
    }
    if mesh.triangles.is_empty() {
        return Err("voxelize mesh has no triangles".into());
    }
    mesh.primitives = 1;
    Ok(mesh)
}

impl Scratch {
    /// Voxelize `mesh` into the object being written; new slots join the
    /// run's palette writes.
    fn voxelize(&mut self, mesh: &Mesh, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let mut palette = [[0u8; 4]; 256];
        for (slot, entry) in palette.iter_mut().zip(&self.view.palette_rgba) {
            *slot = *entry;
        }
        let mut used = [false; 256];
        for (slot, flag) in used.iter_mut().zip(&self.view.used) {
            *slot = *flag;
        }
        for &(slot, rgb) in &self.palette {
            palette[slot as usize] = [rgb[0], rgb[1], rgb[2], 255];
            used[slot as usize] = true;
        }
        for built in &self.built {
            for slot in built.cells.values() {
                used[*slot as usize] = true;
            }
        }
        let plan = remote::import_plan(
            mesh,
            &Value::Object(args),
            Up::Z,
            self.cells(),
            &palette,
            &used,
            false,
        )?;
        for &(slot, rgb) in &plan.new_slots {
            self.palette.push((slot, rgb));
        }
        let changed = self.apply(&plan.writes)?;
        let mut reply: Map = plan
            .reply
            .iter()
            .map(|(key, value)| (key.as_str().into(), dynamic(value)))
            .collect();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        Ok(reply)
    }
}
