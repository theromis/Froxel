//! Selection transforms, clipboard, and keyboard command names.

use std::collections::{HashMap, HashSet};

use super::geom::*;
use super::vox::*;
use super::*;

pub(super) enum Cmd {
    Menu(crate::menu::Action),
    Tab,
    Tool(Tool),
    Mode(ToolMode),
    DeletePicked,
    RotateObject(Axis),
    FlipObject(Axis),
}

impl Session {
    pub fn rotate_selection(&mut self, axis: Axis, clockwise: bool) {
        if !self.picked.is_empty() {
            let pivot = min_corner(self.picked.iter().copied());
            self.map_picked(|c| {
                let p = (c.0 - pivot.0, c.1 - pivot.1, c.2 - pivot.2);
                let q = rot90(p, axis, clockwise);
                (q.0 + pivot.0, q.1 + pivot.1, q.2 + pivot.2)
            });
            return;
        }
        if self.active().cells.is_empty() {
            return;
        }
        self.checkpoint();
        let cells = std::mem::take(&mut self.active_mut().cells);
        let pivot = min_corner(cells.keys().copied());
        let mut next = HashMap::with_capacity(cells.len());
        for (c, v) in cells {
            let p = (c.0 - pivot.0, c.1 - pivot.1, c.2 - pivot.2);
            let q = rot90(p, axis, clockwise);
            next.insert((q.0 + pivot.0, q.1 + pivot.1, q.2 + pivot.2), v);
        }
        self.active_mut().cells = next;
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// 90° about a world axis, around the center of the occupied span.
    /// Local cells move; the translation of that center stays put.
    pub fn rotate_object(&mut self, axis: Axis, clockwise: bool) {
        let name = axis_name(axis);
        let verb = if clockwise {
            "clockwise"
        } else {
            "counterclockwise"
        };
        self.transform_object(
            |c, pivot| {
                let p = (c.0 - pivot.0, c.1 - pivot.1, c.2 - pivot.2);
                let q = rot90(p, axis, clockwise);
                (q.0 + pivot.0, q.1 + pivot.1, q.2 + pivot.2)
            },
            axis_matrix(axis, clockwise),
            &format!("Rotated the object 90° {verb} about {name}."),
        );
    }

    /// Mirror across the plane through the span center, perpendicular to `axis`.
    pub fn flip_object(&mut self, axis: Axis) {
        let name = axis_name(axis);
        self.transform_object(
            |c, pivot| match axis {
                Axis::X => (pivot.0 * 2 - c.0, c.1, c.2),
                Axis::Y => (c.0, pivot.1 * 2 - c.1, c.2),
                Axis::Z => (c.0, c.1, pivot.2 * 2 - c.2),
            },
            mirror_matrix(axis),
            &format!("Flipped the object across {name}."),
        );
    }

    pub(super) fn transform_object(
        &mut self,
        map: impl Fn(Cell, Cell) -> Cell,
        turn: [[i32; 3]; 3],
        status: &str,
    ) {
        let members: Vec<usize> = self
            .object_selection
            .iter()
            .copied()
            .filter(|index| {
                self.objects
                    .get(*index)
                    .is_some_and(|volume| !volume.cells.is_empty())
            })
            .collect();
        let members = if members.is_empty() {
            if self.active().cells.is_empty() {
                return;
            }
            vec![self.selected]
        } else {
            members
        };
        let mut lo = (i32::MAX, i32::MAX, i32::MAX);
        let mut hi = (i32::MIN, i32::MIN, i32::MIN);
        for index in &members {
            let Some((min, max)) = world_extents(&self.objects[*index]) else {
                continue;
            };
            lo = (lo.0.min(min.0), lo.1.min(min.1), lo.2.min(min.2));
            hi = (hi.0.max(max.0), hi.1.max(max.1), hi.2.max(max.2));
        }
        let pivot = (
            span_center(lo.0, hi.0),
            span_center(lo.1, hi.1),
            span_center(lo.2, hi.2),
        );
        let mut rewritten = Vec::with_capacity(members.len());
        let mut changed = false;
        for index in &members {
            let volume = &self.objects[*index];
            let shift = volume.translation;
            let mut next = HashMap::with_capacity(volume.cells.len());
            for (cell, slot) in &volume.cells {
                let world = (cell.0 + shift.0, cell.1 + shift.1, cell.2 + shift.2);
                let moved = map(world, pivot);
                next.insert(
                    (moved.0 - shift.0, moved.1 - shift.1, moved.2 - shift.2),
                    *slot,
                );
            }
            changed |= next != volume.cells;
            rewritten.push((*index, next));
        }
        if !changed {
            return;
        }
        self.checkpoint();
        for (index, next) in rewritten {
            let volume = &mut self.objects[index];
            volume.cells = next;
            if volume.model_dims != (0, 0, 0) {
                let delta = [
                    volume.pivot.0 - pivot.0,
                    volume.pivot.1 - pivot.1,
                    volume.pivot.2 - pivot.2,
                ];
                let spun = rot_apply(turn, delta);
                volume.pivot = (pivot.0 + spun[0], pivot.1 + spun[1], pivot.2 + spun[2]);
                volume.rot = rot_mul(turn, volume.rot);
            }
        }
        self.clear_picked();
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.status = status.to_string();
    }

    pub fn flip_selection(&mut self, axis: Axis) {
        if !self.picked.is_empty() {
            let (min, max) = extents(self.picked.iter().copied());
            self.map_picked(|c| mirror(c, axis, min, max));
            return;
        }
        if self.active().cells.is_empty() {
            return;
        }
        self.checkpoint();
        let cells = std::mem::take(&mut self.active_mut().cells);
        let (min, max) = extents(cells.keys().copied());
        let mut next = HashMap::with_capacity(cells.len());
        for (c, v) in cells {
            next.insert(mirror(c, axis, min, max), v);
        }
        self.active_mut().cells = next;
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    pub fn delete_picked(&mut self) {
        if self.picked.is_empty() {
            return;
        }
        self.checkpoint();
        let picked = std::mem::take(&mut self.picked);
        {
            let map = &mut self.active_mut().cells;
            for cell in &picked {
                map.remove(cell);
            }
        }
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.note_selection();
    }

    /// Selected cells only, relative to their minimum corner. Empty slots in
    /// the bounding box are not stored, so paste does not erase neighbors.
    pub fn copy_picked(&mut self) {
        if self.picked.is_empty() {
            return;
        }
        let origin = min_corner(self.picked.iter().copied());
        let mut cells = Vec::new();
        for cell in &self.picked {
            let Some(&slot) = self.active().cells.get(cell) else {
                continue;
            };
            if slot == 0 {
                continue;
            }
            cells.push((
                (cell.0 - origin.0, cell.1 - origin.1, cell.2 - origin.2),
                slot,
            ));
        }
        if cells.is_empty() {
            return;
        }
        let n = cells.len();
        self.clipboard = Some(VoxelClipboard { origin, cells });
        self.status = format!("Copied {n} voxels.");
    }

    pub fn cut_picked(&mut self) {
        if self.picked.is_empty() {
            return;
        }
        self.copy_picked();
        let n = self
            .clipboard
            .as_ref()
            .map(|clip| clip.cells.len())
            .unwrap_or(0);
        self.delete_picked();
        if n > 0 {
            self.status = format!("Cut {n} voxels.");
        }
    }

    /// Writes the clipboard onto the hovered face, or back at its original
    /// origin when the pointer is not on a voxel. The pasted cells become
    /// the selection.
    pub fn paste_clipboard(&mut self) {
        let Some(clip) = self.clipboard.clone() else {
            return;
        };
        let origin = self.paste_origin(&clip);
        self.checkpoint();
        let mut pasted = HashSet::with_capacity(clip.cells.len());
        {
            let map = &mut self.active_mut().cells;
            for (offset, slot) in &clip.cells {
                let at = (
                    origin.0 + offset.0,
                    origin.1 + offset.1,
                    origin.2 + offset.2,
                );
                map.insert(at, *slot);
                pasted.insert(at);
            }
        }
        self.picked = pasted;
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.note_selection();
    }

    pub(super) fn paste_origin(&self, clip: &VoxelClipboard) -> Cell {
        match self.hover {
            Some(hit) => (
                hit.cell.0 + hit.normal.0,
                hit.cell.1 + hit.normal.1,
                hit.cell.2 + hit.normal.2,
            ),
            None => clip.origin,
        }
    }
}
