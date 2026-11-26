//! Faces a fixed game camera never sees, left out of the export (MCP `gameView`).
//!
//! A game with one fixed view (a camera that pans but never turns) never
//! shows an asset's back faces, its undersides, or the faces its own cells
//! hide. `gameView` names the camera's direction in the asset's frame, as the
//! preview's `yaw` and `pitch` do, and a spread that covers the perspective
//! lens and the pan: every view direction within `yaw ± spread[0]` and
//! `pitch ± spread[1]`. A face is kept when one of those directions sees its
//! front, and, with `occlusion`, when a ray from it toward the camera along
//! one of them leaves the model without passing through a solid cell (or one
//! of the `solid` boxes, cells a neighbour piece fills). The rest are left
//! out before the faces merge, so the glb has fewer triangles and the colour
//! atlas fewer texels; occlusion and the bake still read every cell, so the
//! faces that stay shade as before.

use serde_json::{json, Value};

use crate::state::{Cell, Session};

use super::quads::OFFSETS;

/// The keys `gameView` takes.
const KEYS: [&str; 5] = ["yaw", "pitch", "spread", "occlusion", "solid"];
/// Default half-angles in degrees: about up, and up or down. A narrow
/// perspective lens on a wide phone screen sees about 20° to each side of
/// its centre and 9° up or down; the rest covers the camera's pan and shake.
pub const DEFAULT_SPREAD: [f32; 2] = [25.0, 14.0];
/// Largest angle in degrees between neighbouring sampled directions. A face
/// nearly edge-on to the camera faces only the edge of the spread, so the
/// samples must be dense there, or a face the lens sees between two of them
/// is taken for hidden.
const STEP_DEGREES: [f32; 2] = [5.0, 7.0];
/// Points sampled on a face for its occlusion rays: corners inset by this
/// share of a cell, and the centre.
const INSET: f32 = 0.08;
/// Most `solid` boxes `gameView` takes.
const MAX_SOLID: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// Degrees, as the preview's yaw: 0 puts the camera on +X, in the
    /// asset's own frame (a level's turn of the asset already taken off).
    pub yaw: f32,
    /// Degrees, −90..90; 90 looks straight down.
    pub pitch: f32,
    /// Half-angles in degrees about up and up or down, 0..80.
    pub spread: [f32; 2],
    /// Also leave out faces hidden behind the model's own cells.
    pub occlusion: bool,
    /// Inclusive cell boxes `[x0, y0, z0, x1, y1, z1]` in the document's
    /// frame that count as solid for the occlusion rays: where a neighbour
    /// piece stands against this one. They are not written.
    pub solid: Vec<[i32; 6]>,
}

impl View {
    /// `None` for an absent, null, or false `view`.
    pub fn parse(value: Option<&Value>) -> Result<Option<View>, String> {
        let map = match value {
            None | Some(Value::Null) | Some(Value::Bool(false)) => return Ok(None),
            Some(Value::Object(map)) => map,
            Some(_) => {
                return Err(
                    "gameView must be an object {yaw, pitch, spread?, occlusion?, solid?}".into(),
                )
            }
        };
        if let Some(key) = map.keys().find(|key| !KEYS.contains(&key.as_str())) {
            return Err(format!("gameView takes {}, not `{key}`", KEYS.join(", ")));
        }
        let number = |key: &str| -> Result<f32, String> {
            map.get(key)
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .map(|value| value as f32)
                .ok_or_else(|| format!("gameView needs {key} in degrees"))
        };
        let yaw = number("yaw")?;
        let pitch = number("pitch")?;
        if !(-90.0..=90.0).contains(&pitch) {
            return Err("gameView pitch must be from -90 to 90".into());
        }
        let spread = match map.get("spread") {
            None | Some(Value::Null) => DEFAULT_SPREAD,
            Some(Value::Number(n)) => {
                let s = n.as_f64().unwrap_or(-1.0) as f32;
                [s, s]
            }
            Some(Value::Array(items)) if items.len() == 2 => [
                items[0].as_f64().unwrap_or(-1.0) as f32,
                items[1].as_f64().unwrap_or(-1.0) as f32,
            ],
            Some(_) => {
                return Err("gameView spread must be degrees or [about up, up or down]".into())
            }
        };
        if spread.iter().any(|s| !(0.0..=80.0).contains(s)) {
            return Err("gameView spread must be from 0 to 80 degrees".into());
        }
        let occlusion = match map.get("occlusion") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("gameView occlusion must be true or false".into()),
        };
        let solid = match map.get("solid") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(boxes)) => {
                if boxes.len() > MAX_SOLID {
                    return Err(format!("gameView solid takes at most {MAX_SOLID} boxes"));
                }
                boxes
                    .iter()
                    .map(|b| {
                        let cells: Option<Vec<i32>> = b.as_array().and_then(|b| {
                            (b.len() == 6)
                                .then(|| b.iter().map(|v| v.as_i64().map(|v| v as i32)).collect())
                                .flatten()
                        });
                        let c = cells.ok_or(
                            "gameView solid boxes are [x0, y0, z0, x1, y1, z1] whole cells",
                        )?;
                        Ok([
                            c[0].min(c[3]),
                            c[1].min(c[4]),
                            c[2].min(c[5]),
                            c[0].max(c[3]),
                            c[1].max(c[4]),
                            c[2].max(c[5]),
                        ])
                    })
                    .collect::<Result<Vec<_>, String>>()?
            }
            Some(_) => return Err("gameView solid must be a list of boxes".into()),
        };
        Ok(Some(View {
            yaw,
            pitch,
            spread,
            occlusion,
            solid,
        }))
    }

    pub fn json(&self) -> Value {
        let mut value = json!({
            "yaw": self.yaw,
            "pitch": self.pitch,
            "spread": self.spread,
            "occlusion": self.occlusion,
        });
        if !self.solid.is_empty() {
            value["solid"] = json!(self.solid);
        }
        value
    }

    /// Unit directions toward the camera, in cells (Z up): a grid over the
    /// spread at most `STEP_DEGREES` apart, its corners and centre included.
    pub fn directions(&self) -> Vec<[f32; 3]> {
        let steps = |axis: usize| {
            (((2.0 * self.spread[axis]) / STEP_DEGREES[axis]).ceil() as usize).max(1) / 2 * 2 + 1
        };
        let (ny, np) = (steps(0), steps(1));
        let t = |k: usize, n: usize| {
            if n < 2 {
                0.0
            } else {
                k as f32 / (n - 1) as f32 * 2.0 - 1.0
            }
        };
        let mut out = Vec::new();
        for i in 0..ny {
            for j in 0..np {
                let yaw = (self.yaw + self.spread[0] * t(i, ny)).to_radians();
                let pitch = (self.pitch + self.spread[1] * t(j, np))
                    .clamp(-90.0, 90.0)
                    .to_radians();
                out.push([
                    yaw.cos() * pitch.cos(),
                    yaw.sin() * pitch.cos(),
                    pitch.sin(),
                ]);
            }
        }
        out
    }
}

/// Decides per face whether `gameView` keeps it.
pub(super) struct Culler {
    directions: Vec<[f32; 3]>,
    occlusion: bool,
    min: Cell,
    size: (i32, i32, i32),
    bits: Vec<u64>,
}

impl Culler {
    /// `None` without `gameView`, so the export is unchanged.
    pub(super) fn new(session: &Session, options: &MeshExport) -> Option<Culler> {
        let view = options.game_view.as_ref()?;
        // Every exported object hides, as for occlusion; translucent slots
        // let the camera through.
        let mut cells = Vec::new();
        for object in &session.objects {
            if object.hidden && !options.export_hidden {
                continue;
            }
            for (cell, slot) in &object.cells {
                if *slot != 0 && session.palette[*slot as usize][3] == 255 {
                    cells.push((
                        cell.0 + object.translation.0,
                        cell.1 + object.translation.1,
                        cell.2 + object.translation.2,
                    ));
                }
            }
        }
        let mut lo = (i32::MAX, i32::MAX, i32::MAX);
        let mut hi = (i32::MIN, i32::MIN, i32::MIN);
        let mut grow = |c: Cell| {
            lo = (lo.0.min(c.0), lo.1.min(c.1), lo.2.min(c.2));
            hi = (hi.0.max(c.0), hi.1.max(c.1), hi.2.max(c.2));
        };
        for c in &cells {
            grow(*c);
        }
        for b in &view.solid {
            grow((b[0], b[1], b[2]));
            grow((b[3], b[4], b[5]));
        }
        let size = if lo.0 > hi.0 {
            (0, 0, 0)
        } else {
            (hi.0 - lo.0 + 1, hi.1 - lo.1 + 1, hi.2 - lo.2 + 1)
        };
        let total = size.0 as usize * size.1 as usize * size.2 as usize;
        let mut culler = Culler {
            directions: view.directions(),
            occlusion: view.occlusion,
            min: lo,
            size,
            bits: vec![0; total.div_ceil(64)],
        };
        if view.occlusion {
            for c in cells {
                culler.set(c);
            }
            for b in &view.solid {
                for x in b[0]..=b[3] {
                    for y in b[1]..=b[4] {
                        for z in b[2]..=b[5] {
                            culler.set((x, y, z));
                        }
                    }
                }
            }
        }
        Some(culler)
    }

    fn index(&self, c: Cell) -> Option<usize> {
        let (x, y, z) = (c.0 - self.min.0, c.1 - self.min.1, c.2 - self.min.2);
        if x < 0 || y < 0 || z < 0 || x >= self.size.0 || y >= self.size.1 || z >= self.size.2 {
            return None;
        }
        Some((x as usize * self.size.1 as usize + y as usize) * self.size.2 as usize + z as usize)
    }

    fn set(&mut self, c: Cell) {
        if let Some(i) = self.index(c) {
            self.bits[i / 64] |= 1 << (i % 64);
        }
    }

    fn solid(&self, c: Cell) -> bool {
        self.index(c)
            .is_some_and(|i| self.bits[i / 64] & (1 << (i % 64)) != 0)
    }

    /// Whether the face `face` (an index of `OFFSETS`) of the world cell
    /// `cell` is seen from some direction of the view.
    pub(super) fn keep(&self, face: usize, cell: Cell) -> bool {
        let n = OFFSETS[face];
        let normal = [n.0 as f32, n.1 as f32, n.2 as f32];
        let facing: Vec<&[f32; 3]> = self
            .directions
            .iter()
            .filter(|d| d[0] * normal[0] + d[1] * normal[1] + d[2] * normal[2] > 1e-4)
            .collect();
        if facing.is_empty() {
            return false;
        }
        if !self.occlusion {
            return true;
        }
        // The face's plane and its two in-plane axes.
        let axis = (0..3).find(|&a| normal[a] != 0.0).unwrap_or(0);
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let base = [cell.0 as f32, cell.1 as f32, cell.2 as f32];
        let plane = base[axis] + if normal[axis] > 0.0 { 1.0 } else { 0.0 };
        let samples = [
            (0.5, 0.5),
            (INSET, INSET),
            (1.0 - INSET, INSET),
            (INSET, 1.0 - INSET),
            (1.0 - INSET, 1.0 - INSET),
        ];
        for d in facing {
            for (su, sv) in samples {
                let mut p = [0.0f32; 3];
                p[axis] = plane + normal[axis] * 1e-3;
                p[u] = base[u] + su;
                p[v] = base[v] + sv;
                if !self.blocked(p, *d) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether a ray from `p` along `d` meets a solid cell before it leaves
    /// the grid (Amanatides and Woo's traversal).
    fn blocked(&self, p: [f32; 3], d: [f32; 3]) -> bool {
        let size = [self.size.0, self.size.1, self.size.2];
        let min = [self.min.0, self.min.1, self.min.2];
        let mut cell = [0i32; 3];
        let mut step = [0i32; 3];
        let mut next = [f32::INFINITY; 3];
        let mut delta = [f32::INFINITY; 3];
        for a in 0..3 {
            cell[a] = p[a].floor() as i32;
            if d[a] > 1e-6 {
                step[a] = 1;
                delta[a] = 1.0 / d[a];
                next[a] = (cell[a] as f32 + 1.0 - p[a]) / d[a];
            } else if d[a] < -1e-6 {
                step[a] = -1;
                delta[a] = -1.0 / d[a];
                next[a] = (p[a] - cell[a] as f32) / -d[a];
            }
        }
        let limit = (size[0] + size[1] + size[2]) as usize + 3;
        for _ in 0..limit {
            let outside = (0..3).any(|a| {
                (step[a] >= 0 && cell[a] >= min[a] + size[a]) || (step[a] <= 0 && cell[a] < min[a])
            });
            if outside {
                return false;
            }
            if self.solid((cell[0], cell[1], cell[2])) {
                return true;
            }
            let a = if next[0] <= next[1] && next[0] <= next[2] {
                0
            } else if next[1] <= next[2] {
                1
            } else {
                2
            };
            cell[a] += step[a];
            next[a] += delta[a];
        }
        false
    }
}

use super::MeshExport;

#[cfg(test)]
#[path = "../../tests/game_view.rs"]
mod tests;
