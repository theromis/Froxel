//! The MCP stone tools in Rhai: `joints`, `column_relief`, `weather`,
//! `moss`, `algae`, `vines`, and `sweep`.
//!
//! Each call turns its arguments into the MCP tool's arguments and runs
//! `remote::stone_writes` on the object being written, as the MCP handler
//! does on the active object. Options use the MCP names in a trailing map.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, Map, INT};
use serde_json::Value;

use super::shapes::{box_args, json, options, Args, Fallible};
use super::{runtime, Scratch};
use crate::remote::{self, Stone};

const JOINTS: &[&str] = &[
    "spacing", "course", "width", "depth", "dirt", "only", "density", "exclude", "seed",
];

const RELIEF: &[&str] = &[
    "width", "out", "crown", "top", "lower", "depth", "exclude", "seed",
];

const WEATHER: &[&str] = &[
    "chip",
    "chipSize",
    "pits",
    "pitDepth",
    "broken",
    "noise",
    "noiseDensity",
    "noiseScale",
    "only",
    "exclude",
    "seed",
];

const MOSS: &[&str] = &[
    "amount",
    "noise",
    "faces",
    "facing",
    "band",
    "only",
    "exclude",
    "layer",
    "curtains",
    "length",
    "thickness",
    "seed",
];
const ALGAE: &[&str] = &[
    "amount", "noise", "faces", "facing", "band", "only", "exclude", "layer", "seed",
];
const VINES: &[&str] = &[
    "amount",
    "noise",
    "faces",
    "facing",
    "band",
    "only",
    "exclude",
    "length",
    "blooms",
    "bloomChance",
    "seed",
];

const SWEEP: &[&str] = &["curve", "pattern", "period", "seed", "mode"];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run =
            move |points: Array, profile: Dynamic, palette: Dynamic, opts: &Map| -> Fallible<Map> {
                let mut args = options(opts, SWEEP, "sweep")?;
                args.insert("points".into(), json(&Dynamic::from_array(points))?);
                if profile.is_array() {
                    args.insert("profile".into(), json(&profile)?);
                } else if profile.is_map() {
                    let Value::Object(rect) = json(&profile)? else {
                        unreachable!("a map is an object");
                    };
                    for (key, value) in rect {
                        if !["width", "height", "radius"].contains(&key.as_str()) {
                            return Err(runtime(format!(
                                "sweep profile map takes width, height, and radius, not `{key}`"
                            )));
                        }
                        args.insert(key, value);
                    }
                } else {
                    return Err(runtime(
                        "sweep profile must be a list of [u, v] points or #{width, height, radius}",
                    ));
                }
                if palette.is_array() {
                    args.insert("palettes".into(), json(&palette)?);
                } else if palette.is_int() {
                    args.insert("palette".into(), json(&palette)?);
                } else {
                    return Err(runtime(
                        "sweep palette must be an index or a list of indices",
                    ));
                }
                state
                    .borrow_mut()
                    .stone(args, Stone::Sweep)
                    .map_err(runtime)
            };
        if with_opts {
            engine.register_fn(
                "sweep",
                move |points: Array, profile: Dynamic, palette: Dynamic, opts: Map| {
                    run(points, profile, palette, &opts)
                },
            );
        } else {
            engine.register_fn(
                "sweep",
                move |points: Array, profile: Dynamic, palette: Dynamic| {
                    run(points, profile, palette, &Map::new())
                },
            );
        }
    }
    for (name, tool, allowed) in [
        ("moss", Stone::Moss, MOSS),
        ("algae", Stone::Algae, ALGAE),
        ("vines", Stone::Vines, VINES),
    ] {
        for with_opts in [false, true] {
            let state = scratch.clone();
            let run = move |corners: [INT; 6], palettes: Dynamic, opts: &Map| -> Fallible<Map> {
                let mut args = options(opts, allowed, name)?;
                args.extend(box_args(corners)?);
                args.insert("palettes".into(), json(&palettes)?);
                state.borrow_mut().stone(args, tool).map_err(runtime)
            };
            if with_opts {
                engine.register_fn(
                    name,
                    move |x0: INT,
                          y0: INT,
                          z0: INT,
                          x1: INT,
                          y1: INT,
                          z1: INT,
                          palettes: Dynamic,
                          opts: Map| {
                        run([x0, y0, z0, x1, y1, z1], palettes, &opts)
                    },
                );
            } else {
                engine.register_fn(
                    name,
                    move |x0: INT,
                          y0: INT,
                          z0: INT,
                          x1: INT,
                          y1: INT,
                          z1: INT,
                          palettes: Dynamic| {
                        run([x0, y0, z0, x1, y1, z1], palettes, &Map::new())
                    },
                );
            }
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run =
            move |corners: [INT; 6], facing: Dynamic, palette: INT, opts: &Map| -> Fallible<Map> {
                let mut args = options(opts, RELIEF, "column_relief")?;
                args.extend(box_args(corners)?);
                args.insert("facing".into(), json(&facing)?);
                args.insert("palette".into(), palette.into());
                state
                    .borrow_mut()
                    .stone(args, Stone::Relief)
                    .map_err(runtime)
            };
        if with_opts {
            engine.register_fn(
                "column_relief",
                move |x0: INT,
                      y0: INT,
                      z0: INT,
                      x1: INT,
                      y1: INT,
                      z1: INT,
                      facing: Dynamic,
                      palette: INT,
                      opts: Map| {
                    run([x0, y0, z0, x1, y1, z1], facing, palette, &opts)
                },
            );
        } else {
            engine.register_fn(
                "column_relief",
                move |x0: INT,
                      y0: INT,
                      z0: INT,
                      x1: INT,
                      y1: INT,
                      z1: INT,
                      facing: Dynamic,
                      palette: INT| {
                    run([x0, y0, z0, x1, y1, z1], facing, palette, &Map::new())
                },
            );
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 6], opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, WEATHER, "weather")?;
            args.extend(box_args(corners)?);
            state
                .borrow_mut()
                .stone(args, Stone::Weather)
                .map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "weather",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, opts: Map| {
                    run([x0, y0, z0, x1, y1, z1], &opts)
                },
            );
        } else {
            engine.register_fn(
                "weather",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT| {
                    run([x0, y0, z0, x1, y1, z1], &Map::new())
                },
            );
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 6], pattern: &str, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, JOINTS, "joints")?;
            args.extend(box_args(corners)?);
            args.insert("pattern".into(), pattern.into());
            state
                .borrow_mut()
                .stone(args, Stone::Joints)
                .map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "joints",
                move |x0: INT,
                      y0: INT,
                      z0: INT,
                      x1: INT,
                      y1: INT,
                      z1: INT,
                      pattern: &str,
                      opts: Map| { run([x0, y0, z0, x1, y1, z1], pattern, &opts) },
            );
        } else {
            engine.register_fn(
                "joints",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, pattern: &str| {
                    run([x0, y0, z0, x1, y1, z1], pattern, &Map::new())
                },
            );
        }
    }
}

impl Scratch {
    /// Run a stone tool on the object being written. The reply holds
    /// `changed` and the tool's own counts.
    fn stone(&mut self, args: Args, tool: Stone) -> Result<Map, String> {
        self.keep_going()?;
        let (writes, counts) = remote::stone_writes(self.cells(), &Value::Object(args), tool)?;
        let changed = self.apply(&writes)?;
        let mut reply: Map = counts
            .into_iter()
            .map(|(key, value)| {
                let value = match value.as_i64() {
                    Some(int) => Dynamic::from_int(int),
                    None => Dynamic::from_float(value.as_f64().unwrap_or(0.0)),
                };
                (key.into(), value)
            })
            .collect();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        Ok(reply)
    }
}
