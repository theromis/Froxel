//! The Foliage panel: the fields of MCP `foliage`. Its button builds the
//! tool's arguments and runs the MCP handler, so the editor and MCP share
//! one path.

use serde_json::{json, Value};

use super::geom::cell_bounds;
use super::stone::rows;
use super::{parse_slots, Session};

/// Where the Foliage panel's crown volume comes from.
pub const FOLIAGE_SOURCES: [&str; 2] = ["Ellipsoids", "From slots"];

/// The Foliage panel: MCP `foliage`.
#[derive(Clone, PartialEq)]
pub struct FoliagePanel {
    /// Index into [`FOLIAGE_SOURCES`].
    pub source: usize,
    /// One ellipsoid per line: `x y z radius` or `x y z rx ry rz`.
    pub ellipsoids: String,
    /// The slots of an existing crown.
    pub from: String,
    /// Leaf slots, darkest first; empty is the active slot.
    pub palettes: String,
    /// 0 is the first of the palettes.
    pub core: i32,
    pub size: [f32; 2],
    pub gap: f32,
    pub flatten: f32,
    pub ragged: f32,
    pub light: f32,
    pub dark: f32,
    pub masses: i32,
    pub groove: i32,
    pub protect: String,
    pub seed: i32,
}

impl Default for FoliagePanel {
    fn default() -> Self {
        Self {
            source: 0,
            ellipsoids: "0 0 24 14 14 9".into(),
            from: String::new(),
            palettes: String::new(),
            core: 0,
            size: [3.0, 6.0],
            gap: 1.0,
            flatten: 0.7,
            ragged: 0.4,
            light: 1.0,
            dark: 1.0,
            masses: 0,
            groove: 2,
            protect: String::new(),
            seed: 0,
        }
    }
}

impl FoliagePanel {
    pub(crate) fn args(&self, active: u8) -> Result<Value, String> {
        let mut palettes = parse_slots(&self.palettes).map_err(|err| format!("Palettes: {err}"))?;
        if palettes.is_empty() {
            palettes.push(active.max(1));
        }
        let protect = parse_slots(&self.protect).map_err(|err| format!("Protect: {err}"))?;
        let mut args = json!({
            "palettes": palettes,
            "size": [self.size[0], self.size[1].max(self.size[0])],
            "gap": self.gap,
            "flatten": self.flatten,
            "ragged": self.ragged,
            "light": self.light,
            "dark": self.dark,
            "masses": self.masses.max(0),
            "groove": self.groove.max(0),
            "protect": protect,
            "seed": self.seed.max(0),
        });
        if self.source == 0 {
            let shapes = rows(&self.ellipsoids, "Ellipsoids", 4..=6)?;
            if shapes.is_empty() {
                return Err("Ellipsoids: give at least one, x y z radius or x y z rx ry rz".into());
            }
            args["ellipsoids"] = json!(shapes);
        } else {
            let from = parse_slots(&self.from).map_err(|err| format!("From: {err}"))?;
            if from.is_empty() {
                return Err("From: name the slots of the crown to re-cluster".into());
            }
            args["from"] = json!(from);
        }
        if self.core > 0 {
            args["core"] = json!(self.core);
        }
        Ok(args)
    }
}

impl Session {
    /// The Foliage panel's button: MCP `foliage`, kept to the selection's
    /// box when there is one, with the Protect boxes.
    pub fn foliage_from_panel(&mut self) {
        let args = self
            .stone
            .foliage
            .args(self.active_slot)
            .and_then(|args| Ok((args, self.stone.exclude_arg()?)));
        let mut args = match args {
            Ok((mut args, exclude)) => {
                args["exclude"] = exclude;
                args
            }
            Err(err) => {
                self.status = err;
                return;
            }
        };
        if let Some((min, max)) = cell_bounds(self.picked.iter().copied()) {
            for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"]
                .into_iter()
                .zip([min.0, min.1, min.2, max.0, max.1, max.2])
            {
                args[key] = json!(value);
            }
        }
        self.status = match crate::remote::apply(self, "voxel.foliage", &args) {
            Ok(reply) => format!(
                "Foliage: {} clumps, {} cells added, {} recoloured, {} removed.",
                reply["clumps"], reply["added"], reply["recoloured"], reply["removed"]
            ),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Add the selection's box as the next crown ellipsoid.
    pub fn foliage_ellipsoid_at_selection(&mut self) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Select cells to add the ellipsoid that fills their box.".into();
            return;
        };
        let mid = |a: i32, b: i32| f64::from(a + b) / 2.0;
        let half = |a: i32, b: i32| f64::from(b - a + 1) / 2.0;
        let line = format!(
            "{} {} {} {} {} {}",
            mid(min.0, max.0),
            mid(min.1, max.1),
            mid(min.2, max.2),
            half(min.0, max.0),
            half(min.1, max.1),
            half(min.2, max.2)
        );
        let panel = &mut self.stone.foliage;
        panel.source = 0;
        if !panel.ellipsoids.is_empty() && !panel.ellipsoids.ends_with('\n') {
            panel.ellipsoids.push('\n');
        }
        panel.ellipsoids.push_str(&line);
    }
}
