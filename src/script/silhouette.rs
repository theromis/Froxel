//! MCP `carve_from_views` in Rhai: the visual hull of two or three views,
//! each a map of rows and a legend, since a script reads no files.
//!
//! `carve_from_views(views)` and `carve_from_views(views, opts)` run
//! `remote::hull_writes` on the object being written, as the MCP handler
//! does on the active object. Colours quantise to the document's palette
//! as it was when the run started, with the run's `palette` calls on top.

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

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
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
    /// The hull on the object being written. The reply holds `changed`,
    /// `cells`, and `bounds` as `#{min, max}` (unit when nothing is carved).
    fn carve_views(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let mut palette = if self.view.palette.len() == 256 {
            std::array::from_fn(|slot| self.view.palette[slot])
        } else {
            remote::session_palette(&crate::state::Session::new())
        };
        for (slot, rgb) in &self.palette {
            palette[*slot as usize] = *rgb;
        }
        let (writes, counts) =
            remote::hull_writes(self.cells(), &palette, &Value::Object(args), false)?;
        let changed = self.apply(&writes)?;
        let mut reply = Map::new();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        reply.insert(
            "cells".into(),
            Dynamic::from_int(counts["cells"].as_i64().unwrap_or(0)),
        );
        let corner = |value: &Value| -> Dynamic {
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
        };
        let bounds = match &counts["bounds"] {
            Value::Object(found) => {
                let mut map = Map::new();
                map.insert("min".into(), corner(&found["min"]));
                map.insert("max".into(), corner(&found["max"]));
                Dynamic::from_map(map)
            }
            _ => Dynamic::UNIT,
        };
        reply.insert("bounds".into(), bounds);
        reply.insert("size".into(), corner(&counts["size"]));
        Ok(reply)
    }
}
