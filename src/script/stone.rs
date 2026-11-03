//! The MCP stone tools in Rhai: `joints`.
//!
//! Each call turns its arguments into the MCP tool's arguments and runs
//! `remote::stone_writes` on the object being written, as the MCP handler
//! does on the active object. Options use the MCP names in a trailing map.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Dynamic, Engine, Map, INT};
use serde_json::Value;

use super::shapes::{box_args, options, Args, Fallible};
use super::{runtime, Scratch};
use crate::remote::{self, Stone};

const JOINTS: &[&str] = &[
    "spacing", "course", "width", "depth", "dirt", "only", "density", "seed",
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
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
            .map(|(key, value)| (key.into(), Dynamic::from_int(value.as_i64().unwrap_or(0))))
            .collect();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        Ok(reply)
    }
}
