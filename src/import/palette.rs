//! Colours of imported cells mapped into the document palette.
//!
//! With `slots`, every colour takes the nearest of those slots and the
//! palette is left alone. Otherwise a colour within `tolerance` of a slot
//! reuses it; the rest are reduced by median cut to at most `max_new`
//! colours, each written into a free slot unless it too is within
//! `tolerance` of one, and every colour then takes the nearest slot.

use std::collections::HashMap;

pub(crate) struct PaletteFit<'a> {
    pub palette: &'a [[u8; 4]; 256],
    /// Slots import may overwrite, in the order it takes them.
    pub free: Vec<u8>,
    /// Map only to these slots.
    pub slots: Option<Vec<u8>>,
    pub max_new: usize,
    /// sRGB distance (0..=441) under which a slot is reused.
    pub tolerance: f64,
}

pub(crate) struct Quantized {
    pub map: HashMap<[u8; 3], u8>,
    /// Slots written, with their colours.
    pub new_slots: Vec<(u8, [u8; 3])>,
}

fn distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    (0..3)
        .map(|c| (f64::from(a[c]) - f64::from(b[c])).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn nearest(color: [u8; 3], candidates: &[(u8, [u8; 3])]) -> Option<(u8, f64)> {
    candidates
        .iter()
        .map(|&(slot, rgb)| (slot, distance(color, rgb)))
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
}

/// `colors` are the distinct colours with their cell counts.
pub(crate) fn quantize(colors: &[([u8; 3], usize)], fit: &PaletteFit) -> Result<Quantized, String> {
    let rgb = |slot: u8| {
        let c = fit.palette[slot as usize];
        [c[0], c[1], c[2]]
    };
    if let Some(slots) = &fit.slots {
        if slots.is_empty() {
            return Err("slots must name at least one palette index".into());
        }
        let candidates: Vec<(u8, [u8; 3])> = slots.iter().map(|&s| (s, rgb(s))).collect();
        let map = colors
            .iter()
            .map(|&(color, _)| (color, nearest(color, &candidates).map_or(slots[0], |n| n.0)))
            .collect();
        return Ok(Quantized {
            map,
            new_slots: Vec::new(),
        });
    }
    // Every visible slot may be reused; free ones may also be overwritten.
    let mut candidates: Vec<(u8, [u8; 3])> = (1..=255u8)
        .filter(|&s| fit.palette[s as usize][3] > 0 || fit.free.contains(&s))
        .map(|s| (s, rgb(s)))
        .collect();
    let mut map = HashMap::new();
    let mut taken: Vec<u8> = Vec::new();
    let mut rest = Vec::new();
    let visible: Vec<(u8, [u8; 3])> = candidates
        .iter()
        .copied()
        .filter(|(s, _)| fit.palette[*s as usize][3] > 0)
        .collect();
    for &(color, count) in colors {
        match nearest(color, &visible) {
            Some((slot, d)) if d <= fit.tolerance => {
                map.insert(color, slot);
                taken.push(slot);
            }
            _ => rest.push((color, count)),
        }
    }
    let mut free: Vec<u8> = fit
        .free
        .iter()
        .copied()
        .filter(|s| !taken.contains(s))
        .collect();
    free.reverse();
    let mut new_slots = Vec::new();
    let room = fit.max_new.min(free.len());
    if !rest.is_empty() && room > 0 {
        for centroid in median_cut(&rest, room) {
            let reuse = nearest(centroid, &candidates).filter(|(slot, d)| {
                *d <= fit.tolerance
                    && (fit.palette[*slot as usize][3] > 0
                        || new_slots.iter().any(|(s, _)| s == slot))
            });
            if reuse.is_some() {
                continue;
            }
            let Some(slot) = free.pop() else { break };
            new_slots.push((slot, centroid));
            for entry in &mut candidates {
                if entry.0 == slot {
                    entry.1 = centroid;
                }
            }
        }
    }
    // The slots a colour may land on: visible ones and the ones just written.
    let usable: Vec<(u8, [u8; 3])> = candidates
        .iter()
        .copied()
        .filter(|(s, _)| fit.palette[*s as usize][3] > 0 || new_slots.iter().any(|(n, _)| n == s))
        .collect();
    if usable.is_empty() {
        return Err("the palette has no visible slot to map colours to".into());
    }
    for (color, _) in rest {
        let slot = nearest(color, &usable).map(|n| n.0).unwrap_or(usable[0].0);
        map.insert(color, slot);
    }
    Ok(Quantized { map, new_slots })
}

/// At most `count` colours standing for `colors`, by median cut weighted by
/// cell count.
fn median_cut(colors: &[([u8; 3], usize)], count: usize) -> Vec<[u8; 3]> {
    let mut boxes: Vec<Vec<([u8; 3], usize)>> = vec![colors.to_vec()];
    while boxes.len() < count {
        let widest = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let (channel, range) = widest_channel(b);
                (i, channel, range)
            })
            .max_by_key(|&(i, _, range)| (range, std::cmp::Reverse(i)));
        let Some((index, channel, range)) = widest else {
            break;
        };
        if range == 0 {
            break;
        }
        let mut group = boxes.swap_remove(index);
        group.sort_by_key(|(c, _)| c[channel]);
        let total: usize = group.iter().map(|(_, n)| n).sum();
        let mut seen = 0;
        let mut split = 1;
        for (i, (_, n)) in group.iter().enumerate() {
            seen += n;
            if seen * 2 >= total {
                split = (i + 1).clamp(1, group.len() - 1);
                break;
            }
        }
        let upper = group.split_off(split);
        boxes.push(group);
        boxes.push(upper);
    }
    boxes
        .iter()
        .map(|group| {
            let total: f64 = group.iter().map(|(_, n)| *n as f64).sum();
            let mut mean = [0.0; 3];
            for (color, n) in group {
                for c in 0..3 {
                    mean[c] += f64::from(color[c]) * *n as f64;
                }
            }
            mean.map(|m| (m / total).round().clamp(0.0, 255.0) as u8)
        })
        .collect()
}

fn widest_channel(group: &[([u8; 3], usize)]) -> (usize, u8) {
    (0..3)
        .map(|c| {
            let lo = group.iter().map(|(color, _)| color[c]).min().unwrap_or(0);
            let hi = group.iter().map(|(color, _)| color[c]).max().unwrap_or(0);
            (c, hi - lo)
        })
        .max_by_key(|&(c, range)| (range, std::cmp::Reverse(c)))
        .unwrap_or((0, 0))
}
