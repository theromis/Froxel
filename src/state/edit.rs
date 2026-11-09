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
        (in_box && in_slots) != self.invert
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
        if spec.boxes.is_empty() && spec.slots.is_none() {
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
        Ok(args)
    }
}

/// The fields of the Object edits sections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditPanels {
    pub split: SplitPanel,
    pub recolor: RecolorPanel,
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
                "Recoloured {} of {} matching cells.",
                reply["changed"], reply["matched"]
            ),
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
