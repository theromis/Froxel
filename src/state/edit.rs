//! Whole-object edits: MCP `split_object` and `clear_object`, and the
//! fields of the Object edits section, whose buttons build the MCP arguments and run the MCP
//! handler.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::geom::*;
use super::vox::*;
use super::*;

/// Where `split_object` puts the cells it takes.
#[derive(Clone, Debug, PartialEq)]
pub enum SplitInto {
    /// An existing object, by index.
    Object(usize),
    /// A new object with this name, at the source's position.
    New(String),
}

/// The cells `split_object` takes from one object: those inside any of
/// `boxes` (local to the source, inclusive) and of one of `slots`, or every
/// other cell with `invert`.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitSpec {
    pub source: usize,
    pub boxes: Vec<(Cell, Cell)>,
    pub slots: Option<Vec<u8>>,
    /// Only these cells (MCP `components` with op `split`).
    pub cells: Option<std::collections::HashSet<Cell>>,
    pub invert: bool,
    /// Leave the cells in the source too.
    pub copy: bool,
    pub into: SplitInto,
}

/// What `split_object` did.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub target: usize,
    pub created: bool,
    /// Cells taken from the source.
    pub cells: usize,
    /// Cells of the target that held another slot before.
    pub replaced: usize,
}

impl Session {
    /// Empty one object, whatever its size: no box limit. Its name,
    /// position, bone, script, and markers stay. One undo step; an empty
    /// object is no step. Returns how many cells were removed.
    pub fn clear_object(&mut self, index: usize) -> Result<usize, String> {
        let count = self.objects.len();
        let object = self
            .objects
            .get(index)
            .ok_or_else(|| format!("object index {index} is out of range ({count} objects)"))?;
        let removed = object.cells.len();
        if removed == 0 {
            return Ok(0);
        }
        self.commit();
        self.checkpoint();
        self.objects[index].cells.clear();
        self.clear_picked();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok(removed)
    }

    /// The Clear object button: MCP `clear_object` on the active object.
    pub fn clear_from_panel(&mut self) {
        let args = json!({"index": self.selected});
        self.status = match crate::remote::apply(self, "object.clear", &args) {
            Ok(reply) => format!("Cleared {} cells of {}.", reply["removed"], reply["name"]),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }
}

impl SplitSpec {
    /// Whether the source's local cell `cell` of slot `slot` is taken.
    pub fn takes(&self, cell: Cell, slot: u8) -> bool {
        let in_box = self.boxes.is_empty()
            || self.boxes.iter().any(|(min, max)| {
                (min.0..=max.0).contains(&cell.0)
                    && (min.1..=max.1).contains(&cell.1)
                    && (min.2..=max.2).contains(&cell.2)
            });
        let in_slots = self
            .slots
            .as_ref()
            .is_none_or(|slots| slots.contains(&slot));
        let in_cells = self
            .cells
            .as_ref()
            .is_none_or(|cells| cells.contains(&cell));
        (in_box && in_slots && in_cells) != self.invert
    }
}

impl Session {
    /// Move (or copy) the cells of one object inside boxes or of some slots
    /// into a new or an existing object. Each cell keeps its world position:
    /// a new object takes the source's position, so its local cells are the
    /// source's; an existing one takes them at its own offset. One undo step.
    pub fn split_object(&mut self, spec: &SplitSpec) -> Result<Split, String> {
        let count = self.objects.len();
        if spec.source >= count {
            return Err(format!(
                "object index {} is out of range ({count} objects)",
                spec.source
            ));
        }
        if spec.boxes.is_empty() && spec.slots.is_none() && spec.cells.is_none() {
            return Err("pass boxes, slots, or both: the cells to split off".into());
        }
        let new_name = match &spec.into {
            SplitInto::Object(target) => {
                if *target >= count {
                    return Err(format!(
                        "into object {target} is out of range ({count} objects)"
                    ));
                }
                if *target == spec.source {
                    return Err("into is the source object itself".into());
                }
                None
            }
            SplitInto::New(name) => Some(super::objects::check_object_name(name)?),
        };
        let source = &self.objects[spec.source];
        let offset = source.translation;
        let taken: Vec<(Cell, u8)> = source
            .cells
            .iter()
            .filter(|(cell, slot)| spec.takes(**cell, **slot))
            .map(|(cell, slot)| (*cell, *slot))
            .collect();
        if taken.is_empty() {
            return Err(format!(
                "the boxes and slots select no cell of `{}`",
                source.name
            ));
        }
        let (target_offset, mut cells) = match &spec.into {
            SplitInto::Object(target) => {
                let object = &self.objects[*target];
                (object.translation, object.cells.clone())
            }
            SplitInto::New(_) => (offset, HashMap::new()),
        };
        let mut replaced = 0;
        for (cell, slot) in &taken {
            let local = (
                cell.0 + offset.0 - target_offset.0,
                cell.1 + offset.1 - target_offset.1,
                cell.2 + offset.2 - target_offset.2,
            );
            if !in_world(local.0, local.1, local.2) {
                return Err("a split cell is outside the world [-1024, 1024) of the target".into());
            }
            if cells.insert(local, *slot).is_some_and(|old| old != *slot) {
                replaced += 1;
            }
        }
        if let Some((min, max)) = cell_bounds(cells.keys().copied()) {
            let span = [max.0 - min.0 + 1, max.1 - min.1 + 1, max.2 - min.2 + 1];
            if span.iter().any(|axis| *axis > 256) {
                return Err(format!(
                    "the target would be {}x{}x{}; .vox models are at most 256 per axis",
                    span[0], span[1], span[2]
                ));
            }
        }
        self.commit();
        self.checkpoint();
        if !spec.copy {
            let source = &mut self.objects[spec.source].cells;
            for (cell, _) in &taken {
                source.remove(cell);
            }
        }
        let (target, created) = match new_name {
            None => {
                let SplitInto::Object(target) = spec.into else {
                    unreachable!("an existing target has no new name");
                };
                self.objects[target].cells = cells;
                (target, false)
            }
            Some(name) => {
                let hidden = self.objects[spec.source].hidden;
                self.objects.push(Volume {
                    name,
                    cells,
                    translation: offset,
                    box_min: (0, 0, 0),
                    dims: (0, 0, 0),
                    rot: ROT_IDENTITY,
                    pivot: (0, 0, 0),
                    model_dims: (0, 0, 0),
                    hidden,
                    file: None,
                    bone: None,
                    script: Default::default(),
                });
                (self.objects.len() - 1, true)
            }
        };
        self.clear_picked();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok(Split {
            target,
            created,
            cells: taken.len(),
            replaced,
        })
    }
}

/// The Split object section: the cells of the active object to move into
/// another object.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitPanel {
    /// Boxes `x0 y0 z0 x1 y1 z1`, one per line or separated by `;`.
    pub boxes: String,
    /// Slots, separated by spaces; empty is every slot.
    pub slots: String,
    pub invert: bool,
    pub copy: bool,
    /// The boxes are world cells (MCP `space: "world"`).
    pub world: bool,
    /// `None` makes a new object named `name`; else the target's index.
    pub into: Option<usize>,
    pub name: String,
}

impl Default for SplitPanel {
    fn default() -> Self {
        Self {
            boxes: String::new(),
            slots: String::new(),
            invert: false,
            copy: false,
            world: false,
            into: None,
            name: "part".into(),
        }
    }
}

impl SplitPanel {
    /// The MCP `split_object` arguments for the active object `source`.
    pub(crate) fn args(&self, source: usize) -> Result<Value, String> {
        let mut args = json!({"index": source, "invert": self.invert, "copy": self.copy});
        let mut boxes = Vec::new();
        for line in self.boxes.split(['\n', ';']) {
            let parts: Vec<&str> = line
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .collect();
            if parts.is_empty() {
                continue;
            }
            if parts.len() != 6 {
                return Err(format!(
                    "Split: a box is 6 whole numbers, not `{}`",
                    line.trim()
                ));
            }
            let corners = parts
                .iter()
                .map(|part| {
                    part.parse::<i32>()
                        .map_err(|_| format!("Split: `{part}` is not a whole number"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            boxes.push(corners);
        }
        if !boxes.is_empty() {
            args["boxes"] = json!(boxes);
            if self.world {
                args["space"] = json!("world");
            }
        }
        let slots = slot_field("Split", &self.slots)?;
        if !slots.is_empty() {
            args["slots"] = json!(slots);
        }
        match self.into {
            Some(target) => args["into"] = json!(target),
            None => args["name"] = json!(self.name.trim()),
        }
        Ok(args)
    }
}

/// Slots 1..255 separated by spaces or commas.
pub(crate) fn slot_field(what: &str, text: &str) -> Result<Vec<u8>, String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<u8>()
                .ok()
                .filter(|slot| *slot > 0)
                .ok_or_else(|| format!("{what}: `{part}` is not a slot 1..255"))
        })
        .collect()
}

/// The Recolor section: palette slot pairs `from>to`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecolorPanel {
    /// Pairs such as `3>5 4>6`, separated by spaces or commas.
    pub map: String,
    pub mirror: MirrorField,
}

impl RecolorPanel {
    /// The MCP `recolor` arguments over the inclusive box `region`, or the
    /// whole object.
    pub(crate) fn args(&self, region: Option<(Cell, Cell)>) -> Result<Value, String> {
        let mut pairs = Vec::new();
        for part in self
            .map
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
        {
            let (from, to) = part
                .split_once('>')
                .ok_or_else(|| format!("Recolor: `{part}` is not from>to, such as 3>5"))?;
            let slot = |text: &str| {
                text.trim()
                    .parse::<u8>()
                    .map_err(|_| format!("Recolor: `{text}` is not a slot 0..255"))
            };
            pairs.push([slot(from)?, slot(to)?]);
        }
        if pairs.is_empty() {
            return Err("Recolor: give pairs from>to, such as 3>5".into());
        }
        let mut args = json!({"map": pairs});
        if let Some((min, max)) = region {
            for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"]
                .iter()
                .zip([min.0, min.1, min.2, max.0, max.1, max.2])
            {
                args[*key] = json!(value);
            }
        }
        self.mirror.write(&mut args)?;
        Ok(args)
    }
}

/// The Intersect section: a shape over the selection's box that the active
/// object is cut to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntersectPanel {
    /// 0 box, 1 ellipsoid, 2 rounded box.
    pub shape: usize,
    pub radius: f32,
}

/// The shapes of the Intersect section and their MCP methods.
pub const INTERSECT_SHAPES: [(&str, &str); 3] = [
    ("Box", "voxel.fill"),
    ("Ellipsoid", "voxel.ellipsoid"),
    ("Rounded box", "voxel.rounded"),
];

impl IntersectPanel {
    /// The MCP arguments of the shape's fill with `mode: "intersect"` over
    /// the inclusive box `min`..`max`, and its method.
    pub(crate) fn args(&self, min: Cell, max: Cell) -> (&'static str, Value) {
        let (_, method) = INTERSECT_SHAPES[self.shape.min(INTERSECT_SHAPES.len() - 1)];
        let mut args = json!({
            "x0": min.0, "y0": min.1, "z0": min.2,
            "x1": max.0, "y1": max.1, "z1": max.2,
            "mode": "intersect"
        });
        if method == "voxel.rounded" {
            args["radius"] = json!(self.radius.max(0.0));
        }
        (method, args)
    }
}

/// The Mirror row of the paint and edit sections (MCP `mirror`): off, or
/// an axis and a plane, the object's centre when `at` is empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MirrorField {
    pub on: bool,
    /// 0 x, 1 y, 2 z.
    pub axis: usize,
    /// The plane's coordinate in cells; empty is the object's centre.
    pub at: String,
}

impl MirrorField {
    /// The MCP `mirror` argument, or `None` when off.
    pub(crate) fn arg(&self) -> Result<Option<Value>, String> {
        if !self.on {
            return Ok(None);
        }
        let axis = ["x", "y", "z"][self.axis.min(2)];
        let at = self.at.trim();
        if at.is_empty() {
            return Ok(Some(json!({ "axis": axis })));
        }
        let value: f64 = at
            .parse()
            .map_err(|_| format!("Mirror: `{at}` is not a number"))?;
        crate::tools::Mirror::at(self.axis.min(2), value)?;
        Ok(Some(json!({ "axis": axis, "at": value })))
    }

    /// Put the argument into `args` when on.
    pub(crate) fn write(&self, args: &mut Value) -> Result<(), String> {
        if let Some(mirror) = self.arg()? {
            args["mirror"] = mirror;
        }
        Ok(())
    }

    /// The plane for a viewport edit of `cells`: `at`, or their centre.
    pub(crate) fn plane(&self, cells: &HashMap<Cell, u8>) -> Option<crate::tools::Mirror> {
        if !self.on {
            return None;
        }
        let axis = self.axis.min(2);
        match self.at.trim().parse::<f64>() {
            Ok(at) => crate::tools::Mirror::at(axis, at).ok(),
            Err(_) => crate::tools::Mirror::about_centre(axis, cells.keys().copied()),
        }
    }
}

/// The Surface stroke section: a stroke through points that snaps to the
/// active object's surface.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokePanel {
    /// One point `x y z` per line.
    pub points: String,
    pub smooth: bool,
    pub width: f32,
    pub depth: i32,
    pub reach: f32,
    /// Erase instead of painting the active slot.
    pub groove: bool,
    /// Slots that may change; empty is every slot.
    pub only: String,
    /// The points are world cells (MCP `space: "world"`).
    pub world: bool,
    pub mirror: MirrorField,
}

impl Default for StrokePanel {
    fn default() -> Self {
        Self {
            points: String::new(),
            smooth: true,
            width: 1.0,
            depth: 1,
            reach: 8.0,
            groove: false,
            only: String::new(),
            world: false,
            mirror: MirrorField::default(),
        }
    }
}

impl StrokePanel {
    /// The MCP `surface_stroke` arguments, painting `active` unless a groove.
    pub(crate) fn args(&self, active: u8) -> Result<Value, String> {
        let mut points = Vec::new();
        for line in self.points.lines() {
            let parts: Vec<&str> = line
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .collect();
            if parts.is_empty() {
                continue;
            }
            if parts.len() != 3 {
                return Err(format!("Stroke: a point is x y z, not `{}`", line.trim()));
            }
            let point = parts
                .iter()
                .map(|part| {
                    part.parse::<f64>()
                        .map_err(|_| format!("Stroke: `{part}` is not a number"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            points.push(point);
        }
        if points.len() < 2 {
            return Err("Stroke: give at least two points, one per line.".into());
        }
        let mut args = json!({
            "points": points,
            "palette": if self.groove { 0 } else { active.max(1) },
            "curve": if self.smooth { "smooth" } else { "polyline" },
            "width": self.width,
            "depth": self.depth.max(1),
            "reach": self.reach,
        });
        let only = slot_field("Stroke", &self.only)?;
        if !only.is_empty() {
            args["only"] = json!(only);
        }
        if self.world {
            args["space"] = json!("world");
        }
        self.mirror.write(&mut args)?;
        Ok(args)
    }
}

/// `, N on the mirrored side` when a reply has `mirrored`.
pub(crate) fn mirrored_note(reply: &Value) -> String {
    match reply.get("mirrored").and_then(Value::as_u64) {
        Some(count) => format!(", {count} more on the mirrored side"),
        None => String::new(),
    }
}

/// The Components section: connected parts of the active object.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentsPanel {
    /// 6, 18, or 26.
    pub connectivity: u8,
    pub keep: u32,
    pub min_cells: u32,
    /// `x y z` of the part to act on, and of the part that stays.
    pub seed: String,
    pub anchor: String,
    pub offset: [i32; 3],
    /// Name of the object Split makes.
    pub name: String,
}

impl Default for ComponentsPanel {
    fn default() -> Self {
        Self {
            connectivity: 26,
            keep: 1,
            min_cells: 8,
            seed: String::new(),
            anchor: String::new(),
            offset: [0, 0, 0],
            name: String::new(),
        }
    }
}

/// A point `x y z` typed in a panel, or `None` when the field is empty.
fn point_field(what: &str, text: &str) -> Result<Option<Vec<f64>>, String> {
    let parts: Vec<&str> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return Ok(None);
    }
    if parts.len() != 3 {
        return Err(format!("{what}: a cell is x y z, not `{}`", text.trim()));
    }
    parts
        .iter()
        .map(|part| {
            part.parse::<f64>()
                .map_err(|_| format!("{what}: `{part}` is not a number"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

impl ComponentsPanel {
    /// The MCP `components` arguments of `op`.
    pub(crate) fn args(&self, op: &str) -> Result<Value, String> {
        let mut args = json!({"op": op, "connectivity": self.connectivity});
        match op {
            "keep_largest" => args["keep"] = json!(self.keep.max(1)),
            "remove_small" => args["minCells"] = json!(self.min_cells),
            "move" => args["offset"] = json!(self.offset),
            _ => {}
        }
        if matches!(op, "delete" | "move" | "split") {
            let seed = point_field("Seed", &self.seed)?
                .ok_or("Components: set a seed cell of the part first (Seed at selection).")?;
            args["seed"] = json!(seed);
            if let Some(anchor) = point_field("Anchor", &self.anchor)? {
                args["anchor"] = json!(anchor);
            }
        }
        if op == "split" && !self.name.trim().is_empty() {
            args["name"] = json!(self.name.trim());
        }
        Ok(args)
    }
}

/// The fields of the Object edits sections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditPanels {
    pub components: ComponentsPanel,
    pub split: SplitPanel,
    pub recolor: RecolorPanel,
    pub intersect: IntersectPanel,
    pub stroke: StrokePanel,
    /// Mirror of the viewport's brush and shape tools.
    pub viewport_mirror: MirrorField,
    /// The palette dock's hex field for the active slot.
    pub hex: String,
    /// The Bones panel's joint snap, an index of none, corner, center, half.
    pub joint_snap: usize,
}

impl Session {
    /// The Recolor button: MCP `recolor` over the selection's box, or the
    /// whole active object.
    pub fn recolor_from_panel(&mut self) {
        let region = cell_bounds(self.picked.iter().copied());
        let args = match self.edit.recolor.args(region) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.recolor", &args) {
            Ok(reply) => format!(
                "Recoloured {} of {} matching cells{}.",
                reply["changed"],
                reply["matched"],
                mirrored_note(&reply)
            ),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The Intersect button: the active object cut to the shape inscribed
    /// in the selection's box (MCP fill with `mode: "intersect"`).
    pub fn intersect_from_panel(&mut self) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Intersect: select the box the shape fills first.".into();
            return;
        };
        let (method, args) = self.edit.intersect.args(min, max);
        self.status = match crate::remote::apply(self, method, &args) {
            Ok(reply) => format!("Kept the cells inside; {} removed.", reply["changed"]),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// A Components button: MCP `components` with `op` on the active object.
    pub fn components_from_panel(&mut self, op: &str) {
        let args = match self.edit.components.args(op) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.components", &args) {
            Ok(reply) => {
                let sizes: Vec<String> = reply["components"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(6)
                    .map(|part| part["cells"].to_string())
                    .collect();
                let mut text = format!(
                    "{} parts ({}{}).",
                    reply["count"],
                    sizes.join(", "),
                    if reply["count"].as_u64().unwrap_or(0) > 6 {
                        ", ..."
                    } else {
                        ""
                    }
                );
                match op {
                    "list" => {}
                    "split" => text.push_str(&format!(
                        " Split {} cells into {}.",
                        reply["selected"], reply["name"]
                    )),
                    _ => text.push_str(&format!(" {} cells changed.", reply["changed"])),
                }
                if reply.get("cut").is_some() {
                    text.push_str(&format!(" Neck cut: {} cells.", reply["cut"]));
                }
                text
            }
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Set the Components seed (or anchor) to the selection's centre.
    pub fn components_cell_at_selection(&mut self, anchor: bool) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Components: select cells first.".into();
            return;
        };
        let centre = |a: i32, b: i32| (a + b).div_euclid(2);
        let line = format!(
            "{} {} {}",
            centre(min.0, max.0),
            centre(min.1, max.1),
            centre(min.2, max.2)
        );
        let panel = &mut self.edit.components;
        if anchor {
            panel.anchor = line;
        } else {
            panel.seed = line;
        }
    }

    /// The Stroke button: MCP `surface_stroke` on the active object.
    pub fn stroke_from_panel(&mut self) {
        let args = match self.edit.stroke.args(self.active_slot) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.surface_stroke", &args) {
            Ok(reply) => {
                let mut text = format!(
                    "Stroked {} cells ({} changed){}.",
                    reply["cells"],
                    reply["changed"],
                    mirrored_note(&reply)
                );
                if reply["missed"].as_u64().unwrap_or(0) > 0 {
                    text.push_str(&format!(
                        " {} samples found no surface within reach.",
                        reply["missed"]
                    ));
                }
                text
            }
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Add the selection's centre as the stroke's next point.
    pub fn stroke_point_at_selection(&mut self) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Stroke: select cells first.".into();
            return;
        };
        let centre = |a: i32, b: i32| f64::from(a + b) / 2.0;
        let line = format!(
            "{} {} {}",
            centre(min.0, max.0),
            centre(min.1, max.1),
            centre(min.2, max.2)
        );
        let points = &mut self.edit.stroke.points;
        if !points.is_empty() && !points.ends_with('\n') {
            points.push('\n');
        }
        points.push_str(&line);
    }

    /// The palette dock's hex field: MCP `set_palette` with `color` on the
    /// active slot.
    pub fn palette_hex_from_panel(&mut self) {
        let args = json!({"index": self.active_slot, "color": self.edit.hex.trim()});
        self.status = match crate::remote::apply(self, "palette.set", &args) {
            Ok(_) => {
                self.edit.hex.clear();
                format!(
                    "Slot {} is {}.",
                    self.active_slot,
                    args["color"].as_str().unwrap_or("")
                )
            }
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The Split object button: MCP `split_object` on the active object.
    pub fn split_from_panel(&mut self) {
        let args = match self.edit.split.args(self.selected) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "object.split", &args) {
            Ok(reply) => format!(
                "Split {} cells into {} (object {}).",
                reply["cells"], reply["name"], reply["target"]
            ),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }
}
