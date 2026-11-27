//! Strands on the session: add, replace, remove, bake, and the world boxes
//! every drawing, tracing, and export path reads (`tools/strand.rs`).

use std::collections::HashMap;

use super::*;
use crate::tools::{Strand, StrandBox};

/// One strand box carried into the world: the map from the unit cube
/// `-1..1` to it, its inverse, and its palette slot.
#[derive(Clone, Copy, Debug)]
pub struct WorldBox {
    pub matrix: glam::Mat4,
    pub inverse: glam::Mat4,
    pub slot: u8,
}

impl WorldBox {
    pub fn new(matrix: glam::Mat4, slot: u8) -> WorldBox {
        WorldBox {
            matrix,
            inverse: matrix.inverse(),
            slot,
        }
    }

    /// The eight corners.
    pub fn corners(&self) -> [glam::Vec3; 8] {
        let mut out = [glam::Vec3::ZERO; 8];
        for (index, corner) in out.iter_mut().enumerate() {
            let unit = glam::Vec3::new(
                if index & 1 == 1 { 1.0 } else { -1.0 },
                if index & 2 == 2 { 1.0 } else { -1.0 },
                if index & 4 == 4 { 1.0 } else { -1.0 },
            );
            *corner = self.matrix.transform_point3(unit);
        }
        out
    }

    /// The axis-aligned bounds of the corners.
    pub fn aabb(&self) -> (glam::Vec3, glam::Vec3) {
        let corners = self.corners();
        let mut min = corners[0];
        let mut max = corners[0];
        for c in &corners[1..] {
            min = min.min(*c);
            max = max.max(*c);
        }
        (min, max)
    }

    /// The six faces as `(corners in winding order seen from outside, outward normal)`.
    pub fn faces(&self) -> [([glam::Vec3; 4], glam::Vec3); 6] {
        let c = self.corners();
        let n = |local: glam::Vec3| {
            self.inverse
                .transpose()
                .transform_vector3(local)
                .normalize_or_zero()
        };
        // Corner index bits: 1 = +x, 2 = +y, 4 = +z of the unit cube.
        let quad = |a: usize, b: usize, cc: usize, d: usize| [c[a], c[b], c[cc], c[d]];
        let mut faces = [
            (quad(1, 3, 7, 5), n(glam::Vec3::X)),
            (quad(0, 4, 6, 2), n(-glam::Vec3::X)),
            (quad(2, 6, 7, 3), n(glam::Vec3::Y)),
            (quad(0, 1, 5, 4), n(-glam::Vec3::Y)),
            (quad(4, 5, 7, 6), n(glam::Vec3::Z)),
            (quad(0, 2, 3, 1), n(-glam::Vec3::Z)),
        ];
        // A mirroring map turns the winding inside out; put it back.
        if self.matrix.determinant() < 0.0 {
            for face in &mut faces {
                face.0.reverse();
            }
        }
        faces
    }
}

/// One object's bricks and what they were built from: the cells (shared,
/// so an unchanged object compares by pointer), the translation, and the
/// skin.
struct SkinEntry {
    cells: Cells,
    translation: Cell,
    skin: crate::tools::Skin,
    boxes: std::sync::Arc<Vec<StrandBox>>,
}

/// Bricks by object index. Rebuilt only for an object whose inputs differ.
#[derive(Default)]
pub struct SkinCache(std::sync::Mutex<HashMap<usize, SkinEntry>>);

impl Volume {
    /// Whether the object has boxes besides its cells: strands or a skin.
    pub fn has_free_boxes(&self) -> bool {
        !self.strands.is_empty() || self.skin.is_some()
    }
}

impl Session {
    /// One object's skin bricks in its local geometric frame, from the cache
    /// while its cells, translation, and skin are unchanged.
    pub fn skin_boxes(&self, index: usize) -> std::sync::Arc<Vec<StrandBox>> {
        let Some(object) = self.objects.get(index) else {
            return Default::default();
        };
        let Some(skin) = &object.skin else {
            return Default::default();
        };
        let mut cache = self
            .skin_cache
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(entry) = cache.get(&index) {
            if entry.translation == object.translation
                && entry.skin == *skin
                && entry.cells == object.cells
            {
                return entry.boxes.clone();
            }
        }
        let boxes = std::sync::Arc::new(crate::tools::skin_boxes(
            &object.cells,
            skin,
            object.translation,
        ));
        cache.insert(
            index,
            SkinEntry {
                cells: object.cells.clone(),
                translation: object.translation,
                skin: skin.clone(),
                boxes: boxes.clone(),
            },
        );
        boxes
    }

    /// The bevel the viewport draws: the Export section's, while the
    /// viewport's Bevel box is on in the edit view.
    pub fn view_bevel(&self) -> Option<crate::bevel::Bevel> {
        if self.bevel_preview && self.render_mode == RenderMode::Edit {
            self.export.bevel
        } else {
            None
        }
    }

    /// Local boxes of one object's strands and skin bricks, in its local
    /// geometric frame.
    pub fn strand_boxes(&self, index: usize) -> Vec<StrandBox> {
        let Some(object) = self.objects.get(index) else {
            return Vec::new();
        };
        let mut boxes = crate::tools::strand_object_boxes(&object.strands);
        if object.skin.is_some() {
            boxes.extend(self.skin_boxes(index).iter().copied());
        }
        boxes
    }

    /// Every box of one object's strands in the world, after its translation,
    /// bone, rest, and preview pose, as the viewport draws the object.
    pub fn strand_world_boxes(&self, index: usize) -> Vec<WorldBox> {
        let Some(object) = self.objects.get(index) else {
            return Vec::new();
        };
        if !object.has_free_boxes() {
            return Vec::new();
        }
        let map = self.object_to_world(index);
        self.strand_boxes(index)
            .iter()
            .map(|b| WorldBox::new(map * b.matrix(), b.slot))
            .collect()
    }

    /// The same boxes in the object's bind frame: its translation only, as
    /// mesh export places cells.
    pub fn strand_bind_boxes(&self, index: usize) -> Vec<WorldBox> {
        let Some(object) = self.objects.get(index) else {
            return Vec::new();
        };
        let t = object.translation;
        let map = glam::Mat4::from_translation(glam::Vec3::new(t.0 as f32, t.1 as f32, t.2 as f32));
        self.strand_boxes(index)
            .iter()
            .map(|b| WorldBox::new(map * b.matrix(), b.slot))
            .collect()
    }

    /// World boxes of every visible object's strands, or only `only`'s (when
    /// visible), as `posed_voxels` takes cells.
    pub fn visible_strand_boxes(&self, only: Option<usize>) -> Vec<WorldBox> {
        let mut out = Vec::new();
        for (index, object) in self.objects.iter().enumerate() {
            if !object.has_free_boxes() {
                continue;
            }
            let wanted = !object.hidden && only.is_none_or(|only| only == index);
            if wanted {
                out.extend(self.strand_world_boxes(index));
            }
        }
        out
    }

    /// Replaces one object's strand list as one undo step, after checking
    /// every strand and the object's box limit.
    pub fn set_strands(&mut self, index: usize, strands: Vec<Strand>) -> Result<usize, String> {
        let count = self.objects.len();
        if index >= count {
            return Err(format!(
                "object index {index} is out of range ({count} objects)"
            ));
        }
        for (n, strand) in strands.iter().enumerate() {
            strand.check().map_err(|e| format!("strand {n}: {e}"))?;
        }
        let boxes = crate::tools::check_strands(&strands)?;
        if self.objects[index].strands == strands {
            return Ok(boxes);
        }
        self.commit();
        self.checkpoint();
        self.objects[index].strands = strands;
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        Ok(boxes)
    }
}

impl Session {
    /// Writes `cells` into one object (over what is there) and removes the
    /// strands listed in `remove`, as one undo step. Returns how many cells
    /// changed.
    pub fn bake_strand_cells(
        &mut self,
        index: usize,
        cells: &HashMap<Cell, u8>,
        remove: &[usize],
    ) -> usize {
        if index >= self.objects.len() {
            return 0;
        }
        self.commit();
        self.checkpoint();
        let object = &mut self.objects[index];
        let mut changed = 0;
        for (cell, slot) in cells {
            if object.cells.insert(*cell, *slot) != Some(*slot) {
                changed += 1;
            }
        }
        let mut n = 0;
        object.strands.retain(|_| {
            let keep = !remove.contains(&n);
            n += 1;
            keep
        });
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.commit();
        changed
    }
}
