//! The Silhouette panels: the fields of MCP `carve_from_views`,
//! `project_paint`, `fit_to_views`, and `compare_silhouette`, and the comparison overlay the viewport shows.
//! Each button builds the MCP arguments and runs the MCP handler.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::Session;

/// The views a row can look from, as `carve_from_views` names them.
pub const SIDES: [(&str, &str); 6] = [
    ("Front", "front"),
    ("Back", "back"),
    ("Left (side)", "left"),
    ("Right", "right"),
    ("Top", "top"),
    ("Bottom", "bottom"),
];

/// The cameras Compare offers: the viewport's own, or a preview preset or
/// named view.
pub const CAMERAS: [&str; 5] = ["Viewport", "Game", "Front", "Left (side)", "Top"];

/// A background rule: auto, alpha, or a key colour.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundField {
    /// 0 auto, 1 alpha, 2 key colour.
    pub kind: usize,
    pub key: [u8; 3],
    pub tolerance: f32,
}

impl Default for BackgroundField {
    fn default() -> Self {
        Self {
            kind: 0,
            key: [255, 255, 255],
            tolerance: 40.0,
        }
    }
}

impl BackgroundField {
    fn write(&self, args: &mut Value) {
        args["background"] = match self.kind {
            1 => json!("alpha"),
            2 => json!(format!(
                "#{:02x}{:02x}{:02x}",
                self.key[0], self.key[1], self.key[2]
            )),
            _ => json!("auto"),
        };
        args["tolerance"] = json!(self.tolerance);
    }
}

/// One view of the Carve from views panel.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewRow {
    pub on: bool,
    /// Index into [`SIDES`].
    pub side: usize,
    pub path: String,
    /// `x y w h` in pixels; empty is the whole image.
    pub crop: String,
    pub flip: bool,
    /// A grey depth map of the same crop (white near); empty is none.
    pub depth: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CarvePanel {
    pub views: [ViewRow; 3],
    /// `x y z` cells; empty takes the views at `cells_per_pixel`.
    pub size: String,
    pub cells_per_pixel: f32,
    pub origin: [i32; 3],
    /// The origin is a world cell (MCP `space: "world"`).
    pub world: bool,
    pub background: BackgroundField,
    /// Colour from the views; off fills the active slot.
    pub colour_views: bool,
    /// 0 is solid.
    pub shell: i32,
    pub smooth: i32,
    /// MCP `mode: "intersect"`: keep the cells inside the hull only.
    pub intersect: bool,
    /// Depth maps: black is near.
    pub depth_invert: bool,
    /// Depth maps: 0 hull, 1 box calibration.
    pub depth_calibrate: usize,
    /// Depth maps: mirror the surface for the far side.
    pub depth_back: bool,
    /// Depth maps: `depthTolerance` cells.
    pub depth_tolerance: f32,
    /// Depth maps: `gamma: "auto"`, the curve fitted to the contour.
    pub depth_auto_curve: bool,
    /// Depth maps: calibrate each region on its own.
    pub depth_regions: bool,
    /// Depth maps: the grey step between regions.
    pub depth_edge: f32,
    /// A detail sheet laid out as the views (its crops are theirs): each
    /// region's curvature; empty is none.
    pub depth_detail: String,
    /// MCP `downscale`: pixelate the views at the box first.
    pub pixel_art: bool,
    /// `downscale` `colors`.
    pub pixel_colors: i32,
}

impl Default for CarvePanel {
    fn default() -> Self {
        let row = |side: usize, on: bool| ViewRow {
            on,
            side,
            path: String::new(),
            crop: String::new(),
            flip: false,
            depth: String::new(),
        };
        Self {
            views: [row(0, true), row(2, true), row(4, false)],
            size: String::new(),
            cells_per_pixel: 0.25,
            origin: [0, 0, 0],
            world: false,
            background: BackgroundField::default(),
            colour_views: true,
            shell: 0,
            smooth: 0,
            intersect: false,
            depth_invert: false,
            depth_calibrate: 0,
            depth_back: false,
            depth_tolerance: 0.0,
            depth_auto_curve: false,
            depth_regions: false,
            depth_edge: 0.06,
            depth_detail: String::new(),
            pixel_art: false,
            pixel_colors: 6,
        }
    }
}

/// The Paint from view panel: one image onto the surface a view sees.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintPanel {
    /// Index into [`SIDES`].
    pub side: usize,
    pub path: String,
    /// `x y w h` in pixels; empty is the whole image.
    pub crop: String,
    pub flip: bool,
    pub background: BackgroundField,
    /// Stretch the image over the box; off uses `cells_per_pixel`.
    pub fit: bool,
    pub cells_per_pixel: f32,
    /// `x y z` origin and size of the box; empty is the object's bounds.
    pub origin: String,
    pub size: String,
    /// The origin is a world cell (MCP `space: "world"`).
    pub world: bool,
    pub depth: i32,
    /// Degrees; 90 is no limit.
    pub normal_limit: f32,
    /// Slots that may be repainted, separated by spaces; empty is all.
    pub only: String,
    /// Colour from the image; off paints the active slot.
    pub colour_image: bool,
    pub mirror: super::edit::MirrorField,
}

impl Default for PaintPanel {
    fn default() -> Self {
        Self {
            side: 4,
            path: String::new(),
            crop: String::new(),
            flip: false,
            background: BackgroundField::default(),
            fit: true,
            cells_per_pixel: 0.25,
            origin: String::new(),
            size: String::new(),
            world: false,
            depth: 1,
            normal_limit: 90.0,
            only: String::new(),
            colour_image: true,
            mirror: super::edit::MirrorField::default(),
        }
    }
}

impl PaintPanel {
    /// The MCP `project_paint` arguments. `active` is the slot used when
    /// the colour does not come from the image.
    pub(crate) fn args(&self, active: u8) -> Result<Value, String> {
        if self.path.trim().is_empty() {
            return Err("Paint from view: choose an image.".into());
        }
        let mut args = json!({
            "view": SIDES[self.side.min(SIDES.len() - 1)].1,
            "path": self.path.trim(),
            "flip": self.flip,
            "depth": self.depth.max(1)
        });
        if let Some(crop) = numbers("Crop", &self.crop, 4)? {
            args["crop"] = json!(crop);
        }
        self.background.write(&mut args);
        if !self.fit {
            args["cellsPerPixel"] = json!(self.cells_per_pixel);
        }
        if let Some(origin) = signed("Origin", &self.origin)? {
            args["origin"] = json!(origin);
            if self.world {
                args["space"] = json!("world");
            }
        }
        if let Some(size) = numbers("Size", &self.size, 3)? {
            args["size"] = json!(size);
        }
        if self.normal_limit < 90.0 {
            args["normalLimit"] = json!(self.normal_limit.max(0.0));
        }
        let only: Vec<&str> = self
            .only
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .collect();
        if !only.is_empty() {
            let slots = only
                .iter()
                .map(|part| {
                    part.parse::<u8>()
                        .ok()
                        .filter(|slot| *slot > 0)
                        .ok_or_else(|| format!("Only: `{part}` is not a slot 1..255"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            args["only"] = json!(slots);
        }
        if !self.colour_image {
            args["palette"] = json!(active.max(1));
        }
        self.mirror.write(&mut args)?;
        Ok(args)
    }
}

/// Three whole numbers, which may be negative, separated by spaces or
/// commas.
fn signed(what: &str, text: &str) -> Result<Option<[i64; 3]>, String> {
    let parts: Vec<&str> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return Ok(None);
    }
    if parts.len() != 3 {
        return Err(format!("{what}: 3 whole numbers"));
    }
    let mut out = [0i64; 3];
    for (slot, part) in out.iter_mut().zip(&parts) {
        *slot = part
            .parse()
            .map_err(|_| format!("{what}: `{part}` is not a whole number"))?;
    }
    Ok(Some(out))
}

/// The Fit to views panel: the Carve from views panel's views, and the
/// options of MCP `fit_to_views`.
#[derive(Clone, Debug, PartialEq)]
pub struct FitPanel {
    /// 0 fit, 1 height, 2 width.
    pub align: usize,
    pub iterations: i32,
    pub seed: i32,
    /// Slots colours may take, separated by spaces; empty is the object's.
    pub slots: String,
    /// An object index or name for depth; empty is none.
    pub reference: String,
    pub isolate: bool,
    pub shape: bool,
    pub recolor: bool,
    pub mask_weight: f32,
    pub color_weight: f32,
    pub depth_weight: f32,
    pub smooth_weight: f32,
    /// Degrees; 90 is no limit.
    pub normal_limit: f32,
    /// Slots never removed or recoloured, separated by spaces.
    pub keep: String,
    /// Per Carve from views row: its colours recolour cells.
    pub paint_views: [bool; 3],
    pub round_weight: f32,
    pub roundness: f32,
    /// Axes whose sections the rounding prior rounds, such as `yz`.
    pub round_axes: String,
}

impl Default for FitPanel {
    fn default() -> Self {
        let spec = crate::tools::FitSpec::default();
        Self {
            align: 0,
            iterations: spec.iterations as i32,
            seed: spec.seed as i32,
            slots: String::new(),
            reference: String::new(),
            isolate: false,
            shape: spec.shape,
            recolor: spec.recolor,
            mask_weight: spec.mask_weight,
            color_weight: spec.color_weight,
            depth_weight: spec.depth_weight,
            smooth_weight: spec.smooth_weight,
            normal_limit: spec.normal_limit,
            keep: String::new(),
            paint_views: [true; 3],
            round_weight: spec.round_weight,
            roundness: spec.roundness,
            round_axes: "yz".into(),
        }
    }
}

impl FitPanel {
    /// The MCP `fit_to_views` arguments, with the views of `carve`.
    pub(crate) fn args(&self, carve: &CarvePanel) -> Result<Value, String> {
        let carved = carve.args(1)?;
        let align = ["fit", "height", "width"][self.align.min(2)];
        let mut args = json!({
            "views": carved["views"],
            "depthTolerance": carved.get("depthTolerance").cloned().unwrap_or(json!(0)),
            "align": align,
            "iterations": self.iterations.max(0),
            "seed": self.seed.max(0),
            "isolate": self.isolate,
            "shape": self.shape,
            "recolor": self.recolor,
            "maskWeight": self.mask_weight,
            "colorWeight": self.color_weight,
            "depthWeight": self.depth_weight,
            "smoothWeight": self.smooth_weight,
            "roundWeight": self.round_weight,
            "roundness": self.roundness
        });
        if !self.round_axes.trim().is_empty() {
            args["roundAxes"] = json!(self.round_axes.trim());
        }
        if args["views"]
            .as_array()
            .is_none_or(|views| views.is_empty())
        {
            return Err("Fit to views: turn on a view in Carve from views.".into());
        }
        // The views that are on, in row order, with their Colour boxes.
        let painting: Vec<bool> = carve
            .views
            .iter()
            .zip(self.paint_views)
            .filter(|(row, _)| row.on)
            .map(|(_, paints)| paints)
            .collect();
        if let Some(views) = args["views"].as_array_mut() {
            for (view, paints) in views.iter_mut().zip(painting) {
                if !paints {
                    view["color"] = json!(false);
                }
            }
        }
        args["normalLimit"] = json!(self.normal_limit.clamp(0.0, 90.0));
        for (key, what, text) in [
            ("slots", "Slots", &self.slots),
            ("keep", "Keep", &self.keep),
        ] {
            let parts: Vec<&str> = text
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .collect();
            if parts.is_empty() {
                continue;
            }
            let list = parts
                .iter()
                .map(|part| {
                    part.parse::<u8>()
                        .ok()
                        .filter(|slot| *slot > 0)
                        .ok_or_else(|| format!("{what}: `{part}` is not a slot 1..255"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            args[key] = json!(list);
        }
        let reference = self.reference.trim();
        if !reference.is_empty() {
            args["reference"] = match reference.parse::<u64>() {
                Ok(index) => json!(index),
                Err(_) => json!(reference),
            };
        }
        Ok(args)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComparePanel {
    pub path: String,
    pub crop: String,
    pub background: BackgroundField,
    pub flip: bool,
    /// Index into [`CAMERAS`].
    pub camera: usize,
    /// 0 fit, 1 height, 2 width.
    pub align: usize,
    /// Mask edits: colours to drop (#rrggbb, by spaces or commas) and
    /// their tolerance.
    pub exclude: String,
    pub exclude_tolerance: f32,
    /// Flood the background from the crop's border, with this colour step.
    pub flood: bool,
    pub flood_tolerance: f32,
    /// A mask PNG whose ink keeps the concept's pixels (or drops them).
    pub mask: String,
    pub invert_mask: bool,
    /// Rectangles to erase, `x y w h` per line, in image pixels.
    pub erase: String,
    pub keep_largest: bool,
}

impl Default for ComparePanel {
    fn default() -> Self {
        Self {
            path: String::new(),
            crop: String::new(),
            background: BackgroundField::default(),
            flip: false,
            camera: 0,
            align: 0,
            exclude: String::new(),
            exclude_tolerance: 40.0,
            flood: false,
            flood_tolerance: 24.0,
            mask: String::new(),
            invert_mask: false,
            erase: String::new(),
            keep_largest: false,
        }
    }
}

/// The last comparison, drawn over the viewport until it is closed.
#[derive(Clone, Debug, PartialEq)]
pub struct SilhouetteOverlay {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub iou: f64,
    pub missing: f64,
    pub excess: f64,
    pub path: PathBuf,
    /// Changes with each comparison, so the viewport uploads it once.
    pub epoch: u64,
}

/// The Decal section: one picture laid onto the surface around a point.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalPanel {
    pub path: String,
    /// `x y w h` in pixels; empty is the whole image.
    pub crop: String,
    pub flip: bool,
    pub background: BackgroundField,
    /// `x y z` near the surface.
    pub point: String,
    /// The point is a world cell (MCP `space: "world"`).
    pub world: bool,
    /// `x y z` of the picture's up.
    pub up: String,
    /// Cells across along the surface.
    pub size: f32,
    pub depth: i32,
    pub reach: f32,
    /// Degrees from the point's normal where the decal stops.
    pub normal_limit: f32,
    /// Slots that may be repainted, separated by spaces; empty is all.
    pub only: String,
    /// Colour from the image; off paints the active slot.
    pub colour_image: bool,
    pub mirror: super::edit::MirrorField,
}

impl Default for DecalPanel {
    fn default() -> Self {
        Self {
            path: String::new(),
            crop: String::new(),
            flip: false,
            background: BackgroundField::default(),
            point: String::new(),
            world: false,
            up: "0 0 1".into(),
            size: 8.0,
            depth: 1,
            reach: 8.0,
            normal_limit: 90.0,
            only: String::new(),
            colour_image: true,
            mirror: super::edit::MirrorField::default(),
        }
    }
}

/// The Orient to views panel: the Carve from views panel's views, and the
/// options of MCP `orient_to_views`.
#[derive(Clone, Debug, PartialEq)]
pub struct OrientPanel {
    /// Only the 4 turns about up.
    pub upright: bool,
    /// Degrees; 0 is no fine steps.
    pub yaw_step: f32,
    pub yaw_range: f32,
    /// Fine steps about X and Y too.
    pub tilt: bool,
    /// 0 keep the scale, 1 cells on the longest axis, 2 cells tall.
    pub size_mode: usize,
    pub size: i32,
}

impl Default for OrientPanel {
    fn default() -> Self {
        Self {
            upright: false,
            yaw_step: 5.0,
            yaw_range: 45.0,
            tilt: false,
            size_mode: 0,
            size: 64,
        }
    }
}

impl DecalPanel {
    /// The MCP `decal` arguments. `active` is the slot used when the colour
    /// does not come from the image.
    pub(crate) fn args(&self, active: u8) -> Result<Value, String> {
        if self.path.trim().is_empty() {
            return Err("Decal: choose an image.".into());
        }
        let vector = |what: &str, text: &str| -> Result<Vec<f64>, String> {
            let parts: Vec<f64> = text
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(|part| {
                    part.parse::<f64>()
                        .map_err(|_| format!("Decal: {what} `{part}` is not a number"))
                })
                .collect::<Result<_, _>>()?;
            if parts.len() != 3 {
                return Err(format!("Decal: {what} is x y z"));
            }
            Ok(parts)
        };
        let mut args = json!({
            "path": self.path.trim(),
            "flip": self.flip,
            "point": vector("Point", &self.point)?,
            "up": vector("Up", &self.up)?,
            "size": self.size,
            "depth": self.depth.max(1),
            "reach": self.reach,
            "normalLimit": self.normal_limit,
        });
        if let Some(crop) = numbers("Crop", &self.crop, 4)? {
            args["crop"] = json!(crop);
        }
        self.background.write(&mut args);
        if self.world {
            args["space"] = json!("world");
        }
        let only = super::edit::slot_field("Decal", &self.only)?;
        if !only.is_empty() {
            args["only"] = json!(only);
        }
        if !self.colour_image {
            args["palette"] = json!(active.max(1));
        }
        self.mirror.write(&mut args)?;
        Ok(args)
    }
}

/// The Measure sheet panel: MCP `measure_sheet` on a views sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurePanel {
    pub path: String,
    pub background: BackgroundField,
    /// Count the top view's parts.
    pub parts: bool,
    /// One line per view, check, and the parts, from the last reading.
    pub lines: Vec<String>,
    /// The last reply's `fitViews`, for Use crops.
    pub views: Vec<(String, [u64; 4])>,
}

impl OrientPanel {
    /// The search options of MCP `orient_to_views` with the views of
    /// `carve`: what `import_mesh` `orient` takes too.
    pub(crate) fn search_args(&self, carve: &CarvePanel) -> Result<Value, String> {
        let carved = carve.args(1)?;
        if carved["views"]
            .as_array()
            .is_none_or(|views| views.is_empty())
        {
            return Err("Orient to views: turn on a view in Carve from views.".into());
        }
        let mut args = json!({
            "views": carved["views"],
            "turns": if self.upright { "upright" } else { "all" },
            "yawStep": self.yaw_step.clamp(0.0, 90.0),
            "yawRange": self.yaw_range.clamp(0.0, 180.0),
        });
        if self.tilt {
            args["fineAxes"] = json!("xyz");
        }
        Ok(args)
    }

    /// The MCP `orient_to_views` arguments.
    pub(crate) fn args(&self, carve: &CarvePanel) -> Result<Value, String> {
        let mut args = self.search_args(carve)?;
        match self.size_mode {
            1 => args["size"] = json!(self.size.clamp(1, 256)),
            2 => args["height"] = json!(self.size.clamp(1, 256)),
            _ => {}
        }
        Ok(args)
    }
}

/// The Paint from views panel: MCP `paint_from_views` from a whole sheet
/// (its measured layout) or the Carve from views panel's views.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintViewsPanel {
    /// The sheet PNG; empty takes the Carve from views views.
    pub path: String,
    pub background: BackgroundField,
    /// 0 nearest, 1 soft.
    pub blend: usize,
    pub seam: f32,
    pub sharpness: f32,
    /// A missing right view from the left one mirrored.
    pub mirror: bool,
    /// 0 the slots the object uses, 1 new slots, 2 the listed slots.
    pub colours: usize,
    pub new_slots: i32,
    pub slots: String,
    /// Keep eyes and mouths their own new slot.
    pub accents: bool,
    pub only: String,
    pub inset: i32,
    pub reach: f32,
    /// Paint every visible object together instead of the active one.
    pub all_objects: bool,
    /// MCP `downscale`: pixelate the views at the object's box first.
    pub pixel_art: bool,
    /// `downscale` `colors`.
    pub pixel_colors: i32,
}

impl Default for PaintViewsPanel {
    fn default() -> Self {
        Self {
            path: String::new(),
            background: BackgroundField::default(),
            blend: 0,
            seam: 12.0,
            sharpness: 4.0,
            mirror: true,
            colours: 0,
            new_slots: 12,
            slots: String::new(),
            accents: true,
            only: String::new(),
            inset: 2,
            reach: 3.0,
            all_objects: false,
            pixel_art: false,
            pixel_colors: 6,
        }
    }
}

impl PaintViewsPanel {
    /// The MCP `paint_from_views` arguments, over `region` (local cells of
    /// the active object) when given; `objects` names the objects painted
    /// together with All objects.
    pub(crate) fn args(
        &self,
        carve: &CarvePanel,
        region: Option<(super::Cell, super::Cell)>,
        objects: &[String],
    ) -> Result<Value, String> {
        let mut args = if self.path.trim().is_empty() {
            let carved = carve.args(1)?;
            if carved["views"]
                .as_array()
                .is_none_or(|views| views.is_empty())
            {
                return Err(
                    "Paint from views: choose a sheet, or turn on views in Carve from views."
                        .into(),
                );
            }
            json!({"views": carved["views"]})
        } else {
            let mut args = json!({"path": self.path.trim()});
            self.background.write(&mut args);
            args
        };
        if self.blend == 1 {
            args["blend"] = json!("soft");
            args["sharpness"] = json!(self.sharpness.clamp(0.0, 64.0));
        } else {
            args["blend"] = json!("nearest");
            args["seam"] = json!(self.seam.clamp(0.0, 45.0));
        }
        args["mirror"] = json!(self.mirror);
        if self.pixel_art {
            args["downscale"] = json!({"colors": self.pixel_colors.clamp(1, 32)});
        }
        args["inset"] = json!(self.inset.clamp(0, 16));
        args["reach"] = json!(self.reach.clamp(0.0, 64.0));
        match self.colours {
            1 => {
                args["newSlots"] = json!(self.new_slots.clamp(1, 64));
                args["accents"] = json!(self.accents);
            }
            2 => {
                let slots = super::edit::slot_field("Paint from views", &self.slots)?;
                if slots.is_empty() {
                    return Err("Paint from views: list the slots to quantise into.".into());
                }
                args["slots"] = json!(slots);
            }
            _ => {}
        }
        let only = super::edit::slot_field("Paint from views", &self.only)?;
        if !only.is_empty() {
            args["only"] = json!(only);
        }
        if self.all_objects && !objects.is_empty() {
            args["objects"] = json!(objects);
        }
        if let Some((min, max)) = region {
            for (key, value) in [
                ("x0", min.0),
                ("y0", min.1),
                ("z0", min.2),
                ("x1", max.0),
                ("y1", max.1),
                ("z1", max.2),
            ] {
                args[key] = json!(value);
            }
        }
        Ok(args)
    }
}

/// The Pixelate views panel: MCP `pixelate_views` on a sheet (or the
/// Carve from views views), written to a PNG for checking.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelatePanel {
    /// The sheet PNG; empty takes the Carve from views views.
    pub path: String,
    pub background: BackgroundField,
    /// `x y z` cells; 0 follows the views' proportions.
    pub size: String,
    pub colors: i32,
    pub features: bool,
    pub thin: bool,
    pub despeckle: i32,
    /// Where the pixelated views are written.
    pub png: String,
    /// One line per view from the last run.
    pub lines: Vec<String>,
}

impl Default for PixelatePanel {
    fn default() -> Self {
        Self {
            path: String::new(),
            background: BackgroundField::default(),
            size: "0 20 0".into(),
            colors: 6,
            features: true,
            thin: true,
            despeckle: 1,
            png: String::new(),
            lines: Vec::new(),
        }
    }
}

impl PixelatePanel {
    /// The MCP `pixelate_views` arguments, with the views of `carve` when
    /// no sheet is given.
    pub(crate) fn args(&self, carve: &CarvePanel) -> Result<Value, String> {
        let mut args = if self.path.trim().is_empty() {
            let carved = carve.args(1)?;
            if carved["views"]
                .as_array()
                .is_none_or(|views| views.is_empty())
            {
                return Err(
                    "Pixelate views: choose a sheet, or turn on views in Carve from views.".into(),
                );
            }
            json!({"views": carved["views"]})
        } else {
            let mut args = json!({"path": self.path.trim()});
            self.background.write(&mut args);
            args
        };
        match numbers("Pixelate views size", &self.size, 3)? {
            Some(size) if size.iter().any(|n| *n > 0) => {
                args["size"] = json!(size
                    .iter()
                    .map(|n| if *n == 0 { Value::Null } else { json!(n) })
                    .collect::<Vec<_>>());
            }
            _ => return Err("Pixelate views: Size needs one axis, such as 0 20 0.".into()),
        }
        args["colors"] = json!(self.colors.clamp(1, 32));
        args["features"] = json!(self.features);
        args["thin"] = if self.thin { json!(0.25) } else { json!(false) };
        args["despeckle"] = json!(self.despeckle.clamp(0, 16));
        if !self.png.trim().is_empty() {
            args["png"] = json!(self.png.trim());
        }
        Ok(args)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SilhouettePanels {
    pub measure: MeasurePanel,
    pub pixelate: PixelatePanel,
    pub carve: CarvePanel,
    pub paint: PaintPanel,
    pub paint_views: PaintViewsPanel,
    pub decal: DecalPanel,
    pub fit: FitPanel,
    pub orient: OrientPanel,
    pub compare: ComparePanel,
    pub overlay: Option<SilhouetteOverlay>,
}

/// Whole numbers separated by spaces or commas.
fn numbers(what: &str, text: &str, count: usize) -> Result<Option<Vec<u64>>, String> {
    let parts: Vec<&str> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return Ok(None);
    }
    if parts.len() != count {
        return Err(format!("{what}: {count} whole numbers"));
    }
    parts
        .iter()
        .map(|part| {
            part.parse::<u64>()
                .map_err(|_| format!("{what}: `{part}` is not a whole number"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

impl CarvePanel {
    /// The MCP `carve_from_views` arguments. `active` is the slot used
    /// when the colour does not come from the views.
    pub(crate) fn args(&self, active: u8) -> Result<Value, String> {
        let mut views = Vec::new();
        let mut depths = false;
        for row in self.views.iter().filter(|row| row.on) {
            let (path, depth) = (row.path.trim(), row.depth.trim());
            if path.is_empty() && depth.is_empty() {
                return Err(format!(
                    "{}: choose an image or a depth map.",
                    SIDES[row.side].0
                ));
            }
            let mut view = json!({
                "view": SIDES[row.side.min(SIDES.len() - 1)].1,
                "flip": row.flip
            });
            if !path.is_empty() {
                view["path"] = json!(path);
            }
            if let Some(crop) = numbers("Crop", &row.crop, 4)? {
                view["crop"] = json!(crop);
            }
            self.background.write(&mut view);
            if !depth.is_empty() {
                depths = true;
                let calibrate = ["hull", "box"][self.depth_calibrate.min(1)];
                view["depth"] = json!({
                    "path": depth,
                    "invert": self.depth_invert,
                    "calibrate": calibrate,
                    "back": self.depth_back,
                    "regions": self.depth_regions,
                    "edge": self.depth_edge
                });
                if self.depth_auto_curve {
                    view["depth"]["gamma"] = json!("auto");
                }
                let detail = self.depth_detail.trim();
                if !detail.is_empty() {
                    view["depth"]["detail"] = json!({"path": detail, "edge": self.depth_edge});
                }
            }
            views.push(view);
        }
        let mut args = json!({
            "views": views,
            "origin": self.origin,
            "cellsPerPixel": self.cells_per_pixel
        });
        if self.world {
            args["space"] = json!("world");
        }
        if let Some(size) = numbers("Size", &self.size, 3)? {
            args["size"] = json!(size);
        }
        if self.pixel_art {
            // An axis of 0 follows the views' proportions.
            match numbers("Size", &self.size, 3)? {
                Some(size) if size.iter().any(|n| *n > 0) => {
                    args["size"] = json!(size
                        .iter()
                        .map(|n| if *n == 0 { Value::Null } else { json!(n) })
                        .collect::<Vec<_>>());
                }
                _ => {
                    return Err(
                        "Carve from views: Pixel art needs Size, such as 0 20 0 for 20 cells long."
                            .into(),
                    )
                }
            }
            args["downscale"] = json!({"colors": self.pixel_colors.clamp(1, 32)});
        }
        if !self.colour_views {
            args["palette"] = json!(active.max(1));
        }
        if self.shell > 0 {
            args["shell"] = json!(self.shell);
        }
        if self.smooth > 0 {
            args["smooth"] = json!(self.smooth);
        }
        if self.intersect {
            args["mode"] = json!("intersect");
        }
        if depths && self.depth_tolerance > 0.0 {
            args["depthTolerance"] = json!(self.depth_tolerance);
        }
        Ok(args)
    }
}

impl ComparePanel {
    /// The mask edit arguments of the panel's fields.
    fn write_mask_edits(&self, args: &mut Value) -> Result<(), String> {
        let colours: Vec<&str> = self
            .exclude
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .collect();
        if !colours.is_empty() {
            args["excludeColors"] = json!(colours);
            args["excludeTolerance"] = json!(self.exclude_tolerance);
        }
        if self.flood {
            args["flood"] = json!(self.flood_tolerance);
        }
        if !self.mask.trim().is_empty() {
            args["mask"] = json!({"path": self.mask.trim(), "invert": self.invert_mask});
        }
        let mut rects = Vec::new();
        for line in self.erase.lines().filter(|line| !line.trim().is_empty()) {
            let rect = numbers("Erase", line, 4)?.ok_or("Erase: x y width height per line")?;
            rects.push(rect);
        }
        if !rects.is_empty() {
            args["erase"] = json!(rects);
        }
        if self.keep_largest {
            args["keepLargest"] = json!(true);
        }
        Ok(())
    }

    /// The MCP `compare_silhouette` arguments, writing the overlay to `out`.
    /// The Viewport camera is the window camera's angles and projection.
    pub(crate) fn args(&self, session: &Session, out: &str) -> Result<Value, String> {
        if self.path.trim().is_empty() {
            return Err("Compare: choose a concept image.".into());
        }
        let align = ["fit", "height", "width"][self.align.min(2)];
        let mut args = json!({
            "path": out,
            "concept": self.path.trim(),
            "flip": self.flip,
            "align": align
        });
        if let Some(crop) = numbers("Crop", &self.crop, 4)? {
            args["crop"] = json!(crop);
        }
        self.background.write(&mut args);
        self.write_mask_edits(&mut args)?;
        match self.camera {
            1 => {
                args["preset"] = json!("game");
                session.add_camera_presets(&mut args);
            }
            2 => {
                args["view"] = json!("front");
                args["orthographic"] = json!(true);
            }
            3 => {
                args["view"] = json!("left");
                args["orthographic"] = json!(true);
            }
            4 => {
                args["view"] = json!("top");
                args["orthographic"] = json!(true);
            }
            _ => {
                args["yaw"] = json!(session.yaw.to_degrees());
                args["pitch"] = json!(session.pitch.to_degrees().clamp(-90.0, 90.0));
                args["orthographic"] = json!(session.orthographic);
            }
        }
        Ok(args)
    }
}

impl Session {
    /// The Carve from views panel's button: MCP `carve_from_views` into
    /// the active object.
    pub fn carve_from_panel(&mut self) {
        let args = match self.silhouette.carve.args(self.active_slot) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.carve_from_views", &args) {
            Ok(reply) => format!(
                "Carved {} cells in a {} box, bounds {} to {}.",
                reply["cells"], reply["size"], reply["bounds"]["min"], reply["bounds"]["max"]
            ),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The Paint from view panel's button: MCP `project_paint` onto the
    /// active object.
    pub fn paint_from_panel(&mut self) {
        let args = match self.silhouette.paint.args(self.active_slot) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.project_paint", &args) {
            Ok(reply) => {
                let mut text = format!(
                    "Painted {} cells ({} changed) from the {} grid{}.",
                    reply["cells"],
                    reply["changed"],
                    reply["grid"],
                    super::edit::mirrored_note(&reply)
                );
                if reply["steep"].as_u64().unwrap_or(0) > 0 {
                    text.push_str(&format!(
                        " {} rays met too steep a surface.",
                        reply["steep"]
                    ));
                }
                text
            }
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The Fit to views panel's button: MCP `fit_to_views` on the active
    /// object, with the views of the Carve from views panel.
    pub fn fit_from_panel(&mut self) {
        let args = match self.silhouette.fit.args(&self.silhouette.carve) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.fit_to_views", &args) {
            Ok(reply) => {
                let views: Vec<String> = reply["views"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|view| {
                        format!(
                            "{} IoU {} → {}",
                            view["view"].as_str().unwrap_or("?"),
                            view["before"]["iou"],
                            view["after"]["iou"]
                        )
                    })
                    .collect();
                format!(
                    "Fitted: {}; {} added, {} removed, {} recoloured.",
                    views.join(", "),
                    reply["added"],
                    reply["removed"],
                    reply["recolored"]
                )
            }
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The Orient to views panel's button: MCP `orient_to_views` on the
    /// active object, with the views of the Carve from views panel.
    pub fn orient_from_panel(&mut self) {
        let args = match self.silhouette.orient.args(&self.silhouette.carve) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.orient_to_views", &args) {
            Ok(reply) => orient_status(&reply, &reply["rotation"]),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The Compare panel's button: MCP `compare_silhouette` to a PNG in the
    /// temporary folder, then shown over the viewport.
    pub fn compare_from_panel(&mut self) {
        let out = std::env::temp_dir().join("froxel-compare-silhouette.png");
        let args = match self
            .silhouette
            .compare
            .args(self, &out.display().to_string())
        {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let reply = match crate::remote::apply(self, "render.compare_silhouette", &args) {
            Ok(reply) => reply,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let number = |key: &str| reply[key].as_f64().unwrap_or(0.0);
        self.status = format!(
            "IoU {:.3}, missing {:.1}%, excess {:.1}%. Overlay: {}",
            number("iou"),
            number("missing") * 100.0,
            number("excess") * 100.0,
            out.display()
        );
    }

    /// The Measure sheet button: MCP `measure_sheet`, its views, checks,
    /// and parts as lines under the panel.
    pub fn measure_from_panel(&mut self) {
        let panel = &self.silhouette.measure;
        if panel.path.trim().is_empty() {
            self.status = "Measure: choose a views sheet.".into();
            return;
        }
        let mut args = json!({"path": panel.path.trim(), "parts": panel.parts});
        panel.background.write(&mut args);
        let reply = match crate::remote::apply(self, "image.measure_sheet", &args) {
            Ok(reply) => reply,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let mut lines = Vec::new();
        let mut views = Vec::new();
        for (index, view) in reply["views"].as_array().into_iter().flatten().enumerate() {
            let name = view["view"]
                .as_str()
                .map_or(format!("view {index}"), str::to_string);
            let crop: Vec<u64> = view["crop"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .collect();
            lines.push(format!(
                "{name}: crop {crop:?}, pitch {} x {} px, {} cubes",
                view["pitchX"], view["pitchY"], view["cubes"]
            ));
            if let (Some(name), Ok(crop)) = (view["view"].as_str(), <[u64; 4]>::try_from(crop)) {
                views.push((name.to_string(), crop));
            }
        }
        for check in reply["checks"].as_array().into_iter().flatten() {
            lines.push(format!(
                "{} {}: {}% ({}% in cubes)",
                if check["ok"] == true { "ok" } else { "off" },
                check["check"].as_str().unwrap_or(""),
                check["percent"],
                check["percentCubes"]
            ));
        }
        if let Some(parts) = reply.get("parts") {
            let dirs: Vec<&str> = parts["parts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p["direction"].as_str())
                .collect();
            lines.push(format!(
                "{} view: {} parts ({})",
                parts["view"].as_str().unwrap_or(""),
                parts["count"],
                dirs.join(" ")
            ));
        }
        for note in reply["notes"].as_array().into_iter().flatten() {
            lines.push(note.as_str().unwrap_or("").to_string());
        }
        self.status = format!(
            "Measured {} views{}.",
            views
                .len()
                .max(reply["views"].as_array().map_or(0, Vec::len)),
            if reply["ok"] == true {
                ", every check ok"
            } else {
                ""
            }
        );
        let panel = &mut self.silhouette.measure;
        panel.lines = lines;
        panel.views = views;
    }

    /// Use crops: the measured views' crops into the Carve from views rows
    /// (front, left (side), top), which Fit to views shares.
    pub fn use_measured_crops(&mut self) {
        let path = self.silhouette.measure.path.trim().to_string();
        let mut used = 0;
        for (name, crop) in self.silhouette.measure.views.clone() {
            let side = match name.as_str() {
                "side" => "left",
                other => other,
            };
            let Some(index) = SIDES.iter().position(|(_, s)| *s == side) else {
                continue;
            };
            let carve = &mut self.silhouette.carve;
            let row = match carve.views.iter().position(|row| row.side == index) {
                Some(row) => row,
                None => match carve.views.iter().position(|row| !row.on) {
                    Some(row) => row,
                    None => continue,
                },
            };
            let row = &mut carve.views[row];
            row.on = true;
            row.side = index;
            row.path = path.clone();
            row.crop = format!("{} {} {} {}", crop[0], crop[1], crop[2], crop[3]);
            used += 1;
        }
        self.silhouette.carve.background = self.silhouette.measure.background.clone();
        self.status = if used == 0 {
            "Measure a 2 x 2 sheet first.".into()
        } else {
            format!("Set {used} Carve from views rows from the sheet.")
        };
    }

    /// Choose the sheet of Measure sheet.
    pub fn pick_measure_sheet(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.measure.path = path.display().to_string();
        }
    }

    /// Keep a comparison's overlay for the viewport. MCP
    /// `compare_silhouette` calls this, so a comparison over the control
    /// port shows in the window too.
    pub(crate) fn show_silhouette_overlay(
        &mut self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        reply: &Value,
        path: PathBuf,
    ) {
        let number = |key: &str| reply[key].as_f64().unwrap_or(0.0);
        let epoch = self
            .silhouette
            .overlay
            .as_ref()
            .map_or(1, |overlay| overlay.epoch.wrapping_add(1));
        self.silhouette.overlay = Some(SilhouetteOverlay {
            width,
            height,
            rgba,
            iou: number("iou"),
            missing: number("missing"),
            excess: number("excess"),
            path,
            epoch,
        });
    }
}

impl Session {
    /// Choose the PNG of Carve from views row `row`.
    pub fn pick_carve_view(&mut self, row: usize) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            if let Some(view) = self.silhouette.carve.views.get_mut(row) {
                view.path = path.display().to_string();
            }
        }
    }

    /// Choose the depth map of Carve from views row `row`.
    pub fn pick_carve_depth(&mut self, row: usize) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            if let Some(view) = self.silhouette.carve.views.get_mut(row) {
                view.depth = path.display().to_string();
            }
        }
    }

    /// Choose the detail depth sheet of Carve from views.
    pub fn pick_carve_detail(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.carve.depth_detail = path.display().to_string();
        }
    }

    /// Choose the image of Paint from view.
    pub fn pick_paint_image(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.paint.path = path.display().to_string();
        }
    }

    /// Choose the sheet of Pixelate views.
    pub fn pick_pixelate_sheet(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.pixelate.path = path.display().to_string();
        }
    }

    /// Choose where Pixelate views writes its PNG.
    pub fn pick_pixelate_png(&mut self) {
        if let Some(path) = super::dialog::pick_save(
            Some("Pixelated views PNG"),
            "PNG",
            &["png"],
            "pixelated.png",
        ) {
            self.silhouette.pixelate.png = path.display().to_string();
        }
    }

    /// The Pixelate views button: MCP `pixelate_views`, its PNG written,
    /// one line per view kept for the panel.
    pub fn pixelate_from_panel(&mut self) {
        let args = match self.silhouette.pixelate.args(&self.silhouette.carve) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        match crate::remote::apply(self, "image.pixelate_views", &args) {
            Ok(reply) => {
                let mut lines = Vec::new();
                for view in reply["views"].as_array().into_iter().flatten() {
                    lines.push(format!(
                        "{}: {} x {} cells, {} inked, {} thin, {} features kept",
                        view["view"].as_str().unwrap_or("?"),
                        view["grid"][0],
                        view["grid"][1],
                        view["inked"],
                        view["thin"],
                        view["features"].as_array().map_or(0, Vec::len)
                    ));
                }
                self.status = format!(
                    "Pixelated {} views into {} colours, box {}{}.",
                    lines.len(),
                    reply["colors"],
                    reply["size"],
                    match reply["png"].as_str() {
                        Some(path) => format!("; wrote {path}"),
                        None => String::new(),
                    }
                );
                self.silhouette.pixelate.lines = lines;
            }
            Err(err) => self.status = err,
        }
    }

    /// Choose the sheet of Paint from views.
    pub fn pick_paint_views_sheet(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.paint_views.path = path.display().to_string();
        }
    }

    /// The Paint from views button: MCP `paint_from_views` over the
    /// selection's box, or the whole object (or every visible object).
    pub fn paint_views_from_panel(&mut self) {
        let region = super::geom::cell_bounds(self.picked.iter().copied());
        let objects: Vec<String> = self
            .objects
            .iter()
            .filter(|o| !o.hidden && !o.cells.is_empty())
            .map(|o| o.name.clone())
            .collect();
        let args = match self
            .silhouette
            .paint_views
            .args(&self.silhouette.carve, region, &objects)
        {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.paint_from_views", &args) {
            Ok(reply) => format!(
                "Painted {} of {} surface cells ({} changed, {} blended at seams); {} seen by no view kept their slot.{}",
                reply["painted"],
                reply["surface"],
                reply["changed"],
                reply["blended"],
                reply["unseen"],
                match reply["newSlots"].as_array().map(Vec::len) {
                    Some(n) if n > 0 => format!(" {n} new slots."),
                    _ => String::new(),
                }
            ),
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// Choose the PNG of Decal.
    pub fn pick_decal_image(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.decal.path = path.display().to_string();
        }
    }

    /// The Decal button: MCP `decal` on the active object.
    pub fn decal_from_panel(&mut self) {
        let args = match self.silhouette.decal.args(self.active_slot) {
            Ok(args) => args,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        self.status = match crate::remote::apply(self, "voxel.decal", &args) {
            Ok(reply) => {
                let mut text = format!(
                    "Laid {} cells ({} changed) around {}{}.",
                    reply["cells"],
                    reply["changed"],
                    reply["centre"],
                    super::edit::mirrored_note(&reply)
                );
                if reply["missed"].as_u64().unwrap_or(0) > 0 {
                    text.push_str(&format!(
                        " {} picture cells ran off the surface.",
                        reply["missed"]
                    ));
                }
                text
            }
            Err(err) => err,
        };
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
    }

    /// The selection's centre as the decal's point.
    pub fn decal_point_at_selection(&mut self) {
        let Some((min, max)) = super::geom::cell_bounds(self.picked.iter().copied()) else {
            self.status = "Decal: select cells first.".into();
            return;
        };
        let centre = |a: i32, b: i32| f64::from(a + b) / 2.0;
        self.silhouette.decal.point = format!(
            "{} {} {}",
            centre(min.0, max.0),
            centre(min.1, max.1),
            centre(min.2, max.2)
        );
        self.silhouette.decal.world = false;
    }

    /// Choose the concept PNG of Compare.
    pub fn pick_compare_concept(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.compare.path = path.display().to_string();
        }
    }

    /// Choose the mask PNG of Compare.
    pub fn pick_compare_mask(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.compare.mask = path.display().to_string();
        }
    }
}

/// The status line of an orientation: the score before and after, the
/// turn, and a warning when the runner-up is close.
pub(crate) fn orient_status(reply: &Value, rotation: &Value) -> String {
    let axes: Vec<&str> = rotation["axes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut text = format!(
        "Oriented: IoU {} → {}; x y z went to {} (yaw {}).",
        reply["before"]["score"],
        reply["after"]["score"],
        axes.join(" "),
        rotation["yaw"]
    );
    if reply["margin"].as_f64().is_some_and(|m| m < 0.01) {
        text.push_str(" The runner-up turn scores within 0.01: check it.");
    }
    text
}
