//! The Silhouette panels: the fields of MCP `carve_from_views` and
//! `compare_silhouette`, and the comparison overlay the viewport shows.
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct CarvePanel {
    pub views: [ViewRow; 3],
    /// `x y z` cells; empty takes the views at `cells_per_pixel`.
    pub size: String,
    pub cells_per_pixel: f32,
    pub origin: [i32; 3],
    pub background: BackgroundField,
    /// Colour from the views; off fills the active slot.
    pub colour_views: bool,
    /// 0 is solid.
    pub shell: i32,
    pub smooth: i32,
}

impl Default for CarvePanel {
    fn default() -> Self {
        let row = |side: usize, on: bool| ViewRow {
            on,
            side,
            path: String::new(),
            crop: String::new(),
            flip: false,
        };
        Self {
            views: [row(0, true), row(2, true), row(4, false)],
            size: String::new(),
            cells_per_pixel: 0.25,
            origin: [0, 0, 0],
            background: BackgroundField::default(),
            colour_views: true,
            shell: 0,
            smooth: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComparePanel {
    pub path: String,
    pub crop: String,
    pub background: BackgroundField,
    pub flip: bool,
    /// Index into [`CAMERAS`].
    pub camera: usize,
    /// 0 fit, 1 height, 2 width.
    pub align: usize,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SilhouettePanels {
    pub carve: CarvePanel,
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
        for row in self.views.iter().filter(|row| row.on) {
            if row.path.trim().is_empty() {
                return Err(format!("{}: choose an image.", SIDES[row.side].0));
            }
            let mut view = json!({
                "view": SIDES[row.side.min(SIDES.len() - 1)].1,
                "path": row.path.trim(),
                "flip": row.flip
            });
            if let Some(crop) = numbers("Crop", &row.crop, 4)? {
                view["crop"] = json!(crop);
            }
            self.background.write(&mut view);
            views.push(view);
        }
        let mut args = json!({
            "views": views,
            "origin": self.origin,
            "cellsPerPixel": self.cells_per_pixel
        });
        if let Some(size) = numbers("Size", &self.size, 3)? {
            args["size"] = json!(size);
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
        Ok(args)
    }
}

impl ComparePanel {
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

    /// Choose the concept PNG of Compare.
    pub fn pick_compare_concept(&mut self) {
        if let Some(path) = super::dialog::pick_open("PNG", &["png"]) {
            self.silhouette.compare.path = path.display().to_string();
        }
    }
}
