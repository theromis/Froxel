//! The organic shaping tools in the editor: the Sculpt tool, which runs MCP
//! `brush` along a drag, and the Deform and Loft panels, which build MCP
//! `deform` and `loft` arguments and run the MCP handler, so the editor and
//! MCP share one path.

use std::collections::HashMap;

use glam::Vec3;
use serde_json::{json, Value};

use super::geom::{cell_bounds, raycast};
use super::stone::rows;
use super::{parse_slots, Cell, Session};
use crate::remote::Organic;
use crate::tools::BrushOp;

/// The Sculpt tool's brush, as MCP `brush` takes it.
#[derive(Clone, PartialEq)]
pub struct SculptBrush {
    pub op: BrushOp,
    pub radius: f32,
    pub strength: f32,
    /// 0 smooth, 1 linear, 2 constant.
    pub falloff: usize,
    /// Palette indices separated by commas or spaces; empty changes every slot.
    pub only: String,
}

impl Default for SculptBrush {
    fn default() -> Self {
        Self {
            op: BrushOp::Inflate,
            radius: 3.0,
            strength: 0.5,
            falloff: 0,
            only: String::new(),
        }
    }
}

pub const SCULPT_OPS: [(&str, BrushOp, &str); 5] = [
    (
        "Inflate",
        BrushOp::Inflate,
        "Push the surface out along the drag",
    ),
    (
        "Deflate",
        BrushOp::Deflate,
        "Push the surface in (dig) along the drag",
    ),
    (
        "Smooth",
        BrushOp::Smooth,
        "Melt bumps and pits under the drag",
    ),
    (
        "Flatten",
        BrushOp::Flatten,
        "Cut bumps and fill pits toward the mean plane",
    ),
    (
        "Grab",
        BrushOp::Grab,
        "Pull the cells under the press along the drag, in the view plane",
    ),
];

impl SculptBrush {
    /// MCP `brush` arguments for a stroke through `points`, with `vector`
    /// for a grab.
    pub(crate) fn args(&self, points: &[[f64; 3]], vector: [f64; 3]) -> Result<Value, String> {
        let only = parse_slots(&self.only).map_err(|err| format!("Only: {err}"))?;
        let mut args = json!({
            "op": self.op.name(),
            "points": points,
            "radius": self.radius,
            "strength": self.strength,
            "falloff": (["smooth", "linear", "constant"][self.falloff.min(2)]),
            "only": only,
        });
        if self.op == BrushOp::Grab {
            args["vector"] = json!(vector);
        }
        Ok(args)
    }
}

/// A Sculpt drag in progress: the cells when it began, the dab centres so
/// far, and for a grab the pressed point and the view direction.
#[derive(Clone)]
pub(crate) struct SculptStroke {
    base: HashMap<Cell, u8>,
    points: Vec<[f64; 3]>,
    anchor: Vec3,
    view: Vec3,
}

/// The Deform panel: MCP `deform`.
#[derive(Clone, PartialEq)]
pub struct DeformPanel {
    /// 0 bend, 1 taper, 2 twist, 3 displace.
    pub op: usize,
    /// 0 x, 1 y, 2 z.
    pub axis: usize,
    /// Origin and length along the axis; `None` is the cells' own.
    pub origin: Option<f32>,
    pub length: Option<f32>,
    pub angle: f32,
    /// Index into [`super::DIRECTIONS`].
    pub direction: usize,
    pub start: f32,
    pub end: f32,
    pub amplitude: f32,
    pub scale: f32,
    pub octaves: i32,
    pub bias: f32,
    pub seed: i32,
}

impl Default for DeformPanel {
    fn default() -> Self {
        Self {
            op: 0,
            axis: 2,
            origin: None,
            length: None,
            angle: 30.0,
            direction: 0,
            start: 1.0,
            end: 0.5,
            amplitude: 2.0,
            scale: 8.0,
            octaves: 2,
            bias: 0.0,
            seed: 0,
        }
    }
}

pub const DEFORM_OPS: [&str; 4] = ["bend", "taper", "twist", "displace"];

impl DeformPanel {
    fn args(&self) -> Value {
        let mut args = json!({
            "op": DEFORM_OPS[self.op.min(3)],
            "axis": (["x", "y", "z"][self.axis.min(2)]),
        });
        if let Some(origin) = self.origin {
            args["origin"] = json!(origin);
        }
        if let Some(length) = self.length {
            args["length"] = json!(length);
        }
        match self.op {
            0 => {
                args["angle"] = json!(self.angle);
                args["direction"] =
                    json!(["+x", "-x", "+y", "-y", "+z", "-z"][self.direction.min(5)]);
            }
            1 => {
                args["start"] = json!(self.start);
                args["end"] = json!(self.end);
            }
            2 => args["angle"] = json!(self.angle),
            _ => {
                args["amplitude"] = json!(self.amplitude);
                args["scale"] = json!(self.scale);
                args["octaves"] = json!(self.octaves);
                args["bias"] = json!(self.bias);
                args["seed"] = json!(self.seed.max(0));
            }
        }
        args
    }
}

/// The Loft panel: MCP `loft`.
#[derive(Clone, PartialEq)]
pub struct LoftPanel {
    /// One point per line: `x y z`, then optional `rx ry roll flatTop noise
    /// frequency`.
    pub points: String,
    pub smooth: bool,
    pub round_caps: bool,
    pub rx: f32,
    pub ry: f32,
    pub roll: f32,
    pub flat_top: f32,
    pub noise: f32,
    pub frequency: f32,
    /// Palette indices separated by commas or spaces; empty is the active slot.
    pub palettes: String,
    /// 0 rings, 1 stripes, 2 random.
    pub pattern: usize,
    pub period: f32,
    /// 0 replace, 1 empty, 2 paint.
    pub mode: usize,
    pub seed: i32,
}

impl Default for LoftPanel {
    fn default() -> Self {
        Self {
            points: "0 0 0 6 5\n12 2 1 4 3.5\n24 0 2 2.5 2".into(),
            smooth: true,
            round_caps: false,
            rx: 3.0,
            ry: 3.0,
            roll: 0.0,
            flat_top: 0.0,
            noise: 0.15,
            frequency: 5.0,
            palettes: String::new(),
            pattern: 0,
            period: 2.0,
            mode: 0,
            seed: 0,
        }
    }
}

impl LoftPanel {
    fn args(&self, active: u8) -> Result<Value, String> {
        let mut palettes = parse_slots(&self.palettes).map_err(|err| format!("Palettes: {err}"))?;
        if palettes.is_empty() {
            palettes.push(active.max(1));
        }
        let keys = [
            "x",
            "y",
            "z",
            "rx",
            "ry",
            "roll",
            "flatTop",
            "noise",
            "frequency",
        ];
        let points: Vec<Value> = rows(&self.points, "Points", 3..=9)?
            .into_iter()
            .map(|row| {
                Value::Object(
                    keys.iter()
                        .zip(row)
                        .map(|(key, value)| ((*key).to_string(), json!(value)))
                        .collect(),
                )
            })
            .collect();
        Ok(json!({
            "points": points,
            "curve": if self.smooth { "smooth" } else { "polyline" },
            "caps": if self.round_caps { "round" } else { "flat" },
            "rx": self.rx,
            "ry": self.ry,
            "roll": self.roll,
            "flatTop": self.flat_top,
            "noise": self.noise,
            "frequency": self.frequency,
            "palettes": palettes,
            "pattern": (["rings", "stripes", "random"][self.pattern.min(2)]),
            "period": self.period,
            "mode": (["replace", "empty", "paint"][self.mode.min(2)]),
            "seed": self.seed.max(0),
        }))
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct OrganicPanels {
    pub deform: DeformPanel,
    pub loft: LoftPanel,
}

impl Session {
    /// The Deform panel's button: MCP `deform` over the selection's box, or
    /// the whole active object.
    pub fn deform_from_panel(&mut self) {
        let mut args = self.organic.deform.args();
        if let Some((min, max)) = cell_bounds(self.picked.iter().copied()) {
            for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"]
                .into_iter()
                .zip([min.0, min.1, min.2, max.0, max.1, max.2])
            {
                args[key] = json!(value);
            }
        }
        self.status = match crate::remote::apply(self, "voxel.deform", &args) {
            Ok(reply) => format!(
                "Deformed {} cells into {}: {} added, {} removed.",
                reply["cells"], reply["result"], reply["added"], reply["removed"]
            ),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The Loft panel's button: MCP `loft` along the panel's points.
    pub fn loft_from_panel(&mut self) {
        let args = match self.organic.loft.args(self.active_slot) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.loft", &args) {
            Ok(reply) => format!(
                "Lofted {} cells along {} cells of path.",
                reply["cells"], reply["length"]
            ),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Add the selection's centre as the next Loft point, with the panel's
    /// radii.
    pub fn loft_point_at_selection(&mut self) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Select cells to add their centre as a point.".into();
            return;
        };
        let mid = |a: i32, b: i32| f64::from(a + b) / 2.0;
        let panel = &mut self.organic.loft;
        let line = format!(
            "{} {} {} {} {}",
            mid(min.0, max.0),
            mid(min.1, max.1),
            mid(min.2, max.2),
            panel.rx,
            panel.ry
        );
        if !panel.points.is_empty() && !panel.points.ends_with('\n') {
            panel.points.push('\n');
        }
        panel.points.push_str(&line);
    }

    /// One frame of a Sculpt drag along the ray `origin`, `dir` in the active
    /// object's cells. The first frame presses on the surface; later frames
    /// add a dab where the drag has moved half a radius, or for a grab move
    /// the pressed point in the view plane. Each frame redraws the whole
    /// stroke over the cells from before it, as one MCP `brush` call would.
    pub(super) fn sculpt_drag(&mut self, origin: Vec3, dir: Vec3) {
        let centre = |cell: Cell| [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
        let grab = self.sculpt.op == BrushOp::Grab;
        let Some(mut stroke) = self.sculpt_stroke.take() else {
            let Some(hit) = raycast(&self.active().cells, origin, dir) else {
                return;
            };
            let anchor = Vec3::new(hit.cell.0 as f32, hit.cell.1 as f32, hit.cell.2 as f32)
                + Vec3::splat(0.5);
            self.sculpt_stroke = Some(SculptStroke {
                base: self.active().cells.clone(),
                points: vec![centre(hit.cell)],
                anchor,
                view: dir.normalize_or_zero(),
            });
            if !grab {
                self.sculpt_redraw([0.0; 3]);
            }
            return;
        };
        let mut vector = [0.0; 3];
        if grab {
            let denom = dir.dot(stroke.view);
            if denom.abs() > 1e-6 {
                let t = (stroke.anchor - origin).dot(stroke.view) / denom;
                let moved = origin + dir * t - stroke.anchor;
                let reach = self.sculpt.radius / self.sculpt.strength.max(0.05);
                let moved = moved.clamp_length_max(reach);
                vector = [f64::from(moved.x), f64::from(moved.y), f64::from(moved.z)];
            }
        } else if let Some(hit) = raycast(&stroke.base, origin, dir) {
            let at = centre(hit.cell);
            let last = stroke.points.last().copied().unwrap_or(at);
            let gap = f64::from(self.sculpt.radius * 0.5).max(1.0);
            let d =
                ((at[0] - last[0]).powi(2) + (at[1] - last[1]).powi(2) + (at[2] - last[2]).powi(2))
                    .sqrt();
            if d < gap {
                self.sculpt_stroke = Some(stroke);
                return;
            }
            stroke.points.push(at);
        }
        self.sculpt_stroke = Some(stroke);
        self.sculpt_redraw(vector);
    }

    fn sculpt_redraw(&mut self, vector: [f64; 3]) {
        let Some(stroke) = self.sculpt_stroke.as_ref() else {
            return;
        };
        let args = match self.sculpt.args(&stroke.points, vector) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        if self.sculpt.op == BrushOp::Grab && vector == [0.0; 3] {
            return;
        }
        let mut next = stroke.base.clone();
        match crate::remote::organic_writes(&stroke.base, &args, Organic::Brush) {
            Ok((writes, _)) => {
                for (x, y, z, slot) in writes {
                    if slot == 0 {
                        next.remove(&(x, y, z));
                    } else {
                        next.insert((x, y, z), slot as u8);
                    }
                }
            }
            Err(err) => {
                self.status = err;
                return;
            }
        }
        if next != self.active().cells {
            self.active_mut().cells = next;
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        }
    }

    /// The end of a Sculpt drag.
    pub(super) fn finish_sculpt(&mut self) {
        self.sculpt_stroke = None;
    }
}
