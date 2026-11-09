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

use super::shapes::{box_args, json, options};
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

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |corners: [INT; 6], opts: &Map| -> Fallible<Array> {
            let mut args = options(opts, &["space"], "get_voxels")?;
            args.extend(box_args(corners)?);
            let scratch = state.borrow();
            let (args, offset) = scratch.localize("voxel.get_box", args).map_err(runtime)?;
            let by = offset.unwrap_or((0, 0, 0));
            let cells = remote::box_cells(scratch.cells(), &args).map_err(runtime)?;
            Ok(cells
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
                .collect())
        };
        if with_opts {
            engine.register_fn(
                "get_voxels",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT, opts: Map| {
                    run([x0, y0, z0, x1, y1, z1], &opts)
                },
            );
        } else {
            engine.register_fn(
                "get_voxels",
                move |x0: INT, y0: INT, z0: INT, x1: INT, y1: INT, z1: INT| {
                    run([x0, y0, z0, x1, y1, z1], &Map::new())
                },
            );
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |cells: Array, opts: &Map| -> Fallible<INT> {
            let mut args = options(opts, &["space", "mirror"], "set_voxels")?;
            args.insert("cells".into(), json(&Dynamic::from_array(cells))?);
            let mut scratch = state.borrow_mut();
            scratch.keep_going().map_err(runtime)?;
            let (args, _) = scratch.localize("voxel.set_many", args).map_err(runtime)?;
            let (writes, _) = remote::set_many_writes(scratch.cells(), &args).map_err(runtime)?;
            scratch.apply(&writes).map_err(runtime)
        };
        if with_opts {
            engine.register_fn("set_voxels", move |cells: Array, opts: Map| {
                run(cells, &opts)
            });
        } else {
            engine.register_fn("set_voxels", move |cells: Array| run(cells, &Map::new()));
        }
    }

    let state = scratch.clone();
    engine.register_fn(
        "set_palette",
        move |ctx: NativeCallContext, entry: Map| -> Fallible<()> {
            let params = json(&Dynamic::from_map(entry))?;
            record(&state, &ctx, "set_palette", "palette.set", params)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "set_palette",
        move |ctx: NativeCallContext, entries: Array| -> Fallible<()> {
            let params = json!({"entries": json(&Dynamic::from_array(entries))?});
            record(&state, &ctx, "set_palette", "palette.set", params)
        },
    );

    let state = scratch.clone();
    engine.register_fn(
        "set_object",
        move |ctx: NativeCallContext, object: Dynamic, opts: Map| -> Fallible<()> {
            let mut args = options(&opts, &["name", "position", "hidden"], "set_object")?;
            args.insert("index".into(), object_ref(&state, &object, false)?);
            record(
                &state,
                &ctx,
                "set_object",
                "object.set",
                Value::Object(args),
            )
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "remove_object",
        move |ctx: NativeCallContext, object: Dynamic| -> Fallible<()> {
            let params = json!({"index": object_ref(&state, &object, true)?});
            record(&state, &ctx, "remove_object", "object.remove", params)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "clear_object",
        move |ctx: NativeCallContext, object: Dynamic| -> Fallible<()> {
            let index = object_ref(&state, &object, false)?;
            if object_ref(&state, &object, true).is_err() {
                return Err(runtime(
                    "clear_object() empties the object being written; clear_object(object) is for another one",
                ));
            }
            record(
                &state,
                &ctx,
                "clear_object",
                "object.clear",
                json!({"index": index}),
            )
        },
    );
    let state = scratch.clone();
    engine.register_fn("clear_object", move || -> INT {
        let mut scratch = state.borrow_mut();
        let cells = scratch.cells_mut();
        let removed = cells.len() as INT;
        cells.clear();
        removed
    });
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |ctx: &NativeCallContext, op: &str, opts: &Map| -> Fallible<Map> {
            let mut args = options(
                opts,
                &[
                    "connectivity",
                    "keep",
                    "minCells",
                    "seed",
                    "anchor",
                    "reach",
                    "radius",
                    "offset",
                    "name",
                    "into",
                    "copy",
                ],
                "components",
            )?;
            args.insert("op".into(), op.into());
            if op == "split" {
                // A split changes the document: it runs after the voxels
                // are written, on the object being written.
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
                    &state,
                    ctx,
                    "components",
                    "voxel.components",
                    Value::Object(args),
                )?;
                return Ok(Map::new());
            }
            let mut scratch = state.borrow_mut();
            scratch.keep_going().map_err(runtime)?;
            let (found, reply) =
                remote::components_run(scratch.cells(), &Value::Object(args)).map_err(runtime)?;
            let changed = scratch.apply(&found.writes).map_err(runtime)?;
            let mut out = match dynamic(&reply).try_cast::<Map>() {
                Some(map) => map,
                None => Map::new(),
            };
            out.insert("changed".into(), Dynamic::from_int(changed));
            Ok(out)
        };
        if with_opts {
            engine.register_fn(
                "components",
                move |ctx: NativeCallContext, op: &str, opts: Map| run(&ctx, op, &opts),
            );
        } else {
            engine.register_fn("components", move |ctx: NativeCallContext, op: &str| {
                run(&ctx, op, &Map::new())
            });
        }
    }
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |ctx: &NativeCallContext,
                        object: Dynamic,
                        parts: Array,
                        opts: &Map|
              -> Fallible<()> {
            let mut args = options(
                opts,
                &[
                    "metric",
                    "connectivity",
                    "reach",
                    "removeSource",
                    "bind",
                    "space",
                ],
                "split_parts",
            )?;
            args.insert("object".into(), object_ref(&state, &object, false)?);
            args.insert(
                "parts".into(),
                super::shapes::json(&Dynamic::from_array(parts))?,
            );
            record(
                &state,
                ctx,
                "split_parts",
                "object.split_parts",
                Value::Object(args),
            )
        };
        if with_opts {
            engine.register_fn(
                "split_parts",
                move |ctx: NativeCallContext, object: Dynamic, parts: Array, opts: Map| {
                    run(&ctx, object, parts, &opts)
                },
            );
        } else {
            engine.register_fn(
                "split_parts",
                move |ctx: NativeCallContext, object: Dynamic, parts: Array| {
                    run(&ctx, object, parts, &Map::new())
                },
            );
        }
    }
    let state = scratch.clone();
    engine.register_fn(
        "split_object",
        move |ctx: NativeCallContext, object: Dynamic, opts: Map| -> Fallible<()> {
            let mut args = options(
                &opts,
                &["boxes", "slots", "invert", "copy", "into", "name", "space"],
                "split_object",
            )?;
            args.insert("index".into(), object_ref(&state, &object, false)?);
            record(
                &state,
                &ctx,
                "split_object",
                "object.split",
                Value::Object(args),
            )
        },
    );
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |ctx: &NativeCallContext,
                        object: Dynamic,
                        axis: &str,
                        name: &str,
                        opts: &Map|
              -> Fallible<()> {
            let mut args = options(opts, &["position", "mirrorBones"], "mirror_object")?;
            args.insert("index".into(), object_ref(&state, &object, false)?);
            args.insert("axis".into(), axis.into());
            args.insert("name".into(), name.into());
            record(
                &state,
                ctx,
                "mirror_object",
                "object.mirror",
                Value::Object(args),
            )
        };
        if with_opts {
            engine.register_fn(
                "mirror_object",
                move |ctx: NativeCallContext,
                      object: Dynamic,
                      axis: &str,
                      name: &str,
                      opts: Map| { run(&ctx, object, axis, name, &opts) },
            );
        } else {
            engine.register_fn(
                "mirror_object",
                move |ctx: NativeCallContext, object: Dynamic, axis: &str, name: &str| {
                    run(&ctx, object, axis, name, &Map::new())
                },
            );
        }
    }

    for with_opts in [false, true] {
        let state = scratch.clone();
        let run =
            move |ctx: &NativeCallContext, name: &str, pivot: Array, opts: &Map| -> Fallible<()> {
                let mut args = options(opts, &["parent", "rotation"], "add_bone")?;
                args.insert("name".into(), name.into());
                args.insert("pivot".into(), json(&Dynamic::from_array(pivot))?);
                record(&state, ctx, "add_bone", "rig.add", Value::Object(args))
            };
        if with_opts {
            engine.register_fn(
                "add_bone",
                move |ctx: NativeCallContext, name: &str, pivot: Array, opts: Map| {
                    run(&ctx, name, pivot, &opts)
                },
            );
        } else {
            engine.register_fn(
                "add_bone",
                move |ctx: NativeCallContext, name: &str, pivot: Array| {
                    run(&ctx, name, pivot, &Map::new())
                },
            );
        }
    }
    let state = scratch.clone();
    engine.register_fn(
        "set_bone",
        move |ctx: NativeCallContext, name: &str, opts: Map| -> Fallible<()> {
            let mut args = options(
                &opts,
                &[
                    "parent", "pivot", "joint", "snap", "band", "rotation", "rename",
                ],
                "set_bone",
            )?;
            args.insert("name".into(), name.into());
            record(&state, &ctx, "set_bone", "rig.set", Value::Object(args))
        },
    );
    for with_opts in [false, true] {
        let state = scratch.clone();
        let run = move |ctx: &NativeCallContext, opts: &Map| -> Fallible<()> {
            let args = options(opts, &["bones", "snap", "band"], "place_joints")?;
            record(
                &state,
                ctx,
                "place_joints",
                "rig.joints",
                Value::Object(args),
            )
        };
        if with_opts {
            engine.register_fn("place_joints", move |ctx: NativeCallContext, opts: Map| {
                run(&ctx, &opts)
            });
        } else {
            engine.register_fn("place_joints", move |ctx: NativeCallContext| {
                run(&ctx, &Map::new())
            });
        }
    }
    let state = scratch.clone();
    engine.register_fn(
        "remove_bone",
        move |ctx: NativeCallContext, name: &str| -> Fallible<()> {
            record(
                &state,
                &ctx,
                "remove_bone",
                "rig.remove",
                json!({"name": name}),
            )
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "bind_object",
        move |ctx: NativeCallContext, object: Dynamic, bone: Dynamic| -> Fallible<()> {
            if !bone.is_string() && !bone.is_unit() {
                return Err(runtime("bind_object bone must be a name or ()"));
            }
            let params =
                json!({"index": object_ref(&state, &object, false)?, "bone": json(&bone)?});
            record(&state, &ctx, "bind_object", "rig.bind", params)
        },
    );
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
    engine.register_fn(
        "set_pose",
        move |ctx: NativeCallContext, pose: Map| -> Fallible<()> {
            let params = json!({"pose": json(&Dynamic::from_map(pose))?});
            record(&state, &ctx, "set_pose", "pose.set", params)
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "clear_pose",
        move |ctx: NativeCallContext| -> Fallible<()> {
            record(&state, &ctx, "clear_pose", "pose.clear", json!({}))
        },
    );

    let state = scratch.clone();
    engine.register_fn(
        "add_clip",
        move |ctx: NativeCallContext, name: &str| -> Fallible<()> {
            record(&state, &ctx, "add_clip", "clip.add", json!({"name": name}))
        },
    );
    let state = scratch.clone();
    engine.register_fn(
        "set_clip_keys",
        move |ctx: NativeCallContext, name: &str, bone: &str, keys: Array| -> Fallible<()> {
            let params = json!({
                "name": name,
                "bone": bone,
                "keys": json(&Dynamic::from_array(keys))?,
            });
            record(&state, &ctx, "set_clip_keys", "clip.keys", params)
        },
    );
    let state = scratch.clone();
    engine.register_fn("list_clips", move || -> Array {
        list(&state.borrow().view.clips, "clips")
    });
    let state = scratch.clone();
    engine.register_fn(
        "remove_clip",
        move |ctx: NativeCallContext, name: &str| -> Fallible<()> {
            record(
                &state,
                &ctx,
                "remove_clip",
                "clip.remove",
                json!({"name": name}),
            )
        },
    );
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

/// An object by index or name, as the `index` argument. `remove` refuses
/// the host object, since the run writes its voxels.
fn object_ref(state: &Rc<RefCell<Scratch>>, object: &Dynamic, remove: bool) -> Fallible<Value> {
    let scratch = state.borrow();
    let BuiltKey::Index(host) = scratch.built[0].key else {
        unreachable!("the host is the first object a run writes");
    };
    let is_host = if let Ok(index) = object.as_int() {
        if index < 0 {
            return Err(runtime(format!("object index {index} is out of range")));
        }
        index as usize == host
    } else if object.is_string() {
        let name = object.clone().into_string().map_err(runtime)?;
        scratch.names.get(host) == Some(&name)
    } else {
        return Err(runtime("an object is an index or a name"));
    };
    if remove && is_host {
        return Err(runtime("a script cannot remove its own object"));
    }
    json(object)
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
