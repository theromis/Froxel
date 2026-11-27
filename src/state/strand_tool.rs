//! The Strand tool: place a strand's points on surfaces, then select a
//! strand to drag its points or the whole of it, insert and delete points,
//! and change its section from the tool panel. Every change goes through
//! the MCP strand code (`remote::strand_*`), and each finished gesture is
//! one undo step.
//!
//! Coordinates are the active object's local cells, as MCP `add_strand`
//! takes them: a whole number is a cell centre, so a point on a face is the
//! cell's centre moved half a cell out along the face.

use glam::Vec3;
use serde_json::{json, Value};

use super::*;
use crate::tools::Strand;

/// The add_strand fields the Strand tool panel edits; the rest (points,
/// palette) come from the gesture and the active slot.
pub(crate) const PANEL_FIELDS: [&str; 7] = [
    "width",
    "thickness",
    "step",
    "curve",
    "pattern",
    "period",
    "seed",
];

#[derive(Clone, Default)]
pub struct StrandTool {
    /// Points of the strand being placed, in local cells.
    pub placing: Vec<[f32; 3]>,
    /// The selected strand of the active object.
    pub selected: Option<usize>,
    /// The selected point of that strand.
    pub point: Option<usize>,
    /// Palette slots of a pattern, as typed: indices separated by commas or
    /// spaces.
    pub slots: String,
    drag: Option<StrandDrag>,
    /// The panel changed the selected strand and the change is not yet an
    /// undo step (a slider still held).
    panel_pending: bool,
}

#[derive(Clone)]
struct StrandDrag {
    /// What moves: a point being placed, a point of a strand, or a strand.
    what: Dragged,
    /// The plane the pointer moves in: through the grabbed point, facing
    /// the camera.
    origin: Vec3,
    normal: Vec3,
    /// The strand as it was at the press.
    start: Option<Strand>,
}

#[derive(Clone, Copy, PartialEq)]
enum Dragged {
    Placing(usize),
    Point(usize, usize),
    Whole(usize),
}

/// A local position in cell coordinates (whole = centre), rounded to an
/// eighth of a cell so typed and dragged values stay readable.
fn to_cells(geometric: Vec3) -> [f32; 3] {
    (geometric - Vec3::splat(0.5))
        .to_array()
        .map(|v| (v * 8.0).round() / 8.0)
}

fn to_geometric(at: [f32; 3]) -> Vec3 {
    Vec3::from_array(at) + Vec3::splat(0.5)
}

impl Session {
    /// The Strand tool works in Model mode on the edit view.
    pub(super) fn strand_tool_active(&self) -> bool {
        self.tool == Tool::Strand
            && self.editor_mode == EditorMode::Model
            && self.render_mode == RenderMode::Edit
    }

    fn strand_local_ray(&self, world_ray: Option<(Vec3, Vec3)>) -> Option<(Vec3, Vec3)> {
        let (origin, dir) = world_ray?;
        let (o, d) = self.ray_to_local(self.selected, origin, dir);
        Some((o, d.try_normalize()?))
    }

    /// The handle under a local ray: placing points first, then the
    /// selected strand's points. Nearest along the ray wins.
    fn strand_handle(&self, o: Vec3, d: Vec3) -> Option<Dragged> {
        let mut best: Option<(f32, Dragged)> = None;
        let mut test = |at: [f32; 3], what: Dragged| {
            let p = to_geometric(at);
            let t = (p - o).dot(d);
            if t <= 0.0 {
                return;
            }
            let reach = (t * 0.015).max(0.45);
            if (o + d * t).distance(p) <= reach && best.is_none_or(|(so_far, _)| t < so_far) {
                best = Some((t, what));
            }
        };
        for (n, at) in self.strand_tool.placing.iter().enumerate() {
            test(*at, Dragged::Placing(n));
        }
        if let Some(index) = self.strand_tool.selected {
            if let Some(strand) = self.active().strands.get(index) {
                for (n, p) in strand.points.iter().enumerate() {
                    test(p.at, Dragged::Point(index, n));
                }
            }
        }
        best.map(|(_, what)| what)
    }

    /// The nearest strand box of the active object under a local ray: the
    /// distance, the strand, and the local hit point.
    fn strand_under(&self, o: Vec3, d: Vec3) -> Option<(f32, usize, Vec3)> {
        let mut best: Option<(f32, usize)> = None;
        for (index, strand) in self.active().strands.iter().enumerate() {
            for b in strand.boxes() {
                let m = b.matrix();
                if let Some((t, _)) = crate::tools::ray_box(&m.inverse(), &m, o, d) {
                    if t > 0.0 && best.is_none_or(|(so_far, _)| t < so_far) {
                        best = Some((t, index));
                    }
                }
            }
        }
        best.map(|(t, index)| (t, index, o + d * t))
    }

    /// Where a click puts a new point: on the face of the cell or the
    /// strand it hits, else in the plane of the last point placed facing the
    /// camera, else on the ground plane of the object's lowest cell.
    fn strand_surface_point(&self, o: Vec3, d: Vec3) -> Option<[f32; 3]> {
        let cell = raycast(&self.active().cells, o, d);
        let cell_t = cell.as_ref().map(|hit| {
            let c = Vec3::new(hit.cell.0 as f32, hit.cell.1 as f32, hit.cell.2 as f32);
            (c + Vec3::splat(0.5) - o).dot(d)
        });
        let strand = self.strand_under(o, d);
        match (cell, strand) {
            (_, Some((t, _, point))) if cell_t.is_none_or(|ct| t < ct) => Some(to_cells(point)),
            (Some(hit), _) => {
                let centre = Vec3::new(hit.cell.0 as f32, hit.cell.1 as f32, hit.cell.2 as f32);
                let n = Vec3::new(
                    hit.normal.0 as f32,
                    hit.normal.1 as f32,
                    hit.normal.2 as f32,
                );
                Some(to_cells(centre + Vec3::splat(0.5) + n * 0.5))
            }
            _ => {
                let (origin, normal) = match self.strand_tool.placing.last() {
                    Some(last) => (to_geometric(*last), -d),
                    None => {
                        let floor = self.active().cells.keys().map(|c| c.2).min().unwrap_or(0);
                        (Vec3::new(0.0, 0.0, floor as f32), Vec3::Z)
                    }
                };
                plane_hit(o, d, origin, normal).map(to_cells)
            }
        }
    }

    /// A primary press with the Strand tool. Returns whether a drag starts.
    pub(super) fn strand_press(&mut self, world_ray: Option<(Vec3, Vec3)>, shift: bool) -> bool {
        let Some((o, d)) = self.strand_local_ray(world_ray) else {
            return false;
        };
        if let Some(what) = self.strand_handle(o, d) {
            let at = match what {
                Dragged::Placing(n) => self.strand_tool.placing[n],
                Dragged::Point(index, n) => {
                    self.strand_tool.point = Some(n);
                    self.active().strands[index].points[n].at
                }
                Dragged::Whole(_) => unreachable!("handles are points"),
            };
            let start = match what {
                Dragged::Point(index, _) => {
                    self.checkpoint();
                    Some(self.active().strands[index].clone())
                }
                _ => None,
            };
            self.strand_tool.drag = Some(StrandDrag {
                what,
                origin: to_geometric(at),
                normal: -d,
                start,
            });
            return true;
        }
        if self.strand_tool.placing.is_empty() {
            if let Some((_, index, point)) = self.strand_under(o, d) {
                if shift && self.strand_tool.selected == Some(index) {
                    self.insert_strand_point(index, to_cells(point));
                    return false;
                }
                self.select_strand(Some(index));
                self.checkpoint();
                self.strand_tool.drag = Some(StrandDrag {
                    what: Dragged::Whole(index),
                    origin: point,
                    normal: -d,
                    start: Some(self.active().strands[index].clone()),
                });
                return true;
            }
            if self.strand_tool.selected.is_some() {
                self.select_strand(None);
                return false;
            }
        }
        if let Some(at) = self.strand_surface_point(o, d) {
            self.strand_tool.placing.push(at);
            let n = self.strand_tool.placing.len();
            self.status = if n < 2 {
                "Strand: click the next point.".into()
            } else {
                format!("Strand: {n} points. Enter or Finish adds it, Esc cancels.")
            };
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        }
        false
    }

    /// The pointer moved with the button held after [`Self::strand_press`].
    pub(super) fn strand_drag(&mut self, world_ray: Option<(Vec3, Vec3)>) {
        let Some(drag) = self.strand_tool.drag.clone() else {
            return;
        };
        let Some((o, d)) = self.strand_local_ray(world_ray) else {
            return;
        };
        let Some(hit) = plane_hit(o, d, drag.origin, drag.normal) else {
            return;
        };
        match drag.what {
            Dragged::Placing(n) => {
                if let Some(at) = self.strand_tool.placing.get_mut(n) {
                    *at = to_cells(hit);
                    self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
                }
            }
            Dragged::Point(index, n) => {
                let args = json!({"index": index, "point": n, "at": to_cells(hit)});
                self.live_strand_edit(index, drag.start.as_ref(), &args);
            }
            Dragged::Whole(index) => {
                let delta = to_cells(hit - drag.origin + Vec3::splat(0.5));
                let args = json!({"index": index, "move": delta});
                self.live_strand_edit(index, drag.start.as_ref(), &args);
            }
        }
    }

    /// The button came up: the drag becomes one undo step.
    pub(super) fn strand_release(&mut self) {
        if let Some(drag) = self.strand_tool.drag.take() {
            if !matches!(drag.what, Dragged::Placing(_)) {
                self.commit();
            }
        }
    }

    /// Apply `args` (MCP `edit_strand`) to the strand as it was at the
    /// press, without an undo step of its own.
    fn live_strand_edit(&mut self, index: usize, start: Option<&Strand>, args: &Value) {
        let object = self.selected;
        let mut strands = self.objects[object].strands.clone();
        if let Some(start) = start {
            if let Some(slot) = strands.get_mut(index) {
                *slot = start.clone();
            }
        }
        match crate::remote::strand_edit(&mut strands, args)
            .and_then(|_| crate::tools::check_strands(&strands))
        {
            Ok(_) => {
                self.objects[object].strands = strands;
                self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
            }
            Err(err) => self.status = format!("Strand: {err}."),
        }
    }

    /// Select a strand of the active object (or none), and load its section
    /// into the tool panel.
    pub(crate) fn select_strand(&mut self, index: Option<usize>) {
        self.commit_strand_panel();
        self.strand_tool.selected = index;
        self.strand_tool.point = None;
        if let Some(strand) = index.and_then(|i| self.active().strands.get(i)).cloned() {
            let fields = crate::remote::strand_list(std::slice::from_ref(&strand))[0].clone();
            for key in PANEL_FIELDS {
                let value = fields.get(key).cloned();
                self.set_op_arg("add_strand", key, value);
            }
            if strand.slots.len() > 1 {
                self.strand_tool.slots = strand
                    .slots
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            self.status = format!(
                "Strand {} of {}: drag a point or the strand, Shift-click inserts a point, Delete removes.",
                index.unwrap_or(0) + 1,
                self.active().strands.len()
            );
        }
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    fn insert_strand_point(&mut self, index: usize, at: [f32; 3]) {
        // Before the point whose neighbour span passes nearest the click.
        let strand = &self.active().strands[index];
        let p = Vec3::from_array(at);
        let mut best = (f32::MAX, strand.points.len());
        for n in 0..strand.points.len() - 1 {
            let a = Vec3::from_array(strand.points[n].at);
            let b = Vec3::from_array(strand.points[n + 1].at);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1.0e-6)).clamp(0.0, 1.0);
            let dist = (a + ab * t).distance(p);
            if dist < best.0 {
                best = (dist, n + 1);
            }
        }
        let args = json!({"index": index, "insert": best.1, "at": at, "object": self.selected});
        match crate::remote::apply(self, "strand.edit", &args) {
            Ok(_) => {
                self.strand_tool.point = Some(best.1);
                self.status = "Strand: point inserted.".into();
            }
            Err(err) => self.status = format!("Strand: {err}."),
        }
    }

    /// The panel's add_strand arguments for a new strand of `points`.
    fn strand_args(&self, points: &[[f32; 3]]) -> Result<Value, String> {
        let mut extra = op_args::OpArgs::new();
        extra.insert("points".into(), json!(points));
        extra.insert("object".into(), json!(self.selected));
        if self.op_arg("add_strand", "pattern").is_null() {
            extra.insert("palette".into(), json!(self.active_slot.max(1)));
        } else {
            extra.insert("palettes".into(), json!(self.strand_slots()?));
        }
        self.op_call("add_strand", extra)
    }

    /// The pattern's slots as typed in the panel.
    pub(crate) fn strand_slots(&self) -> Result<Vec<u8>, String> {
        let slots: Vec<u8> = self
            .strand_tool
            .slots
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<u8>()
                    .ok()
                    .filter(|slot| *slot > 0)
                    .ok_or_else(|| format!("`{s}` is not a palette index 1..255"))
            })
            .collect::<Result<_, _>>()?;
        if slots.is_empty() {
            return Err("a pattern needs its palette slots, such as 2 3 4".into());
        }
        Ok(slots)
    }

    /// The strand being placed, as it would be added, for the viewport.
    pub(crate) fn placing_strand(&self) -> Option<Strand> {
        if self.strand_tool.placing.len() < 2 || !self.strand_tool_active() {
            return None;
        }
        let args = self.strand_args(&self.strand_tool.placing).ok()?;
        crate::remote::new_strand(&args, self.active_slot.max(1)).ok()
    }

    /// Enter or Finish: add the strand being placed, as one undo step.
    pub fn finish_strand(&mut self) {
        if self.strand_tool.placing.len() < 2 {
            self.status = "Strand: place at least two points.".into();
            return;
        }
        let result = self
            .strand_args(&self.strand_tool.placing)
            .and_then(|args| crate::remote::apply(self, "strand.add", &args));
        match result {
            Ok(reply) => {
                self.strand_tool.placing.clear();
                let index = reply["strand"]["index"].as_u64().map(|i| i as usize);
                self.select_strand(index);
            }
            Err(err) => self.status = format!("Strand: {err}."),
        }
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Esc: drop the points being placed, else the selection.
    pub fn cancel_strand(&mut self) {
        if !self.strand_tool.placing.is_empty() {
            self.strand_tool.placing.clear();
            self.status = "Strand cancelled.".into();
        } else {
            self.select_strand(None);
        }
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Delete or Backspace: the last point being placed, else the selected
    /// point (a strand keeps two), else the selected strand.
    pub fn delete_strand_part(&mut self) {
        if self.strand_tool.placing.pop().is_some() {
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
            return;
        }
        let Some(index) = self.strand_tool.selected else {
            return;
        };
        let object = self.selected;
        let points = self
            .active()
            .strands
            .get(index)
            .map_or(0, |s| s.points.len());
        let result = match self.strand_tool.point {
            Some(n) if points > 2 => {
                let args = json!({"object": object, "index": index, "remove": n});
                crate::remote::apply(self, "strand.edit", &args).map(|_| "Point removed.")
            }
            _ => {
                let args = json!({"object": object, "index": index});
                crate::remote::apply(self, "strand.remove", &args).map(|_| "Strand removed.")
            }
        };
        match result {
            Ok(done) => {
                self.status = done.into();
                if done == "Strand removed." {
                    self.select_strand(None);
                } else {
                    self.strand_tool.point = None;
                }
            }
            Err(err) => self.status = format!("Strand: {err}."),
        }
    }

    /// Give the selected strand the panel's section and colours, while a
    /// slider moves without an undo step, and as one step once it is let go.
    pub(crate) fn sync_strand_panel(&mut self, pointer_down: bool) {
        let Some(index) = self.strand_tool.selected else {
            return;
        };
        let object = self.selected;
        let Some(current) = self.objects[object].strands.get(index).cloned() else {
            self.strand_tool.selected = None;
            return;
        };
        let mut args = serde_json::Map::new();
        let pattern = self.op_arg("add_strand", "pattern");
        for key in PANEL_FIELDS {
            // A pattern's own option matters only with that pattern.
            let wanted = match key {
                "period" => pattern == "rings",
                "seed" => pattern == "random",
                _ => true,
            };
            let value = self.op_arg("add_strand", key);
            if wanted && !value.is_null() {
                args.insert(key.to_string(), value);
            }
        }
        if current.slots.len() > 1 || !self.op_arg("add_strand", "pattern").is_null() {
            if let Ok(slots) = self.strand_slots() {
                args.insert("palettes".into(), json!(slots));
            }
        }
        args.insert("index".into(), json!(index));
        let mut changed = vec![current.clone()];
        let mut probe = Value::Object(args);
        probe["index"] = json!(0);
        if crate::remote::strand_edit(&mut changed, &probe).is_err() || changed[0] == current {
            if !pointer_down {
                self.commit_strand_panel();
            }
            return;
        }
        if !self.strand_tool.panel_pending {
            self.commit();
            self.checkpoint();
            self.strand_tool.panel_pending = true;
        }
        self.objects[object].strands[index] = changed.remove(0);
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        if !pointer_down {
            self.commit_strand_panel();
        }
    }

    fn commit_strand_panel(&mut self) {
        if self.strand_tool.panel_pending {
            self.strand_tool.panel_pending = false;
            self.commit();
        }
    }

    /// Recolour the selected strand with the active slot.
    pub fn color_strand(&mut self) {
        let Some(index) = self.strand_tool.selected else {
            return;
        };
        self.set_op_arg("add_strand", "pattern", None);
        let args =
            json!({"object": self.selected, "index": index, "palette": self.active_slot.max(1)});
        if let Err(err) = crate::remote::apply(self, "strand.edit", &args) {
            self.status = format!("Strand: {err}.");
        }
    }

    /// The handles the viewport draws: world position and whether it is the
    /// selected point, for the selected strand's points and the points
    /// being placed (`placing`).
    pub fn strand_handles(&self) -> Vec<(Vec3, bool, bool)> {
        if !self.strand_tool_active() {
            return Vec::new();
        }
        let map = self.object_to_world(self.selected);
        let mut out = Vec::new();
        if let Some(index) = self.strand_tool.selected {
            if let Some(strand) = self.active().strands.get(index) {
                for (n, p) in strand.points.iter().enumerate() {
                    let world = map.transform_point3(to_geometric(p.at));
                    out.push((world, self.strand_tool.point == Some(n), false));
                }
            }
        }
        for at in &self.strand_tool.placing {
            out.push((map.transform_point3(to_geometric(*at)), false, true));
        }
        out
    }

    /// Forget a selection that no longer exists (another object became
    /// active, an undo removed the strand).
    pub(crate) fn settle_strand_tool(&mut self) {
        let count = self.active().strands.len();
        if self.strand_tool.selected.is_some_and(|i| i >= count) {
            self.strand_tool.selected = None;
            self.strand_tool.point = None;
        }
    }
}

/// Where a ray meets a plane, ahead of its origin.
fn plane_hit(o: Vec3, d: Vec3, origin: Vec3, normal: Vec3) -> Option<Vec3> {
    let denom = d.dot(normal);
    if denom.abs() < 1.0e-6 {
        return None;
    }
    let t = (origin - o).dot(normal) / denom;
    (t > 0.0).then(|| o + d * t)
}

#[cfg(test)]
#[path = "../../tests/strand_tool.rs"]
mod tests;
