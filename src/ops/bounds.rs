//! The two forms of a box argument. Every tool whose schema takes the
//! inclusive corners `x0, y0, z0, x1, y1, z1` (or a rectangle's subset of
//! them, `planks`' `x0, y0, x1, y1`) also takes `min: [x, y, z]` and
//! `max: [x, y, z]`, the same inclusive corners as lists. [`with_box_forms`]
//! adds `min` and `max` to those schemas, and [`normalize`] turns `min` and
//! `max` into the corners before a tool reads them, for MCP, `batch`, the
//! control port, the CLI, and Rhai alike, so no tool parses `min` and `max`
//! itself.

use std::sync::OnceLock;

use serde_json::{json, Map, Value};

/// The corner keys in `x0, y0, z0, x1, y1, z1` order.
pub(crate) const CORNERS: [&str; 6] = ["x0", "y0", "z0", "x1", "y1", "z1"];
/// The list form's keys.
pub(crate) const LIST_KEYS: [&str; 2] = ["min", "max"];

/// How one tool takes its box.
#[derive(Clone, Debug)]
pub(crate) struct BoxForm {
    pub tool: String,
    /// The axes the box has, as indices into `x, y, z`: all three, or `x, y`
    /// for `planks`' rectangle.
    pub axes: Vec<usize>,
    /// The tool needs a box: its schema required every corner.
    pub required: bool,
}

impl BoxForm {
    /// The corner keys of the box's minimum and maximum, in axis order.
    pub(crate) fn corner_keys(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        (
            self.axes.iter().map(|axis| CORNERS[*axis]).collect(),
            self.axes.iter().map(|axis| CORNERS[axis + 3]).collect(),
        )
    }

    fn axis_names(&self) -> String {
        self.axes
            .iter()
            .map(|axis| ["x", "y", "z"][*axis])
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The box in words, for an error.
    fn forms(&self) -> String {
        let (lo, hi) = self.corner_keys();
        let names = self.axis_names();
        format!(
            "{} or min: [{names}] and max: [{names}]",
            lo.iter().chain(&hi).copied().collect::<Vec<_>>().join(", ")
        )
    }
}

static FORMS: OnceLock<Vec<BoxForm>> = OnceLock::new();

/// Every tool that takes a box, in [`super::OPS`] order.
pub(crate) fn box_tools() -> &'static [BoxForm] {
    super::schema::tools();
    FORMS.get().expect("schema::tools records the box forms")
}

/// How `tool` takes a box, if it does.
pub(crate) fn box_form(tool: &str) -> Option<&'static BoxForm> {
    box_tools().iter().find(|form| form.tool == tool)
}

/// Add `min` and `max` to every schema that takes the corners, and take the
/// corners out of `required`: a call gives one form or the other, which
/// [`normalize`] checks. Called once, while the schemas are built.
pub(super) fn with_box_forms(tools: &mut [Value]) {
    let mut forms = Vec::new();
    for tool in tools.iter_mut() {
        let name = tool["name"].as_str().unwrap_or_default().to_string();
        let schema = &mut tool["inputSchema"];
        let Some(properties) = schema["properties"].as_object() else {
            continue;
        };
        let axes: Vec<usize> = (0..3)
            .filter(|axis| {
                properties.contains_key(CORNERS[*axis])
                    && properties.contains_key(CORNERS[axis + 3])
            })
            .collect();
        if axes.len() < 2 {
            continue;
        }
        for key in LIST_KEYS {
            assert!(
                !properties.contains_key(key),
                "`{name}` takes corners and its own `{key}`"
            );
        }
        let form = BoxForm {
            tool: name,
            required: false,
            axes,
        };
        let (lo, hi) = form.corner_keys();
        let corner_keys: Vec<&str> = lo.iter().chain(&hi).copied().collect();
        let mut required = false;
        if let Some(list) = schema["required"].as_array_mut() {
            let had = list.len();
            list.retain(|key| !corner_keys.iter().any(|corner| key == corner));
            required = list.len() + corner_keys.len() == had;
            assert!(
                required || list.len() == had,
                "`{}` requires some of its corners but not all",
                form.tool
            );
        }
        let names = form.axis_names();
        let count = form.axes.len();
        let lead = if required {
            "Pass this box or"
        } else {
            "Optional; this box or"
        };
        let side = |which: &str, keys: &[&str]| {
            json!({
                "type": "array",
                "items": {"type": "integer"},
                "minItems": count,
                "maxItems": count,
                "description": format!(
                    "Inclusive {which} corner [{names}], the same as {}. {lead} the corners, not both.",
                    keys.join(", ")
                )
            })
        };
        let (min, max) = (side("minimum", &lo), side("maximum", &hi));
        let properties = schema["properties"].as_object_mut().expect("checked above");
        properties.insert("min".into(), min);
        properties.insert("max".into(), max);
        forms.push(BoxForm { required, ..form });
    }
    let _ = FORMS.set(forms);
}

/// Turn `min` and `max` into the corners for `tool`, so the tool reads one
/// form. Refused: one of `min` and `max` without the other, a list of the
/// wrong length or with a non-integer, both forms that disagree, part of the
/// corners, and neither form when the tool needs a box.
pub(crate) fn normalize(tool: &str, args: &mut Map<String, Value>) -> Result<(), String> {
    let Some(form) = box_form(tool) else {
        return Ok(());
    };
    let (lo, hi) = form.corner_keys();
    let corner_keys: Vec<&str> = lo.iter().chain(&hi).copied().collect();
    let given: Vec<&str> = corner_keys
        .iter()
        .copied()
        .filter(|key| args.get(*key).is_some_and(|value| !value.is_null()))
        .collect();
    let lists: Vec<&str> = LIST_KEYS
        .into_iter()
        .filter(|key| args.get(*key).is_some_and(|value| !value.is_null()))
        .collect();
    for key in LIST_KEYS {
        if args.get(key).is_some_and(Value::is_null) {
            args.remove(key);
        }
    }
    if lists.is_empty() {
        if !given.is_empty() && given.len() < corner_keys.len() {
            let missing: Vec<&str> = corner_keys
                .iter()
                .copied()
                .filter(|key| !given.contains(key))
                .collect();
            let names = form.axis_names();
            return Err(format!(
                "{tool} has part of a box: it takes all of {} ({} missing) or min: [{names}] and max: [{names}]",
                corner_keys.join(", "),
                missing.join(", "),
            ));
        }
        if given.is_empty() && form.required {
            return Err(format!("{tool} needs a box: pass {}", form.forms()));
        }
        return Ok(());
    }
    if lists.len() == 1 {
        let other = if lists[0] == "min" { "max" } else { "min" };
        return Err(format!(
            "{tool} {} needs {other}; pass {}",
            lists[0],
            form.forms()
        ));
    }
    let names = form.axis_names();
    let read = |key: &str| -> Result<Vec<i64>, String> {
        let list = args[key]
            .as_array()
            .filter(|list| list.len() == form.axes.len())
            .ok_or_else(|| format!("{tool} {key} must be [{names}]"))?;
        list.iter()
            .map(|value| {
                value
                    .as_i64()
                    .or_else(|| {
                        value
                            .as_f64()
                            .filter(|v| v.fract() == 0.0)
                            .map(|v| v as i64)
                    })
                    .ok_or_else(|| format!("{tool} {key} must be [{names}] of integers"))
            })
            .collect()
    };
    let (min, max) = (read("min")?, read("max")?);
    let values: Vec<i64> = min.iter().chain(&max).copied().collect();
    if !given.is_empty() {
        let agrees = given.len() == corner_keys.len()
            && corner_keys.iter().zip(&values).all(|(key, value)| {
                let found = &args[*key];
                found.as_i64() == Some(*value) || found.as_f64().is_some_and(|v| v == *value as f64)
            });
        if !agrees {
            return Err(format!(
                "{tool} has the box twice, as {} and as min and max, and they differ; pass one form",
                given.join(", ")
            ));
        }
    }
    for key in LIST_KEYS {
        args.remove(key);
    }
    for (key, value) in corner_keys.iter().zip(values) {
        args.insert((*key).into(), value.into());
    }
    Ok(())
}

/// [`normalize`] for the tool that runs `method`; with several (`carve`
/// and its shapes share none), the first that takes a box.
pub(crate) fn normalize_method(method: &str, args: &mut Value) -> Result<(), String> {
    let Some(map) = args.as_object_mut() else {
        return Ok(());
    };
    match super::by_method(method).find(|op| box_form(op.tool).is_some()) {
        Some(op) => normalize(op.tool, map),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "../../tests/bounds.rs"]
mod tests;
