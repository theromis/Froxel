//! MCP `carve_from_views` and `project_paint` in Rhai: the visual hull of
//! two or three views, each a map of rows and a legend, and one image of
//! rows painted onto the surface a view sees, since a script reads no
//! files.
//!
//! `carve_from_views(views)` and `carve_from_views(views, opts)` run
//! `remote::hull_writes` on the object being written, as the MCP handler
//! does on the active object; `project_paint(view, rows)`,
//! `project_paint(view, rows, opts)`, and `project_paint(rows, opts)` (with
//! `yaw` and `pitch`) run `remote::paint_writes`. Colours quantise to the
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

    /// One image painted onto the object being written. The reply holds
    /// `changed`, `cells`, `steep`, and `bounds` as `#{min, max}` (unit when
    /// nothing is painted).
    fn paint_view(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let palette = self.run_palette();
        let (writes, counts) =
            remote::paint_writes(self.cells(), &palette, &Value::Object(args), false)?;
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
        let (writes, counts) =
            remote::hull_writes(self.cells(), &palette, &Value::Object(args), false)?;
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
