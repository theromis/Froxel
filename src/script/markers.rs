//! Markers in Rhai: `marker`, `collision_box`, `connector`, and
//! `collision_from_voxels`.
//!
//! Each builds MCP arguments and runs the same code as the tool
//! (`remote::marker_kind`, `remote::collision_build`). A marker belongs to
//! the object being written, in its local mesh units; the run's markers
//! replace that object's scripted markers when the result is applied.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, Map, INT};
use serde_json::{json, Value};

use super::document::dynamic;
use super::shapes::{json, options, Fallible};
use super::{runtime, BuiltKey, Scratch};
use crate::state::{check_marker_name, replace_collision_markers, Marker, MAX_MARKERS};

const COLLISION: &[&str] = &[
    "prefix",
    "layer",
    "zBands",
    "minFeature",
    "holeSize",
    "only",
    "exclude",
    "maxBoxes",
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    let state = scratch.clone();
    engine.register_fn(
        "marker",
        move |name: &str, x: Dynamic, y: Dynamic, z: Dynamic| -> Fallible<()> {
            let point = Value::Array(vec![json(&x)?, json(&y)?, json(&z)?]);
            state
                .borrow_mut()
                .marker(name, json!({"kind": "point", "point": point}))
                .map_err(runtime)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "connector",
        move |name: &str, point: Array, direction: Array| -> Fallible<()> {
            let args = json!({
                "kind": "connector",
                "point": json(&Dynamic::from_array(point))?,
                "direction": json(&Dynamic::from_array(direction))?,
            });
            state.borrow_mut().marker(name, args).map_err(runtime)
        },
    );
    let state = scratch.clone();
    let run = move |opts: &Map| -> Fallible<Dynamic> {
        let args = options(opts, COLLISION, "collision_from_voxels")?;
        let reply = state
            .borrow_mut()
            .collision_from_voxels(Value::Object(args))
            .map_err(runtime)?;
        Ok(dynamic(&reply))
    };
    let with_options = run.clone();
    engine.register_fn("collision_from_voxels", move || run(&Map::new()));
    engine.register_fn("collision_from_voxels", move |opts: Map| {
        with_options(&opts)
    });
    for named in [false, true] {
        let state = scratch.clone();
        let run = move |name: Option<&str>, c: [INT; 6]| -> Fallible<()> {
            let mut scratch = state.borrow_mut();
            let name = match name {
                Some(name) => name.to_string(),
                None => format!("collision_{}", scratch.collision_count() + 1),
            };
            let mut args = json!({"kind": "box"});
            for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"].iter().zip(c) {
                args[*key] = json!(value);
            }
            scratch.marker(&name, args).map_err(runtime)
        };
        if named {
            engine.register_fn(
                "collision_box",
                move |name: &str, x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT| {
                    run(Some(name), [x0, y0, z0, x1, y1, z1])
                },
            );
        } else {
            engine.register_fn(
                "collision_box",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT| {
                    run(None, [x0, y0, z0, x1, y1, z1])
                },
            );
        }
    }
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
