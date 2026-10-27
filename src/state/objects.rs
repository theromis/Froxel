//! Objects, bones, and undo.

use std::collections::HashMap;

use super::geom::*;
use super::vox::*;
use super::*;

impl Session {
    /// Empty object at a world voxel position. `size` is the SIZE box when
    /// set; voxels may still grow it up to the per-axis save limit.
    pub fn add_object(
        &mut self,
        name: Option<&str>,
        position: [i32; 3],
        size: Option<[i32; 3]>,
        make_active: bool,
    ) -> Result<usize, String> {
        if let Some(size) = size {
            for axis in size {
                if !(1..=256).contains(&axis) {
                    return Err("size must be 1..=256 on each axis".into());
                }
            }
        }
        let name = match name {
            Some(name) => check_object_name(name)?,
            None => self.fresh_object_name(),
        };
        self.commit();
        self.checkpoint();
        self.objects.push(Volume {
            name,
            cells: HashMap::new(),
            translation: (position[0], position[1], position[2]),
            box_min: (0, 0, 0),
            dims: size
                .map(|size| (size[0], size[1], size[2]))
                .unwrap_or((0, 0, 0)),
            rot: ROT_IDENTITY,
            pivot: (0, 0, 0),
            model_dims: (0, 0, 0),
            hidden: false,
            file: None,
            bone: None,
            script: Default::default(),
        });
        let index = self.objects.len() - 1;
        if make_active {
            self.selected = index;
            self.focus_object();
        }
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok(index)
    }

    pub fn set_object(
        &mut self,
        index: usize,
        name: Option<&str>,
        position: Option<[i32; 3]>,
        hidden: Option<bool>,
    ) -> Result<(), String> {
        if index >= self.objects.len() {
            return Err(format!(
                "object index {index} is out of range ({} objects)",
                self.objects.len()
            ));
        }
        if name.is_none() && position.is_none() && hidden.is_none() {
            return Err("set_object needs a name, position, or hidden".into());
        }
        let next_name = match name {
            Some(name) => check_object_name(name)?,
            None => self.objects[index].name.clone(),
        };
        let next_position = position.unwrap_or([
            self.objects[index].translation.0,
            self.objects[index].translation.1,
            self.objects[index].translation.2,
        ]);
        let next_hidden = hidden.unwrap_or(self.objects[index].hidden);
        let object = &self.objects[index];
        if next_name == object.name
            && next_position
                == [
                    object.translation.0,
                    object.translation.1,
                    object.translation.2,
                ]
            && next_hidden == object.hidden
        {
            return Ok(());
        }
        self.commit();
        self.checkpoint();
        let object = &mut self.objects[index];
        let delta = (
            next_position[0] - object.translation.0,
            next_position[1] - object.translation.1,
            next_position[2] - object.translation.2,
        );
        object.name = next_name;
        object.translation = (next_position[0], next_position[1], next_position[2]);
        if object.model_dims != (0, 0, 0) {
            object.pivot.0 += delta.0;
            object.pivot.1 += delta.1;
            object.pivot.2 += delta.2;
        }
        object.hidden = next_hidden;
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok(())
    }

    /// Drop one object. Its bone stays in the skeleton; the binding goes
    /// with the object. Later indices decrease by one.
    pub fn remove_object(&mut self, index: usize) -> Result<(), String> {
        if self.objects.len() < 2 {
            return Err("cannot remove the last object".into());
        }
        if index >= self.objects.len() {
            return Err(format!(
                "object index {index} is out of range ({} objects)",
                self.objects.len()
            ));
        }
        self.commit();
        self.checkpoint();
        self.objects.remove(index);
        self.object_selection = self
            .object_selection
            .iter()
            .filter_map(|found| {
                if *found == index {
                    None
                } else if *found > index {
                    Some(found - 1)
                } else {
                    Some(*found)
                }
            })
            .collect();
        if self.selected == index {
            self.selected = index.min(self.objects.len() - 1);
        } else if self.selected > index {
            self.selected -= 1;
        }
        if self.object_selection.is_empty() {
            self.focus_object();
        }
        self.clear_picked();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok(())
    }

    fn fresh_object_name(&mut self) -> String {
        let mut number = self.next_object.max(1);
        let name = loop {
            let candidate = format!("Object {number}");
            if self.objects.iter().all(|object| object.name != candidate) {
                break candidate;
            }
            number += 1;
        };
        self.next_object = number.saturating_add(1);
        name
    }

    pub fn new_object(&mut self) {
        self.commit();
        self.checkpoint();
        let name = self.fresh_object_name();
        let translation = beside(self.active());
        self.objects.push(Volume {
            name,
            cells: HashMap::new(),
            translation,
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
        self.selected = self.objects.len() - 1;
        self.focus_object();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
    }

    pub fn duplicate_object(&mut self) {
        self.commit();
        self.checkpoint();
        let source = self.active();
        let translation = beside(source);
        let delta = (
            translation.0 - source.translation.0,
            translation.1 - source.translation.1,
            translation.2 - source.translation.2,
        );
        let copy = Volume {
            name: format!("{} copy", source.name),
            cells: source.cells.clone(),
            translation,
            box_min: source.box_min,
            dims: source.dims,
            rot: source.rot,
            pivot: (
                source.pivot.0 + delta.0,
                source.pivot.1 + delta.1,
                source.pivot.2 + delta.2,
            ),
            model_dims: source.model_dims,
            hidden: source.hidden,
            file: None,
            bone: source.bone.clone(),
            script: source.script.clone(),
        };
        self.objects.insert(self.selected + 1, copy);
        self.selected += 1;
        self.focus_object();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
    }

    pub fn delete_object(&mut self) {
        if self.objects.len() < 2 {
            return;
        }
        self.commit();
        self.checkpoint();
        self.objects.remove(self.selected);
        self.selected = self.selected.min(self.objects.len() - 1);
        self.focus_object();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
    }

    pub fn rename_selected(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() || name == self.active().name {
            return;
        }
        self.checkpoint();
        self.active_mut().name = name;
        self.commit();
    }

    pub fn add_bone(
        &mut self,
        name: &str,
        parent: Option<&str>,
        pivot: [f32; 3],
        rotation: Option<[f32; 4]>,
    ) -> Result<(), String> {
        let mut next = self.skeleton.clone();
        next.add(name, parent, pivot, rotation)?;
        self.checkpoint();
        self.skeleton = next;
        self.selected_bone = Some(self.skeleton.bones.len() - 1);
        self.commit();
        Ok(())
    }

    pub fn set_bone(
        &mut self,
        name: &str,
        parent: Option<Option<String>>,
        pivot: Option<[f32; 3]>,
        rotation: Option<[f32; 4]>,
        rename: Option<&str>,
    ) -> Result<(), String> {
        let mut next = self.skeleton.clone();
        let renamed = next.set(name, parent, pivot, rotation, rename)?;
        self.checkpoint();
        self.skeleton = next;
        if let Some(next_name) = renamed {
            for object in &mut self.objects {
                if object.bone.as_deref() == Some(name) {
                    object.bone = Some(next_name.clone());
                }
            }
            if let Some(index) = self.skeleton.index_of(&next_name) {
                self.selected_bone = Some(index);
            }
        }
        self.commit();
        Ok(())
    }

    pub fn remove_bone(&mut self, name: &str) -> Result<(), String> {
        let removed = self.skeleton.index_of(name);
        let mut next = self.skeleton.clone();
        next.remove(name)?;
        self.checkpoint();
        self.skeleton = next;
        for object in &mut self.objects {
            if object.bone.as_deref() == Some(name) {
                object.bone = None;
            }
        }
        if let Some(index) = self.selected_bone {
            if Some(index) == removed {
                self.selected_bone = None;
            } else if removed.is_some_and(|gone| index > gone) {
                self.selected_bone = Some(index - 1);
            }
        }
        self.commit();
        Ok(())
    }

    /// Replace the preview pose. Omitted bones stay at the bind pose. An
    /// identity entry is dropped. `Bone.rotation` is not written.
    pub fn set_pose(&mut self, pose: crate::rig::Pose) -> Result<(), String> {
        let next = self.checked_pose(pose)?;
        if next == self.pose {
            return Ok(());
        }
        self.checkpoint();
        self.pose = next;
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok(())
    }

    pub fn clear_pose(&mut self) {
        if self.pose.is_empty() {
            return;
        }
        self.checkpoint();
        self.pose.clear();
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
    }

    pub fn checked_pose(&self, pose: crate::rig::Pose) -> Result<crate::rig::Pose, String> {
        let mut next = crate::rig::Pose::new();
        for (name, extra) in pose {
            if self.skeleton.index_of(&name).is_none() {
                return Err(format!("unknown bone `{name}`"));
            }
            if extra
                .scale
                .iter()
                .any(|axis| !axis.is_finite() || !(0.0..=MAX_POSE_SCALE).contains(axis))
            {
                return Err(format!(
                    "pose `{name}` scale must be between 0 and {MAX_POSE_SCALE}"
                ));
            }
            let rotation = normalize_extra(extra.rotation)?;
            let still = extra.scale.iter().all(|axis| (axis - 1.0).abs() < 1e-6);
            if rotation.is_none() && still {
                continue;
            }
            next.insert(
                name,
                crate::rig::BonePose {
                    rotation: rotation.unwrap_or(crate::rig::IDENTITY_ROTATION),
                    scale: extra.scale,
                },
            );
        }
        Ok(next)
    }

    pub fn add_clip(&mut self, name: &str) -> Result<(), String> {
        let name = clip_name(name)?;
        if self.clips.iter().any(|clip| clip.name == name) {
            return Err(format!("clip `{name}` already exists"));
        }
        self.checkpoint();
        self.clips.push(crate::rig::Clip {
            name,
            tracks: Vec::new(),
        });
        self.commit();
        Ok(())
    }

    pub fn set_clip_keys(
        &mut self,
        name: &str,
        bone: &str,
        mut keys: Vec<crate::rig::ClipKey>,
    ) -> Result<(), String> {
        if self.skeleton.index_of(bone).is_none() {
            return Err(format!("unknown bone `{bone}`"));
        }
        let index = self
            .clips
            .iter()
            .position(|clip| clip.name == name)
            .ok_or_else(|| format!("unknown clip `{name}`"))?;
        if keys.is_empty() {
            return Err("clip key list is empty".into());
        }
        for key in &mut keys {
            key.rotation = key.rotation.map(crate::rig::checked_rotation).transpose()?;
            if !key.time.is_finite() || key.time < 0.0 {
                return Err("key time must be finite and at least 0".into());
            }
            if key
                .translation
                .unwrap_or_default()
                .iter()
                .chain(key.scale.iter())
                .any(|value| !value.is_finite())
            {
                return Err("key translation and scale must be finite".into());
            }
        }
        keys.sort_by(|left, right| {
            left.time
                .partial_cmp(&right.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for pair in keys.windows(2) {
            if pair[1].time <= pair[0].time {
                return Err("clip key times must increase".into());
            }
        }
        self.checkpoint();
        let tracks = &mut self.clips[index].tracks;
        if let Some(track) = tracks.iter_mut().find(|track| track.bone == bone) {
            track.keys = keys;
        } else {
            tracks.push(crate::rig::BoneTrack {
                bone: bone.to_string(),
                keys,
            });
        }
        self.commit();
        Ok(())
    }

    pub fn remove_clip(&mut self, name: &str) -> Result<(), String> {
        let index = self
            .clips
            .iter()
            .position(|clip| clip.name == name)
            .ok_or_else(|| format!("unknown clip `{name}`"))?;
        self.checkpoint();
        self.clips.remove(index);
        self.commit();
        Ok(())
    }

    /// Copy an object mirrored through the origin on one axis.
    /// `mirror_bones` also copies the bound bone and its descendants,
    /// swapping an `_l` / `_r` suffix.
    pub fn mirror_object(
        &mut self,
        index: usize,
        axis: usize,
        name: &str,
        position: Option<[i32; 3]>,
        mirror_bones: bool,
    ) -> Result<(usize, Vec<String>), String> {
        if index >= self.objects.len() {
            return Err(format!(
                "object index {index} is out of range ({} objects)",
                self.objects.len()
            ));
        }
        if axis > 2 {
            return Err("axis must be x, y, or z".into());
        }
        let name = check_object_name(name)?;
        let source = &self.objects[index];
        let mut world = Vec::new();
        for (cell, slot) in &source.cells {
            let mut at = [
                cell.0 + source.translation.0,
                cell.1 + source.translation.1,
                cell.2 + source.translation.2,
            ];
            at[axis] = -at[axis] - 1;
            if !in_world(at[0], at[1], at[2]) {
                return Err("mirrored cell is outside the world [-1024, 1024)".into());
            }
            world.push((at, *slot));
        }
        let origin = if let Some(position) = position {
            position
        } else if world.is_empty() {
            let mut at = [
                source.translation.0,
                source.translation.1,
                source.translation.2,
            ];
            at[axis] = -at[axis];
            at
        } else {
            let mut min = world[0].0;
            for (at, _) in &world {
                min[0] = min[0].min(at[0]);
                min[1] = min[1].min(at[1]);
                min[2] = min[2].min(at[2]);
            }
            min
        };
        let mut cells = HashMap::new();
        for (at, slot) in world {
            let local = [at[0] - origin[0], at[1] - origin[1], at[2] - origin[2]];
            if !in_world(local[0], local[1], local[2]) {
                return Err("mirrored cell is outside the world [-1024, 1024)".into());
            }
            cells.insert((local[0], local[1], local[2]), slot);
        }
        if let Some((min, max)) = cell_bounds(cells.keys().copied()) {
            let span = [max.0 - min.0 + 1, max.1 - min.1 + 1, max.2 - min.2 + 1];
            if span.iter().any(|axis| *axis > 256) {
                return Err(format!(
                    "mirrored object would be {}x{}x{}; .vox models are at most 256 per axis",
                    span[0], span[1], span[2]
                ));
            }
        }
        let hidden = source.hidden;
        let (skeleton, bone, added) = if mirror_bones {
            self.mirrored_bones(index, axis)?
        } else {
            (self.skeleton.clone(), None, Vec::new())
        };
        self.checkpoint();
        self.skeleton = skeleton;
        self.objects.push(Volume {
            name,
            cells,
            translation: (origin[0], origin[1], origin[2]),
            box_min: (0, 0, 0),
            dims: (0, 0, 0),
            rot: ROT_IDENTITY,
            pivot: (0, 0, 0),
            model_dims: (0, 0, 0),
            hidden,
            file: None,
            bone,
            script: Default::default(),
        });
        let created = self.objects.len() - 1;
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok((created, added))
    }

    fn mirrored_bones(
        &self,
        index: usize,
        axis: usize,
    ) -> Result<(Skeleton, Option<String>, Vec<String>), String> {
        let Some(bound) = self.objects[index].bone.as_deref() else {
            return Err("object is not bound to a bone".into());
        };
        let root = self
            .skeleton
            .index_of(bound)
            .ok_or_else(|| format!("unknown bone `{bound}`"))?;
        let mut members = Vec::new();
        for bone_index in 0..self.skeleton.bones.len() {
            let mut cursor = Some(bone_index);
            let mut guard = 0;
            while let Some(current) = cursor {
                if current == root {
                    members.push(bone_index);
                    break;
                }
                cursor = self.skeleton.parent_index(current);
                guard += 1;
                if guard > self.skeleton.bones.len() {
                    break;
                }
            }
        }
        let order = parent_order(&self.skeleton);
        members.sort_by_key(|index| order.iter().position(|item| item == index).unwrap_or(0));
        let mut next = self.skeleton.clone();
        let mut renamed: HashMap<String, String> = HashMap::new();
        let mut added = Vec::new();
        for bone_index in members {
            let bone = &self.skeleton.bones[bone_index];
            let new_name = crate::rig::mirrored_bone_name(&bone.name);
            if next.index_of(&new_name).is_some() || renamed.values().any(|name| name == &new_name)
            {
                return Err(format!("bone `{new_name}` already exists"));
            }
            let parent = match &bone.parent {
                Some(parent) => Some(
                    renamed
                        .get(parent)
                        .cloned()
                        .unwrap_or_else(|| parent.clone()),
                ),
                None => None,
            };
            let mut pivot = bone.pivot;
            pivot[axis] = -pivot[axis];
            let rotation = crate::rig::mirror_rotation(bone.rotation, axis);
            next.add(&new_name, parent.as_deref(), pivot, Some(rotation))?;
            renamed.insert(bone.name.clone(), new_name.clone());
            added.push(new_name);
        }
        let bone = renamed.get(bound).cloned();
        Ok((next, bone, added))
    }

    /// World rigid map for one object. `None` when the preview pose is empty,
    /// so the mesh hash and the tracer stay on the bind pose.
    pub fn skin_delta(&self, index: usize) -> Option<glam::Mat4> {
        if self.pose.is_empty() {
            return None;
        }
        let object = self.objects.get(index)?;
        let deltas = crate::rig::skin_deltas(&self.skeleton, &self.pose).ok()?;
        let joint = self.skeleton.joint_or_root(object.bone.as_deref()).ok()? as usize;
        deltas.get(joint).copied()
    }

    pub fn bind_object(&mut self, index: usize, bone: Option<&str>) -> Result<(), String> {
        if index >= self.objects.len() {
            return Err(format!(
                "object index {index} is out of range ({} objects)",
                self.objects.len()
            ));
        }
        if let Some(name) = bone {
            if self.skeleton.index_of(name).is_none() {
                return Err(format!("unknown bone `{name}`"));
            }
        }
        let next = bone.map(str::to_string);
        if self.objects[index].bone == next {
            return Ok(());
        }
        self.checkpoint();
        self.objects[index].bone = next;
        self.commit();
        Ok(())
    }

    /// One undo step for a numeric pivot drag. The first change snapshots.
    pub fn preview_bone_pivot(&mut self, index: usize, axis: usize, value: f32) {
        if axis > 2 || !value.is_finite() {
            return;
        }
        let Some(current) = self.skeleton.bones.get(index).map(|bone| bone.pivot[axis]) else {
            return;
        };
        if current == value {
            return;
        }
        self.checkpoint();
        self.skeleton.bones[index].pivot[axis] = value;
    }

    pub fn finish_bone_edit(&mut self) {
        self.commit();
    }

    /// Gather the edits of a panel drag into one undo step. The Session
    /// methods it calls still checkpoint and commit as they would from MCP.
    pub fn begin_gesture(&mut self) {
        self.commit();
        self.gesture = true;
    }

    pub fn end_gesture(&mut self) {
        if self.gesture {
            self.gesture = false;
            self.commit();
        }
    }

    /// Run `edit` as one undo step. When it fails, the document is put back as
    /// it was and no undo step is added. Inside a panel drag the drag keeps
    /// gathering the edits.
    pub fn transaction<T>(
        &mut self,
        edit: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let outer = self.gesture;
        if !outer {
            self.begin_gesture();
        }
        let before = self.snapshot();
        let result = edit(self);
        if result.is_err() {
            self.restore(before);
            self.clamp_selection();
            self.clear_picked();
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
            if !outer {
                self.pending = None;
                self.gesture = false;
            }
        } else if !outer {
            self.end_gesture();
        }
        result
    }

    /// Mirror the active object through the origin and make the copy active.
    /// `mirror_bones` decides whether its bone subtree is copied too.
    pub fn mirror_selected(&mut self, axis: usize) -> Result<usize, String> {
        let name = crate::rig::mirrored_bone_name(&self.active().name);
        let (created, _) =
            self.mirror_object(self.selected, axis, &name, None, self.mirror_bones)?;
        self.select_object(created);
        Ok(created)
    }

    /// Replace one bone's preview transform. Identity drops it from the pose.
    pub fn set_bone_pose(
        &mut self,
        index: usize,
        extra: crate::rig::BonePose,
    ) -> Result<(), String> {
        let name = self
            .skeleton
            .bones
            .get(index)
            .map(|bone| bone.name.clone())
            .ok_or("no bone selected")?;
        let mut pose = self.pose.clone();
        pose.insert(name, extra);
        self.set_pose(pose)
    }

    /// Add a clip named by `clip_draft`, or `Clip N` when it is blank, and select it.
    pub fn add_drafted_clip(&mut self) -> Result<(), String> {
        let mut name = self.clip_draft.trim().to_string();
        if name.is_empty() {
            let mut number = self.clips.len() + 1;
            loop {
                name = format!("Clip {number}");
                if !self.clips.iter().any(|clip| clip.name == name) {
                    break;
                }
                number += 1;
            }
        }
        self.add_clip(&name)?;
        self.selected_clip = Some(self.clips.len() - 1);
        self.clip_draft.clear();
        Ok(())
    }

    pub fn remove_selected_clip(&mut self) -> Result<(), String> {
        let name = self
            .selected_clip
            .and_then(|index| self.clips.get(index))
            .map(|clip| clip.name.clone())
            .ok_or("no clip selected")?;
        self.remove_clip(&name)?;
        self.selected_clip = None;
        Ok(())
    }

    /// Key the selected bone and every posed bone at `clip_time` in the
    /// selected clip, as one undo step. A key at the same time is replaced.
    /// The rotation is the bind rotation times the pose; an unposed bone
    /// keeps its bind rotation. Returns how many bones were keyed.
    pub fn key_pose(&mut self) -> Result<usize, String> {
        let clip = self
            .selected_clip
            .and_then(|index| self.clips.get(index))
            .cloned()
            .ok_or("no clip selected")?;
        let time = self.clip_time;
        let bones: Vec<usize> = (0..self.skeleton.bones.len())
            .filter(|index| {
                self.selected_bone == Some(*index)
                    || self.pose.contains_key(&self.skeleton.bones[*index].name)
            })
            .collect();
        if bones.is_empty() {
            return Err("select or pose a bone to key".into());
        }
        let mut writes = Vec::new();
        for index in bones {
            let bone = &self.skeleton.bones[index];
            let extra = self.pose.get(&bone.name).copied();
            let rotation = extra.map(|extra| {
                let bind = glam::Quat::from_array(bone.rotation);
                let turn = glam::Quat::from_array(extra.rotation);
                (bind * turn).normalize().to_array()
            });
            let key = crate::rig::ClipKey {
                time,
                rotation,
                translation: None,
                scale: extra.map_or([1.0, 1.0, 1.0], |extra| extra.scale),
            };
            let mut keys: Vec<crate::rig::ClipKey> = clip
                .tracks
                .iter()
                .find(|track| track.bone == bone.name)
                .map(|track| track.keys.clone())
                .unwrap_or_default();
            keys.retain(|old| (old.time - time).abs() > 1.0e-4);
            keys.push(key);
            writes.push((bone.name.clone(), keys));
        }
        let count = writes.len();
        self.begin_gesture();
        let mut result = Ok(());
        for (bone, keys) in writes {
            result = self.set_clip_keys(&clip.name, &bone, keys);
            if result.is_err() {
                break;
            }
        }
        self.end_gesture();
        result.map(|_| count)
    }

    pub fn fresh_bone_name(&self) -> String {
        let mut number = 1;
        loop {
            let name = if number == 1 {
                "Bone".to_string()
            } else {
                format!("Bone {number}")
            };
            if self.skeleton.index_of(&name).is_none() {
                return name;
            }
            number += 1;
        }
    }

    /// Centre of the active object's voxels, in world voxel coordinates.
    pub fn active_bone_pivot(&self) -> [f32; 3] {
        let object = self.active();
        let mut any = false;
        let mut min = [0; 3];
        let mut max = [0; 3];
        for (x, y, z) in object.cells.keys() {
            if !any {
                min = [*x, *y, *z];
                max = [*x, *y, *z];
                any = true;
            } else {
                min[0] = min[0].min(*x);
                min[1] = min[1].min(*y);
                min[2] = min[2].min(*z);
                max[0] = max[0].max(*x);
                max[1] = max[1].max(*y);
                max[2] = max[2].max(*z);
            }
        }
        if !any {
            return [
                object.translation.0 as f32,
                object.translation.1 as f32,
                object.translation.2 as f32,
            ];
        }
        [
            object.translation.0 as f32 + (min[0] + max[0] + 1) as f32 * 0.5,
            object.translation.1 as f32 + (min[1] + max[1] + 1) as f32 * 0.5,
            object.translation.2 as f32 + (min[2] + max[2] + 1) as f32 * 0.5,
        ]
    }

    pub fn select_object(&mut self, index: usize) {
        if index >= self.objects.len() {
            return;
        }
        if index != self.selected {
            self.selected = index;
            self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
            self.clear_picked();
        }
        self.focus_object();
    }

    pub(super) fn focus_object(&mut self) {
        if self.objects.is_empty() {
            self.object_selection.clear();
            return;
        }
        self.selected = self.selected.min(self.objects.len() - 1);
        self.object_selection = vec![self.selected];
    }

    pub(super) fn choose_objects(&mut self, mut members: Vec<usize>, primary: usize) {
        members.retain(|index| *index < self.objects.len());
        members.sort_unstable();
        members.dedup();
        if members.is_empty() {
            self.focus_object();
            return;
        }
        self.object_selection = members;
        self.selected = if self.object_selection.contains(&primary) {
            primary
        } else {
            self.object_selection[self.object_selection.len() - 1]
        };
        self.clear_picked();
    }

    pub(super) fn toggle_object(&mut self, index: usize) {
        if index >= self.objects.len() {
            return;
        }
        if let Some(pos) = self.object_selection.iter().position(|i| *i == index) {
            if self.object_selection.len() == 1 {
                return;
            }
            self.object_selection.remove(pos);
            if self.selected == index {
                self.selected = self.object_selection[self.object_selection.len() - 1];
                self.clear_picked();
            }
            return;
        }
        self.object_selection.push(index);
        self.selected = index;
        self.clear_picked();
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            objects: self.objects.clone(),
            palette: self.palette,
            materials: self.materials,
            graph: self.graph.clone(),
            skeleton: self.skeleton.clone(),
            pose: self.pose.clone(),
            clips: self.clips.clone(),
        }
    }

    pub(super) fn restore(&mut self, snap: Snapshot) -> Snapshot {
        Snapshot {
            objects: std::mem::replace(&mut self.objects, snap.objects),
            palette: std::mem::replace(&mut self.palette, snap.palette),
            materials: std::mem::replace(&mut self.materials, snap.materials),
            graph: std::mem::replace(&mut self.graph, snap.graph),
            skeleton: std::mem::replace(&mut self.skeleton, snap.skeleton),
            pose: std::mem::replace(&mut self.pose, snap.pose),
            clips: std::mem::replace(&mut self.clips, snap.clips),
        }
    }

    pub(super) fn checkpoint(&mut self) {
        if self.pending.is_none() {
            self.pending = Some(self.snapshot());
        }
    }

    pub(super) fn commit(&mut self) {
        if self.gesture {
            return;
        }
        if let Some(prev) = self.pending.take() {
            if prev.objects != self.objects
                || prev.palette != self.palette
                || prev.materials != self.materials
                || prev.graph != self.graph
                || prev.skeleton != self.skeleton
                || prev.pose != self.pose
                || prev.clips != self.clips
            {
                self.undo.push(prev);
                self.redo.clear();
                if self.undo.len() > 40 {
                    self.undo.remove(0);
                }
            }
        }
    }

    pub fn undo(&mut self) {
        self.commit();
        let Some(prev) = self.undo.pop() else {
            return;
        };
        let cur = self.restore(prev);
        self.clamp_selection();
        if cur.objects != self.objects {
            self.clear_picked();
        }
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.redo.push(cur);
    }

    pub fn redo(&mut self) {
        self.commit();
        let Some(next) = self.redo.pop() else {
            return;
        };
        let cur = self.restore(next);
        self.clamp_selection();
        if cur.objects != self.objects {
            self.clear_picked();
        }
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.undo.push(cur);
    }

    pub(super) fn clamp_selection(&mut self) {
        if self.objects.is_empty() {
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
        }
        self.selected = self.selected.min(self.objects.len() - 1);
        if self
            .selected_bone
            .is_some_and(|index| index >= self.skeleton.bones.len())
        {
            self.selected_bone = None;
        }
        self.object_selection
            .retain(|index| *index < self.objects.len());
        if !self.object_selection.contains(&self.selected) {
            self.object_selection.push(self.selected);
        }
    }
}

fn parent_order(skeleton: &Skeleton) -> Vec<usize> {
    let mut order = Vec::new();
    let mut seen = vec![false; skeleton.bones.len()];
    for start in 0..skeleton.bones.len() {
        let mut chain = Vec::new();
        let mut cursor = Some(start);
        while let Some(current) = cursor {
            if seen[current] {
                break;
            }
            chain.push(current);
            cursor = skeleton.parent_index(current);
        }
        while let Some(current) = chain.pop() {
            if !seen[current] {
                seen[current] = true;
                order.push(current);
            }
        }
    }
    order
}

/// Largest preview scale on one axis.
const MAX_POSE_SCALE: f32 = 16.0;

fn normalize_extra(rotation: [f32; 4]) -> Result<Option<[f32; 4]>, String> {
    let rotation = crate::rig::checked_rotation(rotation)?;
    let quat =
        glam::Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]).normalize();
    if quat.x.abs() < 1e-6
        && quat.y.abs() < 1e-6
        && quat.z.abs() < 1e-6
        && (quat.w.abs() - 1.0).abs() < 1e-5
    {
        return Ok(None);
    }
    Ok(Some([quat.x, quat.y, quat.z, quat.w]))
}

fn clip_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("clip name is empty".into());
    }
    if name.len() > 64 || name.contains('\0') {
        return Err("clip name is invalid".into());
    }
    Ok(name.to_string())
}

fn check_object_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    if name.len() > 64 {
        return Err("name is too long".into());
    }
    Ok(name.to_string())
}
