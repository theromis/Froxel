//! The MCP organic shaping tools in Rhai: `deform`, `loft`, and `brush`.
//!
//! Each call turns its arguments into the MCP tool's arguments and runs
//! `remote::organic_writes` on the object being written, as the MCP handler
//! does on the active object. Options use the MCP names in a trailing map.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, Map, INT};

use super::shapes::{box_args, json, options, Args, Fallible};
use super::stone::reply;
use super::{runtime, Scratch};
use crate::remote::{self, Organic};

const DEFORM: &[&str] = &[
    "axis",
    "origin",
    "length",
    "center",
    "angle",
    "direction",
    "start",
    "end",
    "amplitude",
    "scale",
    "octaves",
    "seed",
    "bias",
    "point",
    "factor",
    "radius",
    "falloff",
    "minFeature",
];

const LOFT: &[&str] = &[
    "radius",
    "rx",
    "ry",
    "roll",
    "flatTop",
    "noise",
    "frequency",
    "curve",
    "caps",
    "pattern",
    "period",
    "sectors",
    "seed",
    "mode",
    "space",
];

const BRUSH: &[&str] = &[
    "radius", "strength", "falloff", "only", "palette", "vector", "spacing", "space",
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: Option<[INT; 6]>, op: &str, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, DEFORM, "deform")?;
            if let Some(corners) = corners {
                args.extend(box_args(corners)?);
            }
            args.insert("op".into(), op.into());
            state
                .borrow_mut()
                .organic(args, Organic::Deform)
                .map_err(runtime)
        };
        let boxed = run.clone();
        if with_opts {
            engine.register_fn("deform", move |op: &str, opts: Map| run(None, op, &opts));
            engine.register_fn(
                "deform",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, op: &str, opts: Map| {
                    boxed(Some([x0, y0, z0, x1, y1, z1]), op, &opts)
                },
            );
        } else {
            engine.register_fn("deform", move |op: &str| run(None, op, &Map::new()));
            engine.register_fn(
                "deform",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, op: &str| {
                    boxed(Some([x0, y0, z0, x1, y1, z1]), op, &Map::new())
                },
            );
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |points: Array, palette: Dynamic, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, LOFT, "loft")?;
            args.insert("points".into(), json(&Dynamic::from_array(points))?);
            if palette.is_array() {
                args.insert("palettes".into(), json(&palette)?);
            } else if palette.is_int() {
                args.insert("palette".into(), json(&palette)?);
            } else {
                return Err(runtime(
                    "loft palette must be an index or a list of indices",
                ));
            }
            state
                .borrow_mut()
                .organic(args, Organic::Loft)
                .map_err(runtime)
        };
        if with_opts {
            engine.register_fn("loft", move |points: Array, palette: Dynamic, opts: Map| {
                run(points, palette, &opts)
            });
        } else {
            engine.register_fn("loft", move |points: Array, palette: Dynamic| {
                run(points, palette, &Map::new())
            });
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |op: &str, points: Array, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, BRUSH, "brush")?;
            args.insert("op".into(), op.into());
            args.insert("points".into(), json(&Dynamic::from_array(points))?);
            state
                .borrow_mut()
                .organic(args, Organic::Brush)
                .map_err(runtime)
        };
        if with_opts {
            engine.register_fn("brush", move |op: &str, points: Array, opts: Map| {
                run(op, points, &opts)
            });
        } else {
            engine.register_fn("brush", move |op: &str, points: Array| {
                run(op, points, &Map::new())
            });
        }
    }
}

impl Scratch {
    /// Run an organic tool on the object being written. The reply holds
    /// `changed` and the tool's own counts.
    fn organic(&mut self, args: Args, tool: Organic) -> Result<Map, String> {
        self.keep_going()?;
        let method = match tool {
            Organic::Deform => "voxel.deform",
            Organic::Loft => "voxel.loft",
            Organic::Brush => "voxel.brush",
        };
        let (args, _) = self.localize(method, args)?;
        let (writes, counts) = remote::organic_writes(self.cells(), &args, tool)?;
        let changed = self.apply(&writes)?;
        Ok(reply(counts, changed))
    }
}
