//! The stone panels: the fields of MCP `joints`, `column_relief`, `weather`,
//! `bark`, `cavity_tint`, `moss`, `algae`, `vines`, and `sweep`. Each panel's button builds
//! the tool's MCP arguments over the selection's box (or the active object's
//! box) and runs the MCP handler, so the editor and MCP share one path.

use serde_json::{json, Value};

use super::geom::cell_bounds;
use super::{parse_slots, Session};
use crate::tools::JointPattern;

/// The Joints panel.
#[derive(Clone, PartialEq)]
pub struct JointsPanel {
    pub pattern: JointPattern,
    pub spacing: [f32; 2],
    pub course: [i32; 2],
    pub width: i32,
    pub depth: i32,
    /// 0 keeps the gap bottom's colour.
    pub dirt: i32,
    /// Palette indices separated by commas or spaces; empty cuts every slot.
    pub only: String,
    pub density: f32,
    pub seed: i32,
}

impl Default for JointsPanel {
    fn default() -> Self {
        Self {
            pattern: JointPattern::Columns,
            spacing: [4.0, 9.0],
            course: [10, 24],
            width: 1,
            depth: 2,
            dirt: 0,
            only: String::new(),
            density: 0.6,
            seed: 0,
        }
    }
}

impl JointsPanel {
    /// Spacing and course defaults for a newly picked pattern.
    pub fn set_pattern(&mut self, pattern: JointPattern) {
        let (spacing, course) = pattern.defaults();
        self.pattern = pattern;
        self.spacing = [spacing.0 as f32, spacing.1 as f32];
        if pattern != JointPattern::Cracks {
            self.course = [course.0, course.1];
        }
    }

    fn args(&self) -> Result<Value, String> {
        let mut args = json!({
            "pattern": self.pattern.name(),
            "spacing": [self.spacing[0], self.spacing[1].max(self.spacing[0])],
            "width": self.width,
            "depth": self.depth,
            "only": slots("Only", &self.only)?,
            "density": self.density,
            "seed": self.seed.max(0),
        });
        if self.pattern != JointPattern::Cracks {
            args["course"] = json!([self.course[0], self.course[1].max(self.course[0])]);
        }
        if self.dirt > 0 {
            args["dirt"] = json!(self.dirt);
        }
        Ok(args)
    }
}

/// The Relief panel: MCP `column_relief`.
#[derive(Clone, PartialEq)]
pub struct ReliefPanel {
    /// Index into [`RELIEF_FACINGS`].
    pub facing: usize,
    /// 0 is the active slot.
    pub palette: i32,
    pub width: [i32; 2],
    pub out: [i32; 2],
    /// Off: the highest solid cell of the box.
    pub crown_on: bool,
    pub crown: i32,
    pub top: [i32; 2],
    pub lower: bool,
    pub depth: i32,
    pub seed: i32,
}

/// The Relief panel's face directions.
pub const RELIEF_FACINGS: [(&str, &str); 4] =
    [("−Y", "-y"), ("+Y", "+y"), ("−X", "-x"), ("+X", "+x")];

impl Default for ReliefPanel {
    fn default() -> Self {
        Self {
            facing: 0,
            palette: 0,
            width: [6, 12],
            out: [1, 4],
            crown_on: false,
            crown: 24,
            top: [-8, 4],
            lower: true,
            depth: 3,
            seed: 0,
        }
    }
}

impl ReliefPanel {
    fn args(&self, active: u8) -> Result<Value, String> {
        let palette = if self.palette > 0 {
            self.palette
        } else {
            i32::from(active.max(1))
        };
        let mut args = json!({
            "facing": RELIEF_FACINGS[self.facing.min(RELIEF_FACINGS.len() - 1)].1,
            "palette": palette,
            "width": [self.width[0], self.width[1].max(self.width[0])],
            "out": [self.out[0], self.out[1].max(self.out[0])],
            "top": [self.top[0], self.top[1].max(self.top[0])],
            "lower": self.lower,
            "depth": self.depth,
            "seed": self.seed.max(0),
        });
        if self.crown_on {
            args["crown"] = json!(self.crown);
        }
        Ok(args)
    }
}

/// The Weather panel.
#[derive(Clone, PartialEq)]
pub struct WeatherPanel {
    pub chip: f32,
    pub chip_size: i32,
    pub pits: f32,
    pub pit_depth: i32,
    /// 0 keeps the uncovered faces' colour.
    pub broken: i32,
    /// Noise slots separated by commas or spaces; empty adds no noise.
    pub noise: String,
    pub noise_density: f32,
    pub noise_scale: f32,
    pub only: String,
    pub seed: i32,
}

impl Default for WeatherPanel {
    fn default() -> Self {
        Self {
            chip: 0.15,
            chip_size: 2,
            pits: 0.02,
            pit_depth: 1,
            broken: 0,
            noise: String::new(),
            noise_density: 0.35,
            noise_scale: 3.0,
            only: String::new(),
            seed: 0,
        }
    }
}

impl WeatherPanel {
    fn args(&self) -> Result<Value, String> {
        let mut args = json!({
            "chip": self.chip,
            "chipSize": self.chip_size,
            "pits": self.pits,
            "pitDepth": self.pit_depth,
            "noise": slots("Noise", &self.noise)?,
            "noiseDensity": self.noise_density,
            "noiseScale": self.noise_scale,
            "only": slots("Only", &self.only)?,
            "seed": self.seed.max(0),
        });
        if self.broken > 0 {
            args["broken"] = json!(self.broken);
        }
        Ok(args)
    }
}

/// The Bark panel: MCP `bark`.
#[derive(Clone, PartialEq)]
pub struct BarkPanel {
    /// Index into [`BARK_ALONG`]: an axis, or the panel's points.
    pub along: usize,
    /// The points of the wood, one per line: `x y z`, and the tube's radius
    /// or the sweep's twist and scale, which are ignored.
    pub points: String,
    pub smooth: bool,
    /// Plate slots; empty is the active slot.
    pub palettes: String,
    /// 0 keeps the fissure bottom's colour.
    pub dirt: i32,
    pub width: [f32; 2],
    pub length: [f32; 2],
    pub depth: [i32; 2],
    pub cracks: f32,
    pub only: String,
    pub seed: i32,
}

/// The Bark panel's grain directions; the last follows the panel's points.
pub const BARK_ALONG: [&str; 4] = ["X", "Y", "Z", "Points"];

impl Default for BarkPanel {
    fn default() -> Self {
        Self {
            along: 0,
            points: "0 0 0\n32 0 0".into(),
            smooth: true,
            palettes: String::new(),
            dirt: 0,
            width: [3.0, 6.0],
            length: [8.0, 20.0],
            depth: [1, 2],
            cracks: 0.25,
            only: String::new(),
            seed: 0,
        }
    }
}

impl BarkPanel {
    fn args(&self, active: u8) -> Result<Value, String> {
        let mut palettes = parse_slots(&self.palettes).map_err(|err| format!("Palettes: {err}"))?;
        if palettes.is_empty() {
            palettes.push(active.max(1));
        }
        let mut args = json!({
            "palettes": palettes,
            "width": [self.width[0], self.width[1].max(self.width[0])],
            "length": [self.length[0], self.length[1].max(self.length[0])],
            "depth": [self.depth[0], self.depth[1].max(self.depth[0])],
            "cracks": self.cracks,
            "only": slots("Only", &self.only)?,
            "seed": self.seed.max(0),
        });
        match self.along {
            0 => args["axis"] = json!("x"),
            1 => args["axis"] = json!("y"),
            2 => args["axis"] = json!("z"),
            _ => {
                args["points"] = json!(rows(&self.points, "Points", 3..=5)?);
                args["curve"] = json!(if self.smooth { "smooth" } else { "polyline" });
            }
        }
        if self.dirt > 0 {
            args["dirt"] = json!(self.dirt);
        }
        Ok(args)
    }
}

/// The Cavity panel: MCP `cavity_tint`.
#[derive(Clone, PartialEq)]
pub struct CavityPanel {
    pub radius: i32,
    pub dark: f32,
    pub lighten: bool,
    pub light: f32,
    /// `slot darker [lighter]` separated by semicolons or new lines; empty
    /// makes shades.
    pub pairs: String,
    pub darken: f32,
    pub brighten: f32,
    pub only: String,
}

impl Default for CavityPanel {
    fn default() -> Self {
        Self {
            radius: 2,
            dark: 0.12,
            lighten: false,
            light: 0.25,
            pairs: String::new(),
            darken: 0.72,
            brighten: 1.18,
            only: String::new(),
        }
    }
}

impl CavityPanel {
    fn args(&self) -> Result<Value, String> {
        let mut args = json!({
            "radius": self.radius,
            "dark": self.dark,
            "lighten": self.lighten,
            "light": self.light,
            "darken": self.darken,
            "brighten": self.brighten,
            "only": slots("Only", &self.only)?,
        });
        let pairs = rows(&self.pairs, "Pairs", 2..=3)?;
        if !pairs.is_empty() {
            args["pairs"] = json!(pairs
                .iter()
                .map(|row| row.iter().map(|v| v.round() as i64).collect::<Vec<_>>())
                .collect::<Vec<_>>());
        }
        Ok(args)
    }
}

/// Which plant the Growth panel grows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Plant {
    Moss,
    Algae,
    Vines,
}

/// The Growth panel: MCP `moss`, `algae`, and `vines`.
#[derive(Clone, PartialEq)]
pub struct GrowthPanel {
    pub plant: Plant,
    /// Plant slots separated by commas or spaces; empty is the active slot.
    pub palettes: String,
    pub amount: f32,
    pub noise: f32,
    /// Top, side, bottom.
    pub faces: [bool; 3],
    /// Index into [`FACINGS`]; 0 is any direction.
    pub facing: usize,
    pub band_on: bool,
    pub band: [i32; 2],
    pub only: String,
    pub paint: bool,
    pub curtains: f32,
    pub length: [i32; 2],
    pub thickness: i32,
    /// Moss in separate cushions instead of caps and curtains.
    pub cushions: bool,
    pub size: [i32; 2],
    pub gap: i32,
    pub exposure: f32,
    pub drapes: f32,
    pub blooms: String,
    pub bloom_chance: f32,
    pub seed: i32,
}

/// The Growth panel's facing choices.
pub const FACINGS: [(&str, Option<[i32; 3]>); 7] = [
    ("Any", None),
    ("−Y", Some([0, -1, 0])),
    ("+Y", Some([0, 1, 0])),
    ("−X", Some([-1, 0, 0])),
    ("+X", Some([1, 0, 0])),
    ("+Z", Some([0, 0, 1])),
    ("−Z", Some([0, 0, -1])),
];

impl Default for GrowthPanel {
    fn default() -> Self {
        let mut panel = Self {
            plant: Plant::Moss,
            palettes: String::new(),
            amount: 0.5,
            noise: 0.5,
            faces: [true, true, false],
            facing: 0,
            band_on: false,
            band: [0, 4],
            only: String::new(),
            paint: false,
            curtains: 0.3,
            length: [2, 8],
            thickness: 1,
            cushions: false,
            size: [2, 5],
            gap: 2,
            exposure: 0.35,
            drapes: 0.25,
            blooms: String::new(),
            bloom_chance: 0.06,
            seed: 0,
        };
        panel.set_plant(Plant::Moss);
        panel
    }
}

impl GrowthPanel {
    /// The defaults of a newly picked plant.
    pub fn set_plant(&mut self, plant: Plant) {
        self.plant = plant;
        let (amount, faces, paint, length) = match plant {
            Plant::Moss => (0.5, [true, true, false], false, [2, 8]),
            Plant::Algae => (0.9, [true, true, false], true, self.length),
            Plant::Vines => (0.08, [false, true, false], false, [4, 16]),
        };
        self.amount = amount;
        self.faces = faces;
        self.paint = paint;
        self.length = length;
        self.band_on = plant == Plant::Algae;
    }

    /// Moss cushions on or off, with their defaults.
    pub fn set_cushions(&mut self, on: bool) {
        self.cushions = on;
        self.amount = if on { 0.45 } else { 0.5 };
        self.length = if on { [2, 6] } else { [2, 8] };
        self.thickness = if on { 2 } else { 1 };
    }

    fn method(&self) -> &'static str {
        match self.plant {
            Plant::Moss => "voxel.moss",
            Plant::Algae => "voxel.algae",
            Plant::Vines => "voxel.vines",
        }
    }

    fn args(&self, active: u8) -> Result<Value, String> {
        let mut palettes = parse_slots(&self.palettes).map_err(|err| format!("Palettes: {err}"))?;
        if palettes.is_empty() {
            palettes.push(active.max(1));
        }
        let faces: Vec<&str> = ["top", "side", "bottom"]
            .into_iter()
            .zip(self.faces)
            .filter(|(_, on)| *on)
            .map(|(name, _)| name)
            .collect();
        let mut args = json!({
            "palettes": palettes,
            "amount": self.amount,
            "noise": self.noise,
            "faces": faces,
            "only": slots("Only", &self.only)?,
            "seed": self.seed.max(0),
        });
        if let Some(facing) = FACINGS[self.facing.min(FACINGS.len() - 1)].1 {
            args["facing"] = json!(facing);
        }
        if self.band_on {
            args["band"] = json!([self.band[0], self.band[1].max(self.band[0])]);
        }
        let length = json!([self.length[0], self.length[1].max(self.length[0])]);
        match self.plant {
            Plant::Moss if self.cushions => {
                args["style"] = json!("cushions");
                args["layer"] = json!(if self.paint { "paint" } else { "grow" });
                args["size"] = json!([self.size[0], self.size[1].max(self.size[0])]);
                args["gap"] = json!(self.gap);
                args["exposure"] = json!(self.exposure);
                args["drapes"] = json!(self.drapes);
                args["length"] = length;
                args["thickness"] = json!(self.thickness);
            }
            Plant::Moss => {
                args["layer"] = json!(if self.paint { "paint" } else { "grow" });
                args["curtains"] = json!(self.curtains);
                args["length"] = length;
                args["thickness"] = json!(self.thickness);
            }
            Plant::Algae => {
                args["layer"] = json!(if self.paint { "paint" } else { "grow" });
            }
            Plant::Vines => {
                args["length"] = length;
                args["blooms"] = slots("Blooms", &self.blooms)?;
                args["bloomChance"] = json!(self.bloom_chance);
            }
        }
        Ok(args)
    }
}

/// The Sweep panel.
#[derive(Clone, PartialEq)]
pub struct SweepPanel {
    /// One point per line: `x y z`, optionally `twist` and `scale`.
    pub points: String,
    /// A polygon instead of the rounded rectangle.
    pub polygon: bool,
    /// `u v` pairs separated by semicolons or new lines.
    pub profile: String,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub smooth: bool,
    /// Remove the solid cells the sweep covers: MCP `palette` 0.
    pub erase: bool,
    /// Palette indices; empty is the active slot.
    pub palettes: String,
    /// 0 bands, 1 rings, 2 random.
    pub pattern: usize,
    pub period: f32,
    /// 0 replace, 1 empty, 2 paint.
    pub mode: usize,
    pub seed: i32,
}

impl Default for SweepPanel {
    fn default() -> Self {
        Self {
            points: "0 0 0\n16 8 0\n32 4 0".into(),
            polygon: false,
            profile: "-3 0; 3 0; 2 10; -2 10".into(),
            width: 6.0,
            height: 12.0,
            radius: 1.5,
            smooth: true,
            erase: false,
            palettes: String::new(),
            pattern: 0,
            period: 2.0,
            mode: 0,
            seed: 0,
        }
    }
}

/// Numbers in `text` split by spaces, commas, semicolons, or new lines into
/// rows of 2 to 5.
pub(super) fn rows(
    text: &str,
    what: &str,
    sizes: std::ops::RangeInclusive<usize>,
) -> Result<Vec<Vec<f64>>, String> {
    text.split(|c| c == ';' || c == '\n')
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(|row| {
            let numbers = row
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(|part| {
                    part.parse::<f64>()
                        .map_err(|_| format!("{what}: `{part}` is not a number"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if sizes.contains(&numbers.len()) {
                Ok(numbers)
            } else {
                Err(format!(
                    "{what}: `{row}` needs {} to {} numbers",
                    sizes.start(),
                    sizes.end()
                ))
            }
        })
        .collect()
}

impl SweepPanel {
    fn args(&self, active: u8) -> Result<Value, String> {
        let mut palettes = parse_slots(&self.palettes).map_err(|err| format!("Palettes: {err}"))?;
        if palettes.is_empty() {
            palettes.push(active.max(1));
        }
        let pattern = ["bands", "rings", "random"][self.pattern.min(2)];
        let mode = ["replace", "empty", "paint"][self.mode.min(2)];
        let mut args = json!({
            "points": rows(&self.points, "Points", 3..=5)?,
            "curve": if self.smooth { "smooth" } else { "polyline" },
            "palettes": palettes,
            "pattern": pattern,
            "period": self.period,
            "mode": mode,
            "seed": self.seed.max(0),
        });
        if self.erase {
            let map = args.as_object_mut().expect("the arguments are an object");
            for key in ["palettes", "pattern", "period", "mode", "seed"] {
                map.remove(key);
            }
            args["palette"] = json!(0);
        }
        if self.polygon {
            args["profile"] = json!(rows(&self.profile, "Profile", 2..=2)?);
        } else {
            args["width"] = json!(self.width);
            args["height"] = json!(self.height);
            args["radius"] = json!(self.radius.min(self.width.min(self.height) / 2.0));
        }
        Ok(args)
    }
}

/// The Markers panel: a new marker's name, kind, and connector direction.
#[derive(Clone, PartialEq)]
pub struct MarkersPanel {
    pub name: String,
    /// 0 point, 1 box, 2 connector.
    pub kind: usize,
    /// Index into [`DIRECTIONS`].
    pub direction: usize,
}

impl Default for MarkersPanel {
    fn default() -> Self {
        Self {
            name: "marker".into(),
            kind: 0,
            direction: 3,
        }
    }
}

/// The Markers panel's connector directions.
pub const DIRECTIONS: [(&str, [i32; 3]); 6] = [
    ("+X", [1, 0, 0]),
    ("−X", [-1, 0, 0]),
    ("+Y", [0, 1, 0]),
    ("−Y", [0, -1, 0]),
    ("+Z", [0, 0, 1]),
    ("−Z", [0, 0, -1]),
];

#[derive(Clone, Default, PartialEq)]
pub struct StonePanels {
    /// The Protect section: MCP `exclude` boxes for Scatter, Relief,
    /// Joints, Weather, Bark, Cavity, Foliage, and Growth, `x0 y0 z0 x1 y1 z1`
    /// separated by semicolons or new lines.
    pub exclude: String,
    pub joints: JointsPanel,
    pub relief: ReliefPanel,
    pub weather: WeatherPanel,
    pub bark: BarkPanel,
    pub cavity: CavityPanel,
    pub foliage: super::FoliagePanel,
    pub growth: GrowthPanel,
    pub sweep: SweepPanel,
    pub markers: MarkersPanel,
}

impl StonePanels {
    /// The Protect boxes as MCP `exclude`.
    pub(crate) fn exclude_arg(&self) -> Result<Value, String> {
        let boxes = rows(&self.exclude, "Protect", 6..=6)?;
        if boxes.len() > crate::tools::MAX_EXCLUDE {
            return Err(format!(
                "Protect: at most {} boxes",
                crate::tools::MAX_EXCLUDE
            ));
        }
        Ok(json!(boxes
            .iter()
            .map(|row| row.iter().map(|v| v.round() as i64).collect::<Vec<_>>())
            .collect::<Vec<_>>()))
    }
}

fn slots(what: &str, text: &str) -> Result<Value, String> {
    parse_slots(text)
        .map(|list| json!(list))
        .map_err(|err| format!("{what}: {err}"))
}

impl Session {
    /// The Joints panel's button.
    pub fn joints_from_panel(&mut self) {
        let args = self.stone.joints.args();
        self.stone_from_panel("voxel.joints", args, |reply| {
            format!(
                "Cut {} joint cells, {} with dirt.",
                reply["removed"], reply["dirtied"]
            )
        });
    }

    /// The Relief panel's button.
    pub fn relief_from_panel(&mut self) {
        let panel = &self.stone.relief;
        // Room for a column to stand out of the face and rise above the crown.
        let pad = panel.out[1].max(panel.top[1]).max(1);
        let args = panel.args(self.active_slot);
        self.stone_from_panel_padded("voxel.column_relief", args, pad, |reply| {
            format!(
                "Raised {} columns at crown {}: {} cells added, {} cut.",
                reply["columns"], reply["crown"], reply["added"], reply["removed"]
            )
        });
    }

    /// The Weather panel's button.
    pub fn weather_from_panel(&mut self) {
        let args = self.stone.weather.args();
        self.stone_from_panel("voxel.weather", args, |reply| {
            format!(
                "{} chips and {} pits; {} faces broken, {} cells recoloured.",
                reply["chips"], reply["pits"], reply["broken"], reply["recoloured"]
            )
        });
    }

    /// The Bark panel's button.
    pub fn bark_from_panel(&mut self) {
        let args = self.stone.bark.args(self.active_slot);
        self.stone_from_panel("voxel.bark", args, |reply| {
            let text = format!(
                "Bark: {} plates, {} cells cut, {} with dirt, {} cracks",
                reply["plates"], reply["removed"], reply["dirtied"], reply["cracks"]
            );
            match reply.get("warning").and_then(Value::as_str) {
                Some(warning) => format!("{text}. {warning}."),
                None => text + ".",
            }
        });
    }

    /// The Bark panel's copy of the Sweep panel's points.
    pub fn bark_points_from_sweep(&mut self) {
        self.stone.bark.points = self.stone.sweep.points.clone();
        self.stone.bark.smooth = self.stone.sweep.smooth;
        self.stone.bark.along = BARK_ALONG.len() - 1;
    }

    /// The Cavity panel's button.
    pub fn cavity_from_panel(&mut self) {
        let args = self.stone.cavity.args();
        self.stone_from_panel("voxel.cavity_tint", args, |reply| {
            let made = reply["shades"].as_array().map_or(0, Vec::len);
            format!(
                "Darkened {} cells, lightened {}; {made} new shades.",
                reply["darkened"], reply["lightened"]
            )
        });
    }

    /// The Growth panel's button.
    pub fn growth_from_panel(&mut self) {
        let panel = &self.stone.growth;
        let method = panel.method();
        let args = panel.args(self.active_slot);
        self.stone_from_panel(method, args, |reply| {
            let mut text = format!(
                "Grew {} cells, painted {}",
                reply["added"], reply["painted"]
            );
            if let Some(curtains) = reply.get("curtains") {
                text += &format!(", {curtains} curtains");
            }
            if let Some(cushions) = reply.get("cushions") {
                text += &format!(", {cushions} cushions, {} drapes", reply["drapes"]);
            }
            if let Some(strands) = reply.get("strands") {
                text += &format!(", {strands} strands, {} blooms", reply["blooms"]);
            }
            match reply.get("warning").and_then(Value::as_str) {
                Some(warning) => format!("{text}. {warning}."),
                None => text + ".",
            }
        });
    }

    /// The Sweep panel's button: MCP `sweep` along the panel's points.
    pub fn sweep_from_panel(&mut self) {
        let args = match self.stone.sweep.args(self.active_slot) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.sweep", &args) {
            Ok(reply) => match reply.get("removed") {
                Some(removed) => format!(
                    "Erased {removed} of the {} cells along {} cells of path.",
                    reply["cells"], reply["length"]
                ),
                None => format!(
                    "Swept {} cells along {} cells of path.",
                    reply["cells"], reply["length"]
                ),
            },
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Add the selection's box to the Protect boxes.
    pub fn protect_selection(&mut self) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Select the cells to keep clear, such as a ledge top.".into();
            return;
        };
        let line = format!(
            "{} {} {} {} {} {}",
            min.0, min.1, min.2, max.0, max.1, max.2
        );
        let exclude = &mut self.stone.exclude;
        if !exclude.is_empty() && !exclude.ends_with('\n') {
            exclude.push('\n');
        }
        exclude.push_str(&line);
    }

    /// Add the selection's centre as the next Sweep point.
    pub fn sweep_point_at_selection(&mut self) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Select cells to add their centre as a point.".into();
            return;
        };
        let mid = |a: i32, b: i32| f64::from(a + b) / 2.0;
        let line = format!(
            "{} {} {}",
            mid(min.0, max.0),
            mid(min.1, max.1),
            mid(min.2, max.2)
        );
        let points = &mut self.stone.sweep.points;
        if !points.is_empty() && !points.ends_with('\n') {
            points.push('\n');
        }
        points.push_str(&line);
    }

    /// The Markers panel's button: MCP `set_marker` on the active object at
    /// the selection: a point on the top centre of its box, the box itself,
    /// or a connector at its centre.
    pub fn marker_from_panel(&mut self) {
        let Some((min, max)) = cell_bounds(self.picked.iter().copied()) else {
            self.status = "Select cells to place the marker.".into();
            return;
        };
        let panel = &self.stone.markers;
        let centre = |a: i32, b: i32| f64::from(a + b + 1) / 2.0;
        let mut args = json!({"name": panel.name, "object": self.selected});
        match panel.kind {
            1 => {
                args["kind"] = json!("box");
                for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"]
                    .into_iter()
                    .zip([min.0, min.1, min.2, max.0, max.1, max.2])
                {
                    args[key] = json!(value);
                }
            }
            2 => {
                args["kind"] = json!("connector");
                args["point"] = json!([
                    centre(min.0, max.0),
                    centre(min.1, max.1),
                    centre(min.2, max.2)
                ]);
                args["direction"] = json!(DIRECTIONS[panel.direction.min(5)].1);
            }
            _ => {
                args["kind"] = json!("point");
                args["point"] = json!([centre(min.0, max.0), centre(min.1, max.1), max.2 + 1]);
            }
        }
        self.status = match crate::remote::apply(self, "marker.set", &args) {
            Ok(reply) => format!("Marker {} set.", reply["marker"]["name"]),
            Err(err) => err,
        };
    }

    /// The Markers panel's remove button for one marker.
    pub fn remove_marker_from_panel(&mut self, name: &str) {
        if let Err(err) = crate::remote::apply(self, "marker.remove", &json!({"name": name})) {
            self.status = err;
        }
    }

    /// Run a stone tool's MCP handler over the selection's box, or over the
    /// active object's box grown by `grow` cells, and report in the status.
    fn stone_from_panel(
        &mut self,
        method: &str,
        args: Result<Value, String>,
        report: impl FnOnce(&Value) -> String,
    ) {
        self.stone_from_panel_padded(method, args, 1, report);
    }

    /// [`Session::stone_from_panel`] with the object's box grown by `pad`
    /// cells when nothing is selected.
    fn stone_from_panel_padded(
        &mut self,
        method: &str,
        args: Result<Value, String>,
        pad: i32,
        report: impl FnOnce(&Value) -> String,
    ) {
        let mut args = match args.and_then(|args| Ok((args, self.stone.exclude_arg()?))) {
            Ok((mut args, exclude)) => {
                args["exclude"] = exclude;
                args
            }
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let region = if self.picked.is_empty() {
            cell_bounds(self.active().cells.keys().copied()).map(|(min, max)| {
                (
                    crate::tools::add(min, (-pad, -pad, -pad)),
                    crate::tools::add(max, (pad, pad, pad)),
                )
            })
        } else {
            cell_bounds(self.picked.iter().copied())
        };
        let Some((min, max)) = region else {
            self.status = "Select a region or add voxels first.".into();
            return;
        };
        for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"]
            .into_iter()
            .zip([min.0, min.1, min.2, max.0, max.1, max.2])
        {
            args[key] = json!(value);
        }
        self.status = match crate::remote::apply(self, method, &args) {
            Ok(reply) => report(&reply),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }
}
