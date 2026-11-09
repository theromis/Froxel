//! Colour helpers shared by `paint_from_views` and `reduce_palette`: the
//! Oklab perceptual space, median cut and k-means clustering in it, and the
//! accent guard that keeps a small but distinct colour (an eye, a mouth)
//! from being swallowed by a large cluster.

/// A colour in Oklab: lightness 0..1 and two opponent axes near -0.4..0.4.
pub type Lab = [f32; 3];

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
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
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The Oklab of an sRGB colour.
pub fn oklab(rgb: [u8; 3]) -> Lab {
    let [r, g, b] = rgb.map(to_linear);
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// The sRGB of an Oklab colour, clamped into the gamut.
pub fn srgb(lab: Lab) -> [u8; 3] {
    let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
    let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
    let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        to_srgb(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        to_srgb(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
        to_srgb(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    ]
}

/// Euclidean distance in Oklab (about 0.02 is a just-visible difference,
/// 0.1 a clear one).
pub fn distance(a: Lab, b: Lab) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// The index of the nearest of `centres` to `lab`.
pub fn nearest(lab: Lab, centres: &[Lab]) -> usize {
    let mut best = (0, f32::MAX);
    for (i, centre) in centres.iter().enumerate() {
        let d = distance(lab, *centre);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// The weighted mean of `samples`.
fn mean(samples: &[(Lab, f32)]) -> Lab {
    let mut sum = [0.0f64; 3];
    let mut weight = 0.0f64;
    for (lab, w) in samples {
        for a in 0..3 {
            sum[a] += f64::from(lab[a]) * f64::from(*w);
        }
        weight += f64::from(*w);
    }
    if weight <= 0.0 {
        return [0.0; 3];
    }
    sum.map(|s| (s / weight) as f32)
}

/// Median cut: split the box of samples with the widest weighted spread at
/// the weighted median of its widest axis until there are `k` boxes; the
/// boxes' means are the centres.
pub fn median_cut(samples: &[(Lab, f32)], k: usize) -> Vec<Lab> {
    if samples.is_empty() || k == 0 {
        return Vec::new();
    }
    let mut boxes: Vec<Vec<(Lab, f32)>> = vec![samples.to_vec()];
    while boxes.len() < k {
        // The box whose widest axis, times its weight, is largest.
        let mut pick: Option<(usize, usize, f32)> = None;
        for (i, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            let weight: f32 = b.iter().map(|(_, w)| *w).sum();
            for axis in 0..3 {
                let (lo, hi) = b.iter().fold((f32::MAX, f32::MIN), |(lo, hi), (lab, _)| {
                    (lo.min(lab[axis]), hi.max(lab[axis]))
                });
                // Lightness counts a little less than the hue axes span.
                let span = (hi - lo) * if axis == 0 { 1.0 } else { 1.5 };
                let score = span * weight.sqrt();
                if span > 1e-6 && pick.is_none_or(|p| score > p.2) {
                    pick = Some((i, axis, score));
                }
            }
        }
        let Some((i, axis, _)) = pick else {
            break;
        };
        let mut b = boxes.swap_remove(i);
        b.sort_by(|p, q| p.0[axis].total_cmp(&q.0[axis]));
        let total: f32 = b.iter().map(|(_, w)| *w).sum();
        let mut run = 0.0;
        let mut cut = 1;
        for (j, (_, w)) in b.iter().enumerate() {
            run += *w;
            if run >= total / 2.0 {
                cut = (j + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let upper = b.split_off(cut);
        boxes.push(b);
        boxes.push(upper);
    }
    boxes.iter().map(|b| mean(b)).collect()
}

/// Lloyd's k-means from `centres`, `iterations` rounds at most. A centre
/// that loses every sample keeps its place.
pub fn kmeans(samples: &[(Lab, f32)], centres: &mut [Lab], iterations: usize) {
    if centres.is_empty() {
        return;
    }
    for _ in 0..iterations {
        let mut sums = vec![([0.0f64; 3], 0.0f64); centres.len()];
        for (lab, w) in samples {
            let i = nearest(*lab, centres);
            for a in 0..3 {
                sums[i].0[a] += f64::from(lab[a]) * f64::from(*w);
            }
            sums[i].1 += f64::from(*w);
        }
        let mut moved = 0.0f32;
        for (centre, (sum, weight)) in centres.iter_mut().zip(sums) {
            if weight <= 0.0 {
                continue;
            }
            let next = sum.map(|s| (s / weight) as f32);
            moved = moved.max(distance(*centre, next));
            *centre = next;
        }
        if moved < 1e-4 {
            break;
        }
    }
}

/// How the accent guard keeps small distinct colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guard {
    /// Share of the samples' weight an accent needs, 0..1.
    pub min_share: f32,
    /// Oklab distance from every cluster centre an accent needs.
    pub contrast: f32,
    /// Most accents kept on top of the clusters.
    pub max_accents: usize,
}

impl Default for Guard {
    fn default() -> Self {
        Self {
            min_share: 0.002,
            contrast: 0.12,
            max_accents: 4,
        }
    }
}

/// Clusters of a colour set: the `k` cluster centres and, after them, the
/// accents the guard kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Clusters {
    pub centres: Vec<Lab>,
    /// How many of `centres` (at the end) are accents.
    pub accents: usize,
}

/// Reduce `samples` to `k` clusters by median cut refined with k-means, in
/// Oklab, then keep up to `guard.max_accents` accents: groups of samples
/// further than `guard.contrast` from every centre whose weight is at least
/// `guard.min_share` of the whole, each becoming a centre of its own.
pub fn cluster(samples: &[(Lab, f32)], k: usize, guard: Option<Guard>) -> Clusters {
    let mut centres = median_cut(samples, k.max(1));
    kmeans(samples, &mut centres, 24);
    let mut accents = 0;
    if let Some(guard) = guard {
        let total: f32 = samples.iter().map(|(_, w)| *w).sum();
        for _ in 0..guard.max_accents {
            // The far samples, grouped greedily around the farthest one.
            let far: Vec<(Lab, f32, f32)> = samples
                .iter()
                .map(|(lab, w)| (*lab, *w, distance(*lab, centres[nearest(*lab, &centres)])))
                .filter(|(_, _, d)| *d > guard.contrast)
                .collect();
            if far.is_empty() {
                break;
            }
            // Try group seeds from the farthest down, the first whose group
            // is big enough wins.
            let mut seeds = far.clone();
            seeds.sort_by(|a, b| b.2.total_cmp(&a.2));
            let mut found = None;
            for (seed, _, _) in seeds.iter().take(64) {
                let group: Vec<(Lab, f32)> = far
                    .iter()
                    .filter(|(lab, _, _)| distance(*lab, *seed) <= guard.contrast)
                    .map(|(lab, w, _)| (*lab, *w))
                    .collect();
                let weight: f32 = group.iter().map(|(_, w)| *w).sum();
                if total > 0.0 && weight / total >= guard.min_share {
                    found = Some(mean(&group));
                    break;
                }
            }
            let Some(accent) = found else {
                break;
            };
            centres.push(accent);
            accents += 1;
            // Let the accents settle without moving the clusters away.
            let n = centres.len() - accents;
            let mut all = centres.clone();
            kmeans(samples, &mut all, 4);
            centres[n..].copy_from_slice(&all[n..]);
        }
    }
    Clusters { centres, accents }
}

#[cfg(test)]
#[path = "../../tests/colors.rs"]
mod tests;
