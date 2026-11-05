//! Named markers a game reads: points, collision boxes, and connectors.
//!
//! A marker belongs to an object and sits in that object's local
//! coordinates, so it moves with the object. Coordinates are mesh units: a
//! cell `(x, y, z)` spans `x..x + 1`, so the top centre of cell `(4, 4, 10)`
//! is `(4.5, 4.5, 11)`, the same space as the exported mesh before glTF's
//! Y-up. The `MVMK` chunk keeps them in a `.vox`, each relative to the
//! lowest occupied corner of its object, which survives the recentring a
//! load does. Mesh export writes one glTF node per marker.

use serde_json::{json, Value};

use super::{occupied_bounds, Session};

/// Most markers in one document.
pub const MAX_MARKERS: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub enum MarkerKind {
    Point {
        at: [f32; 3],
    },
    /// A collision box between two corners, `min` below `max` on each axis.
    Box {
        min: [f32; 3],
        max: [f32; 3],
    },
    /// A piece's joining end: where it is and which way it faces.
    Connector {
        at: [f32; 3],
        direction: [f32; 3],
    },
}

impl MarkerKind {
    pub fn name(&self) -> &'static str {
        match self {
            MarkerKind::Point { .. } => "point",
            MarkerKind::Box { .. } => "box",
            MarkerKind::Connector { .. } => "connector",
        }
    }

    fn floats(&self) -> Vec<f32> {
        match self {
            MarkerKind::Point { at } => at.to_vec(),
            MarkerKind::Box { min, max } => min.iter().chain(max).copied().collect(),
            MarkerKind::Connector { at, direction } => {
                at.iter().chain(direction).copied().collect()
            }
        }
    }

    /// Moved by `offset`; a direction stays.
    fn shifted(&self, offset: [f32; 3]) -> MarkerKind {
        let add = |p: &[f32; 3]| [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]];
        match self {
            MarkerKind::Point { at } => MarkerKind::Point { at: add(at) },
            MarkerKind::Box { min, max } => MarkerKind::Box {
                min: add(min),
                max: add(max),
            },
            MarkerKind::Connector { at, direction } => MarkerKind::Connector {
                at: add(at),
                direction: *direction,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub name: String,
    /// The object it belongs to, by name.
    pub object: String,
    pub kind: MarkerKind,
    /// Made by the object's script: the next run replaces it.
    pub scripted: bool,
}

/// A marker name: 1 to 64 characters, no control characters.
pub fn check_marker_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("a marker name is 1 to 64 characters with no control characters".into());
    }
    Ok(name.to_string())
}

/// A finite coordinate within the world.
pub fn check_point(p: [f32; 3], what: &str) -> Result<[f32; 3], String> {
    if p.iter().any(|v| !v.is_finite() || v.abs() > 4096.0) {
        return Err(format!("{what} must be finite and within 4096 cells"));
    }
    Ok(p)
}

/// A unit direction from any non-zero vector.
pub fn check_direction(d: [f32; 3]) -> Result<[f32; 3], String> {
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if !len.is_finite() || len < 1.0e-6 {
        return Err("direction must be a non-zero [x, y, z]".into());
    }
    Ok([d[0] / len, d[1] / len, d[2] / len])
}

/// A collision box over the inclusive cells `a..=b`, in mesh units.
pub fn cell_box(a: [i32; 3], b: [i32; 3]) -> MarkerKind {
    let mut min = [0.0; 3];
    let mut max = [0.0; 3];
    for axis in 0..3 {
        min[axis] = a[axis].min(b[axis]) as f32;
        max[axis] = (a[axis].max(b[axis]) + 1) as f32;
    }
    MarkerKind::Box { min, max }
}

/// Whether `marker` is a box MCP `collision_from_voxels` made on `object`
/// under `prefix`: `<prefix>_<number>`.
fn collision_owned(marker: &Marker, object: &str, prefix: &str) -> bool {
    marker.object == object
        && matches!(marker.kind, MarkerKind::Box { .. })
        && marker
            .name
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}

/// Replace the boxes `prefix` made on `object` with `boxes`. Any other marker
/// with one of the new names is an error, and `markers` stays as it was.
/// Returns how many went.
pub(crate) fn replace_collision_markers(
    markers: &mut Vec<Marker>,
    object: &str,
    prefix: &str,
    boxes: Vec<(String, MarkerKind)>,
    scripted: bool,
) -> Result<usize, String> {
    let kept: Vec<&Marker> = markers
        .iter()
        .filter(|m| !collision_owned(m, object, prefix))
        .collect();
    for (name, _) in &boxes {
        if let Some(other) = kept.iter().find(|m| m.name == *name) {
            return Err(format!(
                "a {} marker named `{name}` is already on `{}`: pass another prefix",
                other.kind.name(),
                other.object
            ));
        }
    }
    if kept.len() + boxes.len() > MAX_MARKERS {
        return Err(format!(
            "a document holds at most {MAX_MARKERS} markers: {} boxes do not fit beside {} others",
            boxes.len(),
            kept.len()
        ));
    }
    let replaced = markers.len() - kept.len();
    markers.retain(|m| !collision_owned(m, object, prefix));
    markers.extend(boxes.into_iter().map(|(name, kind)| Marker {
        name,
        object: object.to_string(),
        kind,
        scripted,
    }));
    Ok(replaced)
}

/// glTF Y-up `(x, z, -y)`.
fn y_up(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

impl Session {
    /// The world offset of a marker's object, zero when it is gone.
    fn marker_origin(&self, marker: &Marker) -> [f32; 3] {
        self.objects
            .iter()
            .find(|object| object.name == marker.object)
            .map(|object| {
                let t = object.translation;
                [t.0 as f32, t.1 as f32, t.2 as f32]
            })
            .unwrap_or([0.0; 3])
    }

    /// The marker in world mesh units: moved by its object's position.
    pub(crate) fn marker_world(&self, marker: &Marker) -> MarkerKind {
        marker.kind.shifted(self.marker_origin(marker))
    }

    /// Add a marker to object `index`, or replace the one of that name. One
    /// undo step.
    pub fn set_marker(&mut self, index: usize, name: &str, kind: MarkerKind) -> Result<(), String> {
        let name = check_marker_name(name)?;
        let object = self
            .objects
            .get(index)
            .ok_or_else(|| {
                format!(
                    "object index {index} is out of range ({} objects)",
                    self.objects.len()
                )
            })?
            .name
            .clone();
        let found = self.markers.iter().position(|m| m.name == name);
        if found.is_none() && self.markers.len() >= MAX_MARKERS {
            return Err(format!("a document holds at most {MAX_MARKERS} markers"));
        }
        let marker = Marker {
            name,
            object,
            kind,
            scripted: false,
        };
        self.commit();
        self.checkpoint();
        match found {
            Some(at) => self.markers[at] = marker,
            None => self.markers.push(marker),
        }
        self.commit();
        Ok(())
    }

    /// Remove the marker called `name`, or every marker when `None`. Returns
    /// how many went. One undo step.
    pub fn remove_markers(&mut self, name: Option<&str>) -> Result<usize, String> {
        let before = self.markers.len();
        let keep = |m: &Marker| name.is_some_and(|name| m.name != name);
        if let Some(name) = name {
            if !self.markers.iter().any(|m| m.name == name) {
                return Err(format!("no marker named `{name}`"));
            }
        }
        if before == 0 {
            return Ok(0);
        }
        self.commit();
        self.checkpoint();
        self.markers.retain(keep);
        self.commit();
        Ok(before - self.markers.len())
    }

    /// Replace the document's markers. One undo step.
    pub(crate) fn set_markers_undoable(&mut self, markers: Vec<Marker>) {
        self.commit();
        self.checkpoint();
        self.markers = markers;
        self.commit();
    }

    /// Every marker as JSON: `local` in its object's cells, `world` in
    /// document mesh units, and `gltf` as the exported glb has it.
    pub fn markers_json(&self) -> Value {
        Value::Array(self.markers.iter().map(|m| self.marker_json(m)).collect())
    }

    fn marker_json(&self, marker: &Marker) -> Value {
        let origin = self.marker_origin(marker);
        let world = marker.kind.shifted(origin);
        let round = |p: [f32; 3]| p.map(|v| (v * 1000.0).round() / 1000.0);
        let mut out = json!({
            "name": marker.name,
            "kind": marker.kind.name(),
            "object": marker.object,
        });
        if marker.scripted {
            out["scripted"] = json!(true);
        }
        match (&marker.kind, &world) {
            (MarkerKind::Point { at }, MarkerKind::Point { at: w }) => {
                out["local"] = json!({"point": round(*at)});
                out["world"] = json!({"point": round(*w)});
                out["gltf"] = json!({"translation": round(y_up(*w))});
            }
            (
                MarkerKind::Box { min, max },
                MarkerKind::Box {
                    min: wmin,
                    max: wmax,
                },
            ) => {
                out["local"] = json!({"min": round(*min), "max": round(*max)});
                out["world"] = json!({"min": round(*wmin), "max": round(*wmax)});
                let (a, b) = (y_up(*wmin), y_up(*wmax));
                let lo = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
                let hi = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
                out["gltf"] = json!({
                    "translation": round([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]),
                    "size": round([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]),
                    "min": round(lo),
                    "max": round(hi),
                });
            }
            (MarkerKind::Connector { at, direction }, MarkerKind::Connector { at: w, .. }) => {
                out["local"] = json!({"point": round(*at), "direction": round(*direction)});
                out["world"] = json!({"point": round(*w), "direction": round(*direction)});
                out["gltf"] = json!({
                    "translation": round(y_up(*w)),
                    "direction": round(y_up(*direction)),
                    "rotation": facing_rotation(y_up(*direction)).map(|v| (v * 1.0e6).round() / 1.0e6),
                });
            }
            _ => unreachable!("shifted keeps the kind"),
        }
        out
    }

    /// Replace the scripted markers of the objects a script run built.
    pub(crate) fn replace_script_markers(&mut self, objects: &[String], markers: Vec<Marker>) {
        self.markers
            .retain(|m| !(m.scripted && objects.contains(&m.object)));
        for marker in markers {
            match self.markers.iter().position(|m| m.name == marker.name) {
                Some(at) => self.markers[at] = marker,
                None if self.markers.len() < MAX_MARKERS => self.markers.push(marker),
                None => {}
            }
        }
    }

    /// Markers follow their object's new name.
    pub(super) fn rename_markers(&mut self, from: &str, to: &str) {
        for marker in &mut self.markers {
            if marker.object == from {
                marker.object = to.to_string();
            }
        }
    }

    /// The `MVMK` chunk body, or `None` without markers.
    pub(crate) fn encode_markers(&self) -> Option<Vec<u8>> {
        if self.markers.is_empty() {
            return None;
        }
        let mut out = Vec::new();
        push_u32(&mut out, 1);
        push_u32(&mut out, self.markers.len() as u32);
        for marker in &self.markers {
            let anchor = self.anchor(&marker.object);
            push_str(&mut out, &marker.name);
            push_str(&mut out, &marker.object);
            out.push(match marker.kind {
                MarkerKind::Point { .. } => 0,
                MarkerKind::Box { .. } => 1,
                MarkerKind::Connector { .. } => 2,
            });
            out.push(u8::from(marker.scripted));
            let neg = anchor.map(|v| -v);
            for v in marker.kind.shifted(neg).floats() {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        Some(out)
    }

    /// The lowest occupied local corner of an object, which a load keeps.
    fn anchor(&self, object: &str) -> [f32; 3] {
        self.objects
            .iter()
            .find(|o| o.name == object)
            .and_then(|o| occupied_bounds(&o.cells))
            .map(|(min, _)| [min.0 as f32, min.1 as f32, min.2 as f32])
            .unwrap_or([0.0; 3])
    }

    /// Markers from an `MVMK` body, placed on the loaded objects.
    pub(crate) fn decode_markers(&mut self, body: &[u8]) -> Result<(), String> {
        let mut cursor = 0;
        let version = read_u32(body, &mut cursor)?;
        if version != 1 {
            return Err(format!("MVMK version {version} is not supported"));
        }
        let count = read_u32(body, &mut cursor)? as usize;
        if count > MAX_MARKERS {
            return Err("MVMK holds too many markers".into());
        }
        let mut markers = Vec::with_capacity(count);
        for _ in 0..count {
            let name = read_str(body, &mut cursor)?;
            let object = read_str(body, &mut cursor)?;
            let kind = read_n(body, &mut cursor, 1)?[0];
            let scripted = read_n(body, &mut cursor, 1)?[0] != 0;
            let n = match kind {
                0 => 3,
                1 | 2 => 6,
                other => return Err(format!("MVMK marker kind {other} is unknown")),
            };
            let mut f = [0.0f32; 6];
            for v in f.iter_mut().take(n) {
                let bytes = read_n(body, &mut cursor, 4)?;
                *v = f32::from_le_bytes(bytes.try_into().expect("four bytes"));
            }
            let kind = match kind {
                0 => MarkerKind::Point {
                    at: [f[0], f[1], f[2]],
                },
                1 => MarkerKind::Box {
                    min: [f[0], f[1], f[2]],
                    max: [f[3], f[4], f[5]],
                },
                _ => MarkerKind::Connector {
                    at: [f[0], f[1], f[2]],
                    direction: [f[3], f[4], f[5]],
                },
            };
            let anchor = self.anchor(&object);
            markers.push(Marker {
                name,
                object,
                kind: kind.shifted(anchor),
                scripted,
            });
        }
        self.markers = markers;
        Ok(())
    }
}

/// The rotation that turns a node's -Z toward `direction` (glTF axes, Y up),
/// as `[x, y, z, w]`, so Godot's forward is the connector's direction.
fn facing_rotation(direction: [f32; 3]) -> [f32; 4] {
    let from = [0.0f32, 0.0, -1.0];
    let d = direction;
    let dot = from[0] * d[0] + from[1] * d[1] + from[2] * d[2];
    if dot < -0.999_999 {
        // Opposite: half a turn about Y.
        return [0.0, 1.0, 0.0, 0.0];
    }
    let axis = [
        from[1] * d[2] - from[2] * d[1],
        from[2] * d[0] - from[0] * d[2],
        from[0] * d[1] - from[1] * d[0],
    ];
    let w = 1.0 + dot;
    let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2] + w * w).sqrt();
    [axis[0] / len, axis[1] / len, axis[2] / len, w / len]
}

/// glTF nodes for the markers, added to the scene's root list, and the
/// list again in the scene's `extras.froxelMarkers`.
pub(crate) fn append_gltf_markers(session: &Session, root: &mut Value) {
    if session.markers.is_empty() {
        return;
    }
    let mut listed = Vec::new();
    for marker in &session.markers {
        let info = session.marker_json(marker);
        let gltf = &info["gltf"];
        let mut extras = json!({"froxel": {"kind": marker.kind.name(), "object": marker.object}});
        match marker.kind {
            MarkerKind::Box { .. } => {
                extras["froxel"]["size"] = gltf["size"].clone();
                extras["froxel"]["min"] = gltf["min"].clone();
                extras["froxel"]["max"] = gltf["max"].clone();
            }
            MarkerKind::Connector { .. } => {
                extras["froxel"]["direction"] = gltf["direction"].clone();
            }
            MarkerKind::Point { .. } => {}
        }
        let mut node = json!({
            "name": marker.name,
            "translation": gltf["translation"],
            "extras": extras,
        });
        if let MarkerKind::Connector { .. } = marker.kind {
            node["rotation"] = gltf["rotation"].clone();
        }
        let nodes = root["nodes"].as_array_mut().expect("glTF nodes");
        nodes.push(node);
        let index = nodes.len() - 1;
        root["scenes"][0]["nodes"]
            .as_array_mut()
            .expect("scene nodes")
            .push(json!(index));
        let mut entry = json!({"name": marker.name, "kind": marker.kind.name(), "node": index});
        for key in ["translation", "size", "min", "max", "direction", "rotation"] {
            if !gltf[key].is_null() {
                entry[key] = gltf[key].clone();
            }
        }
        listed.push(entry);
    }
    root["scenes"][0]["extras"] = json!({"froxelMarkers": listed});
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_str(out: &mut Vec<u8>, text: &str) {
    push_u32(out, text.len() as u32);
    out.extend_from_slice(text.as_bytes());
}

fn read_n<'a>(body: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8], String> {
    let end = cursor
        .checked_add(len)
        .filter(|end| *end <= body.len())
        .ok_or("MVMK chunk is truncated")?;
    let bytes = &body[*cursor..end];
    *cursor = end;
    Ok(bytes)
}

fn read_u32(body: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let bytes = read_n(body, cursor, 4)?;
    Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
}

fn read_str(body: &[u8], cursor: &mut usize) -> Result<String, String> {
    let len = read_u32(body, cursor)? as usize;
    if len > 256 {
        return Err("MVMK name is too long".into());
    }
    let bytes = read_n(body, cursor, len)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| "MVMK name is not utf-8".into())
}
