//! The MCP stone and surface tools in Rhai: `joints`, `column_relief`,
//! `weather`, `moss`, `algae`, `vines`, `sweep`, `trim_outline`, `bark`, `planks`,
//! `cavity_tint`, and `foliage`.
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
    "style",
    "size",
    "gap",
    "exposure",
    "drapes",
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

const TRIM: &[&str] = &["curve", "only", "skin", "thickness"];

const BARK: &[&str] = &[
    "axis", "points", "curve", "dirt", "width", "length", "depth", "cracks", "only", "exclude",
    "seed",
];

const PLANKS: &[&str] = &[
    "dirt",
    "dark",
    "width",
    "length",
    "gap",
    "thickness",
    "ragged",
    "nails",
    "knots",
    "only",
    "exclude",
    "seed",
];

const FOLIAGE: &[&str] = &[
    "ellipsoids",
    "boxes",
    "from",
    "core",
    "size",
    "gap",
    "flatten",
    "ragged",
    "light",
    "dark",
    "sun",
    "masses",
    "groove",
    "protect",
    "exclude",
    "seed",
];

const CAVITY: &[&str] = &[
    "radius", "dark", "lighten", "light", "pairs", "darken", "brighten", "only", "exclude",
];

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
    // `trim_outline(outline, inset, opts)` tests the whole object;
    // `trim_outline(x0, y0, z0, x1, y1, z1, outline, inset, opts)` keeps to a box.
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: Option<[INT; 6]>,
                        outline: Array,
                        inset: Dynamic,
                        opts: &Map|
              -> Fallible<Map> {
            let mut args = options(opts, TRIM, "trim_outline")?;
            if let Some(corners) = corners {
                args.extend(box_args(corners)?);
            }
            args.insert("outline".into(), json(&Dynamic::from_array(outline))?);
            args.insert("inset".into(), json(&inset)?);
            state.borrow_mut().stone(args, Stone::Trim).map_err(runtime)
        };
        let boxed = run.clone();
        if with_opts {
            engine.register_fn(
                "trim_outline",
                move |outline: Array, inset: Dynamic, opts: Map| run(None, outline, inset, &opts),
            );
            engine.register_fn(
                "trim_outline",
                move |x0: INT,
                      y0: INT,
                      z0: INT,
                      x1: INT,
                      y1: INT,
                      z1: INT,
                      outline: Array,
                      inset: Dynamic,
                      opts: Map| {
                    boxed(Some([x0, y0, z0, x1, y1, z1]), outline, inset, &opts)
                },
            );
        } else {
            engine.register_fn("trim_outline", move |outline: Array, inset: Dynamic| {
                run(None, outline, inset, &Map::new())
            });
            engine.register_fn(
                "trim_outline",
                move |x0: INT,
                      y0: INT,
                      z0: INT,
                      x1: INT,
                      y1: INT,
                      z1: INT,
                      outline: Array,
                      inset: Dynamic| {
                    boxed(Some([x0, y0, z0, x1, y1, z1]), outline, inset, &Map::new())
                },
            );
        }
    }
    for (name, tool, allowed) in [
        ("moss", Stone::Moss, MOSS),
        ("algae", Stone::Algae, ALGAE),
        ("vines", Stone::Vines, VINES),
        ("bark", Stone::Bark, BARK),
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
    // `planks(x0, y0, x1, y1, z, axis, palettes, opts)`: a rectangle at a
    // height, not a box.
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 4],
                        z: INT,
                        axis: &str,
                        palettes: Dynamic,
                        opts: &Map|
              -> Fallible<Map> {
            let mut args = options(opts, PLANKS, "planks")?;
            for (key, value) in ["x0", "y0", "x1", "y1"].into_iter().zip(corners) {
                args.insert(key.into(), value.into());
            }
            args.insert("z".into(), z.into());
            args.insert("axis".into(), axis.into());
            args.insert("palettes".into(), json(&palettes)?);
            state
                .borrow_mut()
                .stone(args, Stone::Planks)
                .map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "planks",
                move |x0: INT,
                      y0: INT,
                      x1: INT,
                      y1: INT,
                      z: INT,
                      axis: &str,
                      palettes: Dynamic,
                      opts: Map| {
                    run([x0, y0, x1, y1], z, axis, palettes, &opts)
                },
            );
        } else {
            engine.register_fn(
                "planks",
                move |x0: INT, y0: INT, x1: INT, y1: INT, z: INT, axis: &str, palettes: Dynamic| {
                    run([x0, y0, x1, y1], z, axis, palettes, &Map::new())
                },
            );
        }
    }
    // `foliage(palettes, opts)` bounds itself by its volume;
    // `foliage(x0, y0, z0, x1, y1, z1, palettes, opts)` keeps to a box. The
    // options name the volume, so there is no form without them.
    let state = scratch.clone();
    let run = move |corners: Option<[INT; 6]>, palettes: Dynamic, opts: &Map| -> Fallible<Map> {
        let mut args = options(opts, FOLIAGE, "foliage")?;
        if let Some(corners) = corners {
            args.extend(box_args(corners)?);
        }
        args.insert("palettes".into(), json(&palettes)?);
        state
            .borrow_mut()
            .stone(args, Stone::Foliage)
            .map_err(runtime)
    };
    let boxed = run.clone();
    engine.register_fn("foliage", move |palettes: Dynamic, opts: Map| {
        run(None, palettes, &opts)
    });
    engine.register_fn(
        "foliage",
        move |x0: INT,
              y0: INT,
              z0: INT,
              x1: INT,
              y1: INT,
              z1: INT,
              palettes: Dynamic,
              opts: Map| { boxed(Some([x0, y0, z0, x1, y1, z1]), palettes, &opts) },
    );
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 6], opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, CAVITY, "cavity_tint")?;
            args.extend(box_args(corners)?);
            state.borrow_mut().cavity(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn(
                "cavity_tint",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, opts: Map| {
                    run([x0, y0, z0, x1, y1, z1], &opts)
                },
            );
        } else {
            engine.register_fn(
                "cavity_tint",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT| {
                    run([x0, y0, z0, x1, y1, z1], &Map::new())
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
    /// MCP `cavity_tint` on the object being written. Shades it makes go
    /// into free slots of the palette as the run leaves it: not one a cell
    /// of the document or of this run uses, nor one `set_palette` named.
    fn cavity(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let mut shades = self.run_shades();
        let (writes, counts) =
            remote::cavity_writes(self.cells(), &Value::Object(args), &mut shades)?;
        self.shades.extend(shades.created.iter().copied());
        let changed = self.apply(&writes)?;
        let mut reply: Map = counts
            .iter()
            .map(|(key, value)| (key.as_str().into(), super::document::dynamic(value)))
            .collect();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        Ok(reply)
    }

    /// The palette as the run leaves it, for a tool that makes slots
    /// (`cavity_tint`, `paint_from_views`, `reduce_palette`): a slot a cell
    /// of the document or of this run uses, or one `set_palette` named or an
    /// earlier tool made, is taken. The caller adds `created` to
    /// `self.shades`.
    pub(super) fn run_shades(&self) -> crate::tools::Shades {
        let mut shades = match &self.view.shades {
            Some(shades) => (**shades).clone(),
            None => crate::state::Session::blank_shades(),
        };
        for (slot, rgb) in &self.palette {
            let color = &mut shades.colors[*slot as usize];
            color[..3].copy_from_slice(rgb);
            if color[3] == 0 {
                color[3] = 255;
            }
            shades.used[*slot as usize] = true;
        }
        for built in &self.built {
            for slot in built.cells.values() {
                shades.used[*slot as usize] = true;
            }
        }
        for shade in &self.shades {
            shades.colors[shade.slot as usize] = shade.rgba;
            shades.materials[shade.slot as usize] = shades.materials[shade.from as usize];
            shades.used[shade.slot as usize] = true;
        }
        shades.created.clear();
        shades
    }

    /// Run a stone tool on the object being written. The reply holds
    /// `changed` and the tool's own counts.
    fn stone(&mut self, args: Args, tool: Stone) -> Result<Map, String> {
        self.keep_going()?;
        let (writes, counts) = remote::stone_writes(self.cells(), &Value::Object(args), tool)?;
        let changed = self.apply(&writes)?;
        Ok(reply(counts, changed))
    }
}

/// A tool's counts and `changed` as a Rhai map.
pub(super) fn reply(counts: serde_json::Map<String, Value>, changed: INT) -> Map {
    let mut reply: Map = counts
        .into_iter()
        .map(|(key, value)| {
            let value = match (value.as_i64(), value.as_str()) {
                (Some(int), _) => Dynamic::from_int(int),
                (None, Some(text)) => Dynamic::from(text.to_string()),
                // A map or a list (deform's `thin`) keeps its shape.
                _ if value.is_object() || value.is_array() => super::document::dynamic(&value),
                (None, None) => Dynamic::from_float(value.as_f64().unwrap_or(0.0)),
            };
            (key.into(), value)
        })
        .collect();
    reply.insert("changed".into(), Dynamic::from_int(changed));
    reply
}
