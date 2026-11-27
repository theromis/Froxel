//! Strands in Rhai: `add_strand`, `edit_strand`, `remove_strand`,
//! `list_strands`, and `bake_strands`.
//!
//! Each runs the MCP tool's code (`remote::strand_*`) on the strands of the
//! object being written, which start empty like its cells. A run that adds,
//! edits, or removes strands replaces that object's strands when the result
//! is applied, so a generator that grows a tuft does not stack a second one
//! on every run; a run that never touches strands leaves them as they were.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Dynamic, Engine, INT};
use serde_json::{json, Value};

use super::bind::{bind, Binding, Pos::*};
use super::document::dynamic;
use super::Scratch;
use crate::remote;

/// The strand functions ([`super::bind`]). A script writes its own object,
/// so none takes `object`.
pub(super) const BINDINGS: &[Binding] = &[
    Binding::new("add_strand", "add_strand", &[List("points")]).excluded(&["object"]),
    Binding::new("edit_strand", "edit_strand", &[Int("index")])
        .excluded(&["object"])
        .with_options_only(),
    Binding::new("remove_strand", "remove_strand", &[Int("index")])
        .excluded(&["object", "all"])
        .without_options(),
    // remove_strand(#{all: true}) clears the run's strands.
    Binding::new("remove_strand", "remove_strand", &[])
        .excluded(&["object", "index"])
        .with_options_only(),
    Binding::new("list_strands", "list_strands", &[])
        .excluded(&["object"])
        .without_options(),
    Binding::new("bake_strands", "bake_strands", &[]).excluded(&["object"]),
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for binding in BINDINGS {
        let state = scratch.clone();
        let name = binding.name;
        bind(engine, *binding, move |args| {
            let params = Value::Object(args);
            let mut scratch = state.borrow_mut();
            let fallback = 1;
            let built = scratch.current_built();
            let mut strands = built.strands.clone().unwrap_or_default();
            let result = match name {
                "add_strand" => {
                    Dynamic::from_int(remote::strand_add(&mut strands, &params, fallback)? as INT)
                }
                "edit_strand" => {
                    Dynamic::from_int(remote::strand_edit(&mut strands, &params)? as INT)
                }
                "remove_strand" => {
                    Dynamic::from_int(remote::strand_remove(&mut strands, &params)? as INT)
                }
                "list_strands" => return Ok(dynamic(&json!(remote::strand_list(&strands)))),
                "bake_strands" => {
                    let (which, keep) = remote::strand_bake_choice(&strands, &params)?;
                    let cells = crate::tools::strand_cells(&strands, &which);
                    let count = cells.len();
                    built.cells.extend(cells);
                    if !keep {
                        let mut n = 0;
                        strands.retain(|_| {
                            let kept = !which.contains(&n);
                            n += 1;
                            kept
                        });
                    }
                    Dynamic::from_int(count as INT)
                }
                _ => unreachable!("a bound strand function"),
            };
            crate::tools::check_strands(&strands)?;
            built.strands = Some(strands);
            Ok(result)
        });
    }
}

impl Scratch {
    fn current_built(&mut self) -> &mut super::Built {
        let current = self.current;
        &mut self.built[current]
    }
}
