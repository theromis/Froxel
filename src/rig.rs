//! Rigid skeleton. Each object binds to one bone or to none.
//!
//! A pivot is a world voxel position, so it can sit on a corner, a centre, or
//! anywhere between cells. Orientation is a quaternion in voxel space, local
//! to the parent, `(x, y, z, w)`. glTF is Y-up `(x, z, -y)`; joint matrices
//! conjugate by that basis.
//!
//! The `.vox` chunk is `MVRG`, version 1. Named clips live in `MVAN`.
//! MagicaVoxel skips unknown chunks. An unknown version is an error.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};

pub const IDENTITY_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<String>,
    pub pivot: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

/// One object in the order it will be written into the file.
pub struct ObjectRef<'a> {
    pub trn: i32,
    pub name: &'a str,
    pub bone: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredBind {
    pub trn: i32,
    pub index: u32,
    pub name: String,
    pub bone: String,
}

/// One sample of a bone's local transform, in voxel space.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipKey {
    pub time: f32,
    /// Local rotation. `None` keeps the bone's bind rotation.
    pub rotation: Option<[f32; 4]>,
    /// Full local offset from the parent. `None` keeps the bone's bind offset.
    pub translation: Option<[f32; 3]>,
    pub scale: [f32; 3],
}

/// Keys for one bone, sorted by time.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneTrack {
    pub bone: String,
    pub keys: Vec<ClipKey>,
}

/// A named clip. glTF writes it as one animation on the joint nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    pub tracks: Vec<BoneTrack>,
}

/// Preview transform of one bone on top of the bind pose: `rotation` is a
/// local quaternion multiplied on the right of the bind rotation, `scale` a
/// local scale about the joint that the bone's children inherit, as in glTF.
/// A zero axis flattens the bone's parts, which hides them in a render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BonePose {
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl BonePose {
    pub const IDENTITY: BonePose = BonePose {
        rotation: IDENTITY_ROTATION,
        scale: [1.0, 1.0, 1.0],
    };

    pub fn rotation(rotation: [f32; 4]) -> BonePose {
        BonePose {
            rotation,
            ..BonePose::IDENTITY
        }
    }
}

/// Bone name to preview transform. A bone not in the map keeps its bind pose.
pub type Pose = HashMap<String, BonePose>;

/// Local joint transform in glTF space, and the inverse bind matrix.
pub struct BindPose {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub inverse_bind: [f32; 16],
}

impl Skeleton {
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|bone| bone.name == name)
    }

    pub fn add(
        &mut self,
        name: &str,
        parent: Option<&str>,
        pivot: [f32; 3],
        rotation: Option<[f32; 4]>,
    ) -> Result<(), String> {
        let name = check_name(name)?;
        if self.index_of(&name).is_some() {
            return Err(format!("bone `{name}` already exists"));
        }
        if let Some(parent) = parent {
            if self.index_of(parent).is_none() {
                return Err(format!("unknown parent `{parent}`"));
            }
            if parent == name {
                return Err(format!("bone `{name}` cannot parent itself"));
            }
        }
        self.bones.push(Bone {
            name,
            parent: parent.map(str::to_string),
            pivot: check_pivot(pivot)?,
            rotation: check_rotation(rotation.unwrap_or(IDENTITY_ROTATION))?,
        });
        Ok(())
    }

    /// `parent`: `None` keeps the current parent, `Some(None)` makes a root,
    /// `Some(Some(name))` reparents. Returns the new name when `rename` is set.
    pub fn set(
        &mut self,
        name: &str,
        parent: Option<Option<String>>,
        pivot: Option<[f32; 3]>,
        rotation: Option<[f32; 4]>,
        rename: Option<&str>,
    ) -> Result<Option<String>, String> {
        let index = self
            .index_of(name)
            .ok_or_else(|| format!("unknown bone `{name}`"))?;
        if let Some(parent) = &parent {
            match parent {
                None => {}
                Some(parent) if parent == name => {
                    return Err(format!("bone `{name}` cannot parent itself"));
                }
                Some(parent) => {
                    if self.index_of(parent).is_none() {
                        return Err(format!("unknown parent `{parent}`"));
                    }
                    if self.would_cycle(name, parent) {
                        return Err(format!("parent `{parent}` would cycle under `{name}`"));
                    }
                }
            }
        }
        let pivot = match pivot {
            Some(pivot) => Some(check_pivot(pivot)?),
            None => None,
        };
        let rotation = match rotation {
            Some(rotation) => Some(check_rotation(rotation)?),
            None => None,
        };
        let renamed = match rename {
            Some(next) => {
                let next = check_name(next)?;
                if next != name && self.index_of(&next).is_some() {
                    return Err(format!("bone `{next}` already exists"));
                }
                Some(next)
            }
            None => None,
        };
        if let Some(parent) = parent {
            self.bones[index].parent = parent;
        }
        if let Some(pivot) = pivot {
            self.bones[index].pivot = pivot;
        }
        if let Some(rotation) = rotation {
            self.bones[index].rotation = rotation;
        }
        if let Some(next) = renamed {
            let previous = self.bones[index].name.clone();
            self.bones[index].name = next.clone();
            for bone in &mut self.bones {
                if bone.parent.as_deref() == Some(previous.as_str()) {
                    bone.parent = Some(next.clone());
                }
            }
            return Ok(Some(next));
        }
        Ok(None)
    }

    /// Drop a bone. Its children keep its parent, so the graph stays connected.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let index = self
            .index_of(name)
            .ok_or_else(|| format!("unknown bone `{name}`"))?;
        let parent = self.bones[index].parent.clone();
        self.bones.remove(index);
        for bone in &mut self.bones {
            if bone.parent.as_deref() == Some(name) {
                bone.parent = parent.clone();
            }
        }
        Ok(())
    }

    pub fn parent_index(&self, index: usize) -> Option<usize> {
        let name = self.bones.get(index)?.parent.as_deref()?;
        self.index_of(name)
    }

    pub fn child_indices(&self, index: usize) -> Vec<usize> {
        let Some(name) = self.bones.get(index).map(|bone| bone.name.as_str()) else {
            return Vec::new();
        };
        self.bones
            .iter()
            .enumerate()
            .filter(|(_, bone)| bone.parent.as_deref() == Some(name))
            .map(|(child, _)| child)
            .collect()
    }

    pub fn root_indices(&self) -> Vec<usize> {
        self.bones
            .iter()
            .enumerate()
            .filter(|(_, bone)| bone.parent.is_none())
            .map(|(index, _)| index)
            .collect()
    }

    /// Joint index into `skin.joints` for an object binding.
    /// `None` uses the first root. A missing name is an error.
    pub fn joint_or_root(&self, bone: Option<&str>) -> Result<u16, String> {
        if self.bones.is_empty() {
            return Err("document has no bones".into());
        }
        let index = match bone {
            Some(name) => self
                .index_of(name)
                .ok_or_else(|| format!("unknown bone `{name}`"))?,
            None => self
                .root_indices()
                .into_iter()
                .next()
                .ok_or_else(|| "skeleton has no root bone".to_string())?,
        };
        u16::try_from(index).map_err(|_| format!("too many bones ({})", self.bones.len()))
    }

    pub fn accepts_parent(&self, name: &str, parent: &str) -> bool {
        parent != name && self.index_of(parent).is_some() && !self.would_cycle(name, parent)
    }

    fn would_cycle(&self, name: &str, parent: &str) -> bool {
        let mut cursor = Some(parent);
        let mut guard = 0;
        while let Some(current) = cursor {
            if current == name {
                return true;
            }
            cursor = self
                .index_of(current)
                .and_then(|index| self.bones[index].parent.as_deref());
            guard += 1;
            if guard > self.bones.len() {
                return true;
            }
        }
        false
    }
}

fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("bone name is empty".into());
    }
    if name.len() > 1024 || name.contains('\0') {
        return Err(format!("bone `{name}` has an invalid name"));
    }
    Ok(name.to_string())
}

/// Moves every pivot by `by` voxels, as when a file's scene is recentred on load.
pub fn shift_pivots(skeleton: &mut Skeleton, by: [i32; 3]) {
    for bone in &mut skeleton.bones {
        for (pivot, by) in bone.pivot.iter_mut().zip(by) {
            *pivot += by as f32;
        }
    }
}

fn check_pivot(pivot: [f32; 3]) -> Result<[f32; 3], String> {
    if pivot.iter().any(|axis| !axis.is_finite()) {
        return Err("pivot is not finite".into());
    }
    Ok(pivot)
}

fn check_rotation(rotation: [f32; 4]) -> Result<[f32; 4], String> {
    if rotation.iter().any(|axis| !axis.is_finite()) {
        return Err("rotation is not finite".into());
    }
    let length = rotation.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    if length < 1e-8 {
        return Err("rotation is zero".into());
    }
    Ok(rotation)
}

/// `None` when there is nothing MagicaVoxel needs to skip.
pub fn encode_chunk(
    objects: &[ObjectRef<'_>],
    skeleton: &Skeleton,
) -> Result<Option<Vec<u8>>, String> {
    let mut binds = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        let Some(bone) = object.bone else {
            continue;
        };
        if skeleton.index_of(bone).is_none() {
            return Err(format!(
                "object `{}` is bound to unknown bone `{bone}`",
                object.name
            ));
        }
        binds.push((index as u32, object.trn, object.name, bone));
    }
    if skeleton.bones.is_empty() && binds.is_empty() {
        return Ok(None);
    }
    skeleton.validate()?;
    let mut body = Vec::new();
    push_u32(&mut body, 1);
    push_u32(&mut body, skeleton.bones.len() as u32);
    for bone in &skeleton.bones {
        push_str(&mut body, &bone.name);
        push_str(&mut body, bone.parent.as_deref().unwrap_or(""));
        for axis in bone.pivot {
            body.extend_from_slice(&axis.to_le_bytes());
        }
        for axis in bone.rotation {
            body.extend_from_slice(&axis.to_le_bytes());
        }
    }
    push_u32(&mut body, binds.len() as u32);
    for (index, trn, name, bone) in binds {
        body.extend_from_slice(&trn.to_le_bytes());
        push_u32(&mut body, index);
        push_str(&mut body, name);
        push_str(&mut body, bone);
    }
    Ok(Some(body))
}

pub fn decode_chunk(body: &[u8]) -> Result<(Skeleton, Vec<StoredBind>), String> {
    let mut cursor = 0;
    let version = read_u32(body, &mut cursor)?;
    if version != 1 {
        return Err(format!("MVRG version {version} is not supported"));
    }
    let bone_count = read_u32(body, &mut cursor)? as usize;
    let mut skeleton = Skeleton::default();
    for _ in 0..bone_count {
        let name = read_str(body, &mut cursor)?;
        let parent = read_str(body, &mut cursor)?;
        let mut pivot = [0.0; 3];
        for axis in &mut pivot {
            *axis = read_f32(body, &mut cursor)?;
        }
        let mut rotation = [0.0; 4];
        for axis in &mut rotation {
            *axis = read_f32(body, &mut cursor)?;
        }
        skeleton.bones.push(Bone {
            name,
            parent: if parent.is_empty() {
                None
            } else {
                Some(parent)
            },
            pivot,
            rotation,
        });
    }
    skeleton.validate()?;
    let bind_count = read_u32(body, &mut cursor)? as usize;
    let mut binds = Vec::with_capacity(bind_count);
    for _ in 0..bind_count {
        let trn = read_i32(body, &mut cursor)?;
        let index = read_u32(body, &mut cursor)?;
        let name = read_str(body, &mut cursor)?;
        let bone = read_str(body, &mut cursor)?;
        if bone.is_empty() {
            return Err(format!("MVRG binding for object `{name}` has no bone"));
        }
        if skeleton.index_of(&bone).is_none() {
            return Err(format!("MVRG binds unknown bone `{bone}`"));
        }
        binds.push(StoredBind {
            trn,
            index,
            name,
            bone,
        });
    }
    if cursor != body.len() {
        return Err("MVRG chunk has trailing bytes".into());
    }
    Ok((skeleton, binds))
}

/// Bone name per object, matched by transform id, then unique name, then index.
pub fn assign_bones(
    objects: &[(i32, &str)],
    binds: &[StoredBind],
    _skeleton: &Skeleton,
) -> Result<Vec<Option<String>>, String> {
    let mut assigned = vec![None; objects.len()];
    for bind in binds {
        let target = if bind.trn >= 0 {
            let hits: Vec<usize> = objects
                .iter()
                .enumerate()
                .filter(|(_, (trn, _))| *trn == bind.trn)
                .map(|(index, _)| index)
                .collect();
            if hits.len() == 1 {
                hits[0]
            } else {
                match_name_or_index(objects, bind)?
            }
        } else {
            match_name_or_index(objects, bind)?
        };
        if assigned[target].is_some() {
            return Err(format!("MVRG binds object `{}` twice", objects[target].1));
        }
        assigned[target] = Some(bind.bone.clone());
    }
    Ok(assigned)
}

fn match_name_or_index(objects: &[(i32, &str)], bind: &StoredBind) -> Result<usize, String> {
    let hits: Vec<usize> = objects
        .iter()
        .enumerate()
        .filter(|(_, (_, name))| *name == bind.name)
        .map(|(index, _)| index)
        .collect();
    if hits.len() == 1 {
        return Ok(hits[0]);
    }
    let index = bind.index as usize;
    if index < objects.len() && objects[index].1 == bind.name {
        return Ok(index);
    }
    Err(format!(
        "MVRG binding for object `{}` does not match an object",
        bind.name
    ))
}

impl Skeleton {
    fn validate(&self) -> Result<(), String> {
        for (index, bone) in self.bones.iter().enumerate() {
            check_name(&bone.name)?;
            check_pivot(bone.pivot)?;
            check_rotation(bone.rotation)?;
            if self
                .bones
                .iter()
                .enumerate()
                .any(|(other, candidate)| other != index && candidate.name == bone.name)
            {
                return Err(format!("MVRG bone `{}` is duplicated", bone.name));
            }
            if let Some(parent) = &bone.parent {
                if self.index_of(parent).is_none() {
                    return Err(format!(
                        "MVRG parent `{parent}` of bone `{}` does not exist",
                        bone.name
                    ));
                }
                if self.would_cycle(&bone.name, parent) {
                    return Err(format!(
                        "MVRG parent `{parent}` of bone `{}` would cycle",
                        bone.name
                    ));
                }
            }
        }
        Ok(())
    }
}

pub fn bind_pose(skeleton: &Skeleton) -> Result<Vec<BindPose>, String> {
    let worlds = world_matrices(skeleton)?;
    let mut poses = Vec::with_capacity(skeleton.bones.len());
    for (index, world) in worlds.iter().enumerate() {
        let parent = skeleton.parent_index(index);
        let local = match parent {
            Some(parent) => worlds[parent].inverse() * *world,
            None => *world,
        };
        let (_scale, rotation, translation) = local.to_scale_rotation_translation();
        let inverse = world.inverse();
        poses.push(BindPose {
            translation: [translation.x, translation.y, translation.z],
            rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
            inverse_bind: inverse.to_cols_array(),
        });
    }
    Ok(poses)
}

fn world_matrices(skeleton: &Skeleton) -> Result<Vec<Mat4>, String> {
    let mut cache = vec![None; skeleton.bones.len()];
    let mut stack = Vec::new();
    for index in 0..skeleton.bones.len() {
        world_at(skeleton, index, &mut cache, &mut stack)?;
    }
    Ok(cache
        .into_iter()
        .map(|ready| ready.expect("joint world"))
        .collect())
}

fn world_at(
    skeleton: &Skeleton,
    index: usize,
    cache: &mut [Option<Mat4>],
    stack: &mut Vec<usize>,
) -> Result<Mat4, String> {
    if let Some(ready) = cache[index] {
        return Ok(ready);
    }
    if stack.contains(&index) {
        return Err(format!(
            "bone `{}` is in a cycle",
            skeleton.bones[index].name
        ));
    }
    stack.push(index);
    let bone = &skeleton.bones[index];
    let local = Quat::from_xyzw(
        bone.rotation[0],
        bone.rotation[1],
        bone.rotation[2],
        bone.rotation[3],
    );
    let local = voxel_to_gltf_quat(local.normalize());
    let pivot = voxel_to_gltf_point(Vec3::from_array(bone.pivot));
    let parent_rotation = if let Some(parent) = skeleton.parent_index(index) {
        world_at(skeleton, parent, cache, stack)?
            .to_scale_rotation_translation()
            .1
    } else {
        Quat::IDENTITY
    };
    let world = Mat4::from_rotation_translation(parent_rotation * local, pivot);
    cache[index] = Some(world);
    stack.pop();
    Ok(world)
}

/// Voxel `(x, y, z)` to glTF `(x, z, -y)`.
fn voxel_to_gltf_point(point: Vec3) -> Vec3 {
    Vec3::new(point.x, point.z, -point.y)
}

fn voxel_to_gltf_quat(rotation: Quat) -> Quat {
    let basis = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    basis * rotation * basis.inverse()
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_str(out: &mut Vec<u8>, text: &str) {
    push_u32(out, text.len() as u32);
    out.extend_from_slice(text.as_bytes());
}

fn read_u32(body: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let bytes = read_n(body, cursor, 4)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_i32(body: &[u8], cursor: &mut usize) -> Result<i32, String> {
    Ok(read_u32(body, cursor)? as i32)
}

fn read_f32(body: &[u8], cursor: &mut usize) -> Result<f32, String> {
    let bytes = read_n(body, cursor, 4)?;
    Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_str(body: &[u8], cursor: &mut usize) -> Result<String, String> {
    let len = read_u32(body, cursor)? as usize;
    if len > 1024 {
        return Err("MVRG string is too long".into());
    }
    let bytes = read_n(body, cursor, len)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| "MVRG string is not utf-8".into())
}

fn read_n<'a>(body: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8], String> {
    if *cursor + len > body.len() {
        return Err("MVRG chunk is truncated".into());
    }
    let slice = &body[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice)
}

/// Joint positions and rotations in voxel space under `pose`. Identity leaves
/// the bind pose; child pivots orbit with the parent and follow its scale.
pub fn posed_joints(skeleton: &Skeleton, pose: &Pose) -> Result<Vec<(Vec3, Quat)>, String> {
    let (worlds, rotations) = posed_worlds(skeleton, pose)?;
    Ok(worlds
        .iter()
        .zip(rotations)
        .map(|(world, rotation)| (world.w_axis.truncate(), rotation))
        .collect())
}

/// Joint world matrices (translation, rotation, scale) and the rotations alone,
/// which stay defined when a scale is zero.
fn posed_worlds(skeleton: &Skeleton, pose: &Pose) -> Result<(Vec<Mat4>, Vec<Quat>), String> {
    let order = parent_first(skeleton)?;
    let mut bind_rot = vec![Quat::IDENTITY; skeleton.bones.len()];
    let mut bind_pos = vec![Vec3::ZERO; skeleton.bones.len()];
    for index in &order {
        let index = *index;
        let bone = &skeleton.bones[index];
        let local = quat(bone.rotation)?;
        if let Some(parent) = skeleton.parent_index(index) {
            bind_rot[index] = bind_rot[parent] * local;
        } else {
            bind_rot[index] = local;
        }
        bind_pos[index] = Vec3::from_array(bone.pivot);
    }
    let mut worlds = vec![Mat4::IDENTITY; skeleton.bones.len()];
    let mut rotations = vec![Quat::IDENTITY; skeleton.bones.len()];
    for index in &order {
        let index = *index;
        let bone = &skeleton.bones[index];
        let extra = pose.get(&bone.name).copied().unwrap_or(BonePose::IDENTITY);
        let local = (quat(bone.rotation)? * quat(extra.rotation)?).normalize();
        let scale = Vec3::from_array(extra.scale);
        if let Some(parent) = skeleton.parent_index(index) {
            let local_t = bind_rot[parent].inverse() * (bind_pos[index] - bind_pos[parent]);
            worlds[index] =
                worlds[parent] * Mat4::from_scale_rotation_translation(scale, local, local_t);
            rotations[index] = rotations[parent] * local;
        } else {
            worlds[index] = Mat4::from_scale_rotation_translation(scale, local, bind_pos[index]);
            rotations[index] = local;
        }
    }
    Ok((worlds, rotations))
}

/// Map from a bind-pose point to the posed point, one matrix per bone. It is
/// rigid unless the pose scales a bone or one of its parents. Empty `pose` is
/// the identity on every bone.
pub fn skin_deltas(skeleton: &Skeleton, pose: &Pose) -> Result<Vec<Mat4>, String> {
    let (bind, _) = posed_worlds(skeleton, &Pose::new())?;
    let (posed, _) = posed_worlds(skeleton, pose)?;
    Ok(bind
        .into_iter()
        .zip(posed)
        .map(|(bind, posed)| posed * bind.inverse())
        .collect())
}

pub fn checked_rotation(rotation: [f32; 4]) -> Result<[f32; 4], String> {
    check_rotation(rotation)
}

/// Reflection of a local quaternion across a world axis through the origin.
pub fn mirror_rotation(rotation: [f32; 4], axis: usize) -> [f32; 4] {
    let mut out = rotation;
    for other in 0..3 {
        if other != axis {
            out[other] = -out[other];
        }
    }
    out
}

pub fn mirrored_bone_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("_l") {
        format!("{stem}_r")
    } else if let Some(stem) = name.strip_suffix("_r") {
        format!("{stem}_l")
    } else {
        format!("{name}_r")
    }
}

/// Voxel `(x, y, z)` quaternion to the glTF joint basis.
pub fn gltf_rotation(rotation: [f32; 4]) -> [f32; 4] {
    let quat = voxel_to_gltf_quat(Quat::from_xyzw(
        rotation[0],
        rotation[1],
        rotation[2],
        rotation[3],
    ));
    [quat.x, quat.y, quat.z, quat.w]
}

/// Voxel vector to glTF `(x, z, -y)`.
pub fn gltf_vector(vector: [f32; 3]) -> [f32; 3] {
    [vector[0], vector[2], -vector[1]]
}

/// Scale axes follow the same swap, without the sign flip.
pub fn gltf_scale(scale: [f32; 3]) -> [f32; 3] {
    [scale[0], scale[2], scale[1]]
}

pub fn encode_clips(clips: &[Clip]) -> Result<Option<Vec<u8>>, String> {
    if clips.is_empty() {
        return Ok(None);
    }
    let mut body = Vec::new();
    // Version 3 flags each key's rotation and translation; version 2 flagged
    // only the translation, and version 1 always stored both.
    push_u32(&mut body, 3);
    push_u32(&mut body, clips.len() as u32);
    for clip in clips {
        let name = check_name(&clip.name)?;
        push_str(&mut body, &name);
        push_u32(&mut body, clip.tracks.len() as u32);
        for track in &clip.tracks {
            push_str(&mut body, &check_name(&track.bone)?);
            push_u32(&mut body, track.keys.len() as u32);
            for key in &track.keys {
                body.extend_from_slice(&key.time.to_le_bytes());
                body.push(
                    u8::from(key.translation.is_some()) | u8::from(key.rotation.is_some()) << 1,
                );
                for axis in key.rotation.unwrap_or(IDENTITY_ROTATION) {
                    body.extend_from_slice(&axis.to_le_bytes());
                }
                for axis in key.translation.unwrap_or_default() {
                    body.extend_from_slice(&axis.to_le_bytes());
                }
                for axis in key.scale {
                    body.extend_from_slice(&axis.to_le_bytes());
                }
            }
        }
    }
    Ok(Some(body))
}

pub fn decode_clips(body: &[u8]) -> Result<Vec<Clip>, String> {
    let mut cursor = 0;
    let version = read_u32(body, &mut cursor)?;
    if !(1..=3).contains(&version) {
        return Err(format!("MVAN version {version} is not supported"));
    }
    let count = read_u32(body, &mut cursor)? as usize;
    if count > 256 {
        return Err("MVAN has too many clips".into());
    }
    let mut clips = Vec::with_capacity(count);
    for _ in 0..count {
        let name = read_str(body, &mut cursor)?;
        let tracks_len = read_u32(body, &mut cursor)? as usize;
        if tracks_len > 256 {
            return Err(format!("clip `{name}` has too many bones"));
        }
        let mut tracks = Vec::with_capacity(tracks_len);
        for _ in 0..tracks_len {
            let bone = read_str(body, &mut cursor)?;
            let keys_len = read_u32(body, &mut cursor)? as usize;
            if keys_len > 10_000 {
                return Err(format!("clip `{name}` bone `{bone}` has too many keys"));
            }
            let mut keys = Vec::with_capacity(keys_len);
            for _ in 0..keys_len {
                let time = read_f32(body, &mut cursor)?;
                let mut flags = 0b11;
                if version == 3 {
                    flags = *body.get(cursor).ok_or("MVAN chunk is truncated")?;
                    cursor += 1;
                }
                let mut rotation = [0.0; 4];
                for axis in &mut rotation {
                    *axis = read_f32(body, &mut cursor)?;
                }
                if version == 2 {
                    flags = *body.get(cursor).ok_or("MVAN chunk is truncated")? | 0b10;
                    cursor += 1;
                }
                let rotation = (flags & 0b10 != 0).then_some(rotation);
                let has_translation = flags & 0b01 != 0;
                let mut offset = [0.0; 3];
                for axis in &mut offset {
                    *axis = read_f32(body, &mut cursor)?;
                }
                let translation = has_translation.then_some(offset);
                let mut scale = [0.0; 3];
                for axis in &mut scale {
                    *axis = read_f32(body, &mut cursor)?;
                }
                keys.push(ClipKey {
                    time,
                    rotation,
                    translation,
                    scale,
                });
            }
            tracks.push(BoneTrack { bone, keys });
        }
        clips.push(Clip { name, tracks });
    }
    if cursor != body.len() {
        return Err("MVAN chunk has trailing bytes".into());
    }
    Ok(clips)
}

fn parent_first(skeleton: &Skeleton) -> Result<Vec<usize>, String> {
    let mut order = Vec::with_capacity(skeleton.bones.len());
    let mut seen = vec![false; skeleton.bones.len()];
    let mut stack = vec![false; skeleton.bones.len()];
    fn visit(
        skeleton: &Skeleton,
        index: usize,
        order: &mut Vec<usize>,
        seen: &mut [bool],
        stack: &mut [bool],
    ) -> Result<(), String> {
        if seen[index] {
            return Ok(());
        }
        if stack[index] {
            return Err(format!(
                "bone `{}` is part of a cycle",
                skeleton.bones[index].name
            ));
        }
        stack[index] = true;
        if let Some(parent) = skeleton.parent_index(index) {
            visit(skeleton, parent, order, seen, stack)?;
        }
        stack[index] = false;
        seen[index] = true;
        order.push(index);
        Ok(())
    }
    for index in 0..skeleton.bones.len() {
        visit(skeleton, index, &mut order, &mut seen, &mut stack)?;
    }
    Ok(order)
}

fn quat(rotation: [f32; 4]) -> Result<Quat, String> {
    let rotation = check_rotation(rotation)?;
    Ok(Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]).normalize())
}

#[cfg(test)]
#[path = "../tests/rig.rs"]
mod tests;
