//! MCP `components`: connected parts of an object, for cleaning up a mesh
//! made from a concept image (a rider on a shell, moss tufts, foam bits).
//!
//! Cells are joined by faces (connectivity 6), faces and edges (18), or
//! faces, edges, and corners (26). A part is selected by a seed cell, and
//! may first be cut from the rest at a neck: the fewest cells whose removal
//! separates the seed from an anchor, found as a minimum vertex cut by
//! max flow (Dinic, unit capacity per cell) between a geodesic ball around
//! each of them. The cut cells stay with the anchor's side.

use std::collections::{HashMap, VecDeque};

use crate::state::Cell;

/// Most cells in an object's bounding box.
pub const MAX_COMPONENT_BOX: usize = 1 << 25;
/// Most cells a neck cut may hold.
pub const MAX_NECK_CUT: usize = 65_536;
/// How many components a reply lists.
pub const LISTED_COMPONENTS: usize = 32;

/// What `components` does.
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentOp {
    /// Report the components only.
    List,
    /// Keep the `keep` largest components and erase the rest.
    KeepLargest { keep: usize },
    /// Erase the components with fewer than `min` cells.
    RemoveSmall { min: usize },
    /// Erase the seed's part.
    Delete,
    /// Move the seed's part by whole cells.
    Move { offset: Cell },
    /// Select the seed's part; the caller moves it into another object.
    Split,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentSpec {
    pub op: ComponentOp,
    /// 6, 18, or 26.
    pub connectivity: u8,
    /// A cell of the part delete, move, and split act on.
    pub seed: Option<[f64; 3]>,
    /// A cell of the part that stays: the seed's part is cut from it at the
    /// thinnest cross-section between them.
    pub anchor: Option<[f64; 3]>,
    /// How far a seed may be from the solid cell it snaps to.
    pub reach: f64,
    /// Geodesic radius in cells of the regions around seed and anchor that
    /// the cut may not enter. `None` is 0.3 of the geodesic distance.
    pub radius: Option<f64>,
}

/// One component, largest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    pub cells: usize,
    pub min: Cell,
    pub max: Cell,
    /// One of its cells.
    pub seed: Cell,
}

/// What `components` found and the writes it makes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Components {
    /// Every component, largest first.
    pub components: Vec<Component>,
    /// The cells selected (erased, moved, or split).
    pub selected: Vec<Cell>,
    /// Cells of the neck cut, left with the anchor.
    pub cut: usize,
    /// The solid cells seed and anchor snapped to.
    pub seed: Option<Cell>,
    pub anchor: Option<Cell>,
    /// Cell writes `(x, y, z, slot)`; a split has none.
    pub writes: Vec<(i32, i32, i32, i32)>,
}

/// The neighbour offsets of a connectivity.
pub fn neighbours(connectivity: u8) -> Result<Vec<Cell>, String> {
    let max = match connectivity {
        6 => 1,
        18 => 2,
        26 => 3,
        other => return Err(format!("connectivity must be 6, 18, or 26, not {other}")),
    };
    let mut out = Vec::new();
    for dz in -1..=1i32 {
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                let n = dx.abs() + dy.abs() + dz.abs();
                if n > 0 && n <= max {
                    out.push((dx, dy, dz));
                }
            }
        }
    }
    Ok(out)
}

/// The solid cells of an object indexed in a dense box, with neighbours by
/// offset.
pub(crate) struct Grid {
    pub cells: Vec<Cell>,
    min: Cell,
    size: [usize; 3],
    index: Vec<u32>,
    offsets: Vec<Cell>,
}

const EMPTY: u32 = u32::MAX;

impl Grid {
    pub fn new(cells: &HashMap<Cell, u8>, connectivity: u8) -> Result<Self, String> {
        let offsets = neighbours(connectivity)?;
        let mut list: Vec<Cell> = cells.keys().copied().collect();
        list.sort_unstable();
        let (mut min, mut max) = (
            (i32::MAX, i32::MAX, i32::MAX),
            (i32::MIN, i32::MIN, i32::MIN),
        );
        for c in &list {
            min = (min.0.min(c.0), min.1.min(c.1), min.2.min(c.2));
            max = (max.0.max(c.0), max.1.max(c.1), max.2.max(c.2));
        }
        if list.is_empty() {
            return Ok(Self {
                cells: list,
                min: (0, 0, 0),
                size: [0; 3],
                index: Vec::new(),
                offsets,
            });
        }
        let size = [
            (max.0 - min.0 + 1) as usize,
            (max.1 - min.1 + 1) as usize,
            (max.2 - min.2 + 1) as usize,
        ];
        let volume = size[0] * size[1] * size[2];
        if volume > MAX_COMPONENT_BOX {
            return Err(format!(
                "the object's box is {}x{}x{}; components takes at most {MAX_COMPONENT_BOX} cells in it",
                size[0], size[1], size[2]
            ));
        }
        let mut index = vec![EMPTY; volume];
        let mut grid = Self {
            cells: Vec::new(),
            min,
            size,
            index: Vec::new(),
            offsets,
        };
        for (i, c) in list.iter().enumerate() {
            index[grid.slot(*c).expect("inside its own box")] = i as u32;
        }
        grid.index = index;
        grid.cells = list;
        Ok(grid)
    }

    fn slot(&self, c: Cell) -> Option<usize> {
        let x = c.0 - self.min.0;
        let y = c.1 - self.min.1;
        let z = c.2 - self.min.2;
        if x < 0 || y < 0 || z < 0 {
            return None;
        }
        let (x, y, z) = (x as usize, y as usize, z as usize);
        if x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return None;
        }
        Some((z * self.size[1] + y) * self.size[0] + x)
    }

    pub fn find(&self, c: Cell) -> Option<usize> {
        self.slot(c)
            .map(|slot| self.index[slot])
            .filter(|&i| i != EMPTY)
            .map(|i| i as usize)
    }

    pub fn degree(&self) -> usize {
        self.offsets.len()
    }

    /// The neighbour of cell `i` along offset `d`, if solid.
    pub fn neighbour(&self, i: usize, d: usize) -> Option<usize> {
        let c = self.cells[i];
        let o = self.offsets[d];
        self.find((c.0 + o.0, c.1 + o.1, c.2 + o.2))
    }

    /// The index of the offset opposite `d`.
    fn opposite(&self, d: usize) -> usize {
        // The offsets are symmetric about the middle of the list.
        self.offsets.len() - 1 - d
    }

    /// A component label per cell, and the components largest first
    /// (labels follow that order).
    pub fn label(&self) -> (Vec<u32>, Vec<Component>) {
        let n = self.cells.len();
        let mut label = vec![EMPTY; n];
        let mut found: Vec<(usize, Component)> = Vec::new();
        let mut queue = VecDeque::new();
        for start in 0..n {
            if label[start] != EMPTY {
                continue;
            }
            let id = found.len() as u32;
            label[start] = id;
            queue.push_back(start);
            let mut part = Component {
                cells: 0,
                min: self.cells[start],
                max: self.cells[start],
                seed: self.cells[start],
            };
            while let Some(i) = queue.pop_front() {
                let c = self.cells[i];
                part.cells += 1;
                part.min = (
                    part.min.0.min(c.0),
                    part.min.1.min(c.1),
                    part.min.2.min(c.2),
                );
                part.max = (
                    part.max.0.max(c.0),
                    part.max.1.max(c.1),
                    part.max.2.max(c.2),
                );
                for d in 0..self.degree() {
                    if let Some(j) = self.neighbour(i, d) {
                        if label[j] == EMPTY {
                            label[j] = id;
                            queue.push_back(j);
                        }
                    }
                }
            }
            found.push((start, part));
        }
        let mut order: Vec<usize> = (0..found.len()).collect();
        order.sort_by(|&a, &b| {
            found[b]
                .1
                .cells
                .cmp(&found[a].1.cells)
                .then(found[a].0.cmp(&found[b].0))
        });
        let mut rank = vec![0u32; found.len()];
        for (r, &old) in order.iter().enumerate() {
            rank[old] = r as u32;
        }
        for l in label.iter_mut() {
            *l = rank[*l as usize];
        }
        let components = order.into_iter().map(|old| found[old].1.clone()).collect();
        (label, components)
    }

    /// Geodesic steps from `start` to every cell reachable through
    /// `allowed`, `u32::MAX` elsewhere.
    fn distances(&self, start: usize, allowed: &dyn Fn(usize) -> bool) -> Vec<u32> {
        let mut dist = vec![u32::MAX; self.cells.len()];
        let mut queue = VecDeque::new();
        dist[start] = 0;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            for d in 0..self.degree() {
                if let Some(j) = self.neighbour(i, d) {
                    if dist[j] == u32::MAX && allowed(j) {
                        dist[j] = dist[i] + 1;
                        queue.push_back(j);
                    }
                }
            }
        }
        dist
    }
}

/// The solid cell nearest `point` within `reach`, ties to the lowest cell.
pub(crate) fn snap(grid: &Grid, point: [f64; 3], reach: f64) -> Option<usize> {
    let centre = (
        point[0].round() as i32,
        point[1].round() as i32,
        point[2].round() as i32,
    );
    if let Some(i) = grid.find(centre) {
        return Some(i);
    }
    let r = reach.ceil().max(0.0) as i32;
    let mut best: Option<(f64, Cell, usize)> = None;
    for dz in -r..=r {
        for dy in -r..=r {
            for dx in -r..=r {
                let c = (centre.0 + dx, centre.1 + dy, centre.2 + dz);
                let Some(i) = grid.find(c) else { continue };
                let d = ((f64::from(c.0) - point[0]).powi(2)
                    + (f64::from(c.1) - point[1]).powi(2)
                    + (f64::from(c.2) - point[2]).powi(2))
                .sqrt();
                if d > reach {
                    continue;
                }
                if best.is_none_or(|(bd, bc, _)| d < bd || (d == bd && c < bc)) {
                    best = Some((d, c, i));
                }
            }
        }
    }
    best.map(|(_, _, i)| i)
}

/// Run `components` on an object's cells.
pub fn components(cells: &HashMap<Cell, u8>, spec: &ComponentSpec) -> Result<Components, String> {
    if !(0.0..=64.0).contains(&spec.reach) {
        return Err("reach must be 0..=64".into());
    }
    let grid = Grid::new(cells, spec.connectivity)?;
    let (label, parts) = grid.label();
    let mut out = Components {
        components: parts.clone(),
        ..Components::default()
    };
    let erase = |out: &mut Components, keep: &dyn Fn(usize) -> bool| {
        for (i, c) in grid.cells.iter().enumerate() {
            if !keep(label[i] as usize) {
                out.selected.push(*c);
                out.writes.push((c.0, c.1, c.2, 0));
            }
        }
    };
    match &spec.op {
        ComponentOp::List => return Ok(out),
        ComponentOp::KeepLargest { keep } => {
            if *keep == 0 {
                return Err("keep must be at least 1".into());
            }
            let keep = *keep;
            erase(&mut out, &|l| l < keep);
            return Ok(out);
        }
        ComponentOp::RemoveSmall { min } => {
            let sizes: Vec<usize> = parts.iter().map(|p| p.cells).collect();
            let min = *min;
            erase(&mut out, &|l| sizes[l] >= min);
            return Ok(out);
        }
        _ => {}
    }
    let point = spec
        .seed
        .ok_or("seed is required: a cell of the part to delete, move, or split")?;
    let seed = snap(&grid, point, spec.reach)
        .ok_or_else(|| format!("no solid cell within reach {} of the seed", spec.reach))?;
    out.seed = Some(grid.cells[seed]);
    let part = label[seed];
    let mut chosen: Vec<bool> = label.iter().map(|&l| l == part).collect();
    if let Some(point) = spec.anchor {
        let anchor = snap(&grid, point, spec.reach)
            .ok_or_else(|| format!("no solid cell within reach {} of the anchor", spec.reach))?;
        out.anchor = Some(grid.cells[anchor]);
        if anchor == seed {
            return Err("seed and anchor snap to the same cell".into());
        }
        if label[anchor] == part {
            let (side, cut) = neck_cut(&grid, &label, seed, anchor, spec.radius)?;
            out.cut = cut;
            chosen = side;
        }
    }
    for (i, c) in grid.cells.iter().enumerate() {
        if chosen[i] {
            out.selected.push(*c);
        }
    }
    match spec.op {
        ComponentOp::Delete => {
            out.writes = out.selected.iter().map(|c| (c.0, c.1, c.2, 0)).collect();
        }
        ComponentOp::Move { offset } => {
            for c in &out.selected {
                out.writes.push((c.0, c.1, c.2, 0));
            }
            for c in &out.selected {
                let slot = cells[c];
                out.writes.push((
                    c.0 + offset.0,
                    c.1 + offset.1,
                    c.2 + offset.2,
                    i32::from(slot),
                ));
            }
        }
        _ => {}
    }
    Ok(out)
}

/// The seed's side of the thinnest cut between seed and anchor in their
/// component, and the cut's size. The side is the seed's connected piece
/// once the cut is removed; the cut stays with the anchor.
fn neck_cut(
    grid: &Grid,
    label: &[u32],
    seed: usize,
    anchor: usize,
    radius: Option<f64>,
) -> Result<(Vec<bool>, usize), String> {
    let part = label[seed];
    let inside = |i: usize| label[i] == part;
    let from_seed = grid.distances(seed, &inside);
    let from_anchor = grid.distances(anchor, &inside);
    let apart = f64::from(from_seed[anchor]);
    let radius = match radius {
        Some(r) if !(0.0..=256.0).contains(&r) => return Err("radius must be 0..=256".into()),
        Some(r) => r,
        None => (apart * 0.3).floor().max(1.0),
    };
    if 2.0 * radius >= apart {
        return Err(format!(
            "seed and anchor are {apart} cells apart; radius {radius} makes their regions meet (use a smaller radius)"
        ));
    }
    let n = grid.cells.len();
    // 0: free, 1: source (anchor's region), 2: sink (seed's region).
    let mut role = vec![0u8; n];
    for i in 0..n {
        if !inside(i) {
            continue;
        }
        if f64::from(from_anchor[i]) <= radius {
            role[i] = 1;
        } else if f64::from(from_seed[i]) <= radius {
            role[i] = 2;
        }
    }
    for i in 0..n {
        if role[i] == 1 {
            for d in 0..grid.degree() {
                if grid.neighbour(i, d).is_some_and(|j| role[j] == 2) {
                    return Err(
                        "the regions around seed and anchor touch; pass a smaller radius".into(),
                    );
                }
            }
        }
    }
    let mut flow = Flow::new(grid, &role, &inside);
    let cut = flow.max_flow()?;
    let reach = flow.reachable();
    // Cells cut: in reachable, out not. The seed's side: neither.
    let mut cut_cells = vec![false; n];
    let mut far = vec![false; n];
    for i in 0..n {
        if !inside(i) {
            continue;
        }
        let (a, b) = (reach[2 * i], reach[2 * i + 1]);
        if a && !b {
            cut_cells[i] = true;
        } else if !a {
            far[i] = true;
        }
    }
    let side = grid.distances(seed, &|j| far[j]);
    let chosen: Vec<bool> = side.iter().map(|&d| d != u32::MAX).collect();
    Ok((chosen, cut))
}

/// Max flow with unit capacity per free cell over the implicit graph of a
/// grid: each cell is an in node and an out node joined by its capacity;
/// out of one cell to in of each neighbour has no limit.
struct Flow<'a> {
    grid: &'a Grid,
    role: &'a [u8],
    inside: &'a dyn Fn(usize) -> bool,
    /// Flow through each free cell (0 or 1).
    through: Vec<u8>,
    /// Flow from out of a cell to in of its neighbour along each offset.
    along: Vec<u32>,
    level: Vec<u32>,
    arc: Vec<u16>,
}

const UNSEEN: u32 = u32::MAX;

impl<'a> Flow<'a> {
    fn new(grid: &'a Grid, role: &'a [u8], inside: &'a dyn Fn(usize) -> bool) -> Self {
        let n = grid.cells.len();
        Self {
            grid,
            role,
            inside,
            through: vec![0; n],
            along: vec![0; n * grid.degree()],
            level: vec![UNSEEN; 2 * n],
            arc: vec![0; 2 * n],
        }
    }

    fn arcs(&self) -> usize {
        self.grid.degree() + 1
    }

    /// Arc `a` of node `v` in the residual graph: its head, if usable.
    fn head(&self, v: usize, a: usize) -> Option<usize> {
        let cell = v / 2;
        let k = self.grid.degree();
        if v.is_multiple_of(2) {
            // In node: to its out (capacity), or back along a neighbour's flow.
            if a == 0 {
                let open = self.role[cell] != 0 || self.through[cell] == 0;
                return open.then_some(2 * cell + 1);
            }
            let d = a - 1;
            let u = self.grid.neighbour(cell, d)?;
            let back = self.grid.opposite(d);
            (self.along[u * k + back] > 0).then_some(2 * u + 1)
        } else {
            if a < k {
                let w = self.grid.neighbour(cell, a)?;
                return (self.inside)(w).then_some(2 * w);
            }
            (self.role[cell] == 0 && self.through[cell] > 0).then_some(2 * cell)
        }
    }

    fn push(&mut self, v: usize, a: usize) {
        let cell = v / 2;
        let k = self.grid.degree();
        if v.is_multiple_of(2) {
            if a == 0 {
                if self.role[cell] == 0 {
                    self.through[cell] += 1;
                }
            } else {
                let d = a - 1;
                let u = self.grid.neighbour(cell, d).expect("usable arc");
                let back = self.grid.opposite(d);
                self.along[u * k + back] -= 1;
            }
        } else if a < k {
            self.along[cell * k + a] += 1;
        } else {
            self.through[cell] -= 1;
        }
    }

    fn sources(&self) -> Vec<usize> {
        (0..self.grid.cells.len())
            .filter(|&i| self.role[i] == 1)
            .map(|i| 2 * i + 1)
            .collect()
    }

    fn is_sink(&self, v: usize) -> bool {
        self.role[v / 2] == 2
    }

    fn bfs(&mut self) -> bool {
        self.level.fill(UNSEEN);
        let mut queue = VecDeque::new();
        for i in 0..self.grid.cells.len() {
            if self.role[i] == 1 {
                self.level[2 * i] = 0;
                self.level[2 * i + 1] = 0;
                queue.push_back(2 * i + 1);
            }
        }
        let mut found = false;
        while let Some(v) = queue.pop_front() {
            for a in 0..self.arcs() {
                if let Some(w) = self.head(v, a) {
                    if self.level[w] == UNSEEN {
                        self.level[w] = self.level[v] + 1;
                        if self.is_sink(w) {
                            found = true;
                        } else {
                            queue.push_back(w);
                        }
                    }
                }
            }
        }
        found
    }

    fn max_flow(&mut self) -> Result<usize, String> {
        let mut total = 0;
        while self.bfs() {
            self.arc.fill(0);
            for start in self.sources() {
                while let Some(path) = self.augment(start) {
                    for (v, a) in path {
                        self.push(v, a);
                    }
                    total += 1;
                    if total > MAX_NECK_CUT {
                        return Err(format!(
                            "the thinnest cut between seed and anchor holds more than {MAX_NECK_CUT} cells: there is no neck"
                        ));
                    }
                }
            }
        }
        Ok(total)
    }

    /// One path from `start` to a sink along the level graph, as arcs.
    fn augment(&mut self, start: usize) -> Option<Vec<(usize, usize)>> {
        let mut path: Vec<(usize, usize)> = Vec::new();
        let mut v = start;
        loop {
            if self.is_sink(v) {
                return Some(path);
            }
            let mut advanced = false;
            while (self.arc[v] as usize) < self.arcs() {
                let a = self.arc[v] as usize;
                if let Some(w) = self.head(v, a) {
                    if self.level[w] == self.level[v] + 1 && self.level[w] != UNSEEN {
                        path.push((v, a));
                        v = w;
                        advanced = true;
                        break;
                    }
                }
                self.arc[v] += 1;
            }
            if advanced {
                continue;
            }
            // Dead end: retreat.
            self.level[v] = UNSEEN;
            let (back, _) = path.pop()?;
            self.arc[back] += 1;
            v = back;
        }
    }

    /// Nodes reachable from the sources in the residual graph.
    fn reachable(&self) -> Vec<bool> {
        let mut seen = vec![false; self.level.len()];
        let mut queue = VecDeque::new();
        for i in 0..self.grid.cells.len() {
            if self.role[i] == 1 {
                seen[2 * i] = true;
                seen[2 * i + 1] = true;
                queue.push_back(2 * i + 1);
            }
        }
        while let Some(v) = queue.pop_front() {
            for a in 0..self.arcs() {
                if let Some(w) = self.head(v, a) {
                    if !seen[w] {
                        seen[w] = true;
                        queue.push_back(w);
                    }
                }
            }
        }
        seen
    }
}

#[cfg(test)]
#[path = "../../tests/components.rs"]
mod tests;
