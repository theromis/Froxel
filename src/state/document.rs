//! New session, palette edits, materials, and the camera frame.

use std::collections::{HashMap, HashSet};

use glam::Vec3;

use super::geom::*;
use super::vox::*;
use super::*;

impl Session {
    pub fn new() -> Self {
        let mut session = Self {
            tool: Tool::Brush,
            tool_mode: ToolMode::Add,
            brush_size: 1,
            brush_shape: BrushShape::Square,
            brush_3d: true,
            line_mode: LineMode::Straight,
            circle_fill: false,
            circle_even: false,
            square_fill: false,
            square_even: false,
            box_fill: false,
            face_extrude: FaceExtrude::Color,
            face_with: FaceWith::Geometry,
            face_eight: false,
            generator: GeneratorKind::Ellipsoid,
            gen_axis: Axis::Y,
            apex_at_max: true,
            torus_tube: 0.3,
            round_radius: 2.0,
            tree_depth: 3,
            tree_wildness: 0.35,
            tree_trunk: 0.45,
            tree_leaf: 1,
            tree_seed: 1,
            conifer_tiers: 3,
            conifer_ragged: 0.25,
            conifer_clump: 6.0,
            conifer_trunk: 0.4,
            conifer_crown: 0.5,
            conifer_needle: 1,
            conifer_seed: 1,
            helix_turns: 2.0,
            helix_radius: 0.75,
            helix_tube: 0.16,
            helix_cw: false,
            noise_scale: 8.0,
            noise_threshold: 0.5,
            noise_octaves: 4,
            noise_seed: 1,
            perlin_lo: 1,
            perlin_hi: 8,
            foliage_seed: 1,
            foliage_density: 0.30,
            palette: default_palette(),
            materials: [Material::default(); 256],
            active_slot: 36,
            sample_armed: false,
            objects: Vec::new(),
            selected: 0,
            object_selection: vec![0],
            next_object: 2,
            path: None,
            name_hint: None,
            graph: None,
            skeleton: Skeleton::default(),
            pose: HashMap::new(),
            clips: Vec::new(),
            method_label: None,
            selected_bone: None,
            orthographic: false,
            show_bones: false,
            selected_clip: None,
            clip_draft: String::new(),
            clip_time: 0.0,
            mirror_bones: false,
            export: crate::export::MeshExport {
                vertex_colors: false,
                merge: true,
                export_hidden: false,
                split_objects: false,
            },
            editor_mode: EditorMode::Model,
            render_mode: RenderMode::Edit,
            stochastic_gi: true,
            glass_mode: GlassMode::Probabilistic,
            max_samples: 1024,
            aces: true,
            denoise: false,
            motion_preview: false,
            script_error: String::new(),
            script_due: None,
            script_job: None,
            aperture: 0.0,
            focus_distance: 30.0,
            max_bounces: 5,
            power_weighted_nee: true,
            nee_ris: 8,
            indirect_clamp: 8.0,
            ema_floor: 0.05,
            atmospheric_sky: false,
            sky_intensity: 1.0,
            horizon_softness: 3.0,
            sun_elevation: 30.0,
            sun_azimuth: 30.0,
            sun_disc: 30.0,
            sun_intensity: 0.0,
            sun_color: [1.0, 0.95, 0.85],
            render_preview: None,
            render_key: 0,
            trace_on_gpu: false,
            render_save_path: None,
            render_save_seen: None,
            yaw: -0.8,
            pitch: 0.55,
            distance: 28.0,
            target: Vec3::new(0.0, 0.0, 4.0),
            mesh_epoch: 1,
            status: IDLE.into(),
            hover: None,
            picked: HashSet::new(),
            selection_epoch: 1,
            select_anchor: None,
            clipboard: None,
            move_drag: None,
            object_drag: None,
            hover_gizmo: None,
            stroke: None,
            stroke_serial: 1,
            grab: Grab::None,
            palette_slots: HashSet::from([36]),
            color_clipboard: None,
            palette_press: None,
            palette_current: None,
            palette_shift: false,
            palette_anchor: 36,
            pending: None,
            clean: None,
            gesture: false,
            undo: Vec::new(),
            redo: Vec::new(),
            ui_regions: Vec::new(),
            recent: load_recent(),
        };
        session.objects.push(Volume {
            name: "Tree".into(),
            cells: demo_tree(),
            translation: (0, 0, 0),
            box_min: (0, 0, 0),
            dims: (0, 0, 0),
            rot: ROT_IDENTITY,
            pivot: (0, 0, 0),
            model_dims: (0, 0, 0),
            hidden: false,
            file: None,
            bone: None,
            script: Default::default(),
        });
        session.frame_camera();
        session.mark_clean();
        session
    }

    pub fn active(&self) -> &Volume {
        &self.objects[self.selected]
    }

    pub(super) fn active_mut(&mut self) -> &mut Volume {
        let i = self.selected.min(self.objects.len().saturating_sub(1));
        self.selected = i;
        &mut self.objects[i]
    }

    pub fn voxel_count(&self) -> usize {
        self.active().cells.len()
    }

    pub fn bounds(&self) -> Option<(Cell, Cell)> {
        let mut iter = self.active().cells.keys().copied();
        let first = iter.next()?;
        let mut min = first;
        let mut max = first;
        for c in iter {
            min.0 = min.0.min(c.0);
            min.1 = min.1.min(c.1);
            min.2 = min.2.min(c.2);
            max.0 = max.0.max(c.0);
            max.1 = max.1.max(c.1);
            max.2 = max.2.max(c.2);
        }
        Some((min, max))
    }

    pub fn world_bounds(&self) -> Option<(Cell, Cell)> {
        let mut min = (i32::MAX, i32::MAX, i32::MAX);
        let mut max = (i32::MIN, i32::MIN, i32::MIN);
        let mut any = false;
        for object in &self.objects {
            for cell in object.cells.keys() {
                any = true;
                let world = (
                    cell.0 + object.translation.0,
                    cell.1 + object.translation.1,
                    cell.2 + object.translation.2,
                );
                min.0 = min.0.min(world.0);
                min.1 = min.1.min(world.1);
                min.2 = min.2.min(world.2);
                max.0 = max.0.max(world.0);
                max.1 = max.1.max(world.1);
                max.2 = max.2.max(world.2);
            }
        }
        any.then_some((min, max))
    }

    pub fn size_xyz(&self) -> Option<(i32, i32, i32)> {
        let object = self.active();
        if stored_rotation(object) {
            return Some(object.model_dims);
        }
        let (_, dims) = resolved_box(object)?;
        Some((dims[0], dims[1], dims[2]))
    }

    pub fn document_label(&self) -> String {
        match &self.path {
            Some(p) => p.display().to_string(),
            None => self.name_hint.clone().unwrap_or_else(|| "Untitled".into()),
        }
    }

    /// The file name without its folder, for the narrow phone header.
    pub fn document_name(&self) -> String {
        match self.path.as_ref().and_then(|p| p.file_name()) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => self.document_label(),
        }
    }

    /// Sway in x and bob in y. A still slot is `(0, 0)`.
    pub fn motion_uv(&self, slot: u8) -> [f32; 2] {
        let material = self.materials[slot as usize];
        match material.motion {
            Motion::Sway => [material.motion_amount, 0.0],
            Motion::Bob => [0.0, material.motion_amount],
            Motion::None => [0.0, 0.0],
        }
    }

    pub fn has_motion(&self) -> bool {
        self.materials
            .iter()
            .any(|material| material.motion != Motion::None)
    }

    pub fn color(&self, slot: u8) -> [u8; 3] {
        let c = self.palette[slot as usize];
        [c[0], c[1], c[2]]
    }

    pub fn set_color(&mut self, slot: u8, rgb: [u8; 3]) {
        if slot == 0 {
            return;
        }
        let next = [rgb[0], rgb[1], rgb[2], 255];
        if self.palette[slot as usize] == next {
            return;
        }
        self.palette[slot as usize] = next;
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Remember the document before a color or material drag. A second call
    /// in the same gesture does nothing.
    pub fn begin_edit(&mut self) {
        self.checkpoint();
    }

    /// Push the color or material gesture if it changed anything.
    pub fn end_edit(&mut self) {
        self.commit();
    }

    pub fn palette_drag(&self) -> Option<((i32, i32), (i32, i32))> {
        Some((self.palette_press?, self.palette_current?))
    }

    pub fn begin_palette_drag(&mut self, cell: (i32, i32), shift: bool) {
        self.palette_press = Some(cell);
        self.palette_current = Some(cell);
        self.palette_shift = shift;
    }

    pub fn update_palette_drag(&mut self, cell: (i32, i32)) {
        if self.palette_press.is_some() {
            self.palette_current = Some(cell);
        }
    }

    pub fn finish_palette_drag(&mut self) {
        let Some(start) = self.palette_press.take() else {
            return;
        };
        let cell = self.palette_current.take().unwrap_or(start);
        let shift = self.palette_shift;
        if start == cell {
            let slot = palette_slot_at(cell.0, cell.1);
            if slot != 0 {
                self.select_palette_slot(slot, shift);
            }
        } else {
            self.select_palette_range(start.0, start.1, cell.0, cell.1, shift);
        }
    }

    pub fn select_palette_slot(&mut self, slot: u8, extend: bool) {
        if slot == 0 {
            return;
        }
        if !extend {
            self.palette_slots.clear();
            self.palette_slots.insert(slot);
        } else if !self.palette_slots.remove(&slot) {
            self.palette_slots.insert(slot);
        } else if self.palette_slots.is_empty() {
            self.palette_slots.insert(slot);
        }
        self.active_slot = slot;
    }

    pub fn select_palette_range(&mut self, c0: i32, r0: i32, c1: i32, r1: i32, shift: bool) {
        if !shift {
            self.palette_slots.clear();
        }
        let min_c = c0.min(c1);
        let max_c = c0.max(c1);
        let min_r = r0.min(r1);
        let max_r = r0.max(r1);
        for row in min_r..=max_r {
            for col in min_c..=max_c {
                let slot = palette_slot_at(col, row);
                if slot == 0 {
                    continue;
                }
                if shift {
                    if !self.palette_slots.remove(&slot) {
                        self.palette_slots.insert(slot);
                    }
                } else {
                    self.palette_slots.insert(slot);
                }
            }
        }
        let end = palette_slot_at(c1, r1);
        if end != 0 {
            self.palette_slots.insert(end);
            self.active_slot = end;
        }
    }

    pub fn note_palette_anchor(&mut self, slot: u8) {
        if slot != 0 {
            self.palette_anchor = slot;
        }
    }

    pub(super) fn ensure_palette_anchor(&mut self, anchor: u8) {
        if anchor == 0 {
            return;
        }
        if !self.palette_slots.contains(&anchor) {
            self.palette_slots.clear();
            self.palette_slots.insert(anchor);
            self.active_slot = anchor;
        }
    }

    pub fn palette_action_count(&self, anchor: u8) -> usize {
        if anchor == 0 {
            return 0;
        }
        if self.palette_slots.contains(&anchor) {
            self.palette_slots.len()
        } else {
            1
        }
    }

    pub fn has_color_clipboard(&self) -> bool {
        self.color_clipboard.is_some()
    }

    pub fn copy_palette_color(&mut self, slot: u8) {
        if slot == 0 {
            return;
        }
        self.color_clipboard = Some(self.palette[slot as usize]);
        self.status = format!("Copied palette {slot}.");
    }

    pub fn paste_palette_color(&mut self, anchor: u8) {
        let Some(color) = self.color_clipboard else {
            return;
        };
        self.ensure_palette_anchor(anchor);
        self.checkpoint();
        for slot in self.palette_slots.iter().copied() {
            if slot != 0 {
                self.palette[slot as usize] = color;
            }
        }
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.status = "Pasted the copied color.".into();
    }

    pub fn make_palette_gradient(&mut self, anchor: u8) {
        self.ensure_palette_anchor(anchor);
        let mut indices: Vec<u8> = self
            .palette_slots
            .iter()
            .copied()
            .filter(|s| *s != 0)
            .collect();
        indices.sort_unstable();
        let Some(&lo) = indices.first() else {
            return;
        };
        let Some(&hi) = indices.last() else {
            return;
        };
        if hi.saturating_sub(lo) < 2 {
            return;
        }
        let c0 = self.palette[lo as usize];
        let c1 = self.palette[hi as usize];
        let span = (hi - lo) as f32;
        self.checkpoint();
        for slot in indices {
            if slot == lo || slot == hi {
                continue;
            }
            let t = (slot - lo) as f32 / span;
            self.palette[slot as usize] = lerp_color(c0, c1, t);
        }
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.status = format!("Gradient from {lo} to {hi}.");
    }

    pub fn select_voxels_by_color(&mut self, anchor: u8) {
        self.ensure_palette_anchor(anchor);
        let slots = self.palette_slots.clone();
        let matched = self
            .active()
            .cells
            .iter()
            .filter(|(_, slot)| slots.contains(*slot))
            .map(|(cell, _)| *cell)
            .collect();
        self.set_picked(matched);
    }

    pub fn paint_picked_with_color(&mut self, anchor: u8) {
        if self.picked.is_empty() {
            return;
        }
        self.ensure_palette_anchor(anchor);
        if self.palette_slots.len() != 1 {
            return;
        }
        let Some(slot) = self.palette_slots.iter().copied().find(|slot| *slot != 0) else {
            return;
        };
        self.recolor_picked(|_| slot);
    }

    pub fn paint_picked_with_random_colors(&mut self, anchor: u8) {
        if self.picked.is_empty() {
            return;
        }
        self.ensure_palette_anchor(anchor);
        let mut slots: Vec<u8> = self
            .palette_slots
            .iter()
            .copied()
            .filter(|s| *s != 0)
            .collect();
        slots.sort_unstable();
        if slots.len() < 2 {
            return;
        }
        self.recolor_picked(|cell| {
            let h = (cell.0 as u32).wrapping_mul(73856093)
                ^ (cell.1 as u32).wrapping_mul(19349663)
                ^ (cell.2 as u32).wrapping_mul(83492791);
            slots[(h as usize) % slots.len()]
        });
    }

    pub(super) fn recolor_picked(&mut self, mut choose: impl FnMut(Cell) -> u8) {
        if self.picked.is_empty() {
            return;
        }
        self.checkpoint();
        let picked: Vec<Cell> = self.picked.iter().copied().collect();
        {
            let map = &mut self.active_mut().cells;
            for cell in picked {
                let slot = choose(cell);
                if slot != 0 {
                    map.insert(cell, slot);
                }
            }
        }
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.status = format!("Recolored {} voxels.", self.picked.len());
    }

    pub fn material_mut(&mut self) -> &mut Material {
        &mut self.materials[self.active_slot as usize]
    }

    pub fn sun_dir(&self) -> Vec3 {
        let e = self.sun_elevation.to_radians();
        let a = self.sun_azimuth.to_radians();
        Vec3::new(a.cos() * e.cos(), a.sin() * e.cos(), e.sin()).normalize_or_zero()
    }

    pub fn eye(&self) -> Vec3 {
        self.target
            + Vec3::new(
                self.yaw.cos() * self.pitch.cos(),
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
            ) * self.distance
    }

    pub fn frame_camera(&mut self) {
        if let Some((min, max)) = self.world_bounds() {
            self.target = Vec3::new(
                (min.0 + max.0 + 1) as f32 * 0.5,
                (min.1 + max.1 + 1) as f32 * 0.5,
                (min.2 + max.2 + 1) as f32 * 0.5,
            );
            let ext = (max.0 - min.0).max(max.1 - min.1).max(max.2 - min.2) as f32 + 1.0;
            self.distance = (ext * 2.4).clamp(6.0, 800.0);
        } else {
            self.target = Vec3::ZERO;
            self.distance = 24.0;
        }
        self.yaw = -0.8;
        self.pitch = 0.55;
    }
}
