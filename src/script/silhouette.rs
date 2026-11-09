//! MCP `carve_from_views`, `project_paint`, and `fit_to_views` in Rhai: the
//! visual hull of two or three views, each a map of rows and a legend, one
//! image of rows painted onto the surface a view sees, and the object
//! refined to match its views, since a script reads no files.
//!
//! `carve_from_views(views)` and `carve_from_views(views, opts)` run
//! `remote::hull_writes` on the object being written, as the MCP handler
//! does on the active object; `project_paint(view, rows)`,
//! `project_paint(view, rows, opts)`, and `project_paint(rows, opts)` (with
//! `yaw` and `pitch`) run `remote::paint_writes`; `fit_to_views(views)` and
//! `fit_to_views(views, opts)` run `remote::fit_writes` with no other
//! objects in view, and `reference` names another object the run builds. Colours quantise to the
//! document's palette as it was when the run started, with the run's
//! `palette` calls on top.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, Map};
use serde_json::Value;

use super::shapes::{json, options, Args, Fallible};
use super::{runtime, Scratch};
use crate::remote;

const CARVE: &[&str] = &[
    "size",
    "origin",
    "cellsPerPixel",
    "palette",
    "slots",
    "coverage",
    "shell",
    "smooth",
    "mode",
    "space",
];

const FIT: &[&str] = &[
    "align",
    "slots",
    "coverage",
    "iterations",
    "seed",
    "margin",
    "maskWeight",
    "colorWeight",
    "depthWeight",
    "smoothWeight",
    "shape",
    "recolor",
    "normalLimit",
    "keep",
    "sharedScale",
    "reference",
];

const PAINT: &[&str] = &[
    "view",
    "yaw",
    "pitch",
    "legend",
    "background",
    "tolerance",
    "flip",
    "origin",
    "size",
    "cellsPerPixel",
    "imageSize",
    "palette",
    "slots",
    "coverage",
    "depth",
    "normalLimit",
    "only",
    "space",
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    let paint = {
        let state = scratch.clone();
        move |view: Option<&str>, rows: Array, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, PAINT, "project_paint")?;
            if let Some(view) = view {
                if args.contains_key("view") {
                    return Err(super::runtime(
                        "project_paint: pass the view once, not also in the options",
                    ));
                }
                args.insert("view".into(), Value::String(view.to_string()));
            }
            args.insert("rows".into(), json(&Dynamic::from_array(rows))?);
            state.borrow_mut().paint_view(args).map_err(runtime)
        }
    };
    {
        let paint = paint.clone();
        engine.register_fn("project_paint", move |view: &str, rows: Array| {
            paint(Some(view), rows, &Map::new())
        });
    }
    {
        let paint = paint.clone();
        engine.register_fn(
            "project_paint",
            move |view: &str, rows: Array, opts: Map| paint(Some(view), rows, &opts),
        );
    }
    engine.register_fn("project_paint", move |rows: Array, opts: Map| {
        paint(None, rows, &opts)
    });
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |views: Array, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, FIT, "fit_to_views")?;
            args.insert("views".into(), json(&Dynamic::from_array(views))?);
            state.borrow_mut().fit_views(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn("fit_to_views", move |views: Array, opts: Map| {
                run(views, &opts)
            });
        } else {
            engine.register_fn("fit_to_views", move |views: Array| run(views, &Map::new()));
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |views: Array, opts: &Map| -> Fallible<Map> {
            let mut args = options(opts, CARVE, "carve_from_views")?;
            args.insert("views".into(), json(&Dynamic::from_array(views))?);
            state.borrow_mut().carve_views(args).map_err(runtime)
        };
        if with_opts {
            engine.register_fn("carve_from_views", move |views: Array, opts: Map| {
                run(views, &opts)
            });
        } else {
            engine.register_fn("carve_from_views", move |views: Array| {
                run(views, &Map::new())
            });
        }
    }
}

impl Scratch {
    /// The document palette a run quantises to: as it was when the run
    /// started, with the run's `palette` calls on top.
    fn run_palette(&self) -> [[u8; 3]; 256] {
        let mut palette = if self.view.palette.len() == 256 {
            std::array::from_fn(|slot| self.view.palette[slot])
        } else {
            remote::session_palette(&crate::state::Session::new())
        };
        for (slot, rgb) in &self.palette {
            palette[*slot as usize] = *rgb;
        }
        palette
    }

    /// The object being written fitted to its views. `reference` names (or
    /// numbers) another object this run builds, in the same local cells.
    /// The reply holds `changed`, `added`, `removed`, `recolored`,
    /// `sweeps`, and `views`, a list of `#{view, before, after}` with
    /// `iou`, `missing`, `excess`, and `colorError` in each.
    fn fit_views(&mut self, mut args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let palette = self.run_palette();
        let reference: Option<Vec<crate::state::Cell>> = match args.remove("reference") {
            None | Some(Value::Null) => None,
            Some(key) => {
                let found = self
                    .built
                    .iter()
                    .position(|built| match (&built.key, &key) {
                        (super::BuiltKey::Name(name), Value::String(want)) => name == want,
                        (super::BuiltKey::Index(index), Value::Number(want)) => {
                            want.as_u64() == Some(*index as u64)
                        }
                        _ => false,
                    });
                let index = found.ok_or_else(|| {
                    format!("fit_to_views: reference {key} is not an object this run builds")
                })?;
                if index == self.current {
                    return Err("fit_to_views: reference must be another object".into());
                }
                Some(self.built[index].cells.keys().copied().collect())
            }
        };
        let (writes, counts) = remote::fit_writes(
            self.cells(),
            &[],
            reference.as_deref(),
            &palette,
            &Value::Object(args),
            false,
        )?;
        let changed = self.apply(&writes)?;
        let mut reply = Map::new();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        for key in ["added", "removed", "recolored", "sweeps"] {
            reply.insert(
                key.into(),
                Dynamic::from_int(counts[key].as_i64().unwrap_or(0)),
            );
        }
        let score = |value: &Value| -> Dynamic {
            let mut map = Map::new();
            for key in ["iou", "missing", "excess", "colorError"] {
                map.insert(
                    key.into(),
                    Dynamic::from_float(value[key].as_f64().unwrap_or(0.0)),
                );
            }
            Dynamic::from_map(map)
        };
        let views: rhai::Array = counts["views"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|view| {
                let mut map = Map::new();
                map.insert(
                    "view".into(),
                    Dynamic::from(view["view"].as_str().unwrap_or("").to_string()),
                );
                map.insert("before".into(), score(&view["before"]));
                map.insert("after".into(), score(&view["after"]));
                Dynamic::from_map(map)
            })
            .collect();
        reply.insert("views".into(), Dynamic::from_array(views));
        Ok(reply)
    }

    /// One image painted onto the object being written. The reply holds
    /// `changed`, `cells`, `steep`, and `bounds` as `#{min, max}` (unit when
    /// nothing is painted).
    fn paint_view(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let palette = self.run_palette();
        let (args, offset) = self.localize("voxel.project_paint", args)?;
        let (writes, counts) = remote::paint_writes(self.cells(), &palette, &args, false)?;
        let counts = world_counts(counts, offset);
        let changed = self.apply(&writes)?;
        let mut reply = Map::new();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        for key in ["cells", "covered", "rays", "steep"] {
            reply.insert(
                key.into(),
                Dynamic::from_int(counts[key].as_i64().unwrap_or(0)),
            );
        }
        reply.insert("bounds".into(), bounds_map(&counts["bounds"]));
        Ok(reply)
    }

    /// The hull on the object being written. The reply holds `changed`,
    /// `cells`, and `bounds` as `#{min, max}` (unit when nothing is carved).
    fn carve_views(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let palette = self.run_palette();
        let (args, offset) = self.localize("voxel.carve_from_views", args)?;
        let (writes, counts) = remote::hull_writes(self.cells(), &palette, &args, false)?;
        let counts = world_counts(counts, offset);
        let changed = self.apply(&writes)?;
        let mut reply = Map::new();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        reply.insert(
            "cells".into(),
            Dynamic::from_int(counts["cells"].as_i64().unwrap_or(0)),
        );
        reply.insert("bounds".into(), bounds_map(&counts["bounds"]));
        reply.insert("size".into(), corner(&counts["size"]));
        Ok(reply)
    }
}

/// A tool's counts with `bounds` and `origin` in world cells when the call
/// was in world space.
fn world_counts(
    counts: serde_json::Map<String, Value>,
    offset: Option<crate::state::Cell>,
) -> serde_json::Map<String, Value> {
    let mut counts = Value::Object(counts);
    if let Some(offset) = offset {
        remote::to_world_reply(&mut counts, offset);
    }
    match counts {
        Value::Object(map) => map,
        _ => unreachable!("counts stay a map"),
    }
}

/// `[x, y, z]` of a reply as a Rhai array.
fn corner(value: &Value) -> Dynamic {
    Dynamic::from_array(
        value
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|v| Dynamic::from_int(v.as_i64().unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// A reply's `bounds` as `#{min, max}`, or unit when there are none.
fn bounds_map(value: &Value) -> Dynamic {
    match value {
        Value::Object(found) => {
            let mut map = Map::new();
            map.insert("min".into(), corner(&found["min"]));
            map.insert("max".into(), corner(&found["max"]));
            Dynamic::from_map(map)
        }
        _ => Dynamic::UNIT,
    }
}
