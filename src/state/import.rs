//! The Import mesh panel: the fields of MCP `import_mesh`. Its button and
//! File > Import Mesh… build the tool's arguments and run the MCP handler,
//! so the editor and MCP share one path.

use serde_json::{json, Value};

use super::{dialog, Session};

#[derive(Clone, PartialEq)]
pub struct ImportPanel {
    pub path: String,
    /// 0 size in cells, 1 voxel size in metres, 2 cells per unit.
    pub fit: usize,
    pub size: i32,
    pub voxel_size: f32,
    pub scale: f32,
    pub z_up: bool,
    pub rotation: [f32; 3],
    pub position: [i32; 3],
    /// 0 min, 1 center, 2 bottom.
    pub anchor: usize,
    /// 0 solid, 1 parity, 2 shell.
    pub fill: usize,
    pub vertex_only: bool,
    /// Palette indices to map to; empty quantizes into the palette.
    pub slots: String,
    pub max_new: i32,
    pub tolerance: f32,
    /// 0 replace, 1 empty, 2 paint.
    pub mode: usize,
    pub new_object: bool,
    /// MCP `orient`: 0 none, 1 upright (PCA), 2 the Carve from views views,
    /// 3 upright and then the views' yaw.
    pub orient: usize,
    /// Degrees `upright` may tilt the mesh's up (MCP `maxTilt`).
    pub max_tilt: f32,
    /// Reduce and despeckle the colours with the Reduce palette section's
    /// options (MCP `clean`).
    pub clean: bool,
}

impl Default for ImportPanel {
    fn default() -> Self {
        Self {
            path: String::new(),
            fit: 0,
            size: crate::remote::DEFAULT_IMPORT_SIZE as i32,
            voxel_size: 0.08,
            scale: 12.5,
            z_up: false,
            rotation: [0.0; 3],
            position: [0; 3],
            anchor: 0,
            fill: 0,
            vertex_only: false,
            slots: String::new(),
            max_new: crate::remote::DEFAULT_MAX_NEW_SLOTS as i32,
            tolerance: crate::remote::DEFAULT_TOLERANCE as f32,
            mode: 0,
            new_object: true,
            orient: 0,
            max_tilt: 40.0,
            clean: false,
        }
    }
}

impl ImportPanel {
    pub(crate) fn args(&self) -> Result<Value, String> {
        if self.path.trim().is_empty() {
            return Err("Choose a .glb, .gltf, or .obj to import.".into());
        }
        let anchor = ["min", "center", "bottom"][self.anchor.min(2)];
        let mode = ["replace", "empty", "paint"][self.mode.min(2)];
        let fill = ["solid", "parity", "shell"][self.fill.min(2)];
        let mut args = json!({
            "path": self.path.trim(),
            "upAxis": if self.z_up { "z" } else { "y" },
            "rotation": self.rotation,
            "position": self.position,
            "anchor": anchor,
            "fill": fill,
            "colors": if self.vertex_only { "vertex" } else { "auto" },
            "maxNewSlots": self.max_new.clamp(0, 255),
            "tolerance": self.tolerance,
            "mode": mode,
            "newObject": self.new_object,
        });
        match self.fit {
            1 => args["voxelSize"] = json!(self.voxel_size),
            2 => args["scale"] = json!(self.scale),
            _ => args["size"] = json!(self.size),
        }
        let slots = super::parse_slots(&self.slots).map_err(|err| format!("Slots: {err}"))?;
        if !slots.is_empty() {
            args["slots"] = json!(slots);
        }
        Ok(args)
    }
}

impl Session {
    /// File > Import Mesh…: pick a mesh and import it with the panel's options.
    pub fn import_mesh_dialog(&mut self) {
        let Some(path) = dialog::pick_open("Mesh", &["glb", "gltf", "obj"]) else {
            return;
        };
        self.import.path = path.display().to_string();
        self.import_from_panel();
    }

    /// The Import mesh panel's button: MCP `import_mesh`.
    pub fn import_from_panel(&mut self) {
        let args = self.import.args().and_then(|mut args| {
            match self.import.orient {
                1 => {
                    args["orient"] = json!({"upright": true, "maxTilt": self.import.max_tilt});
                }
                2 | 3 => {
                    let mut search = self.silhouette.orient.search_args(&self.silhouette.carve)?;
                    if self.import.orient == 3 {
                        search["upright"] = json!(true);
                        search["maxTilt"] = json!(self.import.max_tilt);
                        search["turns"] = json!("upright");
                    }
                    args["orient"] = search;
                }
                _ => {}
            }
            if self.import.clean {
                args["clean"] = self.edit.reduce.args(None)?;
            }
            Ok(args)
        });
        let args = match args {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "mesh.import", &args) {
            Ok(reply) => {
                let size = &reply["bounds"]["size"];
                let mut status = format!(
                    "Imported {} cells ({} inside, {} x {} x {}) into {}, {} slots, {} new.",
                    reply["written"],
                    reply["interior"],
                    size[0],
                    size[1],
                    size[2],
                    reply["name"].as_str().unwrap_or("the object"),
                    reply["slots"].as_array().map_or(0, Vec::len),
                    reply["newSlots"].as_array().map_or(0, Vec::len),
                );
                let open = reply["openEdges"].as_u64().unwrap_or(0);
                if open > 0 {
                    status.push_str(&format!(" The mesh has {open} open edges."));
                }
                if let Some(orientation) = reply.get("orientation") {
                    status.push(' ');
                    status.push_str(&super::silhouette::orient_status(
                        orientation,
                        &orientation["rotation"],
                    ));
                }
                if let Some(warnings) = reply["warnings"].as_array() {
                    for warning in warnings.iter().filter_map(Value::as_str) {
                        status.push(' ');
                        status.push_str(warning);
                    }
                }
                status
            }
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }
}
