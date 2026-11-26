//! The options a tool's panel edits, kept as that tool's MCP arguments
//! (`Session::op_args`), the way a Blender operator keeps its properties.
//! The widgets of `panels/props.rs` read and write them by schema key, and a
//! drag hands them to the same argument check and parser MCP uses.
//! `docs/op-properties.md` is the design.

use serde_json::{json, Map, Value};

use super::{Session, SliderTarget};

/// One tool's panel options as MCP arguments.
pub type OpArgs = Map<String, Value>;

impl Session {
    /// The panel options a new session starts from: the schema defaults,
    /// plus the few a panel starts from otherwise (the Tube tool draws
    /// smooth curves, and stripes in six sectors).
    pub(super) fn panel_op_args() -> std::collections::BTreeMap<&'static str, OpArgs> {
        let mut tube = OpArgs::new();
        tube.insert("curve".into(), json!("smooth"));
        tube.insert("sectors".into(), json!(6));
        [("fill_tube", tube)].into_iter().collect()
    }

    /// A panel option: the value set, else the schema default, else null.
    pub(crate) fn op_arg(&self, tool: &str, key: &str) -> Value {
        self.op_args
            .get(tool)
            .and_then(|args| args.get(key))
            .cloned()
            .or_else(|| {
                crate::ops::schema::properties(tool)?
                    .get(key)?
                    .get("default")
                    .cloned()
            })
            .unwrap_or(Value::Null)
    }

    /// Set a panel option, or leave it out (`None`) so it takes its default.
    pub(crate) fn set_op_arg(&mut self, tool: &'static str, key: &str, value: Option<Value>) {
        let args = self.op_args.entry(tool).or_default();
        match value {
            Some(value) => {
                args.insert(key.to_string(), value);
            }
            None => {
                args.remove(key);
            }
        }
    }

    /// A tool's panel options plus `extra`, checked and filled in exactly as
    /// `remote::apply` does for an MCP call of `tool`, so the panel cannot
    /// send what MCP would refuse.
    pub(crate) fn op_call(&self, tool: &str, extra: OpArgs) -> Result<Value, String> {
        let op = crate::ops::by_tool(tool).ok_or_else(|| format!("no tool `{tool}`"))?;
        let mut args = self.op_args.get(tool).cloned().unwrap_or_default();
        args.extend(extra);
        let mut args = Value::Object(args);
        crate::ops::check_arguments(op.method, &args)?;
        crate::ops::fill_defaults(op.method, &mut args);
        Ok(args)
    }
}

impl Session {
    /// The value of a slider of [`SliderTarget::op_arg`].
    pub(crate) fn op_slider_value(&self, target: SliderTarget) -> f32 {
        let (tool, key) = target.op_arg().expect("a slider over a tool argument");
        self.op_arg(tool, key).as_f64().unwrap_or(0.0) as f32
    }

    /// Write a slider of [`SliderTarget::op_arg`], kept inside the schema's
    /// soft range and to whole numbers for an integer argument.
    pub(crate) fn write_op_slider(&mut self, target: SliderTarget, value: f32) {
        let (tool, key) = target.op_arg().expect("a slider over a tool argument");
        let prop = crate::ops::prop_ui(tool, key);
        let (min, max) = prop.soft.unwrap_or((f64::MIN, f64::MAX));
        let value = f64::from(value).clamp(min, max);
        let value = if prop.integer {
            json!(value.round() as i64)
        } else {
            json!(value)
        };
        self.set_op_arg(prop.tool, key, Some(value));
    }
}

impl SliderTarget {
    /// The tool argument a slider edits in [`Session::op_args`], for the
    /// sliders drawn by `panels/props.rs` from the schema.
    pub(crate) fn op_arg(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            SliderTarget::TubeAspect => ("fill_tube", "aspect"),
            SliderTarget::TubeRoll => ("fill_tube", "roll"),
            SliderTarget::TubePeriod => ("fill_tube", "period"),
            SliderTarget::TubeSectors => ("fill_tube", "sectors"),
            _ => return None,
        })
    }
}
