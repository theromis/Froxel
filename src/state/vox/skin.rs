//! The `MVSK` chunk: each object's skin settings (`tools/skin.rs`), by
//! object name. The bricks are derived from the cells, so only the settings
//! are stored. MagicaVoxel skips the chunk.
//!
//! Version 1 body: version, object count, then per object its name, the
//! axis code (0 auto, 1 x, 2 y, 3 z), length min and max, width, height,
//! depth, stagger, relief, tilt, follow, gap, smooth, thin (f32 each), the
//! bare slot count and slots, and the seed.

use super::*;
use crate::tools::{Skin, SkinAxis};

pub(crate) const MVSK_VERSION: u32 = 1;

pub(crate) fn encode(objects: &[Volume]) -> Option<Vec<u8>> {
    let kept: Vec<(&str, &Skin)> = objects
        .iter()
        .filter_map(|o| o.skin.as_ref().map(|s| (o.name.as_str(), s)))
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut body = Vec::new();
    body.extend_from_slice(&MVSK_VERSION.to_le_bytes());
    body.extend_from_slice(&(kept.len() as u32).to_le_bytes());
    for (name, skin) in kept {
        body.extend_from_slice(&(name.len() as u32).to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        body.push(skin.axis.code());
        for v in [
            skin.length[0],
            skin.length[1],
            skin.width,
            skin.height,
            skin.depth,
            skin.stagger,
            skin.relief,
            skin.tilt,
            skin.follow,
            skin.gap,
            skin.smooth,
            skin.thin,
        ] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.push(skin.bare.len().min(255) as u8);
        body.extend_from_slice(&skin.bare[..skin.bare.len().min(255)]);
        body.extend_from_slice(&skin.seed.to_le_bytes());
    }
    Some(body)
}

fn take<'a>(body: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], String> {
    let slice = body
        .get(*at..*at + n)
        .ok_or_else(|| "MVSK chunk ends early".to_string())?;
    *at += n;
    Ok(slice)
}

fn read_u32(body: &[u8], at: &mut usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(take(body, at, 4)?.try_into().unwrap()))
}

fn read_f32(body: &[u8], at: &mut usize) -> Result<f32, String> {
    Ok(f32::from_le_bytes(take(body, at, 4)?.try_into().unwrap()))
}

pub(crate) fn decode(body: &[u8]) -> Result<Vec<(String, Skin)>, String> {
    let mut at = 0usize;
    let version = read_u32(body, &mut at)?;
    if version != MVSK_VERSION {
        return Err(format!("MVSK version {version} is not supported"));
    }
    let count = read_u32(body, &mut at)? as usize;
    if count > 65_536 {
        return Err("MVSK has too many objects".into());
    }
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let len = read_u32(body, &mut at)? as usize;
        if len > 4096 {
            return Err("MVSK name is too long".into());
        }
        let name = String::from_utf8(take(body, &mut at, len)?.to_vec())
            .map_err(|_| "MVSK name is not UTF-8".to_string())?;
        let code = take(body, &mut at, 1)?[0];
        let axis = SkinAxis::from_code(code).ok_or(format!("MVSK axis {code} is unknown"))?;
        let mut f = [0.0f32; 12];
        for v in &mut f {
            *v = read_f32(body, &mut at)?;
        }
        let bare_count = take(body, &mut at, 1)?[0] as usize;
        let bare = take(body, &mut at, bare_count)?.to_vec();
        let skin = Skin {
            axis,
            length: [f[0], f[1]],
            width: f[2],
            height: f[3],
            depth: f[4],
            stagger: f[5],
            relief: f[6],
            tilt: f[7],
            follow: f[8],
            gap: f[9],
            smooth: f[10],
            thin: f[11],
            bare,
            seed: read_u32(body, &mut at)?,
        };
        skin.check().map_err(|e| format!("MVSK {name}: {e}"))?;
        out.push((name, skin));
    }
    Ok(out)
}

/// Gives the loaded objects their skins by name. A name with no object is
/// dropped: a skin needs cells.
pub(super) fn install(objects: &mut [LoadedObject], stored: Vec<(String, Skin)>) {
    for (name, skin) in stored {
        if let Some(object) = objects.iter_mut().find(|o| o.name == name) {
            object.skin = Some(skin);
        }
    }
}
