//! Cavity tint behind MCP `cavity_tint`: depth by shading. Each exposed
//! cell counts the solid cells in a ball around it. A cell on a flat face
//! has about half of the ball solid; one in a crevice, a fissure, or the
//! crook of a root has more (concave), and one on a ridge or a corner has
//! less (convex). Concave cells take a darker slot and, when asked, convex
//! ones a lighter slot: the caller's pairs, or shades made from the cell's
//! own colour in free palette slots.

use std::collections::HashMap;

use super::surface::{allowed, check_unit, step, Exclude, Region, FACES};
use crate::state::{Cell, Material};

/// Widest ball radius.
pub const MAX_CAVITY_RADIUS: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CavitySpec {
    pub region: Region,
    /// Radius of the ball counted around each cell, 1..=4.
    pub radius: i32,
    /// How concave a cell must be to darken, 0..=1 (0 a flat face, 1 a
    /// fully enclosed pocket).
    pub dark: f64,
    /// How convex a cell must be to lighten, 0..=1 (0 a flat face, 1 a lone
    /// spike). `None` lightens nothing.
    pub light: Option<f64>,
    /// Act only on cells of these slots. Empty acts on every slot.
    pub only: Vec<u8>,
    pub exclude: Exclude,
}

/// One exposed cell to tint, with its slot and how strongly it is concave
/// or convex, 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tinted {
    pub cell: Cell,
    pub slot: u8,
    pub tint: Tint,
    pub amount: f64,
}

/// The exposed cells of the region that pass the thresholds, in z, y, x
/// order.
pub fn cavities(cells: &HashMap<Cell, u8>, spec: &CavitySpec) -> Result<Vec<Tinted>, String> {
    if !(1..=MAX_CAVITY_RADIUS).contains(&spec.radius) {
        return Err(format!("radius must be 1..={MAX_CAVITY_RADIUS}"));
    }
    check_unit(spec.dark, "dark")?;
    if let Some(light) = spec.light {
        check_unit(light, "light")?;
    }
    let r = spec.radius;
    let mut ball = Vec::new();
    for dz in -r..=r {
        for dy in -r..=r {
            for dx in -r..=r {
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 > 0 && d2 * 4 <= (2 * r + 1) * (2 * r + 1) {
                    ball.push((dx, dy, dz));
                }
            }
        }
    }
    // A cell on a flat top has every offset at or below its own layer solid;
    // the ball is symmetric, so that share holds for any flat face.
    let flat = ball.iter().filter(|d| d.2 <= 0).count() as f64 / ball.len() as f64;
    let mut out = Vec::new();
    for cell in spec.region.solid(cells) {
        let slot = cells[&cell];
        if spec.exclude.contains(cell)
            || !allowed(&spec.only, slot)
            || FACES.iter().all(|d| cells.contains_key(&step(cell, *d)))
        {
            continue;
        }
        let solid = ball
            .iter()
            .filter(|d| cells.contains_key(&step(cell, **d)))
            .count() as f64
            / ball.len() as f64;
        let (tint, amount) = if solid > flat {
            (Tint::Dark, (solid - flat) / (1.0 - flat))
        } else {
            (Tint::Light, (flat - solid) / flat)
        };
        let threshold = match tint {
            Tint::Dark => Some(spec.dark),
            Tint::Light => spec.light,
        };
        if let Some(threshold) = threshold {
            // A threshold of 0 still leaves a perfectly flat face alone.
            if amount >= threshold && amount > 1.0e-9 {
                out.push(Tinted {
                    cell,
                    slot,
                    tint,
                    amount,
                });
            }
        }
    }
    Ok(out)
}

/// A shade made by `cavity_tint` in a free slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shade {
    pub slot: u8,
    /// The slot it is a shade of; it takes that slot's material.
    pub from: u8,
    pub rgba: [u8; 4],
}

/// The palette as `cavity_tint` sees it when it makes shades.
#[derive(Clone)]
pub struct Shades {
    pub colors: [[u8; 4]; 256],
    pub materials: [Material; 256],
    /// Slots some cell uses or a pending palette write names; a shade never
    /// takes one of these unless it already holds that shade.
    pub used: [bool; 256],
    pub created: Vec<Shade>,
}

impl Shades {
    /// The slot holding `from`'s colour times `factor` with `from`'s
    /// material: an existing slot of exactly that colour and material, or
    /// the highest free slot, which takes it.
    pub fn shade(&mut self, from: u8, factor: f64) -> Result<u8, String> {
        let base = self.colors[from as usize];
        let mut rgba = base;
        for channel in rgba.iter_mut().take(3) {
            *channel = (f64::from(*channel) * factor).round().clamp(0.0, 255.0) as u8;
        }
        if rgba == base {
            return Ok(from);
        }
        let material = self.materials[from as usize];
        if let Some(found) = (1..=255u8)
            .find(|s| self.colors[*s as usize] == rgba && self.materials[*s as usize] == material)
        {
            return Ok(found);
        }
        let free = (1..=255u8)
            .rev()
            .find(|s| !self.used[*s as usize])
            .ok_or("no free palette slot for a shade; pass pairs instead")?;
        self.colors[free as usize] = rgba;
        self.materials[free as usize] = material;
        self.used[free as usize] = true;
        self.created.push(Shade {
            slot: free,
            from,
            rgba,
        });
        Ok(free)
    }

    /// The slot holding exactly `rgb` (opaque) with `from`'s material: an
    /// existing one, or the highest free slot, which takes it. Used by
    /// `paint_from_views` and `reduce_palette` for their new colours.
    pub fn colour(&mut self, rgb: [u8; 3], from: u8) -> Result<u8, String> {
        let rgba = [rgb[0], rgb[1], rgb[2], 255];
        let material = self.materials[from as usize];
        if let Some(found) = (1..=255u8)
            .find(|s| self.colors[*s as usize] == rgba && self.materials[*s as usize] == material)
        {
            self.used[found as usize] = true;
            return Ok(found);
        }
        let free = (1..=255u8)
            .rev()
            .find(|s| !self.used[*s as usize])
            .ok_or("no free palette slot for a new colour: pass slots, or fewer colours")?;
        self.colors[free as usize] = rgba;
        self.materials[free as usize] = material;
        self.used[free as usize] = true;
        self.created.push(Shade {
            slot: free,
            from,
            rgba,
        });
        Ok(free)
    }
}
