//! MCP `skin`: an object's surface covered with irregular bricks
//! (`tools/skin.rs`). One call sets the whole skin; an omitted field takes
//! its default.

use serde_json::{json, Value};

use super::edit::object_arg;
use super::{opt_bool, opt_f64, opt_str, slot_list};
use crate::state::Session;
use crate::tools::{Skin, SkinAxis};

/// The skin a call asks for, or `None` with `off`.
pub(crate) fn skin_arg(params: &Value) -> Result<Option<Skin>, String> {
    if opt_bool(params, "off")?.unwrap_or(false) {
        return Ok(None);
    }
    let d = Skin::default();
    let num = |key: &str, default: f32| -> Result<f32, String> {
        Ok(opt_f64(params, key)?.map(|v| v as f32).unwrap_or(default))
    };
    let axis = match opt_str(params, "axis")? {
        None => d.axis,
        Some(name) => SkinAxis::parse(&name)
            .ok_or_else(|| format!("axis must be {}, not {name}", SkinAxis::NAMES))?,
    };
    let length = match params.get("length") {
        None | Some(Value::Null) => d.length,
        Some(Value::Array(pair)) if pair.len() == 2 => {
            let end = |v: &Value| {
                v.as_f64()
                    .map(|v| v as f32)
                    .ok_or("length must be [shortest, longest] in cells".to_string())
            };
            [end(&pair[0])?, end(&pair[1])?]
        }
        Some(one) => match one.as_f64() {
            Some(v) => [v as f32, v as f32],
            None => return Err("length must be [shortest, longest] in cells".into()),
        },
    };
    let seed = match params.get("seed") {
        None | Some(Value::Null) => d.seed,
        Some(v) => v
            .as_u64()
            .filter(|v| *v <= u32::MAX as u64)
            .ok_or("seed must be a whole number 0..=4294967295")? as u32,
    };
    let skin = Skin {
        axis,
        length,
        width: num("width", d.width)?,
        height: num("height", d.height)?,
        depth: num("depth", d.depth)?,
        stagger: num("stagger", d.stagger)?,
        relief: num("relief", d.relief)?,
        tilt: num("tilt", d.tilt)?,
        follow: num("follow", d.follow)?,
        gap: num("gap", d.gap)?,
        smooth: num("smooth", d.smooth)?,
        thin: num("thin", d.thin)?,
        bare: slot_list(params, "bare")?,
        seed,
    };
    skin.check()?;
    Ok(Some(skin))
}

/// A skin as the tool takes it.
pub(crate) fn skin_json(skin: &Skin) -> Value {
    json!({
        "axis": skin.axis.name(),
        "length": skin.length,
        "width": skin.width,
        "height": skin.height,
        "depth": skin.depth,
        "stagger": skin.stagger,
        "relief": skin.relief,
        "tilt": skin.tilt,
        "follow": skin.follow,
        "gap": skin.gap,
        "smooth": skin.smooth,
        "thin": skin.thin,
        "bare": skin.bare,
        "seed": skin.seed,
    })
}

pub(super) fn set_skin(session: &mut Session, params: &Value) -> Result<Value, String> {
    let object = object_arg(session, params, "object")?.unwrap_or(session.selected);
    let skin = skin_arg(params)?;
    session.set_skin(object, skin)?;
    Ok(reply(session, object))
}

/// The object, its name, its skin (or null), and its brick count.
pub(crate) fn reply(session: &Session, object: usize) -> Value {
    let o = &session.objects[object];
    json!({
        "object": object,
        "name": o.name,
        "skin": o.skin.as_ref().map(skin_json),
        "bricks": session.skin_boxes(object).len(),
    })
}

#[cfg(test)]
#[path = "../../tests/skin_tool.rs"]
mod tests;
