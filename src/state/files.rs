//! Open, save, export, and the clickable panel regions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use glam::Vec3;

use super::geom::*;
use super::vox::*;
use super::*;

use super::dialog::{self, CloseAnswer};

impl Session {
    pub fn new_document(&mut self) {
        self.commit();
        self.objects.clear();
        self.objects.push(Volume {
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
        self.selected = 0;
        self.focus_object();
        self.next_object = 2;
        self.path = None;
        self.graph = None;
        self.skeleton = Skeleton::default();
        self.pose.clear();
        self.clips.clear();
        self.selected_bone = None;
        self.palette = default_palette();
        self.materials = [Material::default(); 256];
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.mark_clean();
        self.clear_picked();
        self.script_error.clear();
        self.script_due = None;
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.frame_camera();
        self.status = IDLE.into();
    }

    pub fn open_dialog(&mut self) {
        let Some(path) = dialog::pick_open("MagicaVoxel", &["vox"]) else {
            return;
        };
        self.load_path(&path);
    }

    pub fn load_path(&mut self, path: &Path) {
        if let Err(e) = self.open_vox(path) {
            self.status = e;
            self.recent.retain(|stored| stored != path);
            store_recent(&self.recent);
        }
    }

    pub(crate) fn open_vox(&mut self, path: &Path) -> Result<(), String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        let loaded = load_vox(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        self.commit();
        let n: usize = loaded.objects.iter().map(|object| object.cells.len()).sum();
        let models = loaded.objects.len();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Model".into());
        self.install(loaded, &stem);
        self.path = Some(path.to_path_buf());
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.mark_clean();
        self.frame_camera();
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        self.script_error.clear();
        self.script_due = None;
        self.status = if models > 1 {
            format!("Loaded {n} voxels in {models} objects from {file}.")
        } else {
            format!("Loaded {n} voxels from {file}.")
        };
        if self
            .objects
            .iter()
            .any(|object| !object.script.source.is_empty())
        {
            self.status.push_str(" This file carries a script.");
        }
        self.remember_path(path);
        Ok(())
    }

    fn install(&mut self, loaded: Loaded, stem: &str) {
        let models = loaded.objects.len();
        self.objects.clear();
        for (index, object) in loaded.objects.into_iter().enumerate() {
            let name = if !object.name.is_empty() {
                object.name
            } else if models == 1 {
                stem.to_string()
            } else {
                format!("Object {}", index + 1)
            };
            let mut file = object.file;
            if let Some(link) = file.as_mut() {
                link.name = name.clone();
            }
            self.objects.push(Volume {
                name,
                cells: object.cells,
                translation: object.translation,
                box_min: object.box_min,
                dims: object.dims,
                rot: object.rot,
                pivot: object.pivot,
                model_dims: object.model_dims,
                hidden: object.hidden,
                file,
                bone: object.bone,
                script: object.script,
            });
        }
        if self.objects.is_empty() {
            self.objects.push(Volume {
                name: stem.to_string(),
                cells: HashMap::new(),
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
        }
        self.selected = 0;
        self.focus_object();
        self.next_object = (self.objects.len() as u32).saturating_add(1).max(2);
        self.palette = loaded.palette.unwrap_or_else(default_palette);
        self.materials = loaded.materials;
        self.graph = loaded.graph;
        self.skeleton = loaded.skeleton;
        self.clips = loaded.clips;
        self.pose.clear();
        self.selected_bone = None;
        self.clear_picked();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    pub(crate) fn export_bytes(&self) -> Result<Vec<u8>, String> {
        save_document(
            &self.objects,
            &self.palette,
            &self.materials,
            self.graph.as_ref(),
            &self.skeleton,
            &self.clips,
        )
    }

    pub(crate) fn import_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let loaded = load_vox(bytes)?;
        self.install(loaded, "Object");
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.mark_clean();
        Ok(())
    }

    /// Differences a second load of a save should not have.
    pub(crate) fn round_trip_differences(&self, other: &Self) -> Vec<String> {
        let mut out = Vec::new();
        if self.objects.len() != other.objects.len() {
            out.push(format!(
                "object count: {} -> {}",
                self.objects.len(),
                other.objects.len()
            ));
        }
        for (index, (left, right)) in self.objects.iter().zip(&other.objects).enumerate() {
            if left.name != right.name {
                out.push(format!(
                    "object {index} name: '{}' -> '{}'",
                    left.name, right.name
                ));
            }
            if left.hidden != right.hidden {
                out.push(format!("object {index} _hidden changed"));
            }
            if left.translation != right.translation {
                out.push(format!(
                    "object {index} translation: {:?} -> {:?}",
                    left.translation, right.translation
                ));
            }
            if left.rot != right.rot {
                out.push(format!("object {index} rotation changed"));
            }
            if left.cells != right.cells {
                out.push(format!("object {index} cells differ"));
            }
            if left.bone != right.bone {
                out.push(format!(
                    "object {index} bone: {:?} -> {:?}",
                    left.bone, right.bone
                ));
            }
            if left.script.source != right.script.source
                || left.script.params != right.script.params
                || left.script.auto != right.script.auto
            {
                out.push(format!("object {index} script differs"));
            }
        }
        if self.palette != other.palette {
            out.push("palette differs".into());
        }
        if self.materials != other.materials {
            out.push("materials or motion differ".into());
        }
        if self.skeleton != other.skeleton {
            out.push("bones differ".into());
        }
        if self.clips != other.clips {
            out.push("clips differ".into());
        }
        out.extend(graph_differences(self.graph.as_ref(), other.graph.as_ref()));
        out
    }

    /// Empty one-object document. The window's File → New still builds the demo tree.
    pub(crate) fn replace_empty(&mut self) {
        self.commit();
        self.objects.clear();
        self.objects.push(Volume {
            name: "Object".into(),
            cells: HashMap::new(),
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
        self.selected = 0;
        self.focus_object();
        self.next_object = 2;
        self.path = None;
        self.graph = None;
        self.skeleton = Skeleton::default();
        self.pose.clear();
        self.clips.clear();
        self.selected_bone = None;
        self.palette = default_palette();
        self.materials = [Material::default(); 256];
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.mark_clean();
        self.script_error.clear();
        self.script_due = None;
        self.clear_picked();
        self.hover = None;
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.frame_camera();
        self.status = "Empty document.".into();
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Write cells of the active object. Palette 0 erases. One undo step.
    /// Coordinates are local to that object and must lie in `[-1024, 1024)`.
    pub(crate) fn write_cells(&mut self, writes: &[(i32, i32, i32, i32)]) -> Result<i64, String> {
        let mut deduped: HashMap<Cell, u8> = HashMap::new();
        for (x, y, z, palette) in writes {
            if !in_world(*x, *y, *z) {
                return Err("coordinate outside the world [-1024, 1024)".into());
            }
            if !(0..=255).contains(palette) {
                return Err("palette must be 0..255".into());
            }
            deduped.insert((*x, *y, *z), *palette as u8);
        }
        let mut next = self.active().cells.clone();
        let mut changed = 0i64;
        for (cell, palette) in deduped {
            let old = next.get(&cell).copied().unwrap_or(0);
            if old == palette {
                continue;
            }
            changed += 1;
            if palette == 0 {
                next.remove(&cell);
            } else {
                next.insert(cell, palette);
            }
        }
        if changed == 0 {
            return Ok(0);
        }
        if let Some((min, max)) = cell_bounds(next.keys().copied()) {
            let sx = max.0 - min.0 + 1;
            let sy = max.1 - min.1 + 1;
            let sz = max.2 - min.2 + 1;
            if sx > 256 || sy > 256 || sz > 256 {
                return Err(format!(
                    "edit would make the active object {sx}x{sy}x{sz}; .vox models are at most 256 per axis"
                ));
            }
        }
        self.commit();
        self.checkpoint();
        self.active_mut().cells = next;
        self.clear_picked();
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        Ok(changed)
    }

    pub(crate) fn write_palette(&mut self, slots: &[PaletteChange]) -> i64 {
        let mut changed = 0i64;
        for change in slots {
            let index = change.index as usize;
            let material = self.materials[index];
            let motion = change.motion.unwrap_or(material.motion);
            let amount = change.amount.unwrap_or(material.motion_amount);
            if self.palette[index] != change.rgba
                || material.motion != motion
                || material.motion_amount != amount
            {
                changed += 1;
            }
        }
        if changed == 0 {
            return 0;
        }
        self.commit();
        self.checkpoint();
        for change in slots {
            let index = change.index as usize;
            self.palette[index] = change.rgba;
            if let Some(motion) = change.motion {
                self.materials[index].motion = motion;
            }
            if let Some(amount) = change.amount {
                self.materials[index].motion_amount = amount.clamp(0.0, 4.0);
            }
        }
        self.commit();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        changed
    }

    /// Remember the current document as saved. The preview pose is not saved
    /// in the file, so it does not count as a change.
    pub(super) fn mark_clean(&mut self) {
        let mut snapshot = self.snapshot();
        snapshot.pose.clear();
        self.clean = Some(Box::new(snapshot));
    }

    /// Edits since the document was opened, saved, or created.
    pub fn has_unsaved_changes(&mut self) -> bool {
        self.commit();
        let Some(clean) = self.clean.as_deref() else {
            return true;
        };
        clean.objects != self.objects
            || clean.palette != self.palette
            || clean.materials != self.materials
            || clean.graph != self.graph
            || clean.skeleton != self.skeleton
            || clean.clips != self.clips
    }

    /// True when the window may close. Unsaved edits ask Save, Don't Save,
    /// or Cancel. Save that is cancelled or fails keeps the window open.
    pub fn confirm_close(&mut self) -> bool {
        if !self.has_unsaved_changes() {
            return true;
        }
        match dialog::ask_close(&self.document_label()) {
            CloseAnswer::Save => {
                self.save();
                !self.has_unsaved_changes()
            }
            CloseAnswer::DontSave => true,
            CloseAnswer::Cancel => false,
        }
    }

    /// Save every object. An untitled document asks for a path.
    pub fn save(&mut self) {
        match self.path.clone() {
            Some(path) => {
                let _ = self.write_vox(&path);
            }
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        let name = match self.path.as_ref().and_then(|p| p.file_name()) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => format!("{}.vox", self.active().name),
        };
        let Some(path) = dialog::pick_save(None, "MagicaVoxel", &["vox"], &name) else {
            return;
        };
        let _ = self.write_vox(&ensure_vox_extension(path));
    }

    pub(crate) fn write_vox(&mut self, path: &Path) -> Result<(), String> {
        let count: usize = self.objects.iter().map(|object| object.cells.len()).sum();
        let saved = self
            .objects
            .iter()
            .filter(|object| !object.cells.is_empty())
            .count();
        let bytes = match save_document(
            &self.objects,
            &self.palette,
            &self.materials,
            self.graph.as_ref(),
            &self.skeleton,
            &self.clips,
        ) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.status = e.clone();
                return Err(e);
            }
        };
        if let Err(e) = std::fs::write(path, bytes) {
            let message = format!("Could not write {}: {e}", path.display());
            self.status = message.clone();
            return Err(message);
        }
        self.path = Some(path.to_path_buf());
        self.mark_clean();
        let file = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.status = if saved > 1 {
            format!("Saved {count} voxels in {saved} objects to {file}.")
        } else {
            format!("Saved {count} voxels to {file}.")
        };
        self.remember_path(path);
        Ok(())
    }

    pub fn export_mesh_dialog(&mut self) {
        let name = format!("{}.glb", self.active().name);
        let Some(path) = dialog::pick_save(Some(&self.export_summary()), "glTF", &["glb"], &name)
        else {
            return;
        };
        let path = ensure_glb(path);
        match crate::export::export_glb(self, &path) {
            Ok(quads) => {
                let file = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                self.status = format!("Exported {quads} quads to {file}.");
            }
            Err(err) => self.status = err,
        }
    }

    /// The export dialog's message: what kind of file, with the Export panel options.
    pub fn export_summary(&self) -> String {
        let options = &self.export;
        let mut parts = vec![
            if options.merge {
                "merged faces"
            } else {
                "unmerged faces"
            },
            if options.vertex_colors {
                "vertex colors"
            } else {
                "palette texture"
            },
        ];
        if options.split_objects {
            parts.push("one mesh per object");
        }
        if options.export_hidden {
            parts.push("hidden objects included");
        }
        format!("Export Mesh — glTF binary (.glb): {}", parts.join(", "))
    }

    /// Queue a PNG of the Render-mode frame. The file is written once the
    /// preview has reached `max_samples`. A finished CPU frame is written now.
    /// Leaving Render mode, or an edit that starts the frame over, cancels it.
    pub fn save_render(&mut self) {
        if self.render_mode != RenderMode::Render {
            self.status = "Switch to Render and wait for a sample.".into();
            return;
        }
        let Some(path) = self.pick_render_path() else {
            return;
        };
        let cap = self.render_sample_cap();
        let ready = !self.trace_on_gpu
            && self
                .render_preview
                .as_ref()
                .is_some_and(|image| image.samples >= cap);
        if ready {
            let image = self.render_preview.clone().expect("preview");
            self.write_saved_render(&path, &image);
            return;
        }
        let replaced = self.render_save_path.replace(path).is_some();
        self.render_save_seen = None;
        self.status = if replaced {
            format!("Replaced the pending save. Saving when the frame reaches {cap} samples.")
        } else {
            format!("Saving when the frame reaches {cap} samples.")
        };
    }

    /// Drop a queued Save Render, saying why in the status line.
    pub(crate) fn cancel_pending_render(&mut self, why: &str) {
        if self.render_save_path.take().is_some() {
            self.render_save_seen = None;
            self.status = format!("Save Render cancelled: {why}.");
        }
    }

    pub(crate) fn render_sample_cap(&self) -> u32 {
        self.max_samples.clamp(1, 8192) as u32
    }

    /// Hand back the pending PNG path once `samples` has reached the cap.
    /// A sample count lower than the last one seen means the frame started
    /// over, so the save is cancelled rather than written from a new picture.
    pub(crate) fn take_finished_render(&mut self, samples: u32) -> Option<PathBuf> {
        self.render_save_path.as_ref()?;
        if self.render_save_seen.is_some_and(|seen| samples < seen) {
            self.cancel_pending_render("the picture changed before it finished");
            return None;
        }
        self.render_save_seen = Some(samples);
        if samples < self.render_sample_cap() {
            return None;
        }
        self.render_save_seen = None;
        self.render_save_path.take()
    }

    pub(crate) fn write_saved_render(&mut self, path: &Path, image: &crate::trace::Image) {
        match crate::trace::write_png(path, image, self) {
            Ok(()) => {
                let file = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                self.status = format!("Saved render to {file}.");
            }
            Err(err) => self.status = err,
        }
    }

    pub(super) fn pick_render_path(&self) -> Option<PathBuf> {
        let name = format!("{}.png", self.active().name);
        let path = dialog::pick_save(None, "PNG", &["png"], &name)?;
        Some(if path.extension().is_some() {
            path
        } else {
            path.with_extension("png")
        })
    }

    pub fn recent_paths(&self) -> Vec<PathBuf> {
        self.recent.clone()
    }

    pub(super) fn remember_path(&mut self, path: &Path) {
        remember(&mut self.recent, path);
        store_recent(&self.recent);
    }

    pub fn clear_ui(&mut self) {
        self.ui_regions.clear();
    }

    pub fn note_ui(&mut self, id: &str, label: &str, rect: egui::Rect, action: UiAction) {
        self.ui_regions.push(UiRegion {
            id: id.to_string(),
            label: label.to_string(),
            x: rect.min.x,
            y: rect.min.y,
            w: rect.width(),
            h: rect.height(),
            action,
        });
    }

    /// Topmost registered control under a window point. `None` actions,
    /// such as the window rectangle, are not click targets. Phone controls
    /// with a `mobile.` id stay clickable: undo and the sheets have no
    /// session action.
    pub fn ui_target(&self, x: f32, y: f32) -> Option<UiRegion> {
        self.ui_regions
            .iter()
            .rev()
            .find(|region| listed_control(region) && region.contains(x, y))
            .cloned()
    }

    pub fn activate_ui(&mut self, region: &UiRegion, at: Option<(f32, f32)>) -> Result<(), String> {
        match region.action {
            UiAction::None | UiAction::Viewport => {}
            UiAction::Tool(tool) => self.tool = tool,
            UiAction::Mode(mode) => self.tool_mode = mode,
            UiAction::Editor(mode) => self.editor_mode = mode,
            UiAction::Render(mode) => self.render_mode = mode,
            UiAction::NewObject => self.new_object(),
            UiAction::Duplicate => self.duplicate_object(),
            UiAction::DeleteObject => {
                if self.objects.len() < 2 {
                    return Err("the only object cannot be deleted".into());
                }
                self.delete_object();
            }
            UiAction::ObjectHidden(index) => {
                let hidden = !self.objects.get(index).ok_or("no such object")?.hidden;
                self.set_object(index, None, None, Some(hidden))?;
            }
            UiAction::Mirror(axis) => {
                self.mirror_selected(axis)?;
            }
            UiAction::MirrorBones => self.mirror_bones = !self.mirror_bones,
            UiAction::ClearPose => self.clear_pose(),
            UiAction::AddClip => self.add_drafted_clip()?,
            UiAction::KeyPose => {
                self.key_pose()?;
            }
            UiAction::RemoveClip => self.remove_selected_clip()?,
            UiAction::ShowBones => self.show_bones = !self.show_bones,
            UiAction::Export(flag) => {
                let value = match flag {
                    ExportFlag::Merge => &mut self.export.merge,
                    ExportFlag::VertexColors => &mut self.export.vertex_colors,
                    ExportFlag::Hidden => &mut self.export.export_hidden,
                    ExportFlag::Split => &mut self.export.split_objects,
                };
                *value = !*value;
            }
            UiAction::Palette(slot) => self.select_palette_slot(slot, false),
            UiAction::Slider(spec) => {
                let (x, _) = at.ok_or("slider click needs a point")?;
                let t = if region.w <= f32::EPSILON {
                    0.0
                } else {
                    (x - region.x) / region.w
                };
                self.apply_slider(spec, t);
            }
        }
        Ok(())
    }

    pub(super) fn apply_slider(&mut self, spec: SliderSpec, t: f32) {
        let t = t.clamp(0.0, 1.0);
        let value = spec.min + (spec.max - spec.min) * t;
        if spec.target.is_material() {
            self.checkpoint();
        }
        self.write_slider(spec.target, value);
        if spec.target.is_material() {
            self.commit();
        }
    }

    pub(crate) fn write_slider(&mut self, target: SliderTarget, value: f32) {
        let whole = value.round() as i32;
        match target {
            SliderTarget::BrushSize => self.brush_size = whole.clamp(1, 63),
            SliderTarget::FoliageSeed => self.foliage_seed = whole.clamp(1, 255),
            SliderTarget::FoliageDensity => self.foliage_density = value.clamp(0.0, 1.0),
            SliderTarget::TorusTube => self.torus_tube = value.clamp(0.05, 1.0),
            SliderTarget::RoundRadius => self.round_radius = value.clamp(0.0, 32.0),
            SliderTarget::NoiseScale => self.noise_scale = value.clamp(1.0, 64.0),
            SliderTarget::NoiseThreshold => self.noise_threshold = value.clamp(0.0, 1.0),
            SliderTarget::NoiseOctaves => self.noise_octaves = whole.clamp(1, 6),
            SliderTarget::NoiseSeed => self.noise_seed = whole.clamp(0, 9999),
            SliderTarget::PerlinLo => self.perlin_lo = whole.clamp(1, 255),
            SliderTarget::PerlinHi => self.perlin_hi = whole.clamp(1, 255),
            SliderTarget::TreeDepth => self.tree_depth = whole.clamp(1, 5),
            SliderTarget::TreeWildness => self.tree_wildness = value.clamp(0.0, 1.0),
            SliderTarget::TreeTrunk => self.tree_trunk = value.clamp(0.20, 1.50),
            SliderTarget::TreeLeaf => self.tree_leaf = whole.clamp(1, 255),
            SliderTarget::TreeSeed => self.tree_seed = whole.clamp(0, 9999),
            SliderTarget::HelixTurns => self.helix_turns = value.clamp(0.25, 12.0),
            SliderTarget::HelixRadius => self.helix_radius = value.clamp(0.0, 1.0),
            SliderTarget::HelixTube => self.helix_tube = value.clamp(0.02, 0.90),
            SliderTarget::ConiferTiers => self.conifer_tiers = whole.clamp(2, 5),
            SliderTarget::ConiferRagged => self.conifer_ragged = value.clamp(0.0, 1.0),
            SliderTarget::ConiferClump => self.conifer_clump = value.clamp(2.0, 24.0),
            SliderTarget::ConiferTrunk => self.conifer_trunk = value.clamp(0.20, 1.50),
            SliderTarget::ConiferCrown => self.conifer_crown = value.clamp(0.0, 1.0),
            SliderTarget::ConiferNeedle => self.conifer_needle = whole.clamp(1, 255),
            SliderTarget::ConiferSeed => self.conifer_seed = whole.clamp(0, 9999),
            SliderTarget::MaxSamples => self.max_samples = whole.clamp(16, 8192),
            SliderTarget::Aperture => self.aperture = value.clamp(0.0, 3.0),
            SliderTarget::FocusDistance => self.focus_distance = value.clamp(1.0, 500.0),
            SliderTarget::MaterialRoughness => {
                self.material_mut().roughness = value.clamp(0.0, 1.0)
            }
            SliderTarget::MaterialMetallic => self.material_mut().metallic = value.clamp(0.0, 1.0),
            SliderTarget::MaterialTransparency => {
                self.material_mut().transparency = value.clamp(0.0, 1.0);
            }
            SliderTarget::MaterialIor => self.material_mut().ior = value.clamp(1.0, 3.0),
            SliderTarget::MaterialEmission => {
                self.material_mut().emission = (value / 100.0).clamp(0.0, 1.0);
            }
            SliderTarget::MaterialPower => self.material_mut().emission_power = whole.clamp(1, 5),
            SliderTarget::MotionAmount => {
                self.material_mut().motion_amount = value.clamp(0.0, 4.0);
                self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
            }
            SliderTarget::MaxBounces => self.max_bounces = whole.clamp(1, 8),
            SliderTarget::NeeRis => self.nee_ris = whole.clamp(1, 32),
            SliderTarget::IndirectClamp => self.indirect_clamp = value.clamp(0.0, 32.0),
            SliderTarget::EmaFloor => self.ema_floor = value.clamp(0.0, 1.0),
            SliderTarget::SkyIntensity => self.sky_intensity = value.clamp(0.0, 4.0),
            SliderTarget::HorizonSoftness => self.horizon_softness = value.clamp(0.0, 15.0),
            SliderTarget::SunElevation => self.sun_elevation = value.clamp(-90.0, 90.0),
            SliderTarget::SunAzimuth => self.sun_azimuth = value.clamp(0.0, 360.0),
            SliderTarget::SunDisc => self.sun_disc = value.clamp(0.1, 30.0),
            SliderTarget::SunIntensity => self.sun_intensity = value.clamp(0.0, 10.0),
        }
    }

    /// Press and release at one world ray, so a connected click can stamp
    /// the active tool without a second frame.
    pub(crate) fn click_viewport(&mut self, ray: (Vec3, Vec3)) {
        let drag = ToolDrag {
            rays: vec![ray],
            face_steps: 1,
            marquee: None,
            shift: false,
            object_band: None,
        };
        self.pointer(
            true,
            true,
            true,
            true,
            false,
            false,
            false,
            false,
            false,
            glam::Vec2::ZERO,
            0.0,
            Some(ray),
            &drag,
            false,
            false,
        );
    }

    pub fn describe_ui(&self) -> serde_json::Value {
        let window = self.ui_regions.iter().find(|region| region.id == "window");
        let regions: Vec<serde_json::Value> = self
            .ui_regions
            .iter()
            .filter(|region| listed_control(region))
            .map(|region| {
                serde_json::json!({
                    "id": region.id,
                    "label": region.label,
                    "x": region.x,
                    "y": region.y,
                    "width": region.w,
                    "height": region.h,
                })
            })
            .collect();
        serde_json::json!({
            "tool": self.tool.name(),
            "toolMode": match self.tool_mode {
                ToolMode::Add => "Add",
                ToolMode::Remove => "Remove",
                ToolMode::Paint => "Paint",
            },
            "editorMode": match self.editor_mode {
                EditorMode::Model => "Model",
                EditorMode::Object => "Object",
            },
            "renderMode": match self.render_mode {
                RenderMode::Edit => "Edit",
                RenderMode::Render => "Render",
            },
            "activeObject": self.active().name,
            "voxels": self.voxel_count(),
            "status": self.status,
            "window": window.map(|region| serde_json::json!({
                "x": region.x,
                "y": region.y,
                "width": region.w,
                "height": region.h,
            })),
            "regions": regions,
        })
    }

    pub fn take_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.load_path(&path);
        }
    }
}

fn listed_control(region: &UiRegion) -> bool {
    region.action != UiAction::None || region.id.starts_with("mobile.")
}

fn graph_differences(left: Option<&VoxGraph>, right: Option<&VoxGraph>) -> Vec<String> {
    let (Some(left), Some(right)) = (left, right) else {
        return if left.is_none() && right.is_none() {
            Vec::new()
        } else {
            vec!["scene graph presence differs".into()]
        };
    };
    let mut out = Vec::new();
    if left.layers != right.layers {
        out.push(format!(
            "layer count or attributes: {} -> {}",
            left.layers.len(),
            right.layers.len()
        ));
    }
    if left.nodes.len() != right.nodes.len() {
        out.push(format!(
            "scene node count: {} -> {}",
            left.nodes.len(),
            right.nodes.len()
        ));
        return out;
    }
    for (index, (a, b)) in left.nodes.iter().zip(&right.nodes).enumerate() {
        if a.frames != b.frames {
            out.push(format!("node {index} frames differ"));
        }
        if a.name != b.name || a.attrs != b.attrs {
            out.push(format!("node {index} name or attributes differ"));
        }
        if a.t != b.t || a.rot != b.rot {
            out.push(format!("node {index} file transform differs"));
        }
        if a.kind != b.kind
            || a.child != b.child
            || a.children != b.children
            || a.models != b.models
        {
            out.push(format!("node {index} links differ"));
        }
    }
    if left.models != right.models {
        out.push("stored model voxels differ".into());
    }
    if left.shift != right.shift {
        out.push(format!(
            "scene shift: {:?} -> {:?}",
            left.shift, right.shift
        ));
    }
    out
}
