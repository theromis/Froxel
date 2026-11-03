//! The stone panels: the fields of MCP `joints`. Each panel's button builds
//! the tool's MCP arguments over the selection's box (or the active object's
//! box) and runs the MCP handler, so the editor and MCP share one path.

use serde_json::{json, Value};

use super::geom::cell_bounds;
use super::{parse_slots, Session};
use crate::tools::JointPattern;

/// The Joints panel.
#[derive(Clone, PartialEq)]
pub struct JointsPanel {
    pub pattern: JointPattern,
    pub spacing: [f32; 2],
    pub course: [i32; 2],
    pub width: i32,
    pub depth: i32,
    /// 0 keeps the gap bottom's colour.
    pub dirt: i32,
    /// Palette indices separated by commas or spaces; empty cuts every slot.
    pub only: String,
    pub density: f32,
    pub seed: i32,
}

impl Default for JointsPanel {
    fn default() -> Self {
        Self {
            pattern: JointPattern::Columns,
            spacing: [4.0, 9.0],
            course: [10, 24],
            width: 1,
            depth: 2,
            dirt: 0,
            only: String::new(),
            density: 0.6,
            seed: 0,
        }
    }
}

impl JointsPanel {
    /// Spacing and course defaults for a newly picked pattern.
    pub fn set_pattern(&mut self, pattern: JointPattern) {
        let (spacing, course) = pattern.defaults();
        self.pattern = pattern;
        self.spacing = [spacing.0 as f32, spacing.1 as f32];
        if pattern != JointPattern::Cracks {
            self.course = [course.0, course.1];
        }
    }

    fn args(&self) -> Result<Value, String> {
        let mut args = json!({
            "pattern": self.pattern.name(),
            "spacing": [self.spacing[0], self.spacing[1].max(self.spacing[0])],
            "width": self.width,
            "depth": self.depth,
            "only": slots("Only", &self.only)?,
            "density": self.density,
            "seed": self.seed.max(0),
        });
        if self.pattern != JointPattern::Cracks {
            args["course"] = json!([self.course[0], self.course[1].max(self.course[0])]);
        }
        if self.dirt > 0 {
            args["dirt"] = json!(self.dirt);
        }
        Ok(args)
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct StonePanels {
    pub joints: JointsPanel,
}

fn slots(what: &str, text: &str) -> Result<Value, String> {
    parse_slots(text)
        .map(|list| json!(list))
        .map_err(|err| format!("{what}: {err}"))
}

impl Session {
    /// The Joints panel's button.
    pub fn joints_from_panel(&mut self) {
        let args = self.stone.joints.args();
        self.stone_from_panel("voxel.joints", args, |reply| {
            format!(
                "Cut {} joint cells, {} with dirt.",
                reply["removed"], reply["dirtied"]
            )
        });
    }

    /// Run a stone tool's MCP handler over the selection's box, or over the
    /// active object's box grown by `grow` cells, and report in the status.
    fn stone_from_panel(
        &mut self,
        method: &str,
        args: Result<Value, String>,
        report: impl FnOnce(&Value) -> String,
    ) {
        let mut args = match args {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let region = if self.picked.is_empty() {
            cell_bounds(self.active().cells.keys().copied()).map(|(min, max)| {
                (
                    crate::tools::add(min, (-1, -1, -1)),
                    crate::tools::add(max, (1, 1, 1)),
                )
            })
        } else {
            cell_bounds(self.picked.iter().copied())
        };
        let Some((min, max)) = region else {
            self.status = "Select a region or add voxels first.".into();
            return;
        };
        for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"]
            .into_iter()
            .zip([min.0, min.1, min.2, max.0, max.1, max.2])
        {
            args[key] = json!(value);
        }
        self.status = match crate::remote::apply(self, method, &args) {
            Ok(reply) => report(&reply),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }
}
