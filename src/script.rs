//! Sandboxed Rhai scripts that build voxels.
//!
//! The engine has no file, network, process, or clock access. A run writes a
//! scratch copy and replaces objects only after it finishes, as one undo step.
//! There is no operation or voxel cap: a run stops when its `Stop` is
//! cancelled or its deadline passes. The editor runs scripts on a worker
//! thread (`start_script`) so a long run never blocks the window.
//! Opening a `.vox` never runs the script; the generated voxels are already
//! in the file, and `MVSC` keeps the source and parameter values.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, EvalAltResult, OptimizationLevel, Position, INT};

use crate::state::{Cell, Motion, Session, Volume};

const MAX_DEPTH: usize = 32;
const MAX_ARRAY: usize = 4_096;
const MAX_STRING: usize = 16_384;
const MAX_MAP: usize = 256;
const MAX_OBJECTS: usize = 32;
const MAX_SOURCE: usize = 65_536;
const COORD_LIMIT: i32 = 512;
/// Deadline for a blocking run (MCP, bridge, parameter discovery).
pub const SCRIPT_TIMEOUT: Duration = Duration::from_secs(300);

/// Ends a running script: cancel it, or let its deadline pass.
#[derive(Clone, Default)]
pub struct Stop {
    cancel: Arc<AtomicBool>,
    deadline: Option<(Instant, Duration)>,
}

impl Stop {
    pub fn after(timeout: Option<Duration>) -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            deadline: timeout.map(|timeout| (Instant::now() + timeout, timeout)),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Why the run must end, or `None` to keep going.
    fn reason(&self) -> Option<String> {
        if self.cancel.load(Ordering::Relaxed) {
            return Some("script cancelled".into());
        }
        match self.deadline {
            Some((at, timeout)) if Instant::now() >= at => Some(format!(
                "script timed out after {} s",
                timeout.as_secs_f64()
            )),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ObjectScript {
    pub source: String,
    /// Declared on the last successful run. The editor builds sliders from this.
    pub params: Vec<ScriptParam>,
    /// Rerun after a parameter edit, once the panel debounce elapses.
    pub auto: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptParam {
    pub name: String,
    pub value: ParamValue,
}

#[derive(Clone, Debug)]
pub enum ParamValue {
    Float { value: f32, min: f32, max: f32 },
    Int { value: i32, min: i32, max: i32 },
    Bool(bool),
    Color([u8; 3]),
}

impl PartialEq for ParamValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                ParamValue::Float {
                    value: a,
                    min: amin,
                    max: amax,
                },
                ParamValue::Float {
                    value: b,
                    min: bmin,
                    max: bmax,
                },
            ) => {
                a.to_bits() == b.to_bits()
                    && amin.to_bits() == bmin.to_bits()
                    && amax.to_bits() == bmax.to_bits()
            }
            (
                ParamValue::Int {
                    value: a,
                    min: amin,
                    max: amax,
                },
                ParamValue::Int {
                    value: b,
                    min: bmin,
                    max: bmax,
                },
            ) => a == b && amin == bmin && amax == bmax,
            (ParamValue::Bool(a), ParamValue::Bool(b)) => a == b,
            (ParamValue::Color(a), ParamValue::Color(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for ParamValue {}

struct Built {
    key: BuiltKey,
    cells: HashMap<Cell, u8>,
}

/// A script running on a worker thread for the editor.
pub struct ScriptJob {
    index: usize,
    declare: bool,
    source: String,
    started: Instant,
    stop: Stop,
    done: mpsc::Receiver<Result<Outcome, String>>,
}

#[derive(Clone)]
pub(crate) enum BuiltKey {
    Index(usize),
    Name(String),
}

struct Scratch {
    names: Vec<String>,
    current: usize,
    built: Vec<Built>,
    stored: Vec<ScriptParam>,
    params: Vec<ScriptParam>,
    palette: Vec<(u8, [u8; 3])>,
    motion: Vec<(u8, Motion, f32)>,
    stop: Stop,
}

pub(crate) struct Outcome {
    pub(crate) params: Vec<ScriptParam>,
    pub(crate) built: Vec<(BuiltKey, HashMap<Cell, u8>)>,
    pub(crate) palette: Vec<(u8, [u8; 3])>,
    pub(crate) motion: Vec<(u8, Motion, f32)>,
}

impl Scratch {
    fn cells_mut(&mut self) -> &mut HashMap<Cell, u8> {
        &mut self.built[self.current].cells
    }

    fn paint(&mut self, x: INT, y: INT, z: INT, slot: INT) -> Result<(), String> {
        let cell = coord(x, y, z)?;
        let slot = slot_index(slot)?;
        let cells = self.cells_mut();
        if slot == 0 {
            cells.remove(&cell);
            return Ok(());
        }
        cells.insert(cell, slot);
        Ok(())
    }
}

pub(crate) fn execute(
    source: &str,
    stored: &[ScriptParam],
    host: usize,
    names: &[String],
    stop: &Stop,
) -> Result<Outcome, String> {
    if source.len() > MAX_SOURCE {
        return Err(format!("script is longer than {MAX_SOURCE} bytes"));
    }
    if host >= names.len() {
        return Err(format!(
            "object index {host} is out of range ({} objects)",
            names.len()
        ));
    }
    let scratch = Rc::new(RefCell::new(Scratch {
        names: names.to_vec(),
        current: 0,
        built: vec![Built {
            key: BuiltKey::Index(host),
            cells: HashMap::new(),
        }],
        stored: stored.to_vec(),
        params: Vec::new(),
        palette: Vec::new(),
        motion: Vec::new(),
        stop: stop.clone(),
    }));
    let mut engine = Engine::new_raw();
    engine.set_optimization_level(OptimizationLevel::None);
    let watch = stop.clone();
    engine.on_progress(move |ops| {
        if ops % 1024 != 0 {
            return None;
        }
        watch.reason().map(Dynamic::from)
    });
    engine.set_max_call_levels(MAX_DEPTH);
    engine.set_max_array_size(MAX_ARRAY);
    engine.set_max_map_size(MAX_MAP);
    engine.set_max_string_size(MAX_STRING);
    engine.set_max_expr_depths(64, 32);
    engine.set_strict_variables(true);
    engine.register_iterator::<std::ops::Range<INT>>();
    register(&mut engine, &scratch);
    engine.run(source).map_err(|err| explain(&err))?;
    let scratch = scratch.borrow();
    Ok(Outcome {
        params: scratch.params.clone(),
        built: scratch
            .built
            .iter()
            .map(|built| (built.key.clone(), built.cells.clone()))
            .collect(),
        palette: scratch.palette.clone(),
        motion: scratch.motion.clone(),
    })
}

fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    engine.register_type_with_name::<Rng>("rng");
    engine.register_fn("rng", |seed: INT| Rng { state: seed as u64 });
    engine.register_fn(
        "float",
        |rng: &mut Rng, lo: f64, hi: f64| -> Result<f64, Box<EvalAltResult>> {
            if !lo.is_finite() || !hi.is_finite() || lo > hi {
                return Err(runtime("rng float range is empty"));
            }
            let unit = (rng.next() >> 11) as f64 / ((1u64 << 53) as f64);
            Ok(lo + (hi - lo) * unit)
        },
    );
    engine.register_fn(
        "int",
        |rng: &mut Rng, lo: INT, hi: INT| -> Result<INT, Box<EvalAltResult>> {
            let span = (hi as i128) - (lo as i128) + 1;
            if span <= 0 {
                return Err(runtime("rng int range is empty"));
            }
            Ok(lo + ((rng.next() as i128) % span) as INT)
        },
    );

    engine.register_fn("to_float", |value: INT| value as f64);
    engine.register_fn("to_int", |value: f64| -> Result<INT, Box<EvalAltResult>> {
        if !value.is_finite() {
            return Err(runtime("to_int expected a finite number"));
        }
        Ok(value as INT)
    });
    engine.register_fn("abs", |value: f64| value.abs());
    engine.register_fn("abs", |value: INT| value.saturating_abs());
    engine.register_fn("sin", |value: f64| value.sin());
    engine.register_fn("cos", |value: f64| value.cos());
    engine.register_fn("floor", |value: f64| value.floor());
    engine.register_fn("hypot", |x: f64, y: f64| x.hypot(y));
    engine.register_fn("atan2", |y: f64, x: f64| y.atan2(x));
    engine.register_fn("noise2", |x: f64, y: f64, seed: INT| noise2(x, y, seed));
    engine.register_fn("noise3", |x: f64, y: f64, z: f64, seed: INT| {
        noise3(x, y, z, seed)
    });

    let state = scratch.clone();
    engine.register_fn(
        "param_float",
        move |name: &str, default: f64, min: f64, max: f64| -> Result<f64, Box<EvalAltResult>> {
            state
                .borrow_mut()
                .param_float(name, default, min, max)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "param_int",
        move |name: &str, default: INT, min: INT, max: INT| -> Result<INT, Box<EvalAltResult>> {
            state
                .borrow_mut()
                .param_int(name, default, min, max)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "param_bool",
        move |name: &str, default: bool| -> Result<bool, Box<EvalAltResult>> {
            state
                .borrow_mut()
                .param_bool(name, default)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "param_color",
        move |name: &str, color: Array| -> Result<Array, Box<EvalAltResult>> {
            let rgb = state
                .borrow_mut()
                .param_color(name, &color)
                .map_err(runtime)?;
            Ok(rgb
                .into_iter()
                .map(|channel| Dynamic::from_int(channel as INT))
                .collect())
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "set_slot",
        move |index: INT, color: Array| -> Result<(), Box<EvalAltResult>> {
            state.borrow_mut().set_slot(index, &color).map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "set_motion",
        move |index: INT, kind: &str, amount: f64| -> Result<(), Box<EvalAltResult>> {
            state
                .borrow_mut()
                .set_motion(index, kind, amount)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "set",
        move |x: INT, y: INT, z: INT, slot: INT| -> Result<(), Box<EvalAltResult>> {
            state.borrow_mut().paint(x, y, z, slot).map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "fill_box",
        move |x0: INT,
              y0: INT,
              z0: INT,
              x1: INT,
              y1: INT,
              z1: INT,
              slot: INT|
              -> Result<(), Box<EvalAltResult>> {
            state
                .borrow_mut()
                .fill_box(x0, y0, z0, x1, y1, z1, slot)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "get",
        move |x: INT, y: INT, z: INT| -> Result<INT, Box<EvalAltResult>> {
            let mut scratch = state.borrow_mut();
            let cell = coord(x, y, z).map_err(runtime)?;
            Ok(scratch.cells_mut().get(&cell).copied().unwrap_or(0) as INT)
        },
    );
    let state = scratch.clone();
    engine.register_fn("clear", move || {
        state.borrow_mut().cells_mut().clear();
    });
    let state = scratch.clone();
    engine.register_fn(
        "object",
        move |index: INT| -> Result<(), Box<EvalAltResult>> {
            state.borrow_mut().use_index(index).map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "object",
        move |name: &str| -> Result<(), Box<EvalAltResult>> {
            state.borrow_mut().use_name(name).map_err(runtime)
        },
    );
}

impl Scratch {
    fn declare(&mut self, name: &str, value: ParamValue) -> Result<(), String> {
        let name = check_name(name)?;
        if self.params.iter().any(|param| param.name == name) {
            return Err(format!("parameter `{name}` is declared twice"));
        }
        self.params.push(ScriptParam { name, value });
        Ok(())
    }

    fn stored(&self, name: &str) -> Option<&ParamValue> {
        self.stored
            .iter()
            .find(|param| param.name == name)
            .map(|param| &param.value)
    }

    fn param_float(&mut self, name: &str, default: f64, min: f64, max: f64) -> Result<f64, String> {
        let (default, min, max) = finite3(default, min, max)?;
        if min > max {
            return Err(format!("parameter `{name}` has an empty range"));
        }
        let mut value = match self.stored(name) {
            Some(ParamValue::Float { value, .. }) => *value,
            _ => default,
        };
        value = value.clamp(min, max);
        self.declare(name, ParamValue::Float { value, min, max })?;
        Ok(value as f64)
    }

    fn param_int(&mut self, name: &str, default: INT, min: INT, max: INT) -> Result<INT, String> {
        if min > max {
            return Err(format!("parameter `{name}` has an empty range"));
        }
        let default = i32_from(default, "param_int")?;
        let min = i32_from(min, "param_int")?;
        let max = i32_from(max, "param_int")?;
        let mut value = match self.stored(name) {
            Some(ParamValue::Int { value, .. }) => *value,
            _ => default,
        };
        value = value.clamp(min, max);
        self.declare(name, ParamValue::Int { value, min, max })?;
        Ok(value as INT)
    }

    fn param_bool(&mut self, name: &str, default: bool) -> Result<bool, String> {
        let value = match self.stored(name) {
            Some(ParamValue::Bool(value)) => *value,
            _ => default,
        };
        self.declare(name, ParamValue::Bool(value))?;
        Ok(value)
    }

    fn param_color(&mut self, name: &str, color: &Array) -> Result<[u8; 3], String> {
        let default = rgb(color)?;
        let value = match self.stored(name) {
            Some(ParamValue::Color(value)) => *value,
            _ => default,
        };
        self.declare(name, ParamValue::Color(value))?;
        Ok(value)
    }

    fn set_slot(&mut self, index: INT, color: &Array) -> Result<(), String> {
        let slot = color_slot(index)?;
        let rgb = rgb(color)?;
        if let Some(found) = self.palette.iter_mut().find(|entry| entry.0 == slot) {
            found.1 = rgb;
        } else {
            self.palette.push((slot, rgb));
        }
        Ok(())
    }

    fn set_motion(&mut self, index: INT, kind: &str, amount: f64) -> Result<(), String> {
        let slot = color_slot(index)?;
        let motion = Motion::parse(kind).ok_or("motion must be none, sway, or bob")?;
        if !amount.is_finite() || !(0.0..=4.0).contains(&amount) {
            return Err("motion amount is out of range".into());
        }
        let amount = amount as f32;
        if let Some(found) = self.motion.iter_mut().find(|entry| entry.0 == slot) {
            found.1 = motion;
            found.2 = amount;
        } else {
            self.motion.push((slot, motion, amount));
        }
        Ok(())
    }

    fn fill_box(
        &mut self,
        x0: INT,
        y0: INT,
        z0: INT,
        x1: INT,
        y1: INT,
        z1: INT,
        slot: INT,
    ) -> Result<(), String> {
        let slot = slot_index(slot)?;
        let (x0, x1) = ordered(x0, x1)?;
        let (y0, y1) = ordered(y0, y1)?;
        let (z0, z1) = ordered(z0, z1)?;
        for z in z0..=z1 {
            if let Some(reason) = self.stop.reason() {
                return Err(reason);
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.paint(x as INT, y as INT, z as INT, slot as INT)?;
                }
            }
        }
        Ok(())
    }

    fn use_index(&mut self, index: INT) -> Result<(), String> {
        if index < 0 || index as usize >= self.names.len() {
            return Err(format!(
                "object index {index} is out of range ({} objects)",
                self.names.len()
            ));
        }
        let index = index as usize;
        self.current = self.ensure(BuiltKey::Index(index))?;
        Ok(())
    }

    fn use_name(&mut self, name: &str) -> Result<(), String> {
        let name = check_name(name)?;
        if let Some(index) = self.names.iter().position(|found| found == &name) {
            self.current = self.ensure(BuiltKey::Index(index))?;
            return Ok(());
        }
        self.current = self.ensure(BuiltKey::Name(name))?;
        Ok(())
    }

    fn ensure(&mut self, key: BuiltKey) -> Result<usize, String> {
        if let Some(index) = self.built.iter().position(|built| built.key == key) {
            return Ok(index);
        }
        if self.built.len() >= MAX_OBJECTS {
            return Err(format!("script targets more than {MAX_OBJECTS} objects"));
        }
        self.built.push(Built {
            key,
            cells: HashMap::new(),
        });
        Ok(self.built.len() - 1)
    }
}

impl PartialEq for BuiltKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (BuiltKey::Index(a), BuiltKey::Index(b)) => a == b,
            (BuiltKey::Name(a), BuiltKey::Name(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone)]
struct Rng {
    state: u64,
}

impl Rng {
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn noise2(x: f64, y: f64, seed: INT) -> f64 {
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = fade(x - f64::from(x0));
    let ty = fade(y - f64::from(y0));
    let v00 = hash2(x0, y0, seed);
    let v10 = hash2(x0 + 1, y0, seed);
    let v01 = hash2(x0, y0 + 1, seed);
    let v11 = hash2(x0 + 1, y0 + 1, seed);
    let a = v00 + (v10 - v00) * tx;
    let b = v01 + (v11 - v01) * tx;
    a + (b - a) * ty
}

fn noise3(x: f64, y: f64, z: f64, seed: INT) -> f64 {
    if !z.is_finite() {
        return 0.0;
    }
    let z0 = z.floor() as i32;
    let tz = fade(z - f64::from(z0));
    let a = noise2(x, y, seed.wrapping_add(z0 as INT));
    let b = noise2(x, y, seed.wrapping_add((z0 + 1) as INT));
    a + (b - a) * tz
}

fn hash2(x: i32, y: i32, seed: INT) -> f64 {
    let mut n = (x as u32).wrapping_mul(3_747_61393)
        ^ (y as u32).wrapping_mul(668_265_263)
        ^ (seed as u32).wrapping_mul(1_442_695_041);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    f64::from(n & 0x00FF_FFFF) / f64::from(0x00FF_FFFF)
}

fn fade(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn runtime(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(message.into()),
        Position::NONE,
    ))
}

fn explain(err: &EvalAltResult) -> String {
    let text = match err {
        EvalAltResult::ErrorTerminated(reason, _) => reason.to_string(),
        _ => err.to_string(),
    };
    match err.position().line().filter(|line| *line > 0) {
        Some(line) => format!("line {line}: {text}"),
        None => text,
    }
}

fn check_name(name: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 64 {
        return Err("name must be 1..=64 characters".into());
    }
    Ok(name.to_string())
}

fn finite3(default: f64, min: f64, max: f64) -> Result<(f32, f32, f32), String> {
    for value in [default, min, max] {
        if !value.is_finite() {
            return Err("parameter range must be finite".into());
        }
    }
    Ok((default as f32, min as f32, max as f32))
}

fn i32_from(value: INT, what: &str) -> Result<i32, String> {
    i32::try_from(value).map_err(|_| format!("{what} is outside i32"))
}

fn coord(x: INT, y: INT, z: INT) -> Result<Cell, String> {
    Ok((axis(x)?, axis(y)?, axis(z)?))
}

fn axis(value: INT) -> Result<i32, String> {
    let value = i32::try_from(value)
        .map_err(|_| format!("coordinate is outside -{COORD_LIMIT}..={COORD_LIMIT}"))?;
    if value.abs() > COORD_LIMIT {
        return Err(format!(
            "coordinate is outside -{COORD_LIMIT}..={COORD_LIMIT}"
        ));
    }
    Ok(value)
}

fn ordered(a: INT, b: INT) -> Result<(i32, i32), String> {
    let a = axis(a)?;
    let b = axis(b)?;
    if a <= b {
        Ok((a, b))
    } else {
        Ok((b, a))
    }
}

fn slot_index(slot: INT) -> Result<u8, String> {
    if (0..=255).contains(&slot) {
        Ok(slot as u8)
    } else {
        Err("palette index must be 0..255".into())
    }
}

fn color_slot(slot: INT) -> Result<u8, String> {
    if (1..=255).contains(&slot) {
        Ok(slot as u8)
    } else {
        Err("palette index must be 1..255".into())
    }
}

fn rgb(color: &Array) -> Result<[u8; 3], String> {
    if color.len() != 3 {
        return Err("color must be [r, g, b]".into());
    }
    let mut out = [0u8; 3];
    for (index, channel) in color.iter().enumerate() {
        let value = channel
            .as_int()
            .map_err(|_| "color channel must be an integer")?;
        if !(0..=255).contains(&value) {
            return Err("color channel must be 0..255".into());
        }
        out[index] = value as u8;
    }
    Ok(out)
}

impl Session {
    pub fn set_script(&mut self, index: usize, source: &str) -> Result<(), String> {
        self.script_index(index)?;
        if source.len() > MAX_SOURCE {
            return Err(format!("script is longer than {MAX_SOURCE} bytes"));
        }
        let names = self.object_names();
        let stored = self.objects[index].script.params.clone();
        let declared = if source.is_empty() {
            Vec::new()
        } else {
            match execute(source, &stored, index, &names, &Stop::after(Some(SCRIPT_TIMEOUT))) {
                Ok(outcome) => outcome.params,
                Err(err) => {
                    self.begin_edit();
                    self.objects[index].script.source = source.to_string();
                    self.end_edit();
                    self.script_error = err.clone();
                    return Err(err);
                }
            }
        };
        self.begin_edit();
        self.objects[index].script.source = source.to_string();
        self.objects[index].script.params = declared;
        self.end_edit();
        self.script_error.clear();
        Ok(())
    }

    pub fn set_script_params(
        &mut self,
        index: usize,
        values: &[(String, ParamValue)],
    ) -> Result<(), String> {
        self.script_index(index)?;
        let params = &self.objects[index].script.params;
        for (name, value) in values {
            let Some(found) = params.iter().find(|param| param.name == *name) else {
                return Err(format!("unknown parameter `{name}`"));
            };
            if std::mem::discriminant(&found.value) != std::mem::discriminant(value) {
                return Err(format!("parameter `{name}` has a different type"));
            }
        }
        self.begin_edit();
        for (name, value) in values {
            let param = self.objects[index]
                .script
                .params
                .iter_mut()
                .find(|param| param.name == *name)
                .expect("checked");
            param.value = replace_value(&param.value, value);
        }
        self.end_edit();
        Ok(())
    }

    pub fn write_script_float(&mut self, index: usize, name: &str, value: f32) {
        if let Some(ParamValue::Float {
            value: slot,
            min,
            max,
        }) = self.param_slot(index, name)
        {
            *slot = value.clamp(*min, *max);
        }
    }

    pub fn write_script_int(&mut self, index: usize, name: &str, value: i32) {
        if let Some(ParamValue::Int {
            value: slot,
            min,
            max,
        }) = self.param_slot(index, name)
        {
            *slot = value.clamp(*min, *max);
        }
    }

    pub fn write_script_bool(&mut self, index: usize, name: &str, value: bool) {
        if let Some(ParamValue::Bool(slot)) = self.param_slot(index, name) {
            *slot = value;
        }
    }

    pub fn write_script_color(&mut self, index: usize, name: &str, value: [u8; 3]) {
        if let Some(ParamValue::Color(slot)) = self.param_slot(index, name) {
            *slot = value;
        }
    }

    /// Run the object's script on this thread, stopping at `SCRIPT_TIMEOUT`.
    pub fn regenerate_script(&mut self, index: usize) -> Result<(), String> {
        self.regenerate_script_within(index, Some(SCRIPT_TIMEOUT))
    }

    /// Run the object's script on this thread. `None` waits as long as it takes.
    pub fn regenerate_script_within(
        &mut self,
        index: usize,
        timeout: Option<Duration>,
    ) -> Result<(), String> {
        self.script_index(index)?;
        // A worker run for the editor would land on top of this one.
        self.cancel_script();
        let source = self.objects[index].script.source.clone();
        if source.is_empty() {
            return Err("object has no script".into());
        }
        let names = self.object_names();
        let stored = self.objects[index].script.params.clone();
        let outcome = match execute(&source, &stored, index, &names, &Stop::after(timeout)) {
            Ok(outcome) => outcome,
            Err(err) => {
                self.script_error = err.clone();
                return Err(err);
            }
        };
        self.begin_edit();
        self.apply_outcome(index, outcome);
        self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
        self.end_edit();
        self.script_error.clear();
        Ok(())
    }

    /// Start the object's script on a worker thread. A run already in flight
    /// is cancelled. `declare` only refreshes the parameter list, as
    /// `redeclare_script` does; otherwise the result replaces voxels as one
    /// undo step when `poll_script` collects it.
    pub fn start_script(&mut self, index: usize, declare: bool) -> Result<(), String> {
        self.script_index(index)?;
        let source = self.objects[index].script.source.clone();
        if source.is_empty() {
            if declare {
                self.objects[index].script.params.clear();
                self.script_error.clear();
                return Ok(());
            }
            return Err("object has no script".into());
        }
        self.cancel_script();
        let names = self.object_names();
        let stored = self.objects[index].script.params.clone();
        let stop = Stop::after(None);
        let (send, done) = mpsc::channel();
        let worker = stop.clone();
        let run = source.clone();
        std::thread::Builder::new()
            .name("froxel-script".into())
            .stack_size(16 << 20)
            .spawn(move || {
                let _ = send.send(execute(&run, &stored, index, &names, &worker));
            })
            .map_err(|err| format!("cannot start the script: {err}"))?;
        self.script_job = Some(ScriptJob {
            index,
            declare,
            source,
            started: Instant::now(),
            stop,
            done,
        });
        Ok(())
    }

    /// Stop the run in flight, if any. Its result is dropped.
    pub fn cancel_script(&mut self) {
        if let Some(job) = self.script_job.take() {
            job.stop.cancel();
        }
    }

    /// `(object index, seconds)` of the run in flight.
    pub fn script_running(&self) -> Option<(usize, f64)> {
        self.script_job
            .as_ref()
            .map(|job| (job.index, job.started.elapsed().as_secs_f64()))
    }

    /// Apply a finished worker run. Returns true when one finished this call.
    /// A run whose object was removed or whose source changed is dropped.
    pub fn poll_script(&mut self) -> bool {
        let Some(job) = &self.script_job else {
            return false;
        };
        let result = match job.done.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("the script thread stopped".into()),
        };
        let job = self.script_job.take().expect("checked");
        let current = self
            .objects
            .get(job.index)
            .is_some_and(|object| object.script.source == job.source);
        match result {
            Ok(_) if !current => {
                self.script_error = "the object changed while its script ran".into();
            }
            Ok(outcome) if job.declare => {
                self.objects[job.index].script.params = outcome.params;
                self.script_error.clear();
            }
            Ok(outcome) => {
                self.begin_edit();
                self.apply_outcome(job.index, outcome);
                self.mesh_epoch = self.mesh_epoch.wrapping_add(1);
                self.end_edit();
                self.script_error.clear();
            }
            Err(err) => self.script_error = err,
        }
        true
    }

    fn apply_outcome(&mut self, index: usize, outcome: Outcome) {
        self.objects[index].script.params = outcome.params;
        for (key, cells) in outcome.built {
            match key {
                BuiltKey::Index(target) => {
                    if target < self.objects.len() {
                        self.objects[target].cells = cells;
                    }
                }
                BuiltKey::Name(name) => {
                    if let Some(found) = self.objects.iter().position(|object| object.name == name)
                    {
                        self.objects[found].cells = cells;
                    } else {
                        let translation = beside_host(&self.objects[index]);
                        self.objects.push(Volume {
                            name,
                            cells,
                            translation,
                            box_min: (0, 0, 0),
                            dims: (0, 0, 0),
                            rot: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
                            pivot: (0, 0, 0),
                            model_dims: (0, 0, 0),
                            hidden: false,
                            file: None,
                            bone: None,
                            script: ObjectScript::default(),
                        });
                    }
                }
            }
        }
        for (slot, rgb) in outcome.palette {
            let color = &mut self.palette[slot as usize];
            color[0] = rgb[0];
            color[1] = rgb[1];
            color[2] = rgb[2];
            if color[3] == 0 {
                color[3] = 255;
            }
        }
        for (slot, motion, amount) in outcome.motion {
            let material = &mut self.materials[slot as usize];
            material.motion = motion;
            material.motion_amount = amount.clamp(0.0, 4.0);
        }
    }

    fn param_slot(&mut self, index: usize, name: &str) -> Option<&mut ParamValue> {
        self.objects
            .get_mut(index)?
            .script
            .params
            .iter_mut()
            .find(|param| param.name == name)
            .map(|param| &mut param.value)
    }

    fn script_index(&self, index: usize) -> Result<(), String> {
        if index >= self.objects.len() {
            return Err(format!(
                "object index {index} is out of range ({} objects)",
                self.objects.len()
            ));
        }
        Ok(())
    }

    fn object_names(&self) -> Vec<String> {
        self.objects
            .iter()
            .map(|object| object.name.clone())
            .collect()
    }
}

pub fn parse_param(spec: &ParamValue, value: &serde_json::Value) -> Result<ParamValue, String> {
    match spec {
        ParamValue::Float { min, max, .. } => {
            let number = value.as_f64().ok_or("parameter value must be a number")?;
            if !number.is_finite() {
                return Err("parameter value must be finite".into());
            }
            let number = number as f32;
            if number < *min || number > *max {
                return Err("parameter value is out of range".into());
            }
            Ok(ParamValue::Float {
                value: number,
                min: *min,
                max: *max,
            })
        }
        ParamValue::Int { min, max, .. } => {
            let number = value.as_i64().ok_or("parameter value must be an integer")?;
            let number = i32::try_from(number).map_err(|_| "parameter value is out of range")?;
            if number < *min || number > *max {
                return Err("parameter value is out of range".into());
            }
            Ok(ParamValue::Int {
                value: number,
                min: *min,
                max: *max,
            })
        }
        ParamValue::Bool(_) => {
            let flag = value.as_bool().ok_or("parameter value must be a bool")?;
            Ok(ParamValue::Bool(flag))
        }
        ParamValue::Color(_) => {
            let channels = value
                .as_array()
                .ok_or("parameter value must be [r, g, b]")?;
            if channels.len() != 3 {
                return Err("parameter value must be [r, g, b]".into());
            }
            let mut rgb = [0u8; 3];
            for (index, channel) in channels.iter().enumerate() {
                let number = channel.as_i64().ok_or("color channel must be an integer")?;
                if !(0..=255).contains(&number) {
                    return Err("color channel must be 0..255".into());
                }
                rgb[index] = number as u8;
            }
            Ok(ParamValue::Color(rgb))
        }
    }
}

fn json_f32(value: f32) -> serde_json::Value {
    let mut text = format!("{value:.6}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!(value))
}

pub fn param_json(param: &ScriptParam) -> serde_json::Value {
    let (kind, value, min, max) = match param.value {
        ParamValue::Float { value, min, max } => (
            "float",
            json_f32(value),
            Some(json_f32(min)),
            Some(json_f32(max)),
        ),
        ParamValue::Int { value, min, max } => (
            "int",
            serde_json::json!(value),
            Some(serde_json::json!(min)),
            Some(serde_json::json!(max)),
        ),
        ParamValue::Bool(value) => ("bool", serde_json::json!(value), None, None),
        ParamValue::Color(rgb) => ("color", serde_json::json!(rgb), None, None),
    };
    let mut body = serde_json::json!({
        "name": param.name,
        "kind": kind,
        "value": value,
    });
    if let (Some(min), Some(max)) = (min, max) {
        body["min"] = min;
        body["max"] = max;
    }
    body
}

fn replace_value(spec: &ParamValue, incoming: &ParamValue) -> ParamValue {
    match (spec, incoming) {
        (ParamValue::Float { min, max, .. }, ParamValue::Float { value, .. }) => {
            ParamValue::Float {
                value: value.clamp(*min, *max),
                min: *min,
                max: *max,
            }
        }
        (ParamValue::Int { min, max, .. }, ParamValue::Int { value, .. }) => ParamValue::Int {
            value: (*value).clamp(*min, *max),
            min: *min,
            max: *max,
        },
        (_, ParamValue::Bool(value)) => ParamValue::Bool(*value),
        (_, ParamValue::Color(value)) => ParamValue::Color(*value),
        (spec, _) => spec.clone(),
    }
}

fn beside_host(host: &Volume) -> Cell {
    let max_x = host.cells.keys().map(|cell| cell.0).max().unwrap_or(0);
    (
        host.translation.0 + max_x + 2,
        host.translation.1,
        host.translation.2,
    )
}

pub(crate) fn encode_chunk(objects: &[Volume]) -> Option<Vec<u8>> {
    let kept: Vec<&Volume> = objects
        .iter()
        .filter(|object| !object.script.source.is_empty())
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut body = Vec::new();
    push_u32(&mut body, 1);
    push_u32(&mut body, kept.len() as u32);
    for object in kept {
        push_str(&mut body, &object.name);
        push_str(&mut body, &object.script.source);
        body.push(u8::from(object.script.auto));
        push_u32(&mut body, object.script.params.len() as u32);
        for param in &object.script.params {
            push_str(&mut body, &param.name);
            match param.value {
                ParamValue::Float { value, min, max } => {
                    body.push(0);
                    push_f32(&mut body, value);
                    push_f32(&mut body, min);
                    push_f32(&mut body, max);
                }
                ParamValue::Int { value, min, max } => {
                    body.push(1);
                    push_i32(&mut body, value);
                    push_i32(&mut body, min);
                    push_i32(&mut body, max);
                }
                ParamValue::Bool(value) => {
                    body.push(2);
                    body.push(u8::from(value));
                }
                ParamValue::Color(rgb) => {
                    body.push(3);
                    body.extend_from_slice(&rgb);
                }
            }
        }
    }
    Some(body)
}

pub(crate) fn decode_chunk(body: &[u8]) -> Result<Vec<(String, ObjectScript)>, String> {
    let mut cursor = 0;
    let version = read_u32(body, &mut cursor)?;
    if version != 1 {
        return Err(format!("MVSC version {version} is not supported"));
    }
    let count = read_u32(body, &mut cursor)? as usize;
    if count > 256 {
        return Err("MVSC has too many scripts".into());
    }
    let mut scripts = Vec::with_capacity(count);
    for _ in 0..count {
        let name = read_str(body, &mut cursor)?;
        let source = read_str(body, &mut cursor)?;
        if source.len() > MAX_SOURCE {
            return Err(format!("MVSC script is longer than {MAX_SOURCE} bytes"));
        }
        let auto = read_u8(body, &mut cursor)?;
        if auto > 1 {
            return Err("MVSC auto flag is not 0 or 1".into());
        }
        let params_len = read_u32(body, &mut cursor)? as usize;
        if params_len > 64 {
            return Err("MVSC has too many parameters".into());
        }
        let mut params = Vec::with_capacity(params_len);
        for _ in 0..params_len {
            let name = read_str(body, &mut cursor)?;
            let kind = read_u8(body, &mut cursor)?;
            let value = match kind {
                0 => ParamValue::Float {
                    value: read_f32(body, &mut cursor)?,
                    min: read_f32(body, &mut cursor)?,
                    max: read_f32(body, &mut cursor)?,
                },
                1 => ParamValue::Int {
                    value: read_i32(body, &mut cursor)?,
                    min: read_i32(body, &mut cursor)?,
                    max: read_i32(body, &mut cursor)?,
                },
                2 => {
                    let flag = read_u8(body, &mut cursor)?;
                    if flag > 1 {
                        return Err("MVSC bool is not 0 or 1".into());
                    }
                    ParamValue::Bool(flag == 1)
                }
                3 => ParamValue::Color([
                    read_u8(body, &mut cursor)?,
                    read_u8(body, &mut cursor)?,
                    read_u8(body, &mut cursor)?,
                ]),
                _ => return Err(format!("MVSC parameter kind {kind} is not supported")),
            };
            params.push(ScriptParam { name, value });
        }
        scripts.push((
            name,
            ObjectScript {
                source,
                params,
                auto: auto == 1,
            },
        ));
    }
    if cursor != body.len() {
        return Err("MVSC chunk has trailing bytes".into());
    }
    Ok(scripts)
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

fn read_u32(body: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let bytes = read_n(body, cursor, 4)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_i32(body: &[u8], cursor: &mut usize) -> Result<i32, String> {
    Ok(read_u32(body, cursor)? as i32)
}

fn read_f32(body: &[u8], cursor: &mut usize) -> Result<f32, String> {
    let bytes = read_n(body, cursor, 4)?;
    Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u8(body: &[u8], cursor: &mut usize) -> Result<u8, String> {
    Ok(read_n(body, cursor, 1)?[0])
}

fn read_str(body: &[u8], cursor: &mut usize) -> Result<String, String> {
    let len = read_u32(body, cursor)? as usize;
    if len > MAX_SOURCE {
        return Err("MVSC string is too long".into());
    }
    let bytes = read_n(body, cursor, len)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| "MVSC string is not utf-8".into())
}

fn read_n<'a>(body: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8], String> {
    let end = cursor.checked_add(len).ok_or("MVSC chunk is truncated")?;
    if end > body.len() {
        return Err("MVSC chunk is truncated".into());
    }
    let slice = &body[*cursor..end];
    *cursor = end;
    Ok(slice)
}

#[cfg(test)]
#[path = "../tests/script.rs"]
mod tests;
