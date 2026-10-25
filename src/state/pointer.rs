//! Pointer, camera orbit, and tool strokes.

use std::collections::{HashMap, HashSet};

use glam::Vec3;

use super::geom::*;
use super::*;

impl Session {
    pub fn pointer(
        &mut self,
        inside: bool,
        primary_down: bool,
        primary_pressed: bool,
        primary_released: bool,
        secondary_down: bool,
        secondary_pressed: bool,
        middle_down: bool,
        middle_pressed: bool,
        space: bool,
        delta: glam::Vec2,
        scroll: f32,
        ray: Option<(Vec3, Vec3)>,
        drag: &ToolDrag,
        command: bool,
    ) {
        let world_ray = ray;
        let aimed = world_ray.and_then(|(origin, dir)| self.pick_object(origin, dir));
        let ray = self.local_ray(world_ray);
        if inside && scroll.abs() > 0.0 {
            self.distance = (self.distance * (1.0 - scroll * 0.0014)).clamp(2.0, 2000.0);
        }

        if secondary_pressed && inside {
            self.grab = Grab::Orbit;
        } else if middle_pressed && inside {
            self.grab = Grab::Pan;
        } else if primary_pressed && inside {
            if space {
                self.grab = Grab::Pan;
            } else if self.editor_mode == EditorMode::Object {
                if self.render_mode != RenderMode::Edit {
                    self.grab = Grab::None;
                    self.status = self.blocked_reason();
                } else {
                    match self.begin_object_move(world_ray, drag.shift) {
                        ObjectPress::Drag => self.grab = Grab::Object,
                        ObjectPress::Marquee => self.grab = Grab::ObjectMarquee,
                        ObjectPress::Idle => self.grab = Grab::None,
                    }
                }
            } else if let Some((index, _)) = aimed.filter(|(index, _)| *index != self.selected) {
                self.select_object(index);
                self.grab = Grab::None;
            } else if command && self.begin_move(ray) {
                self.grab = Grab::Move;
            } else if self.can_edit() {
                self.grab = Grab::Paint;
                self.checkpoint();
                if self.status_is_blocked() {
                    self.status = IDLE.into();
                }
            } else if self.can_select() {
                self.grab = Grab::Select;
                self.begin_select(ray);
            } else {
                self.grab = Grab::None;
                self.status = self.blocked_reason();
            }
        }

        match self.grab {
            Grab::Orbit if secondary_down => self.orbit(delta.x, delta.y),
            Grab::Pan if middle_down || primary_down => self.pan(delta.x, delta.y),
            Grab::Paint if primary_down => self.apply_stroke(ray, drag),
            Grab::Select if primary_down => self.drag_select(ray),
            Grab::Move if primary_down => self.drag_move(ray),
            Grab::Object if primary_down => self.drag_object(world_ray),
            _ => {}
        }

        if primary_released && self.grab == Grab::Paint {
            // The release frame is not "down", but it carries the end of the drag.
            self.apply_stroke(ray, drag);
            self.stroke = None;
            self.commit();
            self.grab = Grab::None;
        }
        if primary_released && self.grab == Grab::Select {
            self.finish_select(ray, drag);
            self.grab = Grab::None;
        }
        if primary_released && self.grab == Grab::Move {
            self.drag_move(ray);
            self.move_drag = None;
            self.commit();
            self.grab = Grab::None;
        }
        if primary_released && self.grab == Grab::Object {
            self.drag_object(world_ray);
            let changed = self.object_drag.as_ref().is_some_and(|drag| drag.changed);
            self.object_drag = None;
            if changed {
                self.commit();
            }
            self.grab = Grab::None;
        }
        if primary_released && self.grab == Grab::ObjectMarquee {
            if let Some(band) = &drag.object_band {
                self.finish_object_band(band, drag.shift);
            }
            self.grab = Grab::None;
        }
        if !secondary_down && self.grab == Grab::Orbit {
            self.grab = Grab::None;
        }
        if !middle_down && !primary_down && self.grab == Grab::Pan {
            self.grab = Grab::None;
        }

        self.hover = None;
        if inside && self.grab != Grab::Orbit && self.grab != Grab::Pan {
            if let Some((origin, dir)) = ray {
                self.hover = raycast(&self.active().cells, origin, dir);
            }
        }
    }

    pub(super) fn can_select(&self) -> bool {
        matches!(
            self.tool,
            Tool::Marquee | Tool::BoxSelect | Tool::FloodSelect
        ) && self.editor_mode == EditorMode::Model
            && self.render_mode == RenderMode::Edit
    }

    pub(super) fn can_edit(&self) -> bool {
        self.edits_voxels()
            && self.editor_mode == EditorMode::Model
            && self.render_mode == RenderMode::Edit
    }

    pub(super) fn edits_voxels(&self) -> bool {
        matches!(
            self.tool,
            Tool::Brush
                | Tool::Line
                | Tool::Circle
                | Tool::Square
                | Tool::Box
                | Tool::Face
                | Tool::Generator
                | Tool::Mutator
        )
    }

    pub(super) fn status_is_blocked(&self) -> bool {
        self.status.contains("does not edit") || self.status.contains("Switch to Edit")
    }

    pub(super) fn blocked_reason(&self) -> String {
        if self.render_mode == RenderMode::Render {
            "Render mode shows the path-traced frame. Switch to Edit to change voxels.".into()
        } else {
            format!(
                "{} does not edit voxels in this preview yet.",
                self.tool.name()
            )
        }
    }

    /// Anchor of a face stroke, so the viewport can turn mouse movement
    /// into an extrusion distance.
    pub fn stroke_face(&self) -> Option<(Cell, i32, i32)> {
        self.stroke
            .as_ref()
            .filter(|_| self.tool == Tool::Face)
            .map(|stroke| (stroke.anchor.cell, stroke.axis, stroke.sign))
    }

    pub(super) fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-1.15, 1.15);
    }

    pub(super) fn pan(&mut self, dx: f32, dy: f32) {
        let eye = self.eye();
        let forward = (self.target - eye).normalize_or_zero();
        let mut right = forward.cross(Vec3::Z);
        if right.length_squared() < 1e-4 {
            right = forward.cross(Vec3::Y);
        }
        let right = right.normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        // The scene follows the pointer, so the target moves against it.
        let scale = self.distance * 0.0022;
        self.target += right * -dx * scale + up * dy * scale;
    }

    pub(super) fn apply_stroke(&mut self, ray: Option<(Vec3, Vec3)>, drag: &ToolDrag) {
        let Some((origin, dir)) = ray else {
            return;
        };
        if self.tool == Tool::Brush {
            // Aim at the model as it was when the stroke began. A click spans
            // several frames, and aiming at the live cells would stack a voxel
            // on the one this stroke just added (or dig past the one it just
            // removed) on every frame. A drag paints one layer.
            let cells = match &self.stroke {
                Some(stroke) => &stroke.base,
                None => &self.active().cells,
            };
            if let Some(hit) = raycast(cells, origin, dir) {
                if self.stroke.is_none() {
                    self.begin_stroke(hit);
                }
                self.stamp(hit);
            }
        } else {
            self.preview_tool(origin, dir, &drag.rays, drag.face_steps);
        }
    }

    pub(super) fn preview_tool(
        &mut self,
        origin: Vec3,
        dir: Vec3,
        rays: &[(Vec3, Vec3)],
        face_steps: i32,
    ) {
        let hit = self.pick_for_stroke(origin, dir);
        let Some(hit) = hit else {
            return;
        };
        if self.stroke.is_none() {
            self.begin_stroke(hit);
        }
        let Some(stroke) = self.stroke.as_ref() else {
            return;
        };
        let end = self.place(hit);
        let stamp = self.drag_stamp();
        if stroke.key == Some((end, face_steps, stamp)) {
            return;
        }
        let anchor = self.place(stroke.anchor);
        let axis = stroke.axis;
        let sign = stroke.sign;
        let seeds = stroke.seeds.clone();
        let base = stroke.base.clone();
        let stroke_id = stroke.id;
        let centers = match self.tool {
            Tool::Line if self.line_mode == LineMode::Project && !rays.is_empty() => {
                self.project_line(&base, rays)
            }
            Tool::Line => crate::tools::bresenham(anchor, end),
            Tool::Circle => {
                crate::tools::circle(anchor, end, axis, self.circle_fill, self.circle_even)
            }
            Tool::Square => {
                crate::tools::square(anchor, end, axis, self.square_fill, self.square_even)
            }
            Tool::Box => crate::tools::cube(anchor, end, self.box_fill),
            Tool::Face => {
                let steps = if self.tool_mode == ToolMode::Add {
                    if face_steps == 0 {
                        1
                    } else {
                        face_steps
                    }
                } else {
                    face_steps.abs().max(1)
                };
                let outward = self.tool_mode == ToolMode::Add && steps > 0;
                let n = steps.abs();
                crate::tools::extrude(&seeds, axis, sign, n, outward)
            }
            Tool::Generator => Vec::new(),
            Tool::Mutator => {
                let offsets = footprint(self.brush_size, self.brush_shape, self.brush_3d);
                let mapped = map_footprint(&offsets, axis);
                crate::tools::foliage(
                    &base,
                    hit.cell,
                    &mapped,
                    self.foliage_seed.clamp(1, 255) as u8,
                    self.foliage_density.clamp(0.0, 1.0),
                    stroke_id,
                )
            }
            _ => Vec::new(),
        };
        let edits = if self.tool == Tool::Generator {
            let min = (
                anchor.0.min(end.0),
                anchor.1.min(end.1),
                anchor.2.min(end.2),
            );
            let max = (
                anchor.0.max(end.0),
                anchor.1.max(end.1),
                anchor.2.max(end.2),
            );
            {
                let gen = self.gen_params();
                let filled = crate::tools::generate(min, max, &gen);
                if filled.is_empty()
                    && (max.0 - min.0 + 1) as i64
                        * (max.1 - min.1 + 1) as i64
                        * (max.2 - min.2 + 1) as i64
                        > crate::tools::MAX_GENERATOR_CELLS
                {
                    self.status = "That box is too large to fill.".into();
                }
                filled
            }
        } else if self.tool == Tool::Mutator {
            let seed = self.foliage_seed.clamp(1, 255) as u8;
            centers.into_iter().map(|cell| (cell, seed)).collect()
        } else if self.tool == Tool::Face {
            centers
                .into_iter()
                .map(|cell| {
                    let slot = if self.tool_mode == ToolMode::Add && face_steps < 0 {
                        0
                    } else if self.face_with == FaceWith::Geometry
                        && self.tool_mode == ToolMode::Add
                    {
                        let back = match axis {
                            0 => (-sign, 0, 0),
                            1 => (0, -sign, 0),
                            _ => (0, 0, -sign),
                        };
                        let mut cursor = cell;
                        for _ in 0..64 {
                            if seeds.iter().any(|seed| *seed == cursor) {
                                break;
                            }
                            cursor = crate::tools::add(cursor, back);
                        }
                        base.get(&cursor).copied().unwrap_or(self.active_slot)
                    } else {
                        self.paint_slot()
                    };
                    (cell, slot)
                })
                .collect()
        } else {
            let expanded = expand_centers(
                &centers,
                &footprint(self.brush_size, self.brush_shape, self.brush_3d),
                axis,
            );
            expanded
                .into_iter()
                .map(|cell| (cell, self.paint_slot()))
                .collect()
        };
        self.write_preview(base, edits);
        if let Some(stroke) = self.stroke.as_mut() {
            stroke.key = Some((end, face_steps, stamp));
        }
    }

    pub(super) fn begin_stroke(&mut self, hit: Hit) {
        let (axis, sign) = face_axis(hit.normal);
        let base = self.active().cells.clone();
        let seeds = if self.tool == Tool::Face {
            if base.contains_key(&hit.cell) {
                let match_color = if self.face_extrude == FaceExtrude::Color {
                    base.get(&hit.cell).copied().unwrap_or(0)
                } else {
                    0
                };
                crate::tools::flood_face(&base, hit.cell, axis, sign, match_color, self.face_eight)
            } else if let Some((min, max)) = cell_bounds(base.keys().copied()) {
                let (u_axis, v_axis) = crate::tools::plane_axes(axis);
                let flooded = crate::tools::flood_empty(
                    &base,
                    hit.cell,
                    axis,
                    crate::tools::coord(min, u_axis),
                    crate::tools::coord(max, u_axis),
                    crate::tools::coord(min, v_axis),
                    crate::tools::coord(max, v_axis),
                    self.face_eight,
                );
                let back = crate::tools::axis_delta(axis, -sign);
                flooded
                    .into_iter()
                    .map(|cell| crate::tools::add(cell, back))
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        self.stroke_serial = self.stroke_serial.wrapping_add(1);
        self.stroke = Some(Stroke {
            anchor: hit,
            base,
            seeds,
            axis,
            sign,
            id: self.stroke_serial,
            key: None,
        });
    }

    pub(super) fn pick_for_stroke(&self, origin: Vec3, dir: Vec3) -> Option<Hit> {
        let cells = if let Some(stroke) = &self.stroke {
            &stroke.base
        } else {
            &self.active().cells
        };
        if let Some(hit) = raycast(cells, origin, dir) {
            return Some(hit);
        }
        if let Some(stroke) = &self.stroke {
            return plane_hit(origin, dir, stroke.anchor);
        }
        if self.tool == Tool::Face {
            return wall_hit(cells, origin, dir);
        }
        None
    }

    pub(super) fn project_line(
        &self,
        base: &HashMap<Cell, u8>,
        rays: &[(Vec3, Vec3)],
    ) -> Vec<Cell> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (origin, dir) in rays {
            let Some(hit) = raycast(base, *origin, *dir) else {
                continue;
            };
            let cell = self.place(hit);
            if seen.insert(cell) {
                out.push(cell);
            }
        }
        out
    }

    pub(super) fn place(&self, hit: Hit) -> Cell {
        match self.tool_mode {
            ToolMode::Add => crate::tools::add(hit.cell, hit.normal),
            ToolMode::Remove | ToolMode::Paint => hit.cell,
        }
    }

    pub(super) fn paint_slot(&self) -> u8 {
        match self.tool_mode {
            ToolMode::Remove => 0,
            ToolMode::Add | ToolMode::Paint => self.active_slot,
        }
    }

    pub(super) fn write_preview(&mut self, base: HashMap<Cell, u8>, edits: Vec<(Cell, u8)>) {
        let mut next = base.clone();
        let mode = if self.tool == Tool::Mutator {
            ToolMode::Add
        } else {
            self.tool_mode
        };
        for (cell, slot) in edits {
            match mode {
                ToolMode::Add => {
                    if slot == 0 {
                        next.remove(&cell);
                    } else if !base.contains_key(&cell) {
                        next.insert(cell, slot);
                    }
                }
                ToolMode::Remove => {
                    next.remove(&cell);
                }
                ToolMode::Paint => {
                    if base.contains_key(&cell) && slot != 0 {
                        next.insert(cell, slot);
                    }
                }
            }
        }
        if next != self.active().cells {
            self.active_mut().cells = next;
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        }
    }

    pub(super) fn gen_params(&self) -> crate::tools::Gen {
        crate::tools::Gen {
            kind: self.generator,
            axis: self.gen_axis,
            apex_at_max: self.apex_at_max,
            palette: self.active_slot,
            torus_tube: self.torus_tube,
            round_radius: self.round_radius,
            helix_turns: self.helix_turns,
            helix_radius: self.helix_radius,
            helix_tube: self.helix_tube,
            helix_cw: self.helix_cw,
            noise_scale: self.noise_scale,
            noise_threshold: self.noise_threshold,
            noise_octaves: self.noise_octaves,
            noise_seed: self.noise_seed as u32,
            perlin_lo: self.perlin_lo,
            perlin_hi: self.perlin_hi,
            tree_depth: self.tree_depth,
            tree_wildness: self.tree_wildness,
            tree_trunk: self.tree_trunk,
            tree_leaf: self.tree_leaf,
            tree_seed: self.tree_seed as u32,
            conifer_tiers: self.conifer_tiers,
            conifer_ragged: self.conifer_ragged,
            conifer_clump: self.conifer_clump,
            conifer_trunk: self.conifer_trunk,
            conifer_crown: self.conifer_crown,
            conifer_needle: self.conifer_needle,
            conifer_seed: self.conifer_seed as u32,
        }
    }

    pub(super) fn drag_stamp(&self) -> u64 {
        let mode = match self.tool_mode {
            ToolMode::Add => 1u64,
            ToolMode::Remove => 2,
            ToolMode::Paint => 3,
        };
        let mut h = mode;
        h = h
            .wrapping_mul(16777619)
            .wrapping_add(self.brush_size as u64);
        h = h
            .wrapping_mul(16777619)
            .wrapping_add(self.active_slot as u64);
        h = h
            .wrapping_mul(16777619)
            .wrapping_add(self.circle_fill as u64);
        h = h.wrapping_mul(16777619).wrapping_add(self.box_fill as u64);
        h = h
            .wrapping_mul(16777619)
            .wrapping_add(self.foliage_seed as u64);
        h = h
            .wrapping_mul(16777619)
            .wrapping_add(self.generator.name().len() as u64);
        h = h
            .wrapping_mul(16777619)
            .wrapping_add(self.tree_depth as u64);
        h = h
            .wrapping_mul(16777619)
            .wrapping_add(self.noise_seed as u64);
        h
    }

    pub(super) fn stamp(&mut self, hit: Hit) {
        let base = match self.tool_mode {
            ToolMode::Add => (
                hit.cell.0 + hit.normal.0,
                hit.cell.1 + hit.normal.1,
                hit.cell.2 + hit.normal.2,
            ),
            ToolMode::Remove | ToolMode::Paint => hit.cell,
        };
        let axis = if hit.normal.0 != 0 {
            0
        } else if hit.normal.1 != 0 {
            1
        } else {
            2
        };
        let slot = match self.tool_mode {
            ToolMode::Remove => 0,
            ToolMode::Add | ToolMode::Paint => self.active_slot,
        };
        let mut changed = false;
        for (u, v, w) in footprint(self.brush_size, self.brush_shape, self.brush_3d) {
            let (dx, dy, dz) = match axis {
                0 => (w, u, v),
                1 => (u, w, v),
                _ => (u, v, w),
            };
            let cell = (base.0 + dx, base.1 + dy, base.2 + dz);
            changed |= self.write_cell(cell, slot);
        }
        if changed {
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        }
    }

    pub(super) fn write_cell(&mut self, cell: Cell, slot: u8) -> bool {
        let map = &mut self.active_mut().cells;
        if slot == 0 {
            map.remove(&cell).is_some()
        } else if map.get(&cell).copied() == Some(slot) {
            false
        } else {
            map.insert(cell, slot);
            true
        }
    }
}
