//! Growth behind MCP `moss`, `algae`, and `vines`: plant cells on the
//! exposed faces of a region that pass a face filter (which faces, facing a
//! direction, inside a height band).
//!
//! Moss caps tops (and undersides, when faces takes bottom) and runs down
//! side faces in curtains with a ragged end, or (cushions) grows in separate domed cushions where the surface faces
//! up or out, with drapes hanging from the edges over an overhang.
//! Algae is a band at the waterline, dense low and thinning upward in
//! clumps. Vines hang in strands from top edges, graded through their slots
//! from the anchor to the tip, with an occasional bloom.

use std::collections::{HashMap, HashSet};

use super::debris::Rng;
use super::surface::{
    allowed, cell01, check_slots, check_unit, noise3, step, surface_normal, Exclude, Region, FACES,
};
use crate::state::Cell;

/// Most cells one growth call writes.
const MAX_GROWTH_CELLS: usize = 400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Top,
    Side,
    Bottom,
}

impl Face {
    pub fn parse(text: &str) -> Option<Vec<Face>> {
        match text {
            "top" => Some(vec![Face::Top]),
            "side" => Some(vec![Face::Side]),
            "bottom" => Some(vec![Face::Bottom]),
            "all" => Some(vec![Face::Top, Face::Side, Face::Bottom]),
            _ => None,
        }
    }

    fn of(d: Cell) -> Face {
        match d.2 {
            1 => Face::Top,
            -1 => Face::Bottom,
            _ => Face::Side,
        }
    }
}

/// Paint the surface cells, or add a layer on their empty side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Grow,
    Paint,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GrowthKind {
    Moss {
        /// Share of capped top edges a curtain runs down from, 0..=1.
        curtains: f64,
        /// Curtain length range in cells, 1..=64.
        length: (i32, i32),
        /// Cushion thickness on tops, 1..=3 (grow only).
        thickness: i32,
    },
    /// Moss in separate cushions on exposed cells, never one band.
    Cushions {
        /// Cushion radius range in cells, 1..=16.
        size: (i32, i32),
        /// Fewest empty cells between two cushions, 1..=8.
        gap: i32,
        /// How far the surface must face up (or `facing`) to take moss:
        /// the cosine of its normal with that direction, -1..=1.
        exposure: f64,
        /// Share of cushion edges over a drop that a drape hangs from, 0..=1.
        drapes: f64,
        /// Drape length range in cells, 1..=32.
        length: (i32, i32),
        /// Thickness at a cushion's middle, 1..=4 (grow only).
        thickness: i32,
    },
    Algae,
    Vines {
        /// Strand length range in cells, 1..=128.
        length: (i32, i32),
        /// Bloom slots. Empty grows no blooms.
        blooms: Vec<u8>,
        /// Chance of a bloom beside each strand cell, 0..=1.
        bloom_chance: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GrowthSpec {
    pub kind: GrowthKind,
    pub region: Region,
    /// Plant slots. Moss picks among them in patches, algae from the bottom
    /// of the band to its top, vines from the anchor to the tip.
    pub palettes: Vec<u8>,
    /// How much grows, 0..=1: coverage for moss and algae (algae at the
    /// bottom of its band), the share of edge cells a vine starts from.
    pub amount: f64,
    /// 0 grows in smooth patches; 1 is ragged speckle.
    pub noise: f64,
    pub faces: Vec<Face>,
    /// Only faces whose normal points within 60 degrees of this.
    pub facing: Option<[f64; 3]>,
    /// Only cells with z in this inclusive band. Algae thins from its bottom
    /// to its top; without one it uses the region's height.
    pub band: Option<(i32, i32)>,
    /// Grow only on cells of these slots. Empty grows on every slot.
    pub only: Vec<u8>,
    /// Boxes left alone: nothing grows from a cell inside one, and no cell
    /// inside one is added or painted.
    pub exclude: Exclude,
    pub layer: Layer,
    pub seed: u64,
}

#[derive(Debug, PartialEq)]
pub struct Grown {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub added: usize,
    pub painted: usize,
    /// Moss curtains or drapes, or vine strands.
    pub strands: usize,
    /// Moss cushions.
    pub cushions: usize,
    pub blooms: usize,
    /// Why nothing grew, when nothing did.
    pub note: Option<String>,
}

pub fn grow(cells: &HashMap<Cell, u8>, spec: &GrowthSpec) -> Result<Grown, String> {
    if spec.palettes.is_empty() {
        return Err("palettes must hold at least one palette index".into());
    }
    check_slots(&spec.palettes, "palettes")?;
    check_unit(spec.amount, "amount")?;
    check_unit(spec.noise, "noise")?;
    if spec.faces.is_empty() {
        return Err("faces must name top, side, bottom, or all".into());
    }
    if let Some(f) = spec.facing {
        let len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
        if !len.is_finite() || len < 1e-6 {
            return Err("facing must be a non-zero direction".into());
        }
        let faces_any = FACES
            .iter()
            .any(|d| spec.faces.contains(&Face::of(*d)) && dot(*d, f) >= 0.5);
        if !faces_any {
            return Err(format!(
                "facing [{}, {}, {}] is not within 60 degrees of any of faces ({}), so nothing \
                 can grow: add the faces it points at (bottom for a downward facing), or leave \
                 faces out, which with facing defaults to every face",
                f[0],
                f[1],
                f[2],
                face_names(&spec.faces)
            ));
        }
    }
    if let Some((lo, hi)) = spec.band {
        if lo > hi {
            return Err("band must be [low, high] with low <= high".into());
        }
    }
    let mut out = Out::new(cells, &spec.exclude);
    match &spec.kind {
        GrowthKind::Moss {
            curtains,
            length,
            thickness,
        } => {
            check_unit(*curtains, "curtains")?;
            check_range(*length, 1, 64, "length")?;
            if !(1..=3).contains(thickness) {
                return Err("thickness must be 1..=3".into());
            }
            moss(&mut out, spec, *curtains, *length, *thickness)?;
        }
        GrowthKind::Cushions {
            size,
            gap,
            exposure,
            drapes,
            length,
            thickness,
        } => {
            check_range(*size, 1, 16, "size")?;
            if !(1..=8).contains(gap) {
                return Err("gap must be 1..=8".into());
            }
            if !(-1.0..=1.0).contains(exposure) {
                return Err("exposure must be -1..=1".into());
            }
            check_unit(*drapes, "drapes")?;
            check_range(*length, 1, 32, "length")?;
            if !(1..=4).contains(thickness) {
                return Err("thickness must be 1..=4".into());
            }
            cushions(
                &mut out, spec, *size, *gap, *exposure, *drapes, *length, *thickness,
            )?;
        }
        GrowthKind::Algae => algae(&mut out, spec)?,
        GrowthKind::Vines {
            length,
            blooms,
            bloom_chance,
        } => {
            check_range(*length, 1, 128, "length")?;
            check_slots(blooms, "blooms")?;
            check_unit(*bloom_chance, "bloomChance")?;
            vines(&mut out, spec, *length, blooms, *bloom_chance)?;
        }
    }
    let mut done = out.finish();
    if done.writes.is_empty() {
        done.note = Some(why_bare(cells, spec));
    }
    Ok(done)
}

/// The names of `faces` for a message.
fn face_names(faces: &[Face]) -> String {
    faces
        .iter()
        .map(|f| match f {
            Face::Top => "top",
            Face::Side => "side",
            Face::Bottom => "bottom",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The cosine of face `d` with direction `f`.
fn dot(d: Cell, f: [f64; 3]) -> f64 {
    let len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
    (f64::from(d.0) * f[0] + f64::from(d.1) * f[1] + f64::from(d.2) * f[2]) / len
}

/// Why a growth call wrote nothing, for its reply.
fn why_bare(cells: &HashMap<Cell, u8>, spec: &GrowthSpec) -> String {
    let hosts = spec.hosts(cells);
    if hosts.is_empty() {
        return if spec.only.is_empty() {
            "nothing grew: the box holds no solid cell outside exclude".into()
        } else {
            "nothing grew: the box holds no solid cell of only outside exclude".into()
        };
    }
    let open = hosts.iter().any(|c| {
        FACES
            .iter()
            .any(|d| !cells.contains_key(&step(*c, *d)) && spec.passes(*d, c.2))
    });
    if !open {
        return format!(
            "nothing grew: no exposed face of the host cells passes faces ({}), facing, and band",
            face_names(&spec.faces)
        );
    }
    if let GrowthKind::Cushions { exposure, .. } = spec.kind {
        let up = spec.facing.unwrap_or([0.0, 0.0, 1.0]);
        let len = (up[0] * up[0] + up[1] * up[1] + up[2] * up[2]).sqrt();
        let best = hosts
            .iter()
            .filter_map(|c| surface_normal(cells, *c))
            .map(|n| (n[0] * up[0] + n[1] * up[1] + n[2] * up[2]) / len)
            .fold(f64::NEG_INFINITY, f64::max);
        if best < exposure {
            return format!(
                "nothing grew: no host cell's surface normal is within exposure {exposure} of \
                 {} (the best cosine is {best:.2}); lower exposure",
                if spec.facing.is_some() {
                    "facing"
                } else {
                    "up"
                }
            );
        }
    }
    "nothing grew: the faces that pass are already covered, or amount is too low for them".into()
}

fn check_range(range: (i32, i32), min: i32, max: i32, what: &str) -> Result<(), String> {
    let (lo, hi) = range;
    if !(min..=max).contains(&lo) || !(lo..=max).contains(&hi) {
        return Err(format!(
            "{what} must be [min, max] with {min} <= min <= max <= {max}, not [{lo}, {hi}]"
        ));
    }
    Ok(())
}

/// The writes so far, over the document's cells.
struct Out<'a> {
    cells: &'a HashMap<Cell, u8>,
    exclude: &'a Exclude,
    added: HashMap<Cell, u8>,
    painted: HashMap<Cell, u8>,
    order: Vec<Cell>,
    strands: usize,
    cushions: usize,
    blooms: usize,
}

impl<'a> Out<'a> {
    fn new(cells: &'a HashMap<Cell, u8>, exclude: &'a Exclude) -> Self {
        Out {
            cells,
            exclude,
            added: HashMap::new(),
            painted: HashMap::new(),
            order: Vec::new(),
            strands: 0,
            cushions: 0,
            blooms: 0,
        }
    }

    fn solid(&self, c: &Cell) -> bool {
        self.cells.contains_key(c) || self.added.contains_key(c)
    }

    fn room(&self) -> Result<(), String> {
        if self.order.len() >= MAX_GROWTH_CELLS {
            return Err(format!(
                "growth writes more than {MAX_GROWTH_CELLS} cells; use a smaller region or amount"
            ));
        }
        Ok(())
    }

    /// Add a plant cell in an empty spot. Returns false when it is taken.
    fn add(&mut self, c: Cell, slot: u8) -> Result<bool, String> {
        if self.solid(&c) || self.exclude.contains(c) {
            return Ok(false);
        }
        self.room()?;
        self.added.insert(c, slot);
        self.order.push(c);
        Ok(true)
    }

    /// Recolour a document cell once.
    fn paint(&mut self, c: Cell, slot: u8) -> Result<bool, String> {
        if self.painted.contains_key(&c)
            || self.cells.get(&c) == Some(&slot)
            || self.exclude.contains(c)
        {
            return Ok(false);
        }
        self.room()?;
        self.painted.insert(c, slot);
        self.order.push(c);
        Ok(true)
    }

    /// Paint `cell` or add a cell on its `face` side, as `layer` says.
    fn cover(&mut self, layer: Layer, cell: Cell, face: Cell, slot: u8) -> Result<bool, String> {
        match layer {
            Layer::Paint => self.paint(cell, slot),
            Layer::Grow => self.add(step(cell, face), slot),
        }
    }

    fn finish(self) -> Grown {
        let writes = self
            .order
            .iter()
            .map(|c| {
                let slot = self.added.get(c).or_else(|| self.painted.get(c));
                (c.0, c.1, c.2, i32::from(*slot.expect("written")))
            })
            .collect();
        Grown {
            writes,
            added: self.added.len(),
            painted: self.painted.len(),
            strands: self.strands,
            cushions: self.cushions,
            blooms: self.blooms,
            note: None,
        }
    }
}

impl GrowthSpec {
    /// The solid cells plants may grow on.
    fn hosts(&self, cells: &HashMap<Cell, u8>) -> Vec<Cell> {
        self.region
            .solid(cells)
            .into_iter()
            .filter(|c| allowed(&self.only, cells[c]) && !self.exclude.contains(*c))
            .collect()
    }

    /// Whether a plant may cover face `d` of a cell at height `z`.
    fn passes(&self, d: Cell, z: i32) -> bool {
        if !self.faces.contains(&Face::of(d)) {
            return false;
        }
        if let Some((lo, hi)) = self.band {
            if !(lo..=hi).contains(&z) {
                return false;
            }
        }
        if let Some(f) = self.facing {
            if dot(d, f) < 0.5 {
                return false;
            }
        }
        true
    }

    /// 0..1 at a cell: smooth patches mixed with speckle by `noise`.
    fn coverage(&self, c: Cell, scale: f64, salt: u64) -> f64 {
        let p = [f64::from(c.0), f64::from(c.1), f64::from(c.2)];
        let patch = ((noise3(p, scale, self.seed ^ salt) - 0.5) * 2.2 + 0.5).clamp(0.0, 1.0);
        (1.0 - self.noise) * patch + self.noise * cell01(c, self.seed, salt)
    }

    /// A slot from the patch tone at a cell.
    fn tone(&self, c: Cell) -> u8 {
        let p = [f64::from(c.0), f64::from(c.1), f64::from(c.2)];
        let t = 0.7 * noise3(p, 3.0, self.seed ^ 77) + 0.3 * cell01(c, self.seed, 78);
        self.palettes[((t * self.palettes.len() as f64) as usize).min(self.palettes.len() - 1)]
    }
}

const SIDES: [Cell; 4] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0)];
/// Which of [`SIDES`] `d` is, as a salt.
fn side_index(d: Cell) -> u64 {
    SIDES.iter().position(|s| *s == d).unwrap_or(0) as u64
}

const UP: Cell = (0, 0, 1);
const DOWN: Cell = (0, 0, -1);

fn moss(
    out: &mut Out,
    spec: &GrowthSpec,
    curtains: f64,
    length: (i32, i32),
    thickness: i32,
) -> Result<(), String> {
    let hosts = spec.hosts(out.cells);
    let mut capped: HashSet<Cell> = HashSet::new();
    // Caps and cushions on tops.
    for &cell in &hosts {
        if out.cells.contains_key(&step(cell, UP)) || !spec.passes(UP, cell.2) {
            continue;
        }
        let v = spec.coverage(cell, 4.0, 1);
        if v >= spec.amount {
            continue;
        }
        capped.insert(cell);
        let slot = spec.tone(cell);
        out.cover(spec.layer, cell, UP, slot)?;
        if spec.layer == Layer::Grow {
            // Thicker cushions where the cover is densest.
            for level in 2..=thickness {
                if v < spec.amount * (0.55 / f64::from(level - 1)) {
                    out.add((cell.0, cell.1, cell.2 + level), spec.tone(step(cell, UP)))?;
                }
            }
        }
    }
    // Undersides, when faces takes bottom: moss on the face over air,
    // hanging down `thickness` cells where the cover is densest.
    for &cell in &hosts {
        if out.cells.contains_key(&step(cell, DOWN)) || !spec.passes(DOWN, cell.2) {
            continue;
        }
        let v = spec.coverage(cell, 4.0, 2);
        if v >= spec.amount {
            continue;
        }
        out.cover(spec.layer, cell, DOWN, spec.tone(cell))?;
        if spec.layer == Layer::Grow {
            for level in 2..=thickness {
                if v < spec.amount * (0.55 / f64::from(level - 1)) {
                    out.add(
                        (cell.0, cell.1, cell.2 - level),
                        spec.tone(step(cell, DOWN)),
                    )?;
                }
            }
        }
    }
    // Curtains down the side faces below a capped top edge (any top edge
    // when tops are filtered out).
    let caps = spec.faces.contains(&Face::Top);
    let mut rng = Rng::new(spec.seed ^ 0x6d6f_7373);
    for &cell in &hosts {
        if out.cells.contains_key(&step(cell, UP)) || (caps && !capped.contains(&cell)) {
            continue;
        }
        for d in SIDES {
            if out.cells.contains_key(&step(cell, d)) || !spec.passes(d, cell.2) {
                continue;
            }
            // Neighbouring edge cells start together, so a curtain is wide.
            if spec.coverage(cell, 5.0, 20 + side_index(d)) >= curtains {
                continue;
            }
            let (lo, hi) = length;
            let reach = noise3(
                [f64::from(cell.0), f64::from(cell.1), 0.0],
                3.0,
                spec.seed ^ 3,
            );
            // A ragged end: each column stops a little apart from its neighbours.
            let len = (f64::from(lo) + f64::from(hi - lo) * reach).round() as i32
                - (rng.next() % 3) as i32;
            let mut ran = 0;
            for k in 0..len.max(1) {
                let face = (cell.0, cell.1, cell.2 - k);
                if !out.cells.contains_key(&face)
                    || out.cells.contains_key(&step(face, d))
                    || !spec.passes(d, face.2)
                    || !allowed(&spec.only, out.cells[&face])
                {
                    break;
                }
                out.cover(spec.layer, face, d, spec.tone(face))?;
                ran += 1;
            }
            if ran > 0 {
                out.strands += 1;
            }
        }
    }
    Ok(())
}

/// Moss in cushions: seeds on the exposed cells in a stable random order,
/// each grown over the neighbouring exposed cells out to a ragged radius
/// and kept `gap` cells from every other cushion, until `amount` of the
/// exposed cells is covered. In grow layer a cushion is domed, thickest in
/// the middle, and drapes run from its edge down the side and hang free
/// below an overhang.
#[allow(clippy::too_many_arguments)]
fn cushions(
    out: &mut Out,
    spec: &GrowthSpec,
    size: (i32, i32),
    gap: i32,
    exposure: f64,
    drapes: f64,
    length: (i32, i32),
    thickness: i32,
) -> Result<(), String> {
    let up = match spec.facing {
        Some(f) => {
            let len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            [f[0] / len, f[1] / len, f[2] / len]
        }
        None => [0.0, 0.0, 1.0],
    };
    // Exposed cells and the face the moss covers on each: the open face
    // nearest the surface normal.
    let mut spots: HashMap<Cell, Cell> = HashMap::new();
    for cell in spec.hosts(out.cells) {
        let Some(n) = surface_normal(out.cells, cell) else {
            continue;
        };
        if n[0] * up[0] + n[1] * up[1] + n[2] * up[2] < exposure {
            continue;
        }
        let face = FACES
            .iter()
            .copied()
            .filter(|d| !out.cells.contains_key(&step(cell, *d)) && spec.passes(*d, cell.2))
            .max_by(|a, b| {
                let along = |d: &Cell| {
                    f64::from(d.0) * n[0] + f64::from(d.1) * n[1] + f64::from(d.2) * n[2]
                };
                along(a).total_cmp(&along(b))
            });
        if let Some(face) = face {
            spots.insert(cell, face);
        }
    }
    let mut order: Vec<Cell> = spots.keys().copied().collect();
    order.sort_unstable_by(|a, b| {
        cell01(*a, spec.seed, 40)
            .total_cmp(&cell01(*b, spec.seed, 40))
            .then((a.2, a.1, a.0).cmp(&(b.2, b.1, b.0)))
    });
    let target = (spec.amount * spots.len() as f64).round() as usize;
    let mut rng = Rng::new(spec.seed ^ 0x6375_7368);
    let mut claimed: HashSet<Cell> = HashSet::new();
    let mut blocked: HashSet<Cell> = HashSet::new();
    let mut found: Vec<(Cell, f64, Vec<Cell>)> = Vec::new();
    for &seed in &order {
        if claimed.len() >= target {
            break;
        }
        if claimed.contains(&seed) || blocked.contains(&seed) {
            continue;
        }
        let (lo, hi) = size;
        let radius = f64::from(lo) + f64::from(hi - lo) * rng.unit();
        let mut members = vec![seed];
        claimed.insert(seed);
        let mut queue = std::collections::VecDeque::from([seed]);
        while let Some(at) = queue.pop_front() {
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let n = (at.0 + dx, at.1 + dy, at.2 + dz);
                        if !spots.contains_key(&n) || claimed.contains(&n) || blocked.contains(&n) {
                            continue;
                        }
                        let p = [f64::from(n.0), f64::from(n.1), f64::from(n.2)];
                        let ragged = radius * (0.7 + 0.6 * noise3(p, 2.5, spec.seed ^ 41));
                        if distance(n, seed) > ragged {
                            continue;
                        }
                        claimed.insert(n);
                        members.push(n);
                        queue.push_back(n);
                    }
                }
            }
        }
        // Keep the next cushions `gap` cells away.
        for m in &members {
            for dz in -gap..=gap {
                for dy in -gap..=gap {
                    for dx in -gap..=gap {
                        let n = (m.0 + dx, m.1 + dy, m.2 + dz);
                        if spots.contains_key(&n) && !claimed.contains(&n) {
                            blocked.insert(n);
                        }
                    }
                }
            }
        }
        found.push((seed, radius.max(1.0), members));
    }
    let n = spec.palettes.len();
    for (seed, radius, members) in &found {
        let base = (cell01(*seed, spec.seed, 44) * n as f64) as usize;
        let tone = |c: Cell| {
            let p = [f64::from(c.0), f64::from(c.1), f64::from(c.2)];
            let v = noise3(p, 2.0, spec.seed ^ 42);
            let shift = if v > 0.65 {
                1
            } else if v < 0.3 {
                -1
            } else {
                0
            };
            spec.palettes[(base as i64 + shift).clamp(0, n as i64 - 1) as usize]
        };
        for &c in members {
            let face = spots[&c];
            let slot = tone(c);
            match spec.layer {
                Layer::Paint => {
                    out.paint(c, slot)?;
                }
                Layer::Grow => {
                    let t = (distance(c, *seed) / radius).min(1.0);
                    // A low dome: only the middle of a cushion is thick.
                    let jitter = (cell01(c, spec.seed, 43) - 0.5) * 0.5;
                    let extra = (f64::from(thickness - 1) * (1.0 - 1.6 * t).max(0.0) + jitter)
                        .round()
                        .clamp(0.0, f64::from(thickness - 1))
                        as i32;
                    for k in 1..=1 + extra {
                        let at = (c.0 + face.0 * k, c.1 + face.1 * k, c.2 + face.2 * k);
                        if !out.add(at, slot)? {
                            break;
                        }
                    }
                }
            }
        }
        if spec.layer == Layer::Grow && drapes > 0.0 {
            for &c in members {
                for d in SIDES {
                    let beside = step(c, d);
                    if out.solid(&beside) || out.exclude.contains(beside) {
                        continue;
                    }
                    if spec.coverage(c, 3.0, 50 + side_index(d)) >= drapes {
                        continue;
                    }
                    let (lo, hi) = length;
                    let len = lo + (rng.next() % (hi - lo + 1) as u64) as i32;
                    let slot = tone(c);
                    let mut at = beside;
                    if !out.add(at, slot)? {
                        continue;
                    }
                    // Down the side, stepping out where the wood bulges,
                    // then free below the overhang.
                    for _ in 1..len {
                        let below = step(at, DOWN);
                        let next = if !out.solid(&below) {
                            below
                        } else {
                            let outward = step(at, d);
                            if out.solid(&outward) {
                                break;
                            }
                            outward
                        };
                        if !out.add(next, slot)? {
                            break;
                        }
                        at = next;
                    }
                    out.strands += 1;
                }
            }
        }
        out.cushions += 1;
    }
    Ok(())
}

fn distance(a: Cell, b: Cell) -> f64 {
    let d = [
        f64::from(a.0 - b.0),
        f64::from(a.1 - b.1),
        f64::from(a.2 - b.2),
    ];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn algae(out: &mut Out, spec: &GrowthSpec) -> Result<(), String> {
    let (lo, hi) = spec.band.unwrap_or((spec.region.min.2, spec.region.max.2));
    let span = f64::from(hi - lo).max(1.0);
    for cell in spec.hosts(out.cells) {
        if !(lo..=hi).contains(&cell.2) {
            continue;
        }
        let Some(face) = FACES
            .iter()
            .copied()
            .find(|d| !out.cells.contains_key(&step(cell, *d)) && spec.passes(*d, cell.2))
        else {
            continue;
        };
        let t = f64::from(cell.2 - lo) / span;
        let density = spec.amount * (1.0 - t).powf(1.5);
        if spec.coverage(cell, 3.0, 4) >= density {
            continue;
        }
        let jitter = (cell01(cell, spec.seed, 5) - 0.5) * 0.3;
        let n = spec.palettes.len();
        let slot = spec.palettes[(((t + jitter).clamp(0.0, 0.999)) * n as f64) as usize];
        out.cover(spec.layer, cell, face, slot)?;
    }
    Ok(())
}

fn vines(
    out: &mut Out,
    spec: &GrowthSpec,
    length: (i32, i32),
    blooms: &[u8],
    bloom_chance: f64,
) -> Result<(), String> {
    let mut rng = Rng::new(spec.seed ^ 0x7669_6e65);
    for cell in spec.hosts(out.cells) {
        if out.cells.contains_key(&step(cell, UP)) {
            continue;
        }
        for d in SIDES {
            let anchor = step(cell, d);
            if out.solid(&anchor) || !spec.passes(d, cell.2) {
                continue;
            }
            if cell01(cell, spec.seed, 30 + side_index(d)) >= spec.amount {
                continue;
            }
            let (lo, hi) = length;
            let len = lo + (rng.next() % (hi - lo + 1) as u64) as i32;
            let along = (d.1, -d.0, 0);
            let mut at = anchor;
            let mut grown = 0;
            for k in 0..len {
                if k > 0 {
                    let below = step(at, DOWN);
                    if out.solid(&below) {
                        break;
                    }
                    at = below;
                    // A slight wander along the edge.
                    if spec.noise > 0.0 && rng.unit() < spec.noise * 0.25 {
                        let side = if rng.unit() < 0.5 {
                            along
                        } else {
                            (-along.0, -along.1, 0)
                        };
                        let moved = step(at, side);
                        if !out.solid(&moved) && !out.solid(&step(moved, UP)) {
                            out.add(step(moved, UP), slot_at(spec, k, len))?;
                            at = moved;
                        }
                    }
                }
                if !out.add(at, slot_at(spec, k, len))? && k == 0 {
                    break;
                }
                grown += 1;
                if !blooms.is_empty() && rng.unit() < bloom_chance {
                    let side = [along, (-along.0, -along.1, 0), d][(rng.next() % 3) as usize];
                    if out.add(step(at, side), rng.pick(blooms))? {
                        out.blooms += 1;
                    }
                }
            }
            if grown > 0 {
                out.strands += 1;
            }
        }
    }
    Ok(())
}

/// A vine slot graded from the anchor (`k` 0) to the tip.
fn slot_at(spec: &GrowthSpec, k: i32, len: i32) -> u8 {
    let n = spec.palettes.len();
    let t = f64::from(k) / f64::from(len.max(1));
    spec.palettes[((t * n as f64) as usize).min(n - 1)]
}
