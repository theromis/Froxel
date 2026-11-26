//! The MCP object, bone, pose, clip, palette, and voxel-list tools in Rhai.
//!
//! `get_voxels` and `set_voxels` act on the object being written, at once,
//! through `remote::box_cells` and `remote::cell_writes`. The other calls
//! change the document, not a scratch object, so a run records them as the
//! MCP method and arguments, and `Session::apply_outcome` replays them
//! through `remote::apply` after the voxels are written, in call order and
//! in the same undo step. `list_bones` and `list_clips` read the document as
//! it was when the run started.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, NativeCallContext, INT};
use serde_json::{json, Value};

use super::bind::{bind_in_context, Binding, Pos::*};
use super::shapes::Args;
use super::{runtime, BuiltKey, Scratch};
use crate::remote;
use crate::state::Session;

/// Most recorded document calls in one run.
const MAX_OPS: usize = 4_096;

type Fallible<T> = Result<T, Box<EvalAltResult>>;

/// What a run may read of the document: object names, bones, and clips as
/// MCP `list_bones` and `list_clips` return them, the palette colours
/// `carve_from_views` quantises to, the palette and used slots `voxelize`
/// maps into, and the palette as `cavity_tint` makes shades in it (a new
/// document's without one).
#[derive(Clone, Default)]
pub(crate) struct DocView {
    pub(crate) names: Vec<String>,
    /// Each object's world position, for `space: "world"`.
    pub(crate) positions: Vec<crate::state::Cell>,
    pub(crate) bones: Value,
    pub(crate) clips: Value,
    /// 256 colours; empty is the default palette.
    pub(crate) palette: Vec<[u8; 3]>,
    /// The palette with alpha and the slots a cell holds, for `voxelize`.
    pub(crate) palette_rgba: Vec<[u8; 4]>,
    pub(crate) used: Vec<bool>,
    pub(crate) shades: Option<Box<crate::tools::Shades>>,
}

impl DocView {
    pub(crate) fn of(session: &Session) -> Self {
        Self {
            names: session
                .objects
                .iter()
                .map(|object| object.name.clone())
                .collect(),
            positions: session
                .objects
                .iter()
                .map(|object| object.translation)
                .collect(),
            bones: remote::bones_json(session),
            clips: remote::clips_json(session),
            palette: remote::session_palette(session).to_vec(),
            palette_rgba: session.palette.to_vec(),
            used: remote::used_slots(session).to_vec(),
            shades: Some(Box::new(session.shades())),
        }
    }
}

/// A document call recorded by a run: the MCP method and its arguments.
#[derive(Clone, Debug)]
pub(crate) struct DocOp {
    line: usize,
    function: &'static str,
    method: &'static str,
    params: Value,
}

impl DocOp {
    /// Run the call through the MCP handler. An object named by a string is
    /// looked up now, so it may be one the script created with `object`.
    pub(crate) fn run(&self, session: &mut Session) -> Result<(), String> {
        let fail = |err: String| format!("line {}: {}: {err}", self.line, self.function);
        let mut params = self.params.clone();
        if let Some(Value::String(name)) = params.get("index") {
            let index = session
                .objects
                .iter()
                .position(|object| &object.name == name)
                .ok_or_else(|| fail(format!("there is no object named `{name}`")))?;
            params["index"] = json!(index);
        }
        remote::apply(session, self.method, &params)
            .map(|_| ())
            .map_err(fail)
    }
}

/// The document functions declared from their tool ([`super::bind`]).
/// The ones that take an object by name or index (`set_object`,
/// `split_parts`, ...) resolve it first and stay hand-written below.
pub(super) const BINDINGS: &[Binding] = &[
    Binding::new("get_voxels", "get_voxels", &[Corners]),
    Binding::new("set_voxels", "set_voxels", &[List("cells")]),
    Binding::new("add_bone", "add_bone", &[Str("name"), List("pivot")]),
    Binding::new("set_bone", "set_bone", &[Str("name")]).with_options_only(),
    Binding::new("place_joints", "place_joints", &[]),
    Binding::new("set_object", "set_object", &[Any("index")]).with_options_only(),
    Binding::new("remove_object", "remove_object", &[Any("index")]).without_options(),
    Binding::new("clear_object", "clear_object", &[Any("index")]).without_options(),
    // clear_object() empties the object being written, at once.
    Binding::new("clear_object", "clear_object", &[])
        .excluded(&["index"])
        .without_options(),
    // `makeActive` is the window's; a run keeps writing its own object.
    Binding::new("components", "components", &[Str("op")]).excluded(&["index", "makeActive"]),
    Binding::new(
        "split_parts",
        "split_parts",
        &[Any("object"), List("parts")],
    ),
    Binding::new("split_object", "split_object", &[Any("index")])
        .excluded(&["makeActive"])
        .with_options_only(),
    Binding::new(
        "mirror_object",
        "mirror_object",
        &[Any("index"), Str("axis"), Str("name")],
    ),
    Binding::new("remove_bone", "remove_bone", &[Str("name")]).without_options(),
    Binding::new("bind_object", "bind_object", &[Any("index"), Any("bone")]).without_options(),
    Binding::new("set_pose", "set_pose", &[Any("pose")]).without_options(),
    Binding::new("clear_pose", "clear_pose", &[]).without_options(),
    Binding::new("add_clip", "add_clip", &[Str("name")]).without_options(),
    Binding::new(
        "set_clip_keys",
        "set_clip_keys",
        &[Str("name"), Str("bone"), List("keys")],
    )
    .without_options(),
    Binding::new("remove_clip", "remove_clip", &[Str("name")]).without_options(),
    // set_palette(#{index, color}) is the tool's arguments as one map;
    // set_palette([...]) its entries.
    Binding::new("set_palette", "set_palette", &[]).with_options_only(),
    Binding::new("set_palette", "set_palette", &[List("entries")]).without_options(),
];

/// Why a script cannot remove or clear the object it writes.
const REMOVE_HOST: &str = "a script cannot remove its own object";
const CLEAR_HOST: &str =
    "clear_object() empties the object being written; clear_object(object) is for another one";

/// An object argument: an index or a name. `host` is the error when it is
/// the object the run writes, for an operation that would undo the run.
fn check_object(state: &Rc<RefCell<Scratch>>, object: &Value, host: Option<&str>) -> Fallible<()> {
    let scratch = state.borrow();
    let BuiltKey::Index(index) = scratch.built[0].key else {
        unreachable!("the host is the first object a run writes");
    };
    let is_host = match object {
        Value::Number(number) => match number.as_i64() {
            Some(found) if found >= 0 => found as usize == index,
            _ => return Err(runtime(format!("object index {number} is out of range"))),
        },
        Value::String(name) => scratch.names.get(index) == Some(name),
        _ => return Err(runtime("an object is an index or a name")),
    };
    match host {
        Some(error) if is_host => Err(runtime(error)),
        _ => Ok(()),
    }
}

/// `components`: `split` changes the document, so it is recorded and runs
/// after the voxels are written, on the object being written; the other ops
/// edit that object now.
fn components(
    state: &Rc<RefCell<Scratch>>,
    ctx: &NativeCallContext,
    mut args: Args,
) -> Fallible<Dynamic> {
    if args["op"] == "split" {
        let index = {
            let scratch = state.borrow();
            match &scratch.built[scratch.current].key {
                BuiltKey::Index(index) => json!(index),
                BuiltKey::Name(name) => json!(name),
            }
        };
        args.insert("index".into(), index);
        remote::components_spec(&Value::Object(args.clone())).map_err(runtime)?;
        record(
            state,
            ctx,
            "components",
            "voxel.components",
            Value::Object(args),
        )?;
        return Ok(Dynamic::from_map(Map::new()));
    }
    let mut scratch = state.borrow_mut();
    scratch.keep_going().map_err(runtime)?;
    let (found, reply) =
        remote::components_run(scratch.cells(), &Value::Object(args)).map_err(runtime)?;
    let changed = scratch.apply(&found.writes).map_err(runtime)?;
    let mut out = dynamic(&reply).try_cast::<Map>().unwrap_or_default();
    out.insert("changed".into(), Dynamic::from_int(changed));
    Ok(Dynamic::from_map(out))
}

/// `get_voxels`: the box's solid cells as `[x, y, z, slot]`, in the cells
/// the call gave (world cells with `space: "world"`).
fn get_voxels(state: &Rc<RefCell<Scratch>>, args: Args) -> Fallible<Dynamic> {
    let scratch = state.borrow();
    let (args, offset) = scratch.localize("voxel.get_box", args).map_err(runtime)?;
    let by = offset.unwrap_or((0, 0, 0));
    let cells = remote::box_cells(scratch.cells(), &args).map_err(runtime)?;
    Ok(Dynamic::from_array(
        cells
            .into_iter()
            .map(|(x, y, z, slot)| {
                let cell = [
                    (x + by.0).into(),
                    (y + by.1).into(),
                    (z + by.2).into(),
                    slot.into(),
                ];
                Dynamic::from_array(cell.into_iter().map(Dynamic::from_int).collect())
            })
            .collect(),
    ))
}

/// `set_voxels` on the object being written; replies with the cells changed.
fn set_voxels(state: &Rc<RefCell<Scratch>>, args: Args) -> Fallible<Dynamic> {
    let mut scratch = state.borrow_mut();
    scratch.keep_going().map_err(runtime)?;
    let (args, _) = scratch.localize("voxel.set_many", args).map_err(runtime)?;
    let (writes, _) = remote::set_many_writes(scratch.cells(), &args).map_err(runtime)?;
    scratch
        .apply(&writes)
        .map(Dynamic::from_int)
        .map_err(runtime)
}

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for binding in BINDINGS {
        let state = scratch.clone();
        let name = binding.name;
        bind_in_context(engine, *binding, move |ctx, args| match name {
            "get_voxels" => get_voxels(&state, args),
            "set_voxels" => set_voxels(&state, args),
            "components" => components(&state, ctx, args),
            "clear_object" if !args.contains_key("index") => {
                let mut scratch = state.borrow_mut();
                let cells = scratch.cells_mut();
                let removed = cells.len() as INT;
                cells.clear();
                Ok(Dynamic::from_int(removed))
            }
            _ => {
                // An object operation the run records and applies afterwards.
                match name {
                    "remove_object" => check_object(&state, &args["index"], Some(REMOVE_HOST))?,
                    "clear_object" => check_object(&state, &args["index"], Some(CLEAR_HOST))?,
                    "split_parts" => check_object(&state, &args["object"], None)?,
                    "set_object" | "split_object" | "mirror_object" => {
                        check_object(&state, &args["index"], None)?
                    }
                    "bind_object" => {
                        check_object(&state, &args["index"], None)?;
                        if !matches!(args["bone"], Value::String(_) | Value::Null) {
                            return Err(runtime("bind_object bone must be a name or ()"));
                        }
                    }
                    _ => {}
                }
                let method = crate::ops::by_tool(name).expect("a bound tool").method;
                record(&state, ctx, name, method, Value::Object(args)).map(|()| Dynamic::UNIT)
            }
        });
    }

    let state = scratch.clone();
    engine.register_fn("list_bones", move || -> Array {
        list(&state.borrow().view.bones, "bones")
    });
    // Guards for a rerun: the replay fails on a bone, clip, or object name
    // that already exists, as the MCP tools do.
    let state = scratch.clone();
    engine.register_fn("has_bone", move |name: &str| {
        has(&state.borrow().view.bones, "bones", name)
    });
    let state = scratch.clone();
    engine.register_fn("has_clip", move |name: &str| {
        has(&state.borrow().view.clips, "clips", name)
    });
    let state = scratch.clone();
    engine.register_fn("has_object", move |name: &str| {
        state.borrow().view.names.iter().any(|found| found == name)
    });

    let state = scratch.clone();
    engine.register_fn("list_clips", move || -> Array {
        list(&state.borrow().view.clips, "clips")
    });
}

fn record(
    state: &Rc<RefCell<Scratch>>,
    ctx: &NativeCallContext,
    function: &'static str,
    method: &'static str,
    params: Value,
) -> Fallible<()> {
    let mut scratch = state.borrow_mut();
    scratch.keep_going().map_err(runtime)?;
    if scratch.ops.len() >= MAX_OPS {
        return Err(runtime(format!(
            "a script makes at most {MAX_OPS} object, bone, clip, and palette calls"
        )));
    }
    scratch.ops.push(DocOp {
        line: ctx.call_position().line().unwrap_or(0),
        function,
        method,
        params,
    });
    Ok(())
}

/// The list under `key` of a `list_bones` or `list_clips` reply.
fn list(reply: &Value, key: &str) -> Array {
    match reply.get(key) {
        Some(Value::Array(items)) => items.iter().map(dynamic).collect(),
        _ => Array::new(),
    }
}

/// Whether the list under `key` has an entry with this `name`.
fn has(reply: &Value, key: &str, name: &str) -> bool {
    reply
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item["name"] == name))
}

pub(super) fn dynamic(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(flag) => Dynamic::from_bool(*flag),
        Value::Number(number) => match number.as_i64() {
            Some(int) => Dynamic::from_int(int),
            None => Dynamic::from_float(number.as_f64().unwrap_or(0.0)),
        },
        Value::String(text) => Dynamic::from(text.clone()),
        Value::Array(items) => Dynamic::from_array(items.iter().map(dynamic).collect()),
        Value::Object(map) => Dynamic::from_map(
            map.iter()
                .map(|(key, item)| (key.as_str().into(), dynamic(item)))
                .collect(),
        ),
    }
}
