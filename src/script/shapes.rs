//! The MCP shape tools in Rhai: `fill_ellipsoid`, `fill_rounded_box`,
//! `fill_tube`, `fill_box` with a mode, `carve_*`, `smooth`, `break_end`,
//! and `scatter`.
//!
//! Each call turns its arguments into the MCP tool's arguments and runs the
//! same parser and shape code (`remote::fill_writes`, `remote::carve_writes`,
//! `remote::smooth_args`, `remote::break_spec`, `remote::scatter_spec`) on
//! the object being written. Options use the MCP names, so a trailing map
//! such as `#{mode: "empty", aspect: 0.5}` reads like the tool call.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, INT};
use serde_json::Value;

use super::{axis, coord, runtime, slot_index, Scratch};
use crate::remote::{self, Sculpt};

const FILL: &[&str] = &["mode", "space"];
const CARVE: &[&str] = &["space"];
const TUBE_SHAPE: &[&str] = &["radius", "curve", "aspect", "roll", "profile", "space"];
const TUBE: &[&str] = &[
    "mode", "radius", "curve", "aspect", "roll", "profile", "pattern", "period", "sectors", "seed",
    "space",
];
const SMOOTH: &[&str] = &["iterations", "fillAt", "removeAt", "space"];
const BREAK: &[&str] = &[
    "depth",
    "splinters",
    "density",
    "length",
    "thickness",
    "taper",
    "rimBias",
    "bark",
    "sapwood",
    "tips",
    "seed",
];
const SCATTER: &[&str] = &[
    "count", "kinds", "size", "seed", "floor", "clump", "exclude",
];

pub(super) type Fallible<T> = Result<T, Box<EvalAltResult>>;
pub(super) type Args = serde_json::Map<String, Value>;

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    let state = scratch.clone();
    engine.register_fn(
        "fill_box",
        move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, slot: INT, opts: Map| {
            let args = corners([x0, y0, z0, x1, y1, z1], slot, &opts, FILL, "fill_box")?;
            state.borrow_mut().fill(args, Sculpt::Box).map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "fill_ellipsoid",
        move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, slot: INT| {
            let args = corners([x0, y0, z0, x1, y1, z1], slot, &Map::new(), FILL, "")?;
            state
                .borrow_mut()
                .fill(args, Sculpt::Ellipsoid)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "fill_ellipsoid",
        move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, slot: INT, opts: Map| {
            let args = corners(
                [x0, y0, z0, x1, y1, z1],
                slot,
                &opts,
                FILL,
                "fill_ellipsoid",
            )?;
            state
                .borrow_mut()
                .fill(args, Sculpt::Ellipsoid)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "fill_rounded_box",
        move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, radius: Dynamic, slot: INT| {
            let mut args = corners(
                [x0, y0, z0, x1, y1, z1],
                slot,
                &Map::new(),
                FILL,
                "fill_rounded_box",
            )?;
            args.insert("radius".into(), number(&radius, "radius")?);
            state
                .borrow_mut()
                .fill(args, Sculpt::RoundedBox)
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "fill_rounded_box",
        move |x0: INT,
              y0: INT,
              z0: INT,
              x1: INT,
              y1: INT,
              z1: INT,
              radius: Dynamic,
              slot: INT,
              opts: Map| {
            let mut args = corners(
                [x0, y0, z0, x1, y1, z1],
                slot,
                &opts,
                FILL,
                "fill_rounded_box",
            )?;
            args.insert("radius".into(), number(&radius, "radius")?);
            state
                .borrow_mut()
                .fill(args, Sculpt::RoundedBox)
                .map_err(runtime)
        },
    );
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |points: Array, palette: Dynamic, opts: &Map| -> Fallible<INT> {
            let mut args = options(opts, TUBE, "fill_tube")?;
            args.insert("points".into(), json(&Dynamic::from_array(points))?);
            if palette.is_array() {
                args.insert("palettes".into(), json(&palette)?);
            } else if palette.is_int() {
                args.insert("palette".into(), json(&palette)?);
            } else {
                return Err(runtime(
                    "fill_tube palette must be an index or a list of indices",
                ));
            }
            state.borrow_mut().fill(args, Sculpt::Tube).map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "fill_tube",
                move |points: Array, palette: Dynamic, opts: Map| run(points, palette, &opts),
            );
        } else {
            engine.register_fn("fill_tube", move |points: Array, palette: Dynamic| {
                run(points, palette, &Map::new())
            });
        }
    }

    for (name, shape) in [("carve_box", "box"), ("carve_ellipsoid", "ellipsoid")] {
        let state = scratch.clone();
        engine.register_fn(
            name,
            move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT| {
                let mut args = box_args([x0, y0, z0, x1, y1, z1])?;
                args.insert("shape".into(), shape.into());
                state.borrow_mut().carve(args).map_err(runtime)
            },
        );
        let state = scratch.clone();
        engine.register_fn(
            name,
            move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, opts: Map| {
                let mut args = options(&opts, CARVE, name)?;
                args.extend(box_args([x0, y0, z0, x1, y1, z1])?);
                args.insert("shape".into(), shape.into());
                state.borrow_mut().carve(args).map_err(runtime)
            },
        );
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 6], radius: Dynamic, opts: &Map| -> Fallible<INT> {
            let mut args = options(opts, CARVE, "carve_rounded_box")?;
            args.extend(box_args(corners)?);
            args.insert("shape".into(), "rounded_box".into());
            args.insert("radius".into(), number(&radius, "radius")?);
            state.borrow_mut().carve(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "carve_rounded_box",
                move |x0: INT,
                      y0: INT,
                      z0: INT,
                      x1: INT,
                      y1: INT,
                      z1: INT,
                      radius: Dynamic,
                      opts: Map| { run([x0, y0, z0, x1, y1, z1], radius, &opts) },
            );
        } else {
            engine.register_fn(
                "carve_rounded_box",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, radius: Dynamic| {
                    run([x0, y0, z0, x1, y1, z1], radius, &Map::new())
                },
            );
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |points: Array, opts: &Map| -> Fallible<INT> {
            let mut args = options(opts, TUBE_SHAPE, "carve_tube")?;
            args.insert("shape".into(), "tube".into());
            args.insert("points".into(), json(&Dynamic::from_array(points))?);
            state.borrow_mut().carve(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn("carve_tube", move |points: Array, opts: Map| {
                run(points, &opts)
            });
        } else {
            engine.register_fn("carve_tube", move |points: Array| run(points, &Map::new()));
        }
    }

    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 6], opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, SMOOTH, "smooth")?;
            args.extend(box_args(corners)?);
            state.borrow_mut().smooth(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "smooth",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, opts: Map| {
                    run([x0, y0, z0, x1, y1, z1], &opts)
                },
            );
        } else {
            engine.register_fn(
                "smooth",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT| {
                    run([x0, y0, z0, x1, y1, z1], &Map::new())
                },
            );
        }
    }

    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |point: Array, normal: Array, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, BREAK, "break_end")?;
            args.insert("point".into(), json(&Dynamic::from_array(point))?);
            args.insert("normal".into(), json(&Dynamic::from_array(normal))?);
            state.borrow_mut().break_end(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "break_end",
                move |point: Array, normal: Array, opts: Map| run(point, normal, &opts),
            );
        } else {
            engine.register_fn("break_end", move |point: Array, normal: Array| {
                run(point, normal, &Map::new())
            });
        }
    }

    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 6], palettes: Dynamic, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, SCATTER, "scatter")?;
            args.extend(box_args(corners)?);
            args.insert("palettes".into(), json(&palettes)?);
            state.borrow_mut().scatter(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "scatter",
                move |x0: INT,
                      y0: INT,
                      z0: INT,
                      x1: INT,
                      y1: INT,
                      z1: INT,
                      palettes: Dynamic,
                      opts: Map| { run([x0, y0, z0, x1, y1, z1], palettes, &opts) },
            );
        } else {
            engine.register_fn(
                "scatter",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, palettes: Dynamic| {
                    run([x0, y0, z0, x1, y1, z1], palettes, &Map::new())
                },
            );
        }
    }
}

impl Scratch {
    fn fill(&mut self, args: Args, sculpt: Sculpt) -> Result<INT, String> {
        self.keep_going()?;
        let method = match sculpt {
            Sculpt::Box => "voxel.fill",
            Sculpt::Ellipsoid => "voxel.ellipsoid",
            Sculpt::RoundedBox => "voxel.rounded",
            Sculpt::Tube => "voxel.tube",
        };
        let (args, _) = self.localize(method, args)?;
        let writes = remote::fill_writes(self.cells(), &args, sculpt)?;
        self.apply(&writes)
    }

    fn carve(&mut self, args: Args) -> Result<INT, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.carve", args)?;
        let writes = remote::carve_writes(self.cells(), &args)?;
        self.apply(&writes)
    }

    /// The MCP arguments of `method` in local cells of the object being
    /// written: with `space: "world"`, its position when the run started is
    /// subtracted (`remote::to_local`) and returned. An object the run
    /// creates has no position yet, so it takes local cells only.
    pub(super) fn localize(
        &self,
        method: &str,
        args: Args,
    ) -> Result<(Value, Option<crate::state::Cell>), String> {
        let mut args = Value::Object(args);
        if !remote::is_world(&args)? {
            if let Some(map) = args.as_object_mut() {
                map.remove("space");
            }
            return Ok((args, None));
        }
        let offset = match &self.built[self.current].key {
            super::BuiltKey::Index(index) => self.view.positions.get(*index).copied(),
            super::BuiltKey::Name(_) => None,
        }
        .ok_or("space world needs an object of the document; an object this run creates takes local cells")?;
        Ok((remote::to_local(method, &args, offset)?, Some(offset)))
    }

    fn smooth(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.smooth", args)?;
        let (min, max, iterations, fill_at, remove_at) = remote::smooth_args(&args)?;
        let report =
            crate::tools::smooth_checked(self.cells(), min, max, iterations, fill_at, remove_at)?;
        let changed = self.apply(&report.writes)?;
        Ok(reply([
            ("changed", changed),
            ("filled", report.filled as INT),
            ("removed", report.removed as INT),
            ("iterations", INT::from(report.iterations)),
        ]))
    }

    fn break_end(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let spec = remote::break_spec(&Value::Object(args))?;
        let broken = crate::tools::break_end(self.cells(), &spec)?;
        let changed = self.apply(&broken.writes)?;
        Ok(reply([
            ("changed", changed),
            ("removed", broken.removed as INT),
            ("recoloured", broken.recoloured as INT),
            ("splinters", broken.splinters as INT),
        ]))
    }

    fn scatter(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let spec = remote::scatter_spec(&Value::Object(args))?;
        let done = crate::tools::scatter(self.cells(), &spec)?;
        let changed = self.apply(&done.writes)?;
        Ok(reply([
            ("changed", changed),
            ("pieces", done.placed as INT),
            ("skipped", done.skipped as INT),
        ]))
    }

    pub(super) fn keep_going(&self) -> Result<(), String> {
        match self.stop.reason() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    pub(super) fn cells(&self) -> &std::collections::HashMap<crate::state::Cell, u8> {
        &self.built[self.current].cells
    }

    /// Write `(x, y, z, palette)` cells, 0 erasing, after checking every one
    /// is inside the coordinate range. Returns how many cells changed.
    pub(super) fn apply(&mut self, writes: &[(i32, i32, i32, i32)]) -> Result<INT, String> {
        let mut checked = Vec::with_capacity(writes.len());
        for &(x, y, z, slot) in writes {
            checked.push((
                coord(x.into(), y.into(), z.into())?,
                slot_index(slot.into())?,
            ));
        }
        let cells = self.cells_mut();
        let mut changed = 0;
        for (cell, slot) in checked {
            let before = if slot == 0 {
                cells.remove(&cell)
            } else {
                cells.insert(cell, slot)
            };
            if before != (slot != 0).then_some(slot) {
                changed += 1;
            }
        }
        Ok(changed)
    }
}

/// The box corners, each inside the coordinate range, as `x0..z1`.
pub(super) fn box_args(corners: [INT; 6]) -> Fallible<Args> {
    let mut args = Args::new();
    for (name, value) in ["x0", "y0", "z0", "x1", "y1", "z1"].iter().zip(corners) {
        axis(value).map_err(runtime)?;
        args.insert((*name).into(), value.into());
    }
    Ok(args)
}

/// A fill's arguments: its options, the corners, and `palette`.
fn corners(
    corners: [INT; 6],
    slot: INT,
    opts: &Map,
    allowed: &[&str],
    function: &str,
) -> Fallible<Args> {
    let mut args = options(opts, allowed, function)?;
    args.extend(box_args(corners)?);
    args.insert("palette".into(), slot.into());
    Ok(args)
}

/// An options map as MCP arguments. A key the function does not take is an
/// error, so a misspelt option does not pass unnoticed.
pub(super) fn options(opts: &Map, allowed: &[&str], function: &str) -> Fallible<Args> {
    let mut args = Args::new();
    for (key, value) in opts {
        if !allowed.contains(&key.as_str()) {
            return Err(runtime(format!(
                "{function} has no option `{key}`; it takes {}",
                allowed.join(", ")
            )));
        }
        args.insert(key.to_string(), json(value)?);
    }
    Ok(args)
}

fn number(value: &Dynamic, what: &str) -> Fallible<Value> {
    if value.is_int() || value.is_float() {
        json(value)
    } else {
        Err(runtime(format!("{what} must be a number")))
    }
}

/// A Rhai value as JSON: numbers, strings, bools, arrays, and maps.
pub(super) fn json(value: &Dynamic) -> Fallible<Value> {
    if let Ok(int) = value.as_int() {
        return Ok(int.into());
    }
    if let Ok(float) = value.as_float() {
        return serde_json::Number::from_f64(float)
            .map(Value::Number)
            .ok_or_else(|| runtime("numbers must be finite"));
    }
    if let Ok(flag) = value.as_bool() {
        return Ok(flag.into());
    }
    if value.is_unit() {
        return Ok(Value::Null);
    }
    if value.is_string() {
        return Ok(value.clone().into_string().map_err(runtime)?.into());
    }
    if value.is_array() {
        let items = value
            .read_lock::<Array>()
            .ok_or_else(|| runtime("bad list"))?;
        return items
            .iter()
            .map(json)
            .collect::<Fallible<_>>()
            .map(Value::Array);
    }
    if value.is_map() {
        let map = value.read_lock::<Map>().ok_or_else(|| runtime("bad map"))?;
        let mut out = Args::new();
        for (key, item) in map.iter() {
            out.insert(key.to_string(), json(item)?);
        }
        return Ok(Value::Object(out));
    }
    Err(runtime(format!(
        "a {} cannot be a tool argument",
        value.type_name()
    )))
}

fn reply<const N: usize>(fields: [(&str, INT); N]) -> Map {
    fields
        .into_iter()
        .map(|(key, value)| (key.into(), Dynamic::from_int(value)))
        .collect()
}
