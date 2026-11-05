//! Preview lighting: one sun, a flat ambient term, exposure, and an optional
//! colour grade, so MCP `preview` can show colours the way a game engine
//! lights them instead of the editor viewport's light. Named presets come
//! from the call (an object or a JSON file), then from the user's presets
//! file, then from the built-in ones (`viewport`, `old_pond`).
//!
//! The model is Godot's for an unshaded-by-shadow directional light and a
//! colour ambient on a rough, non-metal material:
//! `albedo × (sun × sunEnergy × max(n·l, 0) + ambient × ambientEnergy) ×
//! exposure`, in linear light, clamped (Godot's linear tonemapper), encoded
//! to sRGB, then graded through the 3D LUT when there is one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::Vec3;
use serde_json::{json, Map, Value};

/// The fields a lighting object takes.
const KEYS: [&str; 11] = [
    "preset",
    "description",
    "sunDirection",
    "godotSunBasis",
    "sunColor",
    "sunEnergy",
    "ambientColor",
    "ambientEnergy",
    "exposure",
    "background",
    "lut",
];

/// The built-in presets, in the presets file format. `viewport` is the
/// editor's own light and takes no fields.
const BUILTIN: &str = r##"{
  "viewport": {
    "description": "The edit viewport's light: the document's sun over a grey ambient."
  },
  "old_pond": {
    "description": "frog-world Old Pond: the SunNoShadow DirectionalLight3D of scenes/regions/old_pond/old_pond_region.tscn and the colour ambient, exposure, and background of themes/world/level_01_environment.tres. The level's LUT grade (themes/world/level_grade_lut.png) is the game's file: add it with lut in a project presets file.",
    "godotSunBasis": [-0.9191606, 0.25342205, -0.30153123, 0, 0.765535, 0.64339423, 0.39388302, 0.5913826, -0.70364964],
    "sunColor": [1.0, 0.76, 0.48],
    "sunEnergy": 1.45,
    "ambientColor": [0.62, 0.72, 0.9],
    "ambientEnergy": 1.1,
    "exposure": 1.05,
    "background": "#0a3f57"
  }
}"##;

/// The name of the editor light, which is no `Lighting` at all.
pub const VIEWPORT: &str = "viewport";

#[derive(Clone, Debug)]
pub struct Lighting {
    pub name: String,
    /// Toward the sun in Froxel's Z-up world, unit length. `None` is the
    /// document's sun direction.
    pub sun_direction: Option<Vec3>,
    /// sRGB, as an engine's colour picker stores it. Above 1 is allowed.
    pub sun_color: [f32; 3],
    pub sun_energy: f32,
    pub ambient_color: [f32; 3],
    pub ambient_energy: f32,
    pub exposure: f32,
    /// The background when the call gives none, before the grade.
    pub background: Option<[u8; 4]>,
    pub lut: Option<Arc<Lut>>,
    pub lut_path: Option<PathBuf>,
}

impl Default for Lighting {
    /// A white sun of energy 1 over a white ambient of 0.3: the base of a
    /// preset that names none.
    fn default() -> Self {
        Self {
            name: "custom".into(),
            sun_direction: None,
            sun_color: [1.0; 3],
            sun_energy: 1.0,
            ambient_color: [1.0; 3],
            ambient_energy: 0.3,
            exposure: 1.0,
            background: None,
            lut: None,
            lut_path: None,
        }
    }
}

pub fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

impl Lighting {
    /// Linear radiance of a face of sRGB `color` facing `normal`, with the
    /// sun toward `sun`. Not clamped: the resolve clamps, as Godot's linear
    /// tonemapper does.
    pub fn shade(&self, color: [f32; 3], normal: Vec3, sun: Vec3) -> [f32; 3] {
        let ndl = normal.dot(sun).max(0.0);
        let mut out = [0.0; 3];
        for (i, value) in out.iter_mut().enumerate() {
            let light = to_linear(self.sun_color[i]) * self.sun_energy * ndl
                + to_linear(self.ambient_color[i]) * self.ambient_energy;
            *value = to_linear(color[i]) * light * self.exposure;
        }
        out
    }

    /// The lighting for a reply, with the sun direction it used.
    pub fn json(&self, sun: Vec3) -> Value {
        // f32 to six places, so 1.45 reads 1.45 and not 1.4500000476837158.
        let short = |v: f32| (f64::from(v) * 1e6).round() / 1e6;
        let mut out = json!({
            "name": self.name,
            "sunDirection": sun.to_array().map(short),
            "sunColor": self.sun_color.map(short),
            "sunEnergy": short(self.sun_energy),
            "ambientColor": self.ambient_color.map(short),
            "ambientEnergy": short(self.ambient_energy),
            "exposure": short(self.exposure),
        });
        if let Some([r, g, b, a]) = self.background {
            out["background"] = json!(format!("#{r:02x}{g:02x}{b:02x}{a:02x}"));
        }
        if let Some(path) = &self.lut_path {
            out["lut"] = json!(path.display().to_string());
        }
        out
    }
}

/// A 3D colour lookup table over sRGB, as Godot's colour correction takes
/// it: `size` slices of `size × size` texels, red across a slice, green down
/// it, and blue from slice to slice.
#[derive(Debug)]
pub struct Lut {
    size: usize,
    /// sRGB 0..1, red fastest, then green, then blue.
    texels: Vec<[f32; 3]>,
}

impl Lut {
    /// A PNG with the slices side by side (`size² × size`, Godot's
    /// horizontal slices) or stacked (`size × size²`).
    pub fn read(path: &Path) -> Result<Lut, String> {
        let image = crate::sheet::read_png(path)?;
        Lut::from_rgba(image.width, image.height, &image.rgba)
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Lut, String> {
        let (w, h) = (width as usize, height as usize);
        let (size, horizontal) = if w == h * h {
            (h, true)
        } else if h == w * w {
            (w, false)
        } else {
            return Err(format!(
                "a LUT is N*N by N (slices side by side) or N by N*N, not {width}x{height}"
            ));
        };
        if !(2..=256).contains(&size) {
            return Err(format!("a LUT has 2 to 256 slices, not {size}"));
        }
        let mut texels = vec![[0.0; 3]; size * size * size];
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let (x, y) = if horizontal {
                        (b * size + r, g)
                    } else {
                        (r, b * size + g)
                    };
                    let at = (y * w + x) * 4;
                    texels[(b * size + g) * size + r] =
                        [0, 1, 2].map(|c| rgba[at + c] as f32 / 255.0);
                }
            }
        }
        Ok(Lut { size, texels })
    }

    /// Trilinear lookup of an sRGB colour.
    pub fn apply(&self, rgb: [u8; 3]) -> [u8; 3] {
        let n = self.size;
        let scaled = rgb.map(|c| c as f32 / 255.0 * (n - 1) as f32);
        let lo = scaled.map(|s| (s.floor() as usize).min(n - 2));
        let t = [0, 1, 2].map(|i| scaled[i] - lo[i] as f32);
        let at = |r: usize, g: usize, b: usize| self.texels[(b * n + g) * n + r];
        let mut out = [0.0f32; 3];
        for corner in 0..8 {
            let (dr, dg, db) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let weight = (if dr == 1 { t[0] } else { 1.0 - t[0] })
                * (if dg == 1 { t[1] } else { 1.0 - t[1] })
                * (if db == 1 { t[2] } else { 1.0 - t[2] });
            let texel = at(lo[0] + dr, lo[1] + dg, lo[2] + db);
            for c in 0..3 {
                out[c] += texel[c] * weight;
            }
        }
        out.map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
    }
}

/// The user's presets file: `FROXEL_LIGHTING_PRESETS`, or
/// `lighting_presets.json` in Froxel's support folder.
pub fn user_file() -> Option<PathBuf> {
    support_file("FROXEL_LIGHTING_PRESETS", "lighting_presets.json")
}

/// A user's file: the path in the environment variable `var`, or `name` in
/// Froxel's support folder (macOS `~/Library/Application Support/Froxel`,
/// else `$XDG_DATA_HOME/froxel` or `~/.local/share/froxel`). Tests read none.
pub fn support_file(var: &str, name: &str) -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    if let Some(path) = std::env::var_os(var) {
        return Some(PathBuf::from(path));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Froxel")
    } else if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("froxel")
    } else {
        home.join(".local/share/froxel")
    };
    Some(dir.join(name))
}

/// Presets of one source, and the folder its `lut` paths are relative to.
struct Source {
    presets: Map<String, Value>,
    dir: PathBuf,
}

/// The presets a call can name, first match wins: the call's
/// `lightingPresets`, then the user's file, then the built-in ones.
pub struct Presets {
    sources: Vec<Source>,
}

/// A presets file is an object of name to preset, or one with that object
/// under `lighting`.
fn presets_of(found: Value, what: &str) -> Result<Map<String, Value>, String> {
    let map = match found {
        Value::Object(mut map) if map.get("lighting").is_some_and(Value::is_object) => {
            match map.remove("lighting") {
                Some(Value::Object(inner)) => inner,
                _ => unreachable!("checked"),
            }
        }
        Value::Object(map) => map,
        _ => {
            return Err(format!(
                "{what} must be an object of name: {{lighting fields}}"
            ))
        }
    };
    for (name, preset) in &map {
        if !preset.is_object() {
            return Err(format!("{what}: preset `{name}` must be an object"));
        }
    }
    Ok(map)
}

fn read_file(path: &Path) -> Result<Source, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let found: Value = serde_json::from_str(&text)
        .map_err(|err| format!("{} is not JSON: {err}", path.display()))?;
    Ok(Source {
        presets: presets_of(found, &path.display().to_string())?,
        dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
    })
}

impl Presets {
    /// `call` is MCP `lightingPresets`: an object of presets, or the path
    /// of a presets file. A missing user file is no presets; a broken one
    /// is an error.
    pub fn load(call: Option<&Value>) -> Result<Presets, String> {
        let mut sources = Vec::new();
        match call {
            None | Some(Value::Null) => {}
            Some(Value::String(path)) => sources.push(read_file(Path::new(path))?),
            Some(found @ Value::Object(_)) => sources.push(Source {
                presets: presets_of(found.clone(), "lightingPresets")?,
                dir: PathBuf::new(),
            }),
            Some(_) => {
                return Err(
                    "lightingPresets must be an object of presets or a JSON file path".into(),
                )
            }
        }
        if let Some(path) = user_file().filter(|path| path.is_file()) {
            sources.push(read_file(&path)?);
        }
        let builtin: Value = serde_json::from_str(BUILTIN).expect("built-in presets");
        sources.push(Source {
            presets: presets_of(builtin, "built-in")?,
            dir: PathBuf::new(),
        });
        Ok(Presets { sources })
    }

    /// Every name, the built-in ones last.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for source in &self.sources {
            for name in source.presets.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// MCP `lighting`: a preset name, or an object of lighting fields over
    /// the preset its `preset` names (else over the defaults). `None` is the
    /// viewport's own light.
    pub fn resolve(&self, arg: &Value) -> Result<Option<Lighting>, String> {
        let (name, fields) = match arg {
            Value::Null => return Ok(None),
            Value::String(name) => {
                let fields = self.lookup(name, 0, 0)?;
                (name.clone(), fields)
            }
            Value::Object(given) => {
                let fields = self.merged(given, &PathBuf::new(), None, 0)?;
                let name = given
                    .get("preset")
                    .and_then(Value::as_str)
                    .map_or("custom".to_string(), |base| format!("{base}+"));
                (name, fields)
            }
            _ => {
                return Err("lighting must be a preset name or an object of lighting fields".into())
            }
        };
        if fields.get("viewport").is_some() {
            if fields.len() > 1 {
                return Err(format!(
                    "lighting `{name}`: viewport is the editor light and takes no fields"
                ));
            }
            return Ok(None);
        }
        build(&name, &fields).map(Some)
    }

    /// The fields of preset `name` from the first source at or after
    /// `from`, over its base.
    fn lookup(&self, name: &str, from: usize, depth: usize) -> Result<Map<String, Value>, String> {
        if depth > 8 {
            return Err(format!(
                "lighting preset `{name}`: presets name each other in a loop"
            ));
        }
        let found = self.sources[from.min(self.sources.len())..]
            .iter()
            .enumerate()
            .find_map(|(i, source)| {
                source
                    .presets
                    .get(name)
                    .map(|preset| (from + i, source, preset))
            });
        let Some((level, source, preset)) = found else {
            return Err(format!(
                "lighting must be one of {}, not `{name}`",
                self.names().join(", ")
            ));
        };
        let given = preset.as_object().expect("checked when loaded");
        if level == self.sources.len() - 1 && name == VIEWPORT {
            return Ok(Map::from_iter([("viewport".to_string(), json!(true))]));
        }
        // A preset that names no base overrides the one of its own name
        // further down, field by field.
        let base = match given.get("preset") {
            Some(_) => None,
            None if self.sources[level + 1..]
                .iter()
                .any(|source| source.presets.contains_key(name)) =>
            {
                Some(self.lookup(name, level + 1, depth + 1)?)
            }
            None => None,
        };
        self.merged(given, &source.dir, base, depth)
    }

    /// `given` over its base: the preset it names, or `base`.
    fn merged(
        &self,
        given: &Map<String, Value>,
        dir: &Path,
        base: Option<Map<String, Value>>,
        depth: usize,
    ) -> Result<Map<String, Value>, String> {
        if let Some(key) = given.keys().find(|key| !KEYS.contains(&key.as_str())) {
            return Err(format!("lighting takes {}, not `{key}`", KEYS.join(", ")));
        }
        let mut fields = match given.get("preset") {
            None | Some(Value::Null) => base.unwrap_or_default(),
            Some(Value::String(name)) => self.lookup(name, 0, depth + 1)?,
            Some(_) => return Err("lighting preset must be a preset name".into()),
        };
        if fields.contains_key("viewport") {
            // Fields over the viewport light start from the defaults.
            fields.clear();
        }
        if given.contains_key("sunDirection") || given.contains_key("godotSunBasis") {
            fields.remove("sunDirection");
            fields.remove("godotSunBasis");
        }
        for (key, value) in given {
            if key == "preset" || key == "description" {
                continue;
            }
            let value = match (key.as_str(), value) {
                ("lut", Value::String(path)) if Path::new(path).is_relative() => {
                    json!(dir.join(path).display().to_string())
                }
                _ => value.clone(),
            };
            fields.insert(key.clone(), value);
        }
        Ok(fields)
    }
}

fn number(
    fields: &Map<String, Value>,
    key: &str,
    range: std::ops::RangeInclusive<f32>,
) -> Result<Option<f32>, String> {
    match fields.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found
            .as_f64()
            .map(|v| v as f32)
            .filter(|v| range.contains(v))
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "lighting {key} must be a number from {} to {}",
                    range.start(),
                    range.end()
                )
            }),
    }
}

fn numbers(value: &Value) -> Option<Vec<f32>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_f64().filter(|v| v.is_finite()).map(|v| v as f32))
        .collect()
}

/// `[r, g, b]` in 0..16 (an engine's float colour), or `#rrggbb`.
fn color(fields: &Map<String, Value>, key: &str) -> Result<Option<[f32; 3]>, String> {
    let bad = || format!("lighting {key} must be [r, g, b] (0..16, sRGB) or #rrggbb");
    match fields.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(_)) => {
            let rgba = crate::remote::background(&json!({"background": fields[key]}))
                .map_err(|_| bad())?;
            Ok(Some([0, 1, 2].map(|i| rgba[i] as f32 / 255.0)))
        }
        Some(found) => {
            let list = numbers(found)
                .filter(|list| list.len() == 3)
                .ok_or_else(bad)?;
            if list.iter().any(|v| !(0.0..=16.0).contains(v)) {
                return Err(bad());
            }
            Ok(Some([list[0], list[1], list[2]]))
        }
    }
}

/// Toward the light from a Godot `Transform3D` as a `.tscn` writes it:
/// the basis rows, then optionally the origin. A directional light shines
/// along its −Z, so the sun is toward its +Z axis, the basis's third
/// column. Godot is Y-up and an exported glb maps Froxel `(x, y, z)` to
/// `(x, z, −y)`, so the Froxel direction is `(Z.x, −Z.z, Z.y)`.
pub fn godot_sun(basis: &[f32]) -> Result<Vec3, String> {
    if basis.len() != 9 && basis.len() != 12 {
        return Err(
            "godotSunBasis is the 9 basis numbers (or all 12) of the light's Transform3D".into(),
        );
    }
    let z = Vec3::new(basis[2], basis[5], basis[8]);
    let sun = Vec3::new(z.x, -z.z, z.y);
    if sun.length() < 1e-4 {
        return Err("godotSunBasis has no Z axis".into());
    }
    Ok(sun.normalize())
}

fn build(name: &str, fields: &Map<String, Value>) -> Result<Lighting, String> {
    let mut lighting = Lighting {
        name: name.to_string(),
        ..Default::default()
    };
    if let Some(found) = fields.get("sunDirection").filter(|v| !v.is_null()) {
        let list = numbers(found)
            .filter(|list| list.len() == 3)
            .ok_or("lighting sunDirection must be [x, y, z] toward the sun, Z up")?;
        let direction = Vec3::new(list[0], list[1], list[2]);
        if direction.length() < 1e-4 {
            return Err("lighting sunDirection must not be zero".into());
        }
        lighting.sun_direction = Some(direction.normalize());
    }
    if let Some(found) = fields.get("godotSunBasis").filter(|v| !v.is_null()) {
        let list = numbers(found).ok_or("godotSunBasis must be a list of numbers")?;
        lighting.sun_direction = Some(godot_sun(&list)?);
    }
    if let Some(found) = color(fields, "sunColor")? {
        lighting.sun_color = found;
    }
    if let Some(found) = color(fields, "ambientColor")? {
        lighting.ambient_color = found;
    }
    if let Some(found) = number(fields, "sunEnergy", 0.0..=100.0)? {
        lighting.sun_energy = found;
    }
    if let Some(found) = number(fields, "ambientEnergy", 0.0..=100.0)? {
        lighting.ambient_energy = found;
    }
    if let Some(found) = number(fields, "exposure", 0.001..=16.0)? {
        lighting.exposure = found;
    }
    if let Some(found) = fields.get("background").filter(|v| !v.is_null()) {
        lighting.background = Some(
            crate::remote::background(&json!({"background": found}))
                .map_err(|err| format!("lighting {err}"))?,
        );
    }
    match fields.get("lut") {
        None | Some(Value::Null) => {}
        Some(Value::String(path)) => {
            let path = PathBuf::from(path);
            lighting.lut = Some(Arc::new(Lut::read(&path)?));
            lighting.lut_path = Some(path);
        }
        Some(_) => return Err("lighting lut must be the path of a LUT PNG".into()),
    }
    Ok(lighting)
}

#[cfg(test)]
#[path = "../tests/lighting.rs"]
mod tests;
