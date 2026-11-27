//! The skin in Rhai: `skin(#{...})` and `clear_skin()`.
//!
//! Both run the MCP tool's argument code (`remote::skin_arg`) for the object
//! being written. A run that calls either sets that object's skin when the
//! result is applied; a run that calls neither leaves the skin as it was.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Dynamic, Engine};
use serde_json::Value;

use super::bind::{bind, Binding};
use super::Scratch;
use crate::remote;

/// The skin functions ([`super::bind`]). A script writes its own object, so
/// neither takes `object`.
pub(super) const BINDINGS: &[Binding] = &[
    Binding::new("skin", "skin", &[]).excluded(&["object", "off"]),
    Binding::new("clear_skin", "skin", &[])
        .excluded(&["object"])
        .without_options(),
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for binding in BINDINGS {
        let state = scratch.clone();
        let name = binding.name;
        bind(engine, *binding, move |mut args| {
            if name == "clear_skin" {
                args.insert("off".into(), Value::Bool(true));
            }
            let skin = remote::skin_arg(&Value::Object(args))?;
            let mut scratch = state.borrow_mut();
            let current = scratch.current;
            scratch.built[current].skin = Some(skin);
            Ok(Dynamic::UNIT)
        });
    }
}
