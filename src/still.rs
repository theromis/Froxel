//! Shared still-frame settings for `froxel render` and MCP `render`.

use crate::state::{GlassMode, Session};

#[cfg(test)]
use crate::trace::{self, Image};

/// Default fraction of the frame left empty on each side of a fitted box.
pub const MARGIN: f32 = 0.05;

#[derive(Clone)]
pub struct Still {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub bounces: i32,
    pub seed: u32,
    pub aces: bool,
    pub denoise: bool,
    pub atmospheric: Option<bool>,
    pub sky_intensity: Option<f32>,
    pub sun_intensity: Option<f32>,
    pub aperture: Option<f32>,
    pub focus: Option<f32>,
    pub glass: Option<GlassMode>,
    pub stochastic_gi: Option<bool>,
    pub yaw: f32,
    pub pitch: f32,
    /// Multiple of the fitted distance. Below 1 is closer.
    pub distance: f32,
    pub orthographic: Option<bool>,
    /// What to frame. `None` frames every visible posed cell.
    pub frame: Option<crate::trace::Frame>,
    /// Fraction of the frame left empty on each side of the fitted box.
    pub margin: f32,
    /// Orthographic scale instead of the fitted distance.
    pub pixels_per_voxel: Option<f32>,
}

pub(crate) struct Saved {
    yaw: f32,
    pitch: f32,
    distance: f32,
    target: glam::Vec3,
    max_bounces: i32,
    max_samples: i32,
    aces: bool,
    denoise: bool,
    atmospheric_sky: bool,
    sky_intensity: f32,
    sun_intensity: f32,
    aperture: f32,
    focus_distance: f32,
    glass_mode: GlassMode,
    stochastic_gi: bool,
    orthographic: bool,
}

fn apply(session: &mut Session, still: &Still) -> Saved {
    let saved = Saved {
        yaw: session.yaw,
        pitch: session.pitch,
        distance: session.distance,
        target: session.target,
        max_bounces: session.max_bounces,
        max_samples: session.max_samples,
        aces: session.aces,
        denoise: session.denoise,
        atmospheric_sky: session.atmospheric_sky,
        sky_intensity: session.sky_intensity,
        sun_intensity: session.sun_intensity,
        aperture: session.aperture,
        focus_distance: session.focus_distance,
        glass_mode: session.glass_mode,
        stochastic_gi: session.stochastic_gi,
        orthographic: session.orthographic,
    };
    session.yaw = still.yaw.to_radians();
    session.pitch = still.pitch.to_radians();
    session.max_bounces = still.bounces;
    session.max_samples = still.samples as i32;
    session.aces = still.aces;
    session.denoise = still.denoise;
    if let Some(value) = still.atmospheric {
        session.atmospheric_sky = value;
    }
    if let Some(value) = still.sky_intensity {
        session.sky_intensity = value;
    }
    if let Some(value) = still.sun_intensity {
        session.sun_intensity = value;
    }
    if let Some(value) = still.aperture {
        session.aperture = value;
    }
    if let Some(value) = still.focus {
        session.focus_distance = value;
    }
    if let Some(mode) = still.glass {
        session.glass_mode = mode;
    }
    if let Some(value) = still.stochastic_gi {
        session.stochastic_gi = value;
    }
    if let Some(value) = still.orthographic {
        session.orthographic = value;
    }
    fit(session, still, true);
    saved
}

/// Fit every `(yaw, pitch)` in degrees. One angle centres its silhouette;
/// several keep the farthest distance and the middle of the bounds, so a
/// turnaround shares one centre and one scale. Leaves the first angle set.
pub fn fit_all(session: &mut Session, still: &Still, angles: &[(f32, f32)]) {
    let recentre = angles.len() == 1;
    let mut distance: f32 = 0.0;
    for (yaw, pitch) in angles.iter().rev() {
        session.yaw = yaw.to_radians();
        session.pitch = pitch.to_radians();
        fit(session, still, recentre);
        distance = distance.max(session.distance);
    }
    session.distance = distance;
}

/// Frame `still.frame`, or every visible posed cell, at the session's yaw
/// and pitch so it fills the picture less `still.margin` on each side, then
/// scale the distance by `still.distance`. Call again after the yaw, pitch,
/// or pose changes.
pub fn fit(session: &mut Session, still: &Still, recentre: bool) {
    let cells;
    let frame = match &still.frame {
        Some(frame) => frame,
        None => match crate::trace::Frame::cells(session, None) {
            Some(found) => {
                cells = found;
                &cells
            }
            None => {
                session.target = glam::Vec3::ZERO;
                session.distance = 24.0;
                return;
            }
        },
    };
    let aspect = still.width as f32 / still.height.max(1) as f32;
    let fill = (1.0 - 2.0 * still.margin).clamp(0.05, 1.0);
    crate::trace::fit_frame(session, frame, aspect, fill, recentre);
    match still.pixels_per_voxel {
        Some(ppv) if session.orthographic => {
            session.distance = crate::trace::ortho_distance_for(ppv, still.height);
        }
        _ => session.distance *= still.distance,
    }
}

fn restore(session: &mut Session, saved: Saved) {
    session.yaw = saved.yaw;
    session.pitch = saved.pitch;
    session.distance = saved.distance;
    session.target = saved.target;
    session.max_bounces = saved.max_bounces;
    session.max_samples = saved.max_samples;
    session.aces = saved.aces;
    session.denoise = saved.denoise;
    session.atmospheric_sky = saved.atmospheric_sky;
    session.sky_intensity = saved.sky_intensity;
    session.sun_intensity = saved.sun_intensity;
    session.aperture = saved.aperture;
    session.focus_distance = saved.focus_distance;
    session.glass_mode = saved.glass_mode;
    session.stochastic_gi = saved.stochastic_gi;
    session.orthographic = saved.orthographic;
}

#[cfg(test)]
pub fn render_cpu(session: &mut Session, still: &Still) -> Image {
    let saved = apply(session, still);
    let image = trace::render_still(
        session,
        still.width,
        still.height,
        still.samples,
        still.seed,
    );
    restore(session, saved);
    image
}

pub fn prepare(session: &mut Session, still: &Still) -> Saved {
    apply(session, still)
}

pub fn finish(session: &mut Session, saved: Saved) {
    restore(session, saved);
}
