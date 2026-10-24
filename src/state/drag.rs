//! Moving a selection or an object, and resizing a volume.

use glam::Vec3;

use super::geom::*;
use super::vox::*;
use super::*;

impl Session {
    /// Cmd-drag. The clicked face's axis stays fixed; the selection slides
    /// on that plane by whole voxels. Returns false when the click missed
    /// the selection, so the active tool still runs.
    pub(super) fn begin_move(&mut self, ray: Option<(Vec3, Vec3)>) -> bool {
        if self.picked.is_empty()
            || self.editor_mode != EditorMode::Model
            || self.render_mode != RenderMode::Edit
        {
            return false;
        }
        let Some((origin, dir)) = ray else {
            return false;
        };
        let Some(hit) = raycast(&self.active().cells, origin, dir) else {
            return false;
        };
        if !self.picked.contains(&hit.cell) {
            return false;
        }
        let (axis, sign) = face_axis(hit.normal);
        let plane = match axis {
            0 => hit.cell.0 as f32 + if sign > 0 { 1.0 } else { 0.0 },
            1 => hit.cell.1 as f32 + if sign > 0 { 1.0 } else { 0.0 },
            _ => hit.cell.2 as f32 + if sign > 0 { 1.0 } else { 0.0 },
        };
        let Some(anchor) = intersect_axis_plane(origin, dir, axis, plane) else {
            return false;
        };
        self.checkpoint();
        self.move_drag = Some(MoveDrag {
            axis,
            plane,
            anchor,
            base_cells: self.active().cells.clone(),
            base_picked: self.picked.iter().copied().collect(),
            applied: (0, 0, 0),
        });
        true
    }

    pub(super) fn drag_move(&mut self, ray: Option<(Vec3, Vec3)>) {
        let Some(drag) = self.move_drag.as_ref() else {
            return;
        };
        let Some((origin, dir)) = ray else {
            return;
        };
        let Some(hit) = intersect_axis_plane(origin, dir, drag.axis, drag.plane) else {
            return;
        };
        let delta = plane_delta(drag.axis, drag.anchor, hit);
        if delta == drag.applied {
            return;
        }
        let axis = drag.axis;
        let plane = drag.plane;
        let anchor = drag.anchor;
        let base_cells = drag.base_cells.clone();
        let base_picked = drag.base_picked.clone();
        let (next, picked) = translate_cells(&base_cells, &base_picked, delta);
        self.active_mut().cells = next;
        self.picked = picked;
        self.move_drag = Some(MoveDrag {
            axis,
            plane,
            anchor,
            base_cells,
            base_picked,
            applied: delta,
        });
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.selection_epoch = self.selection_epoch.wrapping_add(1);
    }

    /// Object mode. A hit keeps the rubber-band set when the object is
    /// already in it, and slides every member by the same voxel steps.
    /// Shift toggles membership and does not drag. A miss starts a marquee.
    pub(super) fn begin_object_move(
        &mut self,
        ray: Option<(Vec3, Vec3)>,
        shift: bool,
    ) -> ObjectPress {
        let Some((origin, dir)) = ray else {
            return ObjectPress::Idle;
        };
        let aimed = self.pick_object(origin, dir);
        if shift {
            if let Some((index, _)) = aimed {
                self.toggle_object(index);
            }
            return ObjectPress::Idle;
        }
        let locked = self.hover_gizmo.filter(|axis| (0..3).contains(axis));
        let Some((index, hit)) = aimed.or_else(|| {
            if locked.is_some() {
                Some((
                    self.selected,
                    Hit {
                        cell: (0, 0, 0),
                        normal: (0, 0, 1),
                    },
                ))
            } else {
                None
            }
        }) else {
            return ObjectPress::Marquee;
        };
        if index >= self.objects.len() {
            return ObjectPress::Idle;
        }
        if self.object_selection.contains(&index) {
            if index != self.selected {
                self.selected = index;
                self.clear_picked();
            }
        } else {
            self.select_object(index);
        }
        let translation = self.objects[index].translation;
        let (plane_axis, sign) = face_axis(hit.normal);
        let local_plane = match plane_axis {
            0 => hit.cell.0 as f32 + if sign > 0 { 1.0 } else { 0.0 },
            1 => hit.cell.1 as f32 + if sign > 0 { 1.0 } else { 0.0 },
            _ => hit.cell.2 as f32 + if sign > 0 { 1.0 } else { 0.0 },
        };
        let plane = local_plane
            + match plane_axis {
                0 => translation.0 as f32,
                1 => translation.1 as f32,
                _ => translation.2 as f32,
            };
        let anchor = if locked.is_some() {
            self.object_center().unwrap_or(Vec3::ZERO)
        } else {
            let Some(point) = intersect_axis_plane(origin, dir, plane_axis, plane) else {
                return ObjectPress::Idle;
            };
            point
        };
        let forward = (self.target - self.eye()).normalize_or_zero();
        let normal = if forward.length_squared() < 1e-8 {
            Vec3::Z
        } else {
            forward
        };
        let bases = self
            .object_selection
            .iter()
            .copied()
            .filter(|member| *member < self.objects.len())
            .map(|member| {
                (
                    member,
                    self.objects[member].translation,
                    self.objects[member].pivot,
                )
            })
            .collect();
        self.object_drag = Some(ObjectDrag {
            index,
            locked,
            plane_axis,
            plane,
            anchor,
            normal,
            base: translation,
            bases,
            changed: false,
        });
        ObjectPress::Drag
    }

    pub(super) fn drag_object(&mut self, ray: Option<(Vec3, Vec3)>) {
        let Some(drag) = self.object_drag.clone() else {
            return;
        };
        let Some((origin, dir)) = ray else {
            return;
        };
        let next = if let Some(axis) = drag.locked {
            let Some(hit) = intersect_plane(origin, dir, drag.anchor, drag.normal) else {
                return;
            };
            let along = match axis {
                0 => hit.x - drag.anchor.x,
                1 => hit.y - drag.anchor.y,
                _ => hit.z - drag.anchor.z,
            };
            let steps = along.round() as i32;
            match axis {
                0 => (drag.base.0 + steps, drag.base.1, drag.base.2),
                1 => (drag.base.0, drag.base.1 + steps, drag.base.2),
                _ => (drag.base.0, drag.base.1, drag.base.2 + steps),
            }
        } else {
            let Some(hit) = intersect_axis_plane(origin, dir, drag.plane_axis, drag.plane) else {
                return;
            };
            let delta = plane_delta(drag.plane_axis, drag.anchor, hit);
            (
                drag.base.0 + delta.0,
                drag.base.1 + delta.1,
                drag.base.2 + delta.2,
            )
        };
        if self.objects.get(drag.index).is_none() {
            return;
        }
        let delta = (
            next.0 - drag.base.0,
            next.1 - drag.base.1,
            next.2 - drag.base.2,
        );
        if delta == (0, 0, 0) {
            return;
        }
        if !drag.changed {
            self.checkpoint();
        }
        for (member, base, pivot) in &drag.bases {
            if let Some(volume) = self.objects.get_mut(*member) {
                volume.translation = (base.0 + delta.0, base.1 + delta.1, base.2 + delta.2);
                if volume.model_dims != (0, 0, 0) {
                    volume.pivot = (pivot.0 + delta.0, pivot.1 + delta.1, pivot.2 + delta.2);
                }
            }
        }
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.object_drag = Some(ObjectDrag {
            changed: true,
            ..drag
        });
    }

    pub(super) fn finish_object_band(&mut self, band: &ObjectBand, shift: bool) {
        if band.click {
            if !shift {
                self.focus_object();
            }
            return;
        }
        let mut members = if shift {
            self.object_selection.clone()
        } else {
            Vec::new()
        };
        for index in &band.hits {
            if !members.contains(index) {
                members.push(*index);
            }
        }
        let primary = if members.contains(&self.selected) {
            self.selected
        } else {
            band.hits.last().copied().unwrap_or(self.selected)
        };
        self.choose_objects(members, primary);
        let count = self.object_selection.len();
        self.status = if count == 1 {
            "1 object selected.".into()
        } else {
            format!("{count} objects selected.")
        };
    }

    pub(super) fn object_center(&self) -> Option<Vec3> {
        let (min, max) = self.bounds()?;
        let translation = self.active().translation;
        Some(Vec3::new(
            (min.0 + max.0) as f32 * 0.5 + 0.5 + translation.0 as f32,
            (min.1 + max.1) as f32 * 0.5 + 0.5 + translation.1 as f32,
            (min.2 + max.2) as f32 * 0.5 + 0.5 + translation.2 as f32,
        ))
    }

    /// Set one axis of the active object's SIZE box. The minimum corner stays
    /// put. Shrinking clips voxels past the new face; growing keeps an empty
    /// margin that save writes back into the file. A loaded rotation stays:
    /// the axis is the model SIZE, and the pivot shifts so surviving voxels
    /// do not move.
    pub fn resize_active(&mut self, axis: usize, new_size: i32) {
        if axis > 2 {
            return;
        }
        let new_size = new_size.clamp(1, 256);
        if stored_rotation(self.active()) {
            self.resize_rotated(axis, new_size);
            return;
        }
        let Some((min, mut dims)) = resolved_box(self.active()) else {
            return;
        };
        if new_size == dims[axis] {
            return;
        }
        self.checkpoint();
        if new_size < dims[axis] {
            let limit = [min.0, min.1, min.2][axis] + new_size;
            let cells = std::mem::take(&mut self.active_mut().cells);
            self.active_mut().cells = cells
                .into_iter()
                .filter(|(cell, _)| [cell.0, cell.1, cell.2][axis] < limit)
                .collect();
            self.clear_picked();
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        }
        dims[axis] = new_size;
        let volume = self.active_mut();
        volume.box_min = min;
        volume.dims = (dims[0], dims[1], dims[2]);
        self.commit();
    }

    pub(super) fn resize_rotated(&mut self, axis: usize, new_size: i32) {
        let volume = self.active();
        let mut dims = [
            volume.model_dims.0,
            volume.model_dims.1,
            volume.model_dims.2,
        ];
        if new_size == dims[axis] {
            return;
        }
        let old_half = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
        let rot = volume.rot;
        let pivot = volume.pivot;
        let translation = volume.translation;
        self.checkpoint();
        if new_size < dims[axis] {
            let inverse = rot_transpose(rot);
            let cells = std::mem::take(&mut self.active_mut().cells);
            self.active_mut().cells = cells
                .into_iter()
                .filter(|(cell, slot)| {
                    if *slot == 0 {
                        return false;
                    }
                    let delta = [
                        cell.0 + translation.0 - pivot.0,
                        cell.1 + translation.1 - pivot.1,
                        cell.2 + translation.2 - pivot.2,
                    ];
                    let local = rot_apply(inverse, delta)[axis] + old_half[axis];
                    local >= 0 && local < new_size
                })
                .collect();
            self.clear_picked();
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        }
        dims[axis] = new_size;
        let new_half = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
        let shift = rot_apply(
            self.active().rot,
            [
                new_half[0] - old_half[0],
                new_half[1] - old_half[1],
                new_half[2] - old_half[2],
            ],
        );
        let volume = self.active_mut();
        volume.model_dims = (dims[0], dims[1], dims[2]);
        volume.pivot = (
            volume.pivot.0 + shift[0],
            volume.pivot.1 + shift[1],
            volume.pivot.2 + shift[2],
        );
        let (world_min, aabb) = size_box_world(
            dims,
            new_half,
            [volume.pivot.0, volume.pivot.1, volume.pivot.2],
            volume.rot,
        );
        let translation = volume.translation;
        volume.box_min = (
            world_min.0 - translation.0,
            world_min.1 - translation.1,
            world_min.2 - translation.2,
        );
        volume.dims = aabb;
        self.commit();
    }
}
