//! MCP `set_marker`, `remove_marker`, and `list_markers`, and the marker
//! arguments Rhai's `marker`, `collision_box`, and `connector` share.

use serde_json::{json, Value};

use super::{opt_ivec3, opt_str, opt_vec3, req_str};
use crate::state::{cell_box, check_direction, check_point, MarkerKind, Session};

pub(super) fn set_marker(session: &mut Session, params: &Value) -> Result<Value, String> {
    let name = req_str(params, "name")?;
    let index = marker_object(session, params)?;
    let kind = marker_kind(params)?;
    session.set_marker(index, &name, kind)?;
    let marker = session
        .markers_json()
        .as_array()
        .and_then(|all| all.iter().find(|m| m["name"] == json!(name)).cloned())
        .unwrap_or(Value::Null);
    Ok(json!({"marker": marker, "markers": session.markers.len()}))
}

/// MCP `object`: an index or a name, or the active object when missing.
pub(super) fn marker_object(session: &Session, params: &Value) -> Result<usize, String> {
    match params.get("object") {
        None | Some(Value::Null) => Ok(session.selected),
        Some(Value::String(name)) => session
            .objects
            .iter()
            .position(|object| object.name == *name)
            .ok_or_else(|| format!("no object named `{name}`")),
        Some(found) => found
            .as_u64()
            .map(|index| index as usize)
            .filter(|index| *index < session.objects.len())
            .ok_or_else(|| {
                format!(
                    "object must be an index 0..{} or a name",
                    session.objects.len()
                )
            }),
    }
}

pub(super) fn remove_marker(session: &mut Session, params: &Value) -> Result<Value, String> {
    let all = params.get("all").and_then(Value::as_bool).unwrap_or(false);
    let name = opt_str(params, "name")?;
    let removed = match (name, all) {
        (Some(_), true) => return Err("pass name or all, not both".into()),
        (Some(name), false) => session.remove_markers(Some(&name))?,
        (None, true) => session.remove_markers(None)?,
        (None, false) => return Err("name is required, or all: true".into()),
    };
    Ok(json!({"removed": removed, "markers": session.markers.len()}))
}

pub(super) fn list_markers(session: &Session) -> Value {
    json!({"markers": session.markers_json()})
}

/// The marker in MCP `set_marker` arguments: `kind` `point` with `point`,
/// `box` with the inclusive cell corners `x0..z1`, or `connector` with
/// `point` and `direction`.
pub(crate) fn marker_kind(params: &Value) -> Result<MarkerKind, String> {
    let kind = opt_str(params, "kind")?.unwrap_or_else(|| "point".into());
    let point = || -> Result<[f32; 3], String> {
        let p = opt_vec3(params, "point")?.ok_or("point [x, y, z] is required")?;
        check_point(p, "point")
    };
    match kind.as_str() {
        "point" => Ok(MarkerKind::Point { at: point()? }),
        "connector" => {
            let direction =
                opt_vec3(params, "direction")?.ok_or("direction [x, y, z] is required")?;
            Ok(MarkerKind::Connector {
                at: point()?,
                direction: check_direction(direction)?,
            })
        }
        "box" => {
            let corner = |a: &str, b: &str, c: &str| -> Result<[i32; 3], String> {
                let mut out = [0; 3];
                for (slot, key) in [a, b, c].iter().enumerate() {
                    out[slot] = params
                        .get(*key)
                        .and_then(Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .filter(|v| (-4096..=4096).contains(v))
                        .ok_or_else(|| format!("{key} is required: an integer cell coordinate"))?;
                }
                Ok(out)
            };
            if let Some(min) = opt_ivec3(params, "min")? {
                let max = opt_ivec3(params, "max")?.ok_or("max is required with min")?;
                return Ok(cell_box(min, max));
            }
            Ok(cell_box(
                corner("x0", "y0", "z0")?,
                corner("x1", "y1", "z1")?,
            ))
        }
        other => Err(format!(
            "kind must be point, box, or connector, not `{other}`"
        )),
    }
}
