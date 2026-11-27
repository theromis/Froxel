//! MCP `add_strand`, `edit_strand`, `remove_strand`, `list_strands`, and
//! `bake_strands`: chains of turned cubes along a curve on an object
//! (`tools/strand.rs`). Rhai's functions of the same names come here too.

use serde_json::{json, Value};

use super::edit::object_arg;
use super::{as_i32, opt_bool, opt_f64, opt_str, slot_list};
use crate::state::Session;
use crate::tools::{Strand, StrandPattern, StrandPoint};

/// The defaults a new strand starts from; the schema's "Defaults to" match.
pub(crate) const DEFAULT_WIDTH: f64 = 1.0;
pub(crate) const DEFAULT_THICKNESS: f64 = 1.0;
pub(crate) const DEFAULT_STEP: f64 = 1.0;
pub(crate) const DEFAULT_PERIOD: f64 = 2.0;

fn target(session: &Session, params: &Value) -> Result<usize, String> {
    Ok(object_arg(session, params, "object")?.unwrap_or(session.selected))
}

/// One path point: `[x, y, z]`, `[x, y, z, twist]`, `[x, y, z, twist, scale]`,
/// or `{x, y, z, twist?, scale?}`.
pub(crate) fn point_arg(value: &Value, what: &str) -> Result<StrandPoint, String> {
    let names = ["x", "y", "z", "twist", "scale"];
    let mut values = [None; 5];
    let number = |v: Option<&Value>, name: &str| -> Result<Option<f64>, String> {
        match v {
            None | Some(Value::Null) => Ok(None),
            Some(found) => found
                .as_f64()
                .map(Some)
                .ok_or_else(|| format!("{what} {name} must be a number")),
        }
    };
    if let Some(arr) = value.as_array() {
        if !(3..=5).contains(&arr.len()) {
            return Err(format!(
                "{what} must be [x, y, z], [x, y, z, twist], or [x, y, z, twist, scale]"
            ));
        }
        for (slot, name) in names.iter().enumerate() {
            values[slot] = number(arr.get(slot), name)?;
        }
    } else if value.is_object() {
        for (slot, name) in names.iter().enumerate() {
            values[slot] = number(value.get(*name), name)?;
        }
    } else {
        return Err(format!(
            "{what} must be [x, y, z, twist?, scale?] or {{x, y, z, twist?, scale?}}"
        ));
    }
    let mut at = [0.0f32; 3];
    for axis in 0..3 {
        at[axis] = values[axis].ok_or_else(|| format!("{what} is missing {}", names[axis]))? as f32;
    }
    Ok(StrandPoint {
        at,
        twist: values[3].unwrap_or(0.0) as f32,
        scale: values[4].unwrap_or(1.0) as f32,
    })
}

fn points_arg(value: &Value) -> Result<Vec<StrandPoint>, String> {
    let list = value
        .as_array()
        .ok_or("points must be a list of [x, y, z, twist?, scale?]")?;
    list.iter()
        .enumerate()
        .map(|(index, point)| point_arg(point, &format!("point {index}")))
        .collect()
}

/// Applies every strand field `params` carries to `strand`: the ones
/// `add_strand` and `edit_strand` share.
fn apply_fields(strand: &mut Strand, params: &Value) -> Result<(), String> {
    if let Some(points) = params.get("points").filter(|v| !v.is_null()) {
        strand.points = points_arg(points)?;
    }
    if let Some(curve) = opt_str(params, "curve")? {
        strand.smooth = match curve.as_str() {
            "smooth" => true,
            "polyline" => false,
            other => return Err(format!("curve must be smooth or polyline, not `{other}`")),
        };
    }
    for (key, field) in [
        ("width", &mut strand.width),
        ("thickness", &mut strand.thickness),
        ("step", &mut strand.step),
        ("period", &mut strand.period),
    ] {
        if let Some(value) = opt_f64(params, key)? {
            *field = value as f32;
        }
    }
    let palette = slot_list(params, "palette")?;
    let palettes = slot_list(params, "palettes")?;
    match (palette.is_empty(), palettes.is_empty()) {
        (false, false) => return Err("pass palette or palettes, not both".into()),
        (false, true) => {
            if palette.len() != 1 {
                return Err("palette is one index; pass palettes for several".into());
            }
            strand.slots = palette;
        }
        (true, false) => strand.slots = palettes,
        (true, true) => {}
    }
    if let Some(pattern) = opt_str(params, "pattern")? {
        strand.pattern = StrandPattern::parse(&pattern).ok_or_else(|| {
            format!("pattern must be gradient, rings, or random, not `{pattern}`")
        })?;
    }
    if let Some(seed) = params.get("seed").filter(|v| !v.is_null()) {
        strand.seed = seed
            .as_u64()
            .filter(|s| *s <= u32::MAX as u64)
            .ok_or("seed must be a whole number 0..=4294967295")? as u32;
    }
    Ok(())
}

/// A new strand from `add_strand`'s arguments, with `fallback` as the slot
/// when none is given (the active palette slot in the editor).
pub(crate) fn new_strand(params: &Value, fallback: u8) -> Result<Strand, String> {
    let mut strand = Strand {
        width: DEFAULT_WIDTH as f32,
        thickness: DEFAULT_THICKNESS as f32,
        step: DEFAULT_STEP as f32,
        period: DEFAULT_PERIOD as f32,
        slots: vec![fallback.max(1)],
        ..Strand::default()
    };
    if params.get("points").is_none_or(Value::is_null) {
        return Err("points is required: 2..=256 [x, y, z, twist?, scale?]".into());
    }
    apply_fields(&mut strand, params)?;
    strand.check()?;
    Ok(strand)
}

pub(crate) fn strand_json(index: usize, strand: &Strand) -> Value {
    let points: Vec<Value> = strand
        .points
        .iter()
        .map(|p| json!([p.at[0], p.at[1], p.at[2], p.twist, p.scale]))
        .collect();
    let mut out = json!({
        "index": index,
        "points": points,
        "curve": if strand.smooth { "smooth" } else { "polyline" },
        "width": strand.width,
        "thickness": strand.thickness,
        "step": strand.step,
        "boxes": strand.boxes().len(),
        "length": strand.length(),
    });
    if strand.slots.len() == 1 {
        out["palette"] = json!(strand.slots[0]);
    } else {
        out["palettes"] = json!(strand.slots);
        out["pattern"] = json!(strand.pattern.name());
        match strand.pattern {
            StrandPattern::Rings => out["period"] = json!(strand.period),
            StrandPattern::Random => out["seed"] = json!(strand.seed),
            StrandPattern::Gradient => {}
        }
    }
    out
}

fn reply(session: &Session, object: usize, index: Option<usize>, boxes: usize) -> Value {
    let o = &session.objects[object];
    let mut out = json!({
        "object": object,
        "name": o.name,
        "strands": o.strands.len(),
        "boxes": boxes,
    });
    if let Some(index) = index.filter(|i| *i < o.strands.len()) {
        out["strand"] = strand_json(index, &o.strands[index]);
    }
    out
}

/// `add_strand` on a strand list: appends the new strand and returns its index.
pub(crate) fn add_to(
    strands: &mut Vec<Strand>,
    params: &Value,
    fallback: u8,
) -> Result<usize, String> {
    if strands.len() >= crate::tools::MAX_STRANDS {
        return Err(format!(
            "an object holds at most {} strands",
            crate::tools::MAX_STRANDS
        ));
    }
    strands.push(new_strand(params, fallback)?);
    Ok(strands.len() - 1)
}

fn strand_index(strands: &[Strand], params: &Value) -> Result<usize, String> {
    let count = strands.len();
    let index = params
        .get("index")
        .and_then(as_i32)
        .ok_or("index (the strand's index on the object) is required")?;
    if index < 0 || index as usize >= count {
        return Err(format!(
            "strand index {index} is out of range ({count} strands)"
        ));
    }
    Ok(index as usize)
}

fn point_index(params: &Value, key: &str, count: usize, end_ok: bool) -> Result<usize, String> {
    let limit = if end_ok {
        count
    } else {
        count.saturating_sub(1)
    };
    params
        .get(key)
        .and_then(as_i32)
        .filter(|i| *i >= 0 && *i as usize <= limit)
        .map(|i| i as usize)
        .ok_or_else(|| format!("{key} must be a point index 0..={limit}"))
}

/// `edit_strand` on a strand list: changes the strand at `index` and returns
/// that index.
pub(crate) fn edit_in(strands: &mut [Strand], params: &Value) -> Result<usize, String> {
    let index = strand_index(strands, params)?;
    let mut strand = strands[index].clone();
    apply_fields(&mut strand, params)?;
    let count = strand.points.len();
    let at = params.get("at").filter(|v| !v.is_null());
    let has = |key: &str| params.get(key).is_some_and(|v| !v.is_null());
    let point_ops = [has("point"), has("insert"), has("remove")]
        .iter()
        .filter(|b| **b)
        .count();
    if point_ops > 1 {
        return Err("pass one of point, insert, or remove".into());
    }
    if has("point") {
        let i = point_index(params, "point", count, false)?;
        let value = at.ok_or("point needs at, the point's new [x, y, z, twist?, scale?]")?;
        let mut moved = point_arg(value, "at")?;
        // A position alone keeps the point's twist and scale.
        let given = |slot: usize, key: &str| match value.as_array() {
            Some(list) => list.len() > slot,
            None => value.get(key).is_some_and(|v| !v.is_null()),
        };
        if !given(3, "twist") {
            moved.twist = strand.points[i].twist;
        }
        if !given(4, "scale") {
            moved.scale = strand.points[i].scale;
        }
        strand.points[i] = moved;
    } else if has("insert") {
        let i = point_index(params, "insert", count, true)?;
        let value = at.ok_or("insert needs at, the new point's [x, y, z, twist?, scale?]")?;
        strand.points.insert(i, point_arg(value, "at")?);
    } else if has("remove") {
        let i = point_index(params, "remove", count, false)?;
        strand.points.remove(i);
    } else if at.is_some() {
        return Err("at goes with point or insert".into());
    }
    if let Some(delta) = params.get("move").filter(|v| !v.is_null()) {
        let d = delta
            .as_array()
            .filter(|a| a.len() == 3)
            .and_then(|a| {
                Some([
                    a[0].as_f64()? as f32,
                    a[1].as_f64()? as f32,
                    a[2].as_f64()? as f32,
                ])
            })
            .ok_or("move must be [dx, dy, dz] in cells")?;
        strand = strand.shifted(d);
    }
    strand.check()?;
    strands[index] = strand;
    Ok(index)
}

/// `remove_strand` on a strand list: returns how many were removed.
pub(crate) fn remove_from(strands: &mut Vec<Strand>, params: &Value) -> Result<usize, String> {
    if opt_bool(params, "all")?.unwrap_or(false) {
        if params.get("index").is_some_and(|v| !v.is_null()) {
            return Err("pass index or all, not both".into());
        }
        let n = strands.len();
        strands.clear();
        return Ok(n);
    }
    let index = strand_index(strands, params)?;
    strands.remove(index);
    Ok(1)
}

/// `bake_strands`' choice: the strand indices to bake and whether to keep them.
pub(crate) fn bake_choice(
    strands: &[Strand],
    params: &Value,
) -> Result<(Vec<usize>, bool), String> {
    let count = strands.len();
    let which: Vec<usize> = match params.get("strands") {
        None | Some(Value::Null) => (0..count).collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|v| {
                as_i32(v)
                    .filter(|i| *i >= 0 && (*i as usize) < count)
                    .map(|i| i as usize)
                    .ok_or_else(|| format!("strands must be strand indices 0..{count}"))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("strands must be a list of strand indices".into()),
    };
    if which.is_empty() {
        return Err("there are no strands to bake".into());
    }
    Ok((which, opt_bool(params, "keep")?.unwrap_or(false)))
}

pub(crate) fn list_json(strands: &[Strand]) -> Vec<Value> {
    strands
        .iter()
        .enumerate()
        .map(|(i, s)| strand_json(i, s))
        .collect()
}

pub(super) fn add_strand(session: &mut Session, params: &Value) -> Result<Value, String> {
    let object = target(session, params)?;
    let mut strands = session.objects[object].strands.clone();
    let index = add_to(&mut strands, params, session.active_slot.max(1))?;
    let boxes = session.set_strands(object, strands)?;
    Ok(reply(session, object, Some(index), boxes))
}

pub(super) fn edit_strand(session: &mut Session, params: &Value) -> Result<Value, String> {
    let object = target(session, params)?;
    let mut strands = session.objects[object].strands.clone();
    let index = edit_in(&mut strands, params)?;
    let boxes = session.set_strands(object, strands)?;
    Ok(reply(session, object, Some(index), boxes))
}

pub(super) fn remove_strand(session: &mut Session, params: &Value) -> Result<Value, String> {
    let object = target(session, params)?;
    let mut strands = session.objects[object].strands.clone();
    let removed = remove_from(&mut strands, params)?;
    let boxes = session.set_strands(object, strands)?;
    let mut out = reply(session, object, None, boxes);
    out["removed"] = json!(removed);
    Ok(out)
}

pub(super) fn list_strands(session: &Session, params: &Value) -> Result<Value, String> {
    let object = target(session, params)?;
    let o = &session.objects[object];
    let boxes: usize = o.strands.iter().map(|s| s.boxes().len()).sum();
    Ok(json!({
        "object": object,
        "name": o.name,
        "strands": list_json(&o.strands),
        "boxes": boxes,
    }))
}

/// MCP `bake_strands`: write the cells whose centres lie inside the chosen
/// strands' boxes, and remove those strands unless `keep`.
pub(super) fn bake_strands(session: &mut Session, params: &Value) -> Result<Value, String> {
    let object = target(session, params)?;
    let (which, keep) = bake_choice(&session.objects[object].strands, params)?;
    let cells = crate::tools::strand_cells(&session.objects[object].strands, &which);
    let written = session.bake_strand_cells(object, &cells, if keep { &[] } else { &which });
    let mut out = reply(session, object, None, session.strand_boxes(object).len());
    out["cells"] = json!(cells.len());
    out["changed"] = json!(written);
    out["baked"] = json!(which.len());
    out["kept"] = json!(keep);
    out["voxels"] = json!(session.objects[object].cells.len());
    Ok(out)
}

#[cfg(test)]
#[path = "../../tests/strand_tools.rs"]
mod tests;
