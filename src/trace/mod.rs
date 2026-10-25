//! CPU path tracer for Render mode.
//!
//! The window prefers the wgpu compute tracer. This CPU path is the fallback
//! when a scene does not fit in one grid, and it is what the tests run.
//! Atmospheric sky, metal, and glass match `trace.wgsl`. With the `oidn`
//! feature, a finished frame is denoised by Open Image Denoise.

use std::collections::HashMap;

use glam::Vec3;

use crate::state::{GlassMode, Material, MaterialType, Session};

const FOV_Y: f32 = 50.0_f32.to_radians();
const MAX_STEPS: i32 = 384;

#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    /// Next scanline of the sample being drawn. Wraps to zero when a sample finishes.
    row: u32,
    sum: Vec<[f32; 3]>,
    shown: Vec<[f32; 3]>,
    albedo: Vec<[f32; 3]>,
    /// First-hit geometric normal. Zero when the primary ray misses.
    normal: Vec<[f32; 3]>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        let n = (width as usize) * (height as usize);
        Self {
            width,
            height,
            samples: 0,
            row: 0,
            sum: vec![[0.0; 3]; n],
            shown: vec![[0.0; 3]; n],
            albedo: vec![[0.0; 3]; n],
            normal: vec![[0.0; 3]; n],
        }
    }

    /// Mean of the accumulated samples. A finished frame has `samples` greater than zero.
    pub fn mean(&self) -> Vec<[f32; 3]> {
        let n = (self.samples.max(1)) as f32;
        self.sum
            .iter()
            .map(|color| scale(*color, 1.0 / n))
            .collect()
    }

    pub fn albedo(&self) -> &[[f32; 3]] {
        &self.albedo
    }

    pub fn normal(&self) -> &[[f32; 3]] {
        &self.normal
    }

    pub fn matches(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }
}

/// Changes when the picture must start over: voxels, camera, lights, materials, size.
pub fn frame_key(session: &Session, size: [u32; 2]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mix = |hash: &mut u64, bits: u64| {
        *hash ^= bits;
        *hash = hash.wrapping_mul(0x100000001b3);
    };
    mix(&mut hash, session.mesh_epoch);
    mix(&mut hash, size[0] as u64);
    mix(&mut hash, size[1] as u64);
    mix(&mut hash, session.yaw.to_bits() as u64);
    mix(&mut hash, session.pitch.to_bits() as u64);
    mix(&mut hash, session.distance.to_bits() as u64);
    mix(&mut hash, session.target.x.to_bits() as u64);
    mix(&mut hash, session.target.y.to_bits() as u64);
    mix(&mut hash, session.target.z.to_bits() as u64);
    mix(&mut hash, session.max_bounces as u64);
    mix(&mut hash, session.stochastic_gi as u64);
    mix(&mut hash, session.aperture.to_bits() as u64);
    mix(&mut hash, session.focus_distance.to_bits() as u64);
    mix(&mut hash, session.sun_elevation.to_bits() as u64);
    mix(&mut hash, session.sun_azimuth.to_bits() as u64);
    mix(&mut hash, session.sun_disc.to_bits() as u64);
    mix(&mut hash, session.sun_intensity.to_bits() as u64);
    mix(&mut hash, session.sky_intensity.to_bits() as u64);
    mix(&mut hash, session.horizon_softness.to_bits() as u64);
    mix(&mut hash, session.indirect_clamp.to_bits() as u64);
    mix(&mut hash, session.ema_floor.to_bits() as u64);
    mix(&mut hash, session.nee_ris as u64);
    mix(&mut hash, session.power_weighted_nee as u64);
    mix(&mut hash, session.atmospheric_sky as u64);
    mix(
        &mut hash,
        match session.glass_mode {
            GlassMode::Probabilistic => 0,
            GlassMode::Absorption => 1,
        },
    );
    for (slot, material) in session.materials.iter().enumerate() {
        mix(&mut hash, slot as u64);
        mix(
            &mut hash,
            match material.ty {
                MaterialType::Diffuse => 0,
                MaterialType::Metal => 1,
                MaterialType::Emissive => 2,
                MaterialType::Glass => 3,
            },
        );
        mix(&mut hash, material.roughness.to_bits() as u64);
        mix(&mut hash, material.metallic.to_bits() as u64);
        mix(&mut hash, material.emission.to_bits() as u64);
        mix(&mut hash, material.emission_power as u64);
        mix(&mut hash, material.transparency.to_bits() as u64);
        mix(&mut hash, material.ior.to_bits() as u64);
    }
    for rgba in &session.palette {
        mix(&mut hash, u32::from_le_bytes(*rgba) as u64);
    }
    hash
}

/// Voxels, materials, and palette. The camera is not part of it, so an orbit
/// does not rebuild the grid.
pub fn scene_key(session: &Session) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mix = |hash: &mut u64, bits: u64| {
        *hash ^= bits;
        *hash = hash.wrapping_mul(0x100000001b3);
    };
    mix(&mut hash, session.mesh_epoch);
    for (slot, material) in session.materials.iter().enumerate() {
        mix(&mut hash, slot as u64);
        mix(
            &mut hash,
            match material.ty {
                MaterialType::Diffuse => 0,
                MaterialType::Metal => 1,
                MaterialType::Emissive => 2,
                MaterialType::Glass => 3,
            },
        );
        mix(&mut hash, material.roughness.to_bits() as u64);
        mix(&mut hash, material.metallic.to_bits() as u64);
        mix(&mut hash, material.emission.to_bits() as u64);
        mix(&mut hash, material.emission_power as u64);
        mix(&mut hash, material.transparency.to_bits() as u64);
        mix(&mut hash, material.ior.to_bits() as u64);
    }
    for rgba in &session.palette {
        mix(&mut hash, u32::from_le_bytes(*rgba) as u64);
    }
    hash
}

struct World {
    grid: Grid,
    lights: Vec<Light>,
    min: [i32; 3],
    max: [i32; 3],
    count: usize,
}

enum Grid {
    /// Packed occupancy. Index 0 in the palette is empty, so a zero byte is air.
    Dense {
        origin: [i32; 3],
        dims: [usize; 3],
        cells: Vec<u8>,
    },
    Sparse(HashMap<(i32, i32, i32), u8>),
}

/// Everything a sample needs, owned so a worker thread can keep tracing
/// after the window has moved on to the next input.
struct Shade {
    materials: [Material; 256],
    palette: [[u8; 4]; 256],
    max_bounces: i32,
    stochastic_gi: bool,
    glass_mode: GlassMode,
    aperture: f32,
    focus_distance: f32,
    power_weighted_nee: bool,
    nee_ris: i32,
    indirect_clamp: f32,
    ema_floor: f32,
    atmospheric_sky: bool,
    sky_intensity: f32,
    horizon_softness: f32,
    sun_elevation: f32,
    sun_azimuth: f32,
    sun_disc: f32,
    sun_intensity: f32,
    sun_color: [f32; 3],
    eye: Vec3,
    target: Vec3,
    orthographic: bool,
}

impl Shade {
    fn from_session(session: &Session) -> Self {
        Self {
            materials: session.materials,
            palette: session.palette,
            max_bounces: session.max_bounces,
            stochastic_gi: session.stochastic_gi,
            glass_mode: session.glass_mode,
            aperture: session.aperture,
            focus_distance: session.focus_distance,
            power_weighted_nee: session.power_weighted_nee,
            nee_ris: session.nee_ris,
            indirect_clamp: session.indirect_clamp,
            ema_floor: session.ema_floor,
            atmospheric_sky: session.atmospheric_sky,
            sky_intensity: session.sky_intensity,
            horizon_softness: session.horizon_softness,
            sun_elevation: session.sun_elevation,
            sun_azimuth: session.sun_azimuth,
            sun_disc: session.sun_disc,
            sun_intensity: session.sun_intensity,
            sun_color: session.sun_color,
            eye: session.eye(),
            target: session.target,
            orthographic: session.orthographic,
        }
    }

    fn sun_dir(&self) -> Vec3 {
        let e = self.sun_elevation.to_radians();
        let a = self.sun_azimuth.to_radians();
        let dir = Vec3::new(a.cos() * e.cos(), a.sin() * e.cos(), e.sin()).normalize_or_zero();
        if dir.length_squared() < 1.0e-6 {
            Vec3::new(0.3, 0.2, 0.9).normalize()
        } else {
            dir
        }
    }
}

struct Light {
    point: Vec3,
    normal: Vec3,
    radiance: [f32; 3],
    power: f32,
}

struct Hit {
    point: Vec3,
    normal: Vec3,
    slot: u8,
}

struct Rng(u32);

impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(747796405).wrapping_add(2891336453);
        let word = ((self.0 >> ((self.0 >> 28) + 4)) ^ self.0).wrapping_mul(277803737);
        let bits = (word >> 22) ^ word;
        (bits >> 8) as f32 / 16_777_216.0
    }
}

/// Trace `rows` scanlines into `image`. Returns true when another sample was just finished.
pub fn trace_rows(image: &mut Image, session: &Session, rows: u32) -> bool {
    if image.width == 0 || image.height == 0 || rows == 0 {
        return false;
    }
    let world = build_world(session);
    let shade = Shade::from_session(session);
    let finished_before = image.samples;
    let mut left = rows;
    while left > 0 {
        let y = image.row;
        let sample = image.samples;
        for x in 0..image.width {
            let (color, albedo, normal) =
                shade_pixel(&world, &shade, image.width, image.height, x, y, sample, 0);
            store_pixel(image, &shade, x, y, sample, color, albedo, normal);
        }
        image.row += 1;
        if image.row >= image.height {
            image.row = 0;
            image.samples += 1;
        }
        left -= 1;
    }
    image.samples > finished_before
}

/// Path-trace a finished frame. `seed` is mixed into the sampler; `0` matches
/// one sample of [`trace_rows`].
pub fn render_still(session: &Session, width: u32, height: u32, samples: u32, seed: u32) -> Image {
    let mut image = Image::new(width, height);
    if width == 0 || height == 0 || samples == 0 {
        return image;
    }
    let world = build_world(session);
    let shade = Shade::from_session(session);
    for sample in 0..samples {
        for y in 0..height {
            for x in 0..width {
                let (color, albedo, normal) =
                    shade_pixel(&world, &shade, width, height, x, y, sample, seed);
                store_pixel(&mut image, &shade, x, y, sample, color, albedo, normal);
            }
        }
        image.row = 0;
        image.samples = sample + 1;
    }
    image
}

fn shade_pixel(
    world: &World,
    shade: &Shade,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    sample: u32,
    seed: u32,
) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let mut rng = Rng(hash(x, y, sample).wrapping_add(seed));
    let (origin, dir) = if shade.orthographic {
        let (origin, dir) = ortho_primary(width, height, x, y, shade.eye, shade.target, &mut rng);
        if shade.aperture <= 0.0 {
            (origin, dir)
        } else {
            lens_ray(shade, origin, origin + dir, dir, &mut rng)
        }
    } else {
        let center = primary_dir(width, height, x, y, shade.eye, shade.target, &mut rng);
        lens_ray(shade, shade.eye, shade.target, center, &mut rng)
    };
    let color = radiance(world, shade, origin, dir, 0, &mut rng);
    let (albedo, normal) = if sample == 0 {
        first_guides(world, shade, origin, dir)
    } else {
        ([0.0; 3], [0.0; 3])
    };
    (color, albedo, normal)
}

fn store_pixel(
    image: &mut Image,
    shade: &Shade,
    x: u32,
    y: u32,
    sample: u32,
    color: [f32; 3],
    albedo: [f32; 3],
    normal: [f32; 3],
) {
    let i = (y as usize) * (image.width as usize) + x as usize;
    if sample == 0 {
        image.albedo[i] = albedo;
        image.normal[i] = normal;
    }
    image.sum[i] = add(image.sum[i], color);
    image.shown[i] = display_color(shade, image.sum[i], image.shown[i], color, sample);
}

fn display_color(
    shade: &Shade,
    sum: [f32; 3],
    shown: [f32; 3],
    color: [f32; 3],
    sample: u32,
) -> [f32; 3] {
    let n = (sample + 1) as f32;
    let weight = (1.0 / n).max(shade.ema_floor.clamp(0.0, 1.0));
    let blended = if sample == 0 {
        color
    } else {
        lerp(shown, color, weight)
    };
    // Keep the unbiased average when the floor is zero. A positive floor
    // blends with the previous frames and stays noisier.
    if shade.ema_floor <= 0.0 {
        scale(sum, 1.0 / n)
    } else {
        blended
    }
}

/// The viewport's own pixels, scaled so the long side is at most `cap`.
/// The aspect stays, so the render frames the same picture as the edit view.
pub fn view_size(viewport: [u32; 2], cap: u32) -> [u32; 2] {
    let w = viewport[0].max(1) as f32;
    let h = viewport[1].max(1) as f32;
    let long = w.max(h);
    let scale = if long > cap.max(1) as f32 {
        cap.max(1) as f32 / long
    } else {
        1.0
    };
    [
        (w * scale).round().max(1.0) as u32,
        (h * scale).round().max(1.0) as u32,
    ]
}

/// Linear color after the active denoiser, or the accumulated picture.
/// Open Image Denoise runs once the sample cap is reached. Before that the
/// raw accumulation is shown, so a live preview is not denoised every frame.
pub fn filtered(image: &Image, session: &Session) -> Result<Vec<[f32; 3]>, String> {
    if session.denoise && crate::denoise::available() {
        if image.samples >= session.render_sample_cap() && image.row == 0 {
            crate::denoise::denoise_image(image)
        } else {
            Ok(image.shown.clone())
        }
    } else if session.denoise {
        Ok(denoise(image))
    } else {
        Ok(image.shown.clone())
    }
}

/// Tonemapped RGBA8, top row first. A denoiser error leaves the raw frame;
/// [`encode_png`] reports that error.
pub fn pixels(image: &Image, session: &Session) -> Vec<u8> {
    let linear = filtered(image, session).unwrap_or_else(|_| image.shown.clone());
    tonemap_rgba(&linear, session.aces)
}

pub fn encode_png(image: &Image, session: &Session) -> Result<Vec<u8>, String> {
    if image.samples == 0 && image.row == 0 {
        return Err("Render has no samples yet.".into());
    }
    let linear = filtered(image, session)?;
    let mut rgba = tonemap_rgba(&linear, session.aces);
    if session.show_bones {
        draw_bone_overlay(&mut rgba, image.width, image.height, session);
    }
    encode_rgba(image.width, image.height, &rgba)
}

pub fn write_png(path: &std::path::Path, image: &Image, session: &Session) -> Result<(), String> {
    let bytes = encode_png(image, session)?;
    std::fs::write(path, bytes).map_err(|err| format!("Could not write {}: {err}", path.display()))
}

pub fn write_rgba_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), String> {
    let bytes = encode_rgba(width, height, rgba)?;
    std::fs::write(path, bytes).map_err(|err| format!("Could not write {}: {err}", path.display()))
}

pub(crate) fn tonemap_rgba(linear: &[[f32; 3]], aces_on: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(linear.len() * 4);
    for color in linear {
        let mapped = if aces_on {
            [aces(color[0]), aces(color[1]), aces(color[2])]
        } else {
            [
                color[0].clamp(0.0, 1.0),
                color[1].clamp(0.0, 1.0),
                color[2].clamp(0.0, 1.0),
            ]
        };
        out.push(to_srgb(mapped[0]));
        out.push(to_srgb(mapped[1]));
        out.push(to_srgb(mapped[2]));
        out.push(255);
    }
    out
}

pub fn albedo_rgba(colors: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(colors.len() * 4);
    for color in colors {
        out.push(to_srgb(color[0]));
        out.push(to_srgb(color[1]));
        out.push(to_srgb(color[2]));
        out.push(255);
    }
    out
}

pub fn normal_rgba(normals: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(normals.len() * 4);
    for normal in normals {
        if *normal == [0.0, 0.0, 0.0] {
            out.extend_from_slice(&[0, 0, 0, 255]);
            continue;
        }
        out.push(to_srgb(normal[0] * 0.5 + 0.5));
        out.push(to_srgb(normal[1] * 0.5 + 0.5));
        out.push(to_srgb(normal[2] * 0.5 + 0.5));
        out.push(255);
    }
    out
}

fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("Could not write PNG: {err}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|err| format!("Could not write PNG: {err}"))?;
    writer
        .finish()
        .map_err(|err| format!("Could not write PNG: {err}"))?;
    Ok(bytes)
}

pub(super) fn emission_gain(material: &Material) -> f32 {
    material.emission * (material.emission_power.max(1) as f32) * 8.0
}

pub(super) fn hash(x: u32, y: u32, sample: u32) -> u32 {
    let mut n = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263));
    n = n.wrapping_add(sample.wrapping_mul(1440678137));
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}

pub(super) fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

pub(super) fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(super) fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(scale(a, 1.0 - t), scale(b, t))
}

pub(crate) fn linear_channel(channel: u8) -> f32 {
    let encoded = channel as f32 / 255.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn draw_bone_overlay(rgba: &mut [u8], width: u32, height: u32, session: &Session) {
    if session.skeleton.bones.is_empty() {
        return;
    }
    let Ok(joints) = crate::rig::posed_joints(&session.skeleton, &session.pose) else {
        return;
    };
    let eye = session.eye();
    let (forward, right, up) = camera_basis(eye, session.target);
    let project = |point: Vec3| -> Option<(i32, i32)> {
        let rel = point - eye;
        let z = rel.dot(forward);
        if z <= 0.05 {
            return None;
        }
        let x = rel.dot(right);
        let y = rel.dot(up);
        let aspect = width as f32 / height.max(1) as f32;
        let tan = (FOV_Y * 0.5).tan();
        let (u, v) = if session.orthographic {
            let half_h = session.distance * tan;
            let half_w = half_h * aspect;
            if half_w <= 1.0e-6 || half_h <= 1.0e-6 {
                return None;
            }
            (x / half_w, y / half_h)
        } else {
            (x / (z * aspect * tan), y / (z * tan))
        };
        let px = ((u + 1.0) * 0.5 * width as f32).round() as i32;
        let py = ((1.0 - v) * 0.5 * height as f32).round() as i32;
        Some((px, py))
    };
    let pixels: Vec<Option<(i32, i32)>> = joints.iter().map(|(point, _)| project(*point)).collect();
    for (index, pixel) in pixels.iter().enumerate() {
        let Some(pixel) = *pixel else {
            continue;
        };
        if let Some(parent) = session.skeleton.parent_index(index) {
            if let Some(from) = pixels.get(parent).copied().flatten() {
                bone_line(rgba, width, height, from, pixel);
            }
        }
        for y in pixel.1 - 1..=pixel.1 + 1 {
            for x in pixel.0 - 1..=pixel.0 + 1 {
                bone_pixel(rgba, width, height, x, y);
            }
        }
    }
}

fn bone_pixel(rgba: &mut [u8], width: u32, height: u32, x: i32, y: i32) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let index = (y as u32 * width + x as u32) as usize * 4;
    rgba[index] = 255;
    rgba[index + 1] = 191;
    rgba[index + 2] = 51;
    rgba[index + 3] = 255;
}

fn bone_line(rgba: &mut [u8], width: u32, height: u32, from: (i32, i32), to: (i32, i32)) {
    let (mut x0, mut y0) = from;
    let (x1, y1) = to;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    loop {
        bone_pixel(rgba, width, height, x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice = 2 * err;
        if twice > -dy {
            err -= dy;
            x0 += sx;
        }
        if twice < dx {
            err += dx;
            y0 += sy;
        }
    }
}

pub(crate) fn linear_rgb(rgba: [u8; 4]) -> [f32; 3] {
    [
        linear_channel(rgba[0]),
        linear_channel(rgba[1]),
        linear_channel(rgba[2]),
    ]
}

mod shade;
mod world;

use shade::{
    aces, camera_basis, denoise, first_guides, lens_ray, ortho_primary, primary_dir, radiance,
    to_srgb,
};
use world::build_world;
pub use world::{gpu_scene, GpuLight, GpuScene};

#[cfg(test)]
#[path = "../../tests/trace.rs"]
mod tests;
