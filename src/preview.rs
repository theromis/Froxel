//! Fast lit preview: the viewport's exposed-face mesh and its sun-plus-ambient
//! shading, rasterised on the CPU with a depth buffer. No path tracer, no
//! window, no GPU. MCP `preview` and `froxel preview` both call [`render`].
//! An overlay draws the document's markers over the model: collision boxes
//! as outlines, points as dots, connectors as arrows, with their names.
//! The export counts are the triangles and quads the glb would have, apart
//! from the preview's own unmerged triangles.

use std::collections::HashMap;

use glam::Vec3;

use crate::mesh::{self, Mesh};
use crate::state::Session;

/// The viewport's perspective field of view, vertical, in degrees.
pub const VIEWPORT_FOV: f32 = 50.0;
/// The viewport's clear colour, sRGB.
pub const VIEWPORT_BACKGROUND: [u8; 4] = [22, 24, 28, 255];
/// Supersampling stops above this many output pixels.
const ANTIALIAS_MAX_PIXELS: u64 = 2048 * 2048;
pub const MAX_SIZE: u32 = 4096;
const NEAR: f32 = 0.08;

#[derive(Clone, Debug)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    /// Degrees. 0 puts the camera on +X looking toward −X.
    pub yaw: f32,
    /// Degrees, −90..90. 90 looks straight down.
    pub pitch: f32,
    pub orthographic: bool,
    /// Vertical field of view in degrees for perspective, 10..120.
    pub fov: f32,
    /// Empty border on each side, as a fraction of the image, 0..0.45.
    pub margin: f32,
    /// Frame this object instead of every drawn object.
    pub object: Option<usize>,
    /// Draw only `object`.
    pub isolate: bool,
    pub background: [u8; 4],
    /// 2×2 supersampling, up to 2048×2048 output pixels.
    pub antialias: bool,
    pub overlay: Overlay,
    /// Count the mesh an export with these options writes. The preview
    /// draws the unmerged viewport mesh, which can have ten times as many
    /// triangles as a merged or colour-atlas glb.
    pub export: Option<crate::export::MeshExport>,
    /// Write the preview and export counts in the image's bottom-left corner.
    pub counts_label: bool,
}

/// Which markers the preview draws over the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overlay {
    pub boxes: bool,
    pub points: bool,
    pub connectors: bool,
    /// Each marker's name beside it.
    pub labels: bool,
}

impl Overlay {
    pub fn any(&self) -> bool {
        self.boxes || self.points || self.connectors
    }
}

/// A marker the overlay drew, at its anchor's pixel.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub name: String,
    pub kind: &'static str,
    pub object: String,
    pub pixel: [i32; 2],
    /// Its anchor is behind the model.
    pub hidden: bool,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            yaw: 30.0,
            pitch: 25.0,
            orthographic: false,
            fov: VIEWPORT_FOV,
            margin: 0.06,
            object: None,
            isolate: false,
            background: VIEWPORT_BACKGROUND,
            antialias: true,
            overlay: Overlay::default(),
            export: None,
            counts_label: false,
        }
    }
}

pub struct Frame {
    pub rgba: Vec<u8>,
    /// Triangles of the unmerged viewport mesh that touched the image. Not
    /// what an export writes: that is `export`.
    pub preview_triangles: usize,
    /// The export's counts with `Preview::export`, or why it would fail.
    pub export: Option<Result<crate::export::Counts, String>>,
    /// Output pixels per voxel edge, orthographic only.
    pub pixels_per_voxel: Option<f32>,
    /// The markers the overlay drew.
    pub overlay: Vec<Drawn>,
}

/// An object by index or by name.
pub fn find_object(session: &Session, key: &str) -> Result<usize, String> {
    if let Ok(index) = key.parse::<usize>() {
        if index < session.objects.len() {
            return Ok(index);
        }
        return Err(format!(
            "object index {index} is out of range ({} objects)",
            session.objects.len()
        ));
    }
    session
        .objects
        .iter()
        .position(|object| object.name == key)
        .ok_or_else(|| format!("no object named `{key}`"))
}

pub fn check(preview: &Preview) -> Result<(), String> {
    if !(1..=MAX_SIZE).contains(&preview.width) || !(1..=MAX_SIZE).contains(&preview.height) {
        return Err(format!("width and height must be 1..{MAX_SIZE}"));
    }
    if !(-90.0..=90.0).contains(&preview.pitch) {
        return Err("pitch is out of range".into());
    }
    if !preview.yaw.is_finite() {
        return Err("yaw must be finite".into());
    }
    if !(10.0..=120.0).contains(&preview.fov) {
        return Err("fov must be 10..120 degrees".into());
    }
    if !(0.0..=0.45).contains(&preview.margin) {
        return Err("margin must be 0..0.45".into());
    }
    if preview.isolate && preview.object.is_none() {
        return Err("isolate needs object".into());
    }
    Ok(())
}

/// One face lit like the viewport shader (`gpu.rs` `fs_main`), in linear.
fn shade(color: [f32; 3], normal: Vec3, sun: Vec3, sun_intensity: f32) -> [f32; 3] {
    let ndl = normal.dot(sun).max(0.0);
    let key = (0.45 + sun_intensity * 0.35).clamp(0.45, 1.5);
    let light = (0.30 + key * ndl + 0.08 * normal.z).clamp(0.12, 1.35);
    color.map(|c| to_linear(c) * light)
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

/// World-space meshes of the drawn objects, posed like the viewport, and
/// whether each one is framed.
fn meshes(session: &Session, preview: &Preview) -> Vec<(Mesh, bool)> {
    let mut out = Vec::new();
    for (index, object) in session.objects.iter().enumerate() {
        let chosen = preview.object == Some(index);
        let drawn = if preview.isolate {
            chosen
        } else {
            chosen || !object.hidden
        };
        if !drawn {
            continue;
        }
        let cells: HashMap<_, _> = object
            .cells
            .iter()
            .filter(|(_, slot)| **slot != 0)
            .map(|(cell, slot)| (*cell, *slot))
            .collect();
        if cells.is_empty() {
            continue;
        }
        let mut built = mesh::mesh_cells(&cells, |slot| session.color(slot), |_| [0.0, 0.0]);
        mesh::translate_mesh(&mut built, object.translation);
        if let Some(delta) = session.skin_delta(index) {
            for vert in &mut built.verts {
                vert.pos = delta
                    .transform_point3(Vec3::from_array(vert.pos))
                    .to_array();
                vert.normal = delta
                    .transform_vector3(Vec3::from_array(vert.normal))
                    .normalize_or_zero()
                    .to_array();
            }
        }
        out.push((built, preview.object.is_none() || chosen));
    }
    out
}

/// Orbit basis of the viewport camera: right, up, forward (into the view).
fn basis(yaw: f32, pitch: f32) -> (Vec3, Vec3, Vec3) {
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    let toward_eye = Vec3::new(
        yaw.cos() * pitch.cos(),
        yaw.sin() * pitch.cos(),
        pitch.sin(),
    );
    let forward = -toward_eye;
    let up = if forward.dot(Vec3::Z).abs() > 0.95 {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let right = forward.cross(up).normalize();
    (right, right.cross(forward), forward)
}

/// Maps a world point to sample space: x right, y down, and a nearness
/// that is affine in screen space (greater is closer).
enum Camera {
    Ortho {
        right: Vec3,
        up: Vec3,
        forward: Vec3,
        centre: (f32, f32),
        scale: f32,
        half: (f32, f32),
    },
    Persp {
        right: Vec3,
        up: Vec3,
        forward: Vec3,
        eye: Vec3,
        focal: f32,
        half: (f32, f32),
    },
}

impl Camera {
    fn project(&self, p: Vec3) -> Option<[f32; 3]> {
        match *self {
            Camera::Ortho {
                right,
                up,
                forward,
                centre,
                scale,
                half,
            } => Some([
                half.0 + (p.dot(right) - centre.0) * scale,
                half.1 - (p.dot(up) - centre.1) * scale,
                -p.dot(forward),
            ]),
            Camera::Persp {
                right,
                up,
                forward,
                eye,
                focal,
                half,
            } => {
                let q = p - eye;
                let depth = q.dot(forward);
                if depth < NEAR {
                    return None;
                }
                Some([
                    half.0 + q.dot(right) / depth * focal,
                    half.1 - q.dot(up) / depth * focal,
                    1.0 / depth,
                ])
            }
        }
    }
}

/// Fit every framed vertex inside the image less the margin.
fn frame(meshes: &[(Mesh, bool)], preview: &Preview, w: f32, h: f32) -> Option<Camera> {
    let (right, up, forward) = basis(preview.yaw, preview.pitch);
    let framed = || {
        meshes
            .iter()
            .filter(|(_, framed)| *framed)
            .flat_map(|(mesh, _)| mesh.verts.iter().map(|v| Vec3::from_array(v.pos)))
    };
    let fill = 1.0 - 2.0 * preview.margin;
    let half = (w * 0.5, h * 0.5);
    if preview.orthographic {
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in framed() {
            let (x, y) = (p.dot(right), p.dot(up));
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        if x0 > x1 {
            return None;
        }
        let scale = (w * fill / (x1 - x0).max(1e-3)).min(h * fill / (y1 - y0).max(1e-3));
        return Some(Camera::Ortho {
            right,
            up,
            forward,
            centre: ((x0 + x1) * 0.5, (y0 + y1) * 0.5),
            scale,
            half,
        });
    }
    let tan_y = (preview.fov * 0.5).to_radians().tan();
    let (fit_x, fit_y) = (tan_y * w / h * fill, tan_y * fill);
    // Each side plane of the shrunken frustum must touch the framed vertex
    // that is most extreme toward it. With the eye at e = ex·right + ey·up +
    // ez·forward, a point p is inside the left plane when
    // p·right + fit_x·p·forward ≥ ex + fit_x·ez, so the tightest planes are
    // the minima of those sums. A left/right pair fixes ex and ez, a
    // bottom/top pair ey and ez; the eye takes the further-back ez.
    let (mut left, mut right_side, mut bottom, mut top) = (f32::MAX, f32::MAX, f32::MAX, f32::MAX);
    let mut nearest = f32::MAX;
    for p in framed() {
        let (x, y, z) = (p.dot(right), p.dot(up), p.dot(forward));
        left = left.min(x + fit_x * z);
        right_side = right_side.min(-x + fit_x * z);
        bottom = bottom.min(y + fit_y * z);
        top = top.min(-y + fit_y * z);
        nearest = nearest.min(z);
    }
    if left == f32::MAX {
        return None;
    }
    let ez = ((left + right_side) / (2.0 * fit_x))
        .min((bottom + top) / (2.0 * fit_y))
        .min(nearest - NEAR * 4.0);
    // The looser axis is centred for its own ez; re-centre both for the
    // shared one, so the silhouette sits in the middle of the image.
    let points: Vec<(f32, f32, f32)> = framed()
        .map(|p| (p.dot(right), p.dot(up), p.dot(forward) - ez))
        .collect();
    let ex = centre(points.iter().map(|&(x, _, d)| (x, d)));
    let ey = centre(points.iter().map(|&(_, y, d)| (y, d)));
    Some(Camera::Persp {
        right,
        up,
        forward,
        eye: right * ex + up * ey + forward * ez,
        focal: half.1 / tan_y,
        half,
    })
}

/// The eye offset along one image axis that puts the projected extremes of
/// `(offset, depth)` points symmetric about the centre. Their sum is
/// decreasing in the offset, so bisection between the extremes finds it.
fn centre(points: impl Iterator<Item = (f32, f32)> + Clone) -> f32 {
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for (x, _) in points.clone() {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    let balance = |eye: f32| {
        let (mut min, mut max) = (f32::MAX, f32::MIN);
        for (x, depth) in points.clone() {
            let s = (x - eye) / depth;
            min = min.min(s);
            max = max.max(s);
        }
        min + max
    };
    for _ in 0..32 {
        let mid = (lo + hi) * 0.5;
        if balance(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

/// Draw the document as the viewport lights it.
pub fn render(session: &Session, preview: &Preview) -> Result<Frame, String> {
    check(preview)?;
    let meshes = meshes(session, preview);
    if meshes.is_empty() {
        return Err(match preview.object {
            Some(index) => format!("object {index} has no voxels"),
            None => "document has no voxels".into(),
        });
    }
    let pixels = preview.width as u64 * preview.height as u64;
    let ss = if preview.antialias && pixels <= ANTIALIAS_MAX_PIXELS {
        2
    } else {
        1
    };
    let (sw, sh) = (preview.width * ss, preview.height * ss);
    let camera = frame(&meshes, preview, sw as f32, sh as f32)
        .ok_or_else(|| "nothing to frame".to_string())?;
    let sun = session.sun_dir();
    let mut depth = vec![f32::NEG_INFINITY; (sw * sh) as usize];
    let mut color = vec![[0f32; 3]; (sw * sh) as usize];
    let mut triangles = 0;
    for (mesh, _) in &meshes {
        for tri in mesh.indices.chunks_exact(3) {
            let verts = [tri[0], tri[1], tri[2]].map(|i| &mesh.verts[i as usize]);
            let projected = [
                camera.project(Vec3::from_array(verts[0].pos)),
                camera.project(Vec3::from_array(verts[1].pos)),
                camera.project(Vec3::from_array(verts[2].pos)),
            ];
            let [Some(a), Some(b), Some(c)] = projected else {
                continue;
            };
            let lit = shade(
                verts[0].color,
                Vec3::from_array(verts[0].normal),
                sun,
                session.sun_intensity,
            );
            if raster(&mut depth, &mut color, sw, sh, a, b, c, lit) {
                triangles += 1;
            }
        }
    }
    let mut rgba = resolve(&depth, &color, preview, ss);
    let overlay = if preview.overlay.any() {
        draw_overlay(session, preview, &camera, &depth, ss, &mut rgba)
    } else {
        Vec::new()
    };
    let pixels_per_voxel = match camera {
        Camera::Ortho { scale, .. } => Some(scale / ss as f32),
        Camera::Persp { .. } => None,
    };
    let export = preview
        .export
        .as_ref()
        .map(|options| crate::export::counts(session, options));
    if preview.counts_label {
        draw_counts(&mut rgba, preview, triangles, export.as_ref());
    }
    Ok(Frame {
        rgba,
        preview_triangles: triangles,
        export,
        pixels_per_voxel,
        overlay,
    })
}

/// `12345678` as `12,345,678`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The label lines of [`draw_counts`].
pub fn counts_lines(
    preview: &Preview,
    preview_triangles: usize,
    export: Option<&Result<crate::export::Counts, String>>,
) -> Vec<String> {
    let mut lines = vec![format!(
        "preview {} tris (unmerged)",
        grouped(preview_triangles)
    )];
    if let (Some(found), Some(options)) = (export, &preview.export) {
        let mut kind = Vec::new();
        if options.color_atlas {
            kind.push("atlas");
        } else if options.merge {
            kind.push("merge");
        } else {
            kind.push("no merge");
        }
        if options.split_objects {
            kind.push("split");
        }
        if options.ao_strength > 0.0 {
            kind.push("ao");
        }
        lines.push(match found {
            Ok(counts) => format!(
                "export {} tris {} quads ({})",
                grouped(counts.triangles),
                grouped(counts.quads),
                kind.join(", ")
            ),
            Err(err) => format!("export: {err}"),
        });
    }
    lines
}

/// The counts on a dark plate in the bottom-left corner.
fn draw_counts(
    rgba: &mut [u8],
    preview: &Preview,
    preview_triangles: usize,
    export: Option<&Result<crate::export::Counts, String>>,
) {
    let (w, h) = (preview.width, preview.height);
    let scale = if w.min(h) >= 600 { 2 } else { 1 };
    let lines = counts_lines(preview, preview_triangles, export);
    let line = crate::sheet::line_height(scale) as i32;
    let tw = lines
        .iter()
        .map(|text| crate::sheet::text_width(text, scale))
        .max()
        .unwrap_or(0) as i32;
    let x = 4 * scale as i32;
    let y0 = h as i32 - x - line * lines.len() as i32;
    crate::sheet::fill_rect(
        rgba,
        w,
        h,
        [x - 3, y0 - 3, x + tw + 3, h as i32 - x + 1],
        [12, 12, 14, 200],
    );
    for (i, text) in lines.iter().enumerate() {
        let color = if i == 0 {
            [200, 200, 205, 255]
        } else {
            [255, 220, 120, 255]
        };
        crate::sheet::draw_text(rgba, w, h, x, y0 + line * i as i32, text, scale, color);
    }
}

const BOX_COLOR: [u8; 4] = [255, 150, 30, 255];
const POINT_COLOR: [u8; 4] = [60, 220, 255, 255];
const CONNECTOR_COLOR: [u8; 4] = [255, 80, 210, 255];

/// Draw the markers of the drawn objects over `rgba`. A line or dot behind
/// the model is drawn at half strength, so a box can be checked against
/// the stone it should hold.
fn draw_overlay(
    session: &Session,
    preview: &Preview,
    camera: &Camera,
    depth: &[f32],
    ss: u32,
    rgba: &mut [u8],
) -> Vec<Drawn> {
    use crate::state::MarkerKind;
    let (w, h) = (preview.width, preview.height);
    let (sw, sh) = (w * ss, h * ss);
    let drawn_object = |name: &str| {
        session
            .objects
            .iter()
            .enumerate()
            .find(|(_, object)| object.name == name)
            .is_some_and(|(index, object)| {
                let chosen = preview.object == Some(index);
                if preview.isolate {
                    chosen
                } else {
                    chosen || !object.hidden
                }
            })
    };
    // A projected point in output pixels with its nearness, and whether
    // the model is in front of it.
    let place = |p: Vec3| -> Option<([f32; 2], bool)> {
        let [x, y, z] = camera.project(p)?;
        let (sx, sy) = (x.floor() as i64, y.floor() as i64);
        let hidden = (0..sw as i64).contains(&sx)
            && (0..sh as i64).contains(&sy)
            && depth[(sy as u32 * sw + sx as u32) as usize] > z + z.abs() * 1.0e-4 + 1.0e-4;
        Some(([x / ss as f32, y / ss as f32], hidden))
    };
    let thick = (1 + w.min(h) / 600) as i32;
    let mut out = Vec::new();
    let mut labels: Vec<([i32; 2], String, [u8; 4])> = Vec::new();
    for marker in &session.markers {
        if !drawn_object(&marker.object) {
            continue;
        }
        let world = session.marker_world(marker);
        let (anchor, color) = match &world {
            MarkerKind::Box { min, max } if preview.overlay.boxes => {
                let corner = |i: usize| {
                    Vec3::new(
                        if i & 1 == 0 { min[0] } else { max[0] },
                        if i & 2 == 0 { min[1] } else { max[1] },
                        if i & 4 == 0 { min[2] } else { max[2] },
                    )
                };
                for (a, b) in [
                    (0, 1),
                    (2, 3),
                    (4, 5),
                    (6, 7),
                    (0, 2),
                    (1, 3),
                    (4, 6),
                    (5, 7),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ] {
                    segment(rgba, w, h, &place, corner(a), corner(b), thick, BOX_COLOR);
                }
                let top = Vec3::new((min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, max[2]);
                (top, BOX_COLOR)
            }
            MarkerKind::Point { at } if preview.overlay.points => {
                let p = Vec3::from_array(*at);
                if let Some((q, hidden)) = place(p) {
                    dot(rgba, w, h, q, 3 + thick, fade(POINT_COLOR, hidden));
                }
                (p, POINT_COLOR)
            }
            MarkerKind::Connector { at, direction } if preview.overlay.connectors => {
                let p = Vec3::from_array(*at);
                let d = Vec3::from_array(*direction).normalize_or_zero();
                let tip = p + d * 4.0;
                segment(rgba, w, h, &place, p, tip, thick, CONNECTOR_COLOR);
                // The arrowhead: two short strokes back from the tip, across
                // the direction.
                let side = if d.cross(Vec3::Z).length() > 0.1 {
                    d.cross(Vec3::Z).normalize()
                } else {
                    Vec3::X
                };
                for s in [1.0, -1.0] {
                    let back = tip - d * 1.4 + side * s;
                    segment(rgba, w, h, &place, tip, back, thick, CONNECTOR_COLOR);
                }
                if let Some((q, hidden)) = place(p) {
                    dot(rgba, w, h, q, 2 + thick, fade(CONNECTOR_COLOR, hidden));
                }
                (p, CONNECTOR_COLOR)
            }
            _ => continue,
        };
        let Some((q, hidden)) = place(anchor) else {
            continue;
        };
        let pixel = [q[0].round() as i32, q[1].round() as i32];
        if preview.overlay.labels {
            labels.push((pixel, marker.name.clone(), color));
        }
        out.push(Drawn {
            name: marker.name.clone(),
            kind: world.name(),
            object: marker.object.clone(),
            pixel,
            hidden,
        });
    }
    // Labels last, over every line, each on a dark plate.
    let scale = if w.min(h) >= 600 { 2 } else { 1 };
    for (pixel, name, color) in labels {
        let tw = crate::sheet::text_width(&name, scale) as i32;
        let th = 7 * scale as i32;
        let x = (pixel[0] + 6).clamp(1, (w as i32 - tw - 2).max(1));
        let y = (pixel[1] - th - 6).clamp(1, (h as i32 - th - 2).max(1));
        crate::sheet::fill_rect(
            rgba,
            w,
            h,
            [x - 2, y - 2, x + tw + 2, y + th + 2],
            [12, 12, 14, 200],
        );
        crate::sheet::draw_text(rgba, w, h, x, y, &name, scale, color);
    }
    out
}

fn fade(color: [u8; 4], hidden: bool) -> [u8; 4] {
    if hidden {
        [color[0], color[1], color[2], 110]
    } else {
        color
    }
}

/// A line between two world points, in steps of about a pixel, each step
/// faded where the model hides it.
#[allow(clippy::too_many_arguments)]
fn segment(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    place: &impl Fn(Vec3) -> Option<([f32; 2], bool)>,
    a: Vec3,
    b: Vec3,
    thick: i32,
    color: [u8; 4],
) {
    let (Some((pa, _)), Some((pb, _))) = (place(a), place(b)) else {
        return;
    };
    let length = ((pb[0] - pa[0]).hypot(pb[1] - pa[1])).ceil() as usize;
    let steps = length.clamp(1, 8192);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let Some((q, hidden)) = place(a.lerp(b, t)) else {
            continue;
        };
        // Hidden stretches are dashed.
        if hidden && (i / 4) % 2 == 1 {
            continue;
        }
        dot(rgba, w, h, q, thick, fade(color, hidden));
    }
}

/// A filled square of side `size` centred on `q`.
fn dot(rgba: &mut [u8], w: u32, h: u32, q: [f32; 2], size: i32, color: [u8; 4]) {
    let x0 = (q[0] - size as f32 / 2.0).round() as i32;
    let y0 = (q[1] - size as f32 / 2.0).round() as i32;
    crate::sheet::fill_rect(rgba, w, h, [x0, y0, x0 + size, y0 + size], color);
}

/// Fill one triangle at sample centres with a depth test. Returns whether
/// it touched the image.
#[allow(clippy::too_many_arguments)]
fn raster(
    depth: &mut [f32],
    color: &mut [[f32; 3]],
    w: u32,
    h: u32,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    lit: [f32; 3],
) -> bool {
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < 1e-9 {
        return false;
    }
    let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as i64;
    let x1 = (a[0].max(b[0]).max(c[0]).ceil() as i64).min(w as i64 - 1);
    let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as i64;
    let y1 = (a[1].max(b[1]).max(c[1]).ceil() as i64).min(h as i64 - 1);
    if x0 > x1 || y0 > y1 {
        return false;
    }
    let inv = 1.0 / area;
    let edge = |p: [f32; 3], q: [f32; 3], x: f32, y: f32| {
        ((q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0])) * inv
    };
    for y in y0..=y1 {
        let py = y as f32 + 0.5;
        let row = y as usize * w as usize;
        for x in x0..=x1 {
            let px = x as f32 + 0.5;
            let wa = edge(b, c, px, py);
            let wb = edge(c, a, px, py);
            let wc = 1.0 - wa - wb;
            if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                continue;
            }
            let z = wa * a[2] + wb * b[2] + wc * c[2];
            let index = row + x as usize;
            if z > depth[index] {
                depth[index] = z;
                color[index] = lit;
            }
        }
    }
    true
}

/// Average each ss×ss block in linear light and encode sRGB.
fn resolve(depth: &[f32], color: &[[f32; 3]], preview: &Preview, ss: u32) -> Vec<u8> {
    let bg = preview.background;
    let bg_linear = [bg[0], bg[1], bg[2]].map(|c| to_linear(c as f32 / 255.0));
    let bg_alpha = bg[3] as f32 / 255.0;
    let sw = (preview.width * ss) as usize;
    let count = (ss * ss) as f32;
    let mut out = Vec::with_capacity((preview.width * preview.height * 4) as usize);
    for y in 0..preview.height as usize {
        for x in 0..preview.width as usize {
            let mut sum = [0f32; 3];
            let mut alpha = 0f32;
            for sy in 0..ss as usize {
                for sx in 0..ss as usize {
                    let index = (y * ss as usize + sy) * sw + x * ss as usize + sx;
                    if depth[index] == f32::NEG_INFINITY {
                        // Premultiplied, so a transparent background adds nothing.
                        for (s, b) in sum.iter_mut().zip(bg_linear) {
                            *s += b * bg_alpha;
                        }
                        alpha += bg_alpha;
                    } else {
                        for (s, c) in sum.iter_mut().zip(color[index]) {
                            *s += c;
                        }
                        alpha += 1.0;
                    }
                }
            }
            let a = alpha / count;
            if a <= 0.0 {
                out.extend([0, 0, 0, 0]);
                continue;
            }
            out.extend(sum.map(|s| to_srgb(s / alpha)));
            out.push((a * 255.0 + 0.5) as u8);
        }
    }
    out
}

/// Render and write a PNG, creating its folder.
pub fn write(
    session: &Session,
    preview: &Preview,
    path: &std::path::Path,
) -> Result<Frame, String> {
    let frame = render(session, preview)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
    }
    crate::trace::write_rgba_png(path, preview.width, preview.height, &frame.rgba)?;
    Ok(frame)
}

#[cfg(test)]
#[path = "../tests/preview.rs"]
mod tests;
