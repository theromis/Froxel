//! MCP `collision_from_voxels`: its arguments and its reply. The MCP
//! handler, the Rhai `collision_from_voxels`, and the Markers panel's button
//! all come through [`collision_build`].

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::{as_i32, opt_i32_range, opt_str, slot_list};
use crate::state::{
    cell_box, check_marker_name, replace_collision_markers, Cell, MarkerKind, Session, MAX_MARKERS,
};
use crate::tools::{collision_cover, CollisionCover, CollisionSpec};

/// The default prefix keeps this many characters of the object's name.
const DEFAULT_PREFIX_CHARS: usize = 48;

/// What one call builds: the markers to place, under `prefix`, and the reply.
pub(crate) struct CollisionBuild {
    pub(crate) prefix: String,
    pub(crate) boxes: Vec<(String, MarkerKind)>,
    pub(crate) reply: Map<String, Value>,
}

/// MCP `collision_from_voxels` arguments as a spec.
fn collision_spec(params: &Value) -> Result<CollisionSpec, String> {
    let bands = match params.get("zBands") {
        None | Some(Value::Null) => Vec::new(),
        Some(found) => {
            let bad = "zBands must be at least two ascending z planes in local mesh units";
            let planes: Vec<i32> = found
                .as_array()
                .ok_or(bad)?
                .iter()
                .map(as_i32)
                .collect::<Option<_>>()
                .ok_or(bad)?;
            if planes.len() < 2
                || planes.windows(2).any(|pair| pair[0] >= pair[1])
                || planes.iter().any(|plane| plane.abs() > 4096)
            {
                return Err(bad.into());
            }
            planes
        }
    };
    let has_layer = params.get("layer").is_some_and(|v| !v.is_null());
    if has_layer && !bands.is_empty() {
        return Err("pass layer or zBands, not both".into());
    }
    let max_boxes = match params.get("maxBoxes") {
        None | Some(Value::Null) => None,
        Some(found) => Some(
            found
                .as_u64()
                .filter(|count| (1..=MAX_MARKERS as u64).contains(count))
                .ok_or_else(|| format!("maxBoxes must be a whole number 1..={MAX_MARKERS}"))?
                as usize,
        ),
    };
    Ok(CollisionSpec {
        only: slot_list(params, "only")?,
        exclude: slot_list(params, "exclude")?,
        layer: opt_i32_range(params, "layer", 1, 1, 64)?,
        bands,
        min_feature: opt_i32_range(params, "minFeature", 1, 1, 32)?,
        hole_size: opt_i32_range(params, "holeSize", 3, 1, 64)?,
        max_boxes,
    })
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    (part as f64 * 10_000.0 / whole as f64).round() / 100.0
}

fn cover_reply(prefix: &str, cover: &CollisionCover) -> Map<String, Value> {
    let mut reply = Map::new();
    reply.insert("prefix".into(), json!(prefix));
    reply.insert("boxes".into(), json!(cover.boxes.len()));
    reply.insert("solidCells".into(), json!(cover.solid));
    reply.insert("ignoredBySlot".into(), json!(cover.ignored_slots));
    reply.insert("ignoredThin".into(), json!(cover.ignored_thin));
    reply.insert("targetCells".into(), json!(cover.target));
    reply.insert("coveredCells".into(), json!(cover.covered));
    reply.insert("missedCells".into(), json!(cover.target - cover.covered));
    reply.insert(
        "coveredPercent".into(),
        json!(percent(cover.covered, cover.target)),
    );
    reply.insert("boxCells".into(), json!(cover.box_cells));
    reply.insert("emptyFilledCells".into(), json!(cover.empty_filled));
    reply.insert(
        "emptyFilledPercent".into(),
        json!(percent(cover.empty_filled, cover.box_cells)),
    );
    reply.insert("merged".into(), json!(cover.merged));
    reply.insert("dropped".into(), json!(cover.dropped));
    if cover.dropped > 0 {
        reply.insert(
            "warning".into(),
            json!(format!(
                "{} boxes with the least solid were dropped to meet maxBoxes, because a further merge would close a hollow of holeSize. Raise maxBoxes or holeSize, or leave out thin slots.",
                cover.dropped
            )),
        );
    }
    reply
}

/// The boxes of `object` with `cells` under MCP `collision_from_voxels`
/// arguments.
pub(crate) fn collision_build(
    cells: &HashMap<Cell, u8>,
    object: &str,
    params: &Value,
) -> Result<CollisionBuild, String> {
    let prefix = match opt_str(params, "prefix")? {
        Some(prefix) => check_marker_name(&prefix)?,
        None => {
            let name: String = object.chars().take(DEFAULT_PREFIX_CHARS).collect();
            check_marker_name(&format!("{name}_collider"))?
        }
    };
    let spec = collision_spec(params)?;
    let cover = collision_cover(cells, &spec)?;
    let mut boxes = Vec::with_capacity(cover.boxes.len());
    for (index, (low, high)) in cover.boxes.iter().enumerate() {
        let name = check_marker_name(&format!("{prefix}_{}", index + 1))?;
        boxes.push((
            name,
            cell_box([low.0, low.1, low.2], [high.0, high.1, high.2]),
        ));
    }
    Ok(CollisionBuild {
        reply: cover_reply(&prefix, &cover),
        prefix,
        boxes,
    })
}

/// Add what the call replaced and the document's marker count to the reply.
pub(crate) fn finish_reply(
    mut reply: Map<String, Value>,
    replaced: usize,
    markers: usize,
) -> Value {
    reply.insert("replaced".into(), json!(replaced));
    reply.insert("markers".into(), json!(markers));
    Value::Object(reply)
}

pub(super) fn collision_from_voxels(
    session: &mut Session,
    params: &Value,
) -> Result<Value, String> {
    let index = super::markers::marker_object(session, params)?;
    let object = &session.objects[index];
    let build = collision_build(&object.cells, &object.name, params)?;
    let name = object.name.clone();
    let mut next = session.markers.clone();
    let replaced = replace_collision_markers(&mut next, &name, &build.prefix, build.boxes, false)?;
    session.set_markers_undoable(next);
    Ok(finish_reply(build.reply, replaced, session.markers.len()))
}
