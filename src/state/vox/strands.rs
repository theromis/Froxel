//! The `MVST` chunk: each object's strands (`tools/strand.rs`), by object
//! name, as their definitions. MagicaVoxel skips the chunk.
//!
//! Version 1 body: version, object count, then per object its name, whether
//! it has cells and their local minimum (XYZI rebases local cells to the
//! SIZE corner, so reopening shifts the strands by the same amount), its
//! translation (a strands-only object has no model to carry it), and its
//! strands: point count, points (x, y, z, twist, scale as f32), smooth,
//! width, thickness, step, slot count, slots, pattern, period, seed.

use super::*;
use crate::tools::{Strand, StrandPattern, StrandPoint, MAX_STRANDS, MAX_STRAND_POINTS};

pub(crate) const MVST_VERSION: u32 = 1;

pub(crate) struct StoredStrands {
    pub name: String,
    pub local_min: Option<Cell>,
    pub translation: Cell,
    pub strands: Vec<Strand>,
}

pub(crate) fn encode(objects: &[Volume]) -> Option<Vec<u8>> {
    let kept: Vec<&Volume> = objects.iter().filter(|o| !o.strands.is_empty()).collect();
    if kept.is_empty() {
        return None;
    }
    let mut body = Vec::new();
    push_u32(&mut body, MVST_VERSION);
    push_u32(&mut body, kept.len() as u32);
    for object in kept {
        push_str(&mut body, &object.name);
        match occupied_bounds(&object.cells) {
            Some((min, _)) => {
                body.push(1);
                for v in [min.0, min.1, min.2] {
                    push_i32(&mut body, v);
                }
            }
            None => body.push(0),
        }
        for v in [
            object.translation.0,
            object.translation.1,
            object.translation.2,
        ] {
            push_i32(&mut body, v);
        }
        push_u32(&mut body, object.strands.len() as u32);
        for strand in &object.strands {
            push_u32(&mut body, strand.points.len() as u32);
            for point in &strand.points {
                for v in point.at {
                    push_f32(&mut body, v);
                }
                push_f32(&mut body, point.twist);
                push_f32(&mut body, point.scale);
            }
            body.push(u8::from(strand.smooth));
            push_f32(&mut body, strand.width);
            push_f32(&mut body, strand.thickness);
            push_f32(&mut body, strand.step);
            body.push(strand.slots.len() as u8);
            body.extend_from_slice(&strand.slots);
            body.push(match strand.pattern {
                StrandPattern::Gradient => 0,
                StrandPattern::Rings => 1,
                StrandPattern::Random => 2,
            });
            push_f32(&mut body, strand.period);
            push_u32(&mut body, strand.seed);
        }
    }
    Some(body)
}

pub(crate) fn decode(body: &[u8]) -> Result<Vec<StoredStrands>, String> {
    let mut at = 0usize;
    let version = read_u32(body, &mut at)?;
    if version != MVST_VERSION {
        return Err(format!("MVST version {version} is not supported"));
    }
    let count = read_u32(body, &mut at)? as usize;
    if count > 65_536 {
        return Err("MVST has too many objects".into());
    }
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let name = read_str(body, &mut at)?;
        let local_min = match read_u8(body, &mut at)? {
            0 => None,
            1 => Some((
                read_i32(body, &mut at)?,
                read_i32(body, &mut at)?,
                read_i32(body, &mut at)?,
            )),
            _ => return Err("MVST cell flag is not 0 or 1".into()),
        };
        let translation = (
            read_i32(body, &mut at)?,
            read_i32(body, &mut at)?,
            read_i32(body, &mut at)?,
        );
        let strand_count = read_u32(body, &mut at)? as usize;
        if strand_count > MAX_STRANDS {
            return Err("MVST object has too many strands".into());
        }
        let mut strands = Vec::with_capacity(strand_count);
        for _ in 0..strand_count {
            let point_count = read_u32(body, &mut at)? as usize;
            if point_count > MAX_STRAND_POINTS {
                return Err("MVST strand has too many points".into());
            }
            let mut points = Vec::with_capacity(point_count);
            for _ in 0..point_count {
                let at3 = [
                    read_f32(body, &mut at)?,
                    read_f32(body, &mut at)?,
                    read_f32(body, &mut at)?,
                ];
                points.push(StrandPoint {
                    at: at3,
                    twist: read_f32(body, &mut at)?,
                    scale: read_f32(body, &mut at)?,
                });
            }
            let smooth = match read_u8(body, &mut at)? {
                0 => false,
                1 => true,
                _ => return Err("MVST smooth flag is not 0 or 1".into()),
            };
            let width = read_f32(body, &mut at)?;
            let thickness = read_f32(body, &mut at)?;
            let step = read_f32(body, &mut at)?;
            let slot_count = read_u8(body, &mut at)? as usize;
            let slots = body
                .get(at..at + slot_count)
                .ok_or("MVST ends inside a strand's slots")?
                .to_vec();
            at += slot_count;
            let pattern = match read_u8(body, &mut at)? {
                0 => StrandPattern::Gradient,
                1 => StrandPattern::Rings,
                2 => StrandPattern::Random,
                other => return Err(format!("MVST pattern {other} is unknown")),
            };
            let strand = Strand {
                points,
                smooth,
                width,
                thickness,
                step,
                slots,
                pattern,
                period: read_f32(body, &mut at)?,
                seed: read_u32(body, &mut at)?,
            };
            strand.check().map_err(|e| format!("MVST strand: {e}"))?;
            strands.push(strand);
        }
        out.push(StoredStrands {
            name,
            local_min,
            translation,
            strands,
        });
    }
    Ok(out)
}

/// Gives the loaded objects their strands. An object found by name with
/// cells moves its strands by the shift XYZI gave its cells; a name with no
/// object becomes a strands-only object at its saved translation.
pub(super) fn install(objects: &mut Vec<LoadedObject>, stored: Vec<StoredStrands>) {
    for entry in stored {
        if let Some(object) = objects.iter_mut().find(|o| o.name == entry.name) {
            let loaded = occupied_bounds(&object.cells).map(|(min, _)| min);
            object.strands = match (entry.local_min, loaded) {
                (Some(saved), Some(now)) => {
                    let d = [
                        (now.0 as i64 - saved.0 as i64) as f32,
                        (now.1 as i64 - saved.1 as i64) as f32,
                        (now.2 as i64 - saved.2 as i64) as f32,
                    ];
                    entry.strands.iter().map(|s| s.shifted(d)).collect()
                }
                _ => entry.strands,
            };
            if object.cells.is_empty() {
                object.translation = entry.translation;
            }
        } else {
            objects.push(LoadedObject {
                name: entry.name,
                cells: HashMap::new(),
                translation: entry.translation,
                box_min: (0, 0, 0),
                dims: (0, 0, 0),
                rot: ROT_IDENTITY,
                pivot: (0, 0, 0),
                model_dims: (0, 0, 0),
                hidden: false,
                file: None,
                bone: None,
                script: Default::default(),
                strands: entry.strands,
            });
        }
    }
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_str(out: &mut Vec<u8>, text: &str) {
    push_u32(out, text.len() as u32);
    out.extend_from_slice(text.as_bytes());
}

fn take<'a>(body: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], String> {
    let slice = body
        .get(*at..*at + n)
        .ok_or_else(|| "MVST chunk ends early".to_string())?;
    *at += n;
    Ok(slice)
}

fn read_u8(body: &[u8], at: &mut usize) -> Result<u8, String> {
    Ok(take(body, at, 1)?[0])
}

fn read_u32(body: &[u8], at: &mut usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(take(body, at, 4)?.try_into().unwrap()))
}

fn read_i32(body: &[u8], at: &mut usize) -> Result<i32, String> {
    Ok(i32::from_le_bytes(take(body, at, 4)?.try_into().unwrap()))
}

fn read_f32(body: &[u8], at: &mut usize) -> Result<f32, String> {
    Ok(f32::from_le_bytes(take(body, at, 4)?.try_into().unwrap()))
}

fn read_str(body: &[u8], at: &mut usize) -> Result<String, String> {
    let len = read_u32(body, at)? as usize;
    if len > 4096 {
        return Err("MVST name is too long".into());
    }
    String::from_utf8(take(body, at, len)?.to_vec()).map_err(|_| "MVST name is not UTF-8".into())
}
