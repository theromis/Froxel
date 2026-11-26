//! Markers in Rhai: `marker`, `collision_box`, `connector`, and
//! `collision_from_voxels`.
//!
//! Each builds MCP arguments and runs the same code as the tool
//! (`remote::marker_kind`, `remote::collision_build`). A marker belongs to
//! the object being written, in its local mesh units; the run's markers
//! replace that object's scripted markers when the result is applied.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Dynamic, Engine};
use serde_json::Value;

use super::bind::{bind, Binding, Pos::*};
use super::document::dynamic;
use super::{BuiltKey, Scratch};
use crate::state::{check_marker_name, replace_collision_markers, Marker, MAX_MARKERS};

/// The markers a script places on its own object: `set_marker` with a fixed
/// kind ([`super::bind`]). The object is always the one being written, so
/// none takes `object`; an unnamed `collision_box` is named
/// `collision_<n>`.
pub(super) const BINDINGS: &[Binding] = &[
    Binding::new("marker", "set_marker", &[Str("name"), Triple("point")])
        .fixed(&[("kind", "point")])
        .excluded(&["object", "direction", "x0", "y0", "z0", "x1", "y1", "z1"])
        .without_options(),
    Binding::new(
        "connector",
        "set_marker",
        &[Str("name"), List("point"), List("direction")],
    )
    .fixed(&[("kind", "connector")])
    .excluded(&["object", "x0", "y0", "z0", "x1", "y1", "z1"])
    .without_options(),
    Binding::new("collision_box", "set_marker", &[Str("name"), Corners])
        .fixed(&[("kind", "box")])
        .excluded(&["object", "point", "direction"])
        .without_options(),
    Binding::new("collision_box", "set_marker", &[Corners])
        .fixed(&[("kind", "box")])
        .excluded(&["object", "name", "point", "direction"])
        .without_options(),
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for binding in BINDINGS {
        let state = scratch.clone();
        bind(engine, *binding, move |mut args| {
            let mut scratch = state.borrow_mut();
            let name = match args.remove("name") {
                Some(Value::String(name)) => name,
                _ => format!("collision_{}", scratch.collision_count() + 1),
            };
            scratch.marker(&name, Value::Object(args))?;
            Ok(Dynamic::UNIT)
        });
    }
    let state = scratch.clone();
    // A script edits its own object, so it takes no object.
    let collision =
        Binding::new("collision_from_voxels", "collision_from_voxels", &[]).excluded(&["object"]);
    bind(engine, collision, move |args| {
        let reply = state
            .borrow_mut()
            .collision_from_voxels(Value::Object(args))?;
        Ok(dynamic(&reply))
    });
}

impl Scratch {
    /// Add a marker on the object being written; a later one of the same
    /// name replaces it.
    fn marker(&mut self, name: &str, args: Value) -> Result<(), String> {
        let name = check_marker_name(name)?;
        let kind = crate::remote::marker_kind(&args)?;
        let object = self.current_name();
        let marker = Marker {
            name,
            object,
            kind,
            scripted: true,
        };
        match self.markers.iter().position(|m| m.name == marker.name) {
            Some(at) => self.markers[at] = marker,
            None if self.markers.len() >= MAX_MARKERS => {
                return Err(format!("a script names at most {MAX_MARKERS} markers"));
            }
            None => self.markers.push(marker),
        }
        Ok(())
    }

    fn current_name(&self) -> String {
        match &self.built[self.current].key {
            BuiltKey::Index(index) => self.names[*index].clone(),
            BuiltKey::Name(name) => name.clone(),
        }
    }

    /// MCP `collision_from_voxels` on the cells of the object being written,
    /// as they are at this point of the script.
    fn collision_from_voxels(&mut self, args: Value) -> Result<Value, String> {
        let object = self.current_name();
        let build =
            crate::remote::collision_build(&self.built[self.current].cells, &object, &args)?;
        let replaced = replace_collision_markers(
            &mut self.markers,
            &object,
            &build.prefix,
            build.boxes,
            true,
        )?;
        Ok(crate::remote::collision_reply(
            build.reply,
            replaced,
            self.markers.len(),
        ))
    }

    fn collision_count(&self) -> usize {
        self.markers
            .iter()
            .filter(|m| m.name.starts_with("collision_"))
            .count()
    }
}
