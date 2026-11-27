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
    /// smooth curves, and stripes in six sectors; the Generator tool's kind).
    pub(super) fn panel_op_args() -> std::collections::BTreeMap<&'static str, OpArgs> {
        let mut tube = OpArgs::new();
        tube.insert("curve".into(), json!("smooth"));
        tube.insert("sectors".into(), json!(6));
        // The Generator tool starts on the ellipsoid; `kind` has no default
        // since an MCP call names it.
        let mut generate = OpArgs::new();
        generate.insert("kind".into(), json!("ellipsoid"));
        [("fill_tube", tube), ("generate", generate)]
            .into_iter()
            .collect()
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

    /// Flip a toggle option ([`crate::ops::PropUi::on`], or a boolean):
    /// on becomes left out, off becomes on.
    pub(crate) fn flip_op_arg(&mut self, tool: &'static str, key: &str) {
        let prop = crate::ops::prop_ui(tool, key);
        let current = self.op_arg(tool, key);
        let on = match prop.on {
            Some(item) => current == item,
            None => current == true,
        };
        let value = match (on, prop.on) {
            (true, _) => None,
            (false, Some(item)) => Some(item.into()),
            (false, None) => Some(true.into()),
        };
        self.set_op_arg(tool, key, value);
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
        let value = target.registry_clamp_f64(f64::from(value));
        let value = if prop.integer {
            json!(value as i64)
        } else {
            json!(value)
        };
        self.set_op_arg(prop.tool, key, Some(value));
    }
}

impl SliderTarget {
    /// The tool argument (or `key.field`) a slider drawn from the registry
    /// shows: its label, hint, and range come from there, and
    /// [`Session::write_slider`] keeps a value inside that range.
    pub(crate) fn schema_prop(self) -> Option<(&'static str, &'static str)> {
        if let Some(arg) = self.op_arg() {
            return Some(arg);
        }
        Some(match self {
            SliderTarget::TubeStart | SliderTarget::TubeEnd => ("fill_tube", "radius"),
            SliderTarget::SmoothIterations => ("smooth", "iterations"),
            SliderTarget::SmoothFillAt => ("smooth", "fillAt"),
            SliderTarget::SmoothRemoveAt => ("smooth", "removeAt"),
            SliderTarget::ExportAo => ("export_mesh", "aoStrength"),
            SliderTarget::ExportBevel => ("export_mesh", "bevel"),
            SliderTarget::ExportAoRadius => ("export_mesh", "aoRadius"),
            SliderTarget::BakeOcclusion => ("export_mesh", "bake.occlusion"),
            SliderTarget::BakeDistance => ("export_mesh", "bake.distance"),
            SliderTarget::BakeRays => ("export_mesh", "bake.rays"),
            SliderTarget::BakeSunEnergy => ("export_mesh", "bake.sunEnergy"),
            SliderTarget::BakeAmbientEnergy => ("export_mesh", "bake.ambientEnergy"),
            SliderTarget::BakeSunSoftness => ("export_mesh", "bake.sunSoftness"),
            SliderTarget::MaxSamples => ("render", "samples"),
            SliderTarget::Aperture => ("render", "aperture"),
            SliderTarget::FocusDistance => ("render", "focus"),
            SliderTarget::MaxBounces => ("render", "bounces"),
            SliderTarget::SkyIntensity => ("render", "skyIntensity"),
            SliderTarget::SunIntensity => ("render", "sunIntensity"),
            SliderTarget::NeeRis => ("render", "ris"),
            SliderTarget::IndirectClamp => ("render", "indirectClamp"),
            SliderTarget::HorizonSoftness => ("render", "horizon"),
            SliderTarget::SunElevation => ("render", "sunElevation"),
            SliderTarget::SunAzimuth => ("render", "sunAzimuth"),
            SliderTarget::SunDisc => ("render", "sunDisc"),
            SliderTarget::EmaFloor => ("set_view", "temporalSmooth"),
            SliderTarget::BrushSize => ("set_tool", "size"),
            SliderTarget::FoliageSeed => ("set_tool", "seedColor"),
            SliderTarget::FoliageDensity => ("set_tool", "density"),
            SliderTarget::MaterialRoughness => ("set_palette", "roughness"),
            SliderTarget::MaterialMetallic => ("set_palette", "metallic"),
            SliderTarget::MaterialTransparency => ("set_palette", "transparency"),
            SliderTarget::MaterialIor => ("set_palette", "ior"),
            SliderTarget::MaterialEmission => ("set_palette", "emission"),
            SliderTarget::MaterialPower => ("set_palette", "power"),
            SliderTarget::MotionAmount => ("set_palette", "motionAmount"),
            _ => return None,
        })
    }

    /// [`Self::schema_prop`] as the panel metadata it names.
    pub(crate) fn prop(self) -> Option<crate::ops::PropUi> {
        let (tool, path) = self.schema_prop()?;
        Some(if path.contains('.') {
            crate::ops::field_ui(tool, path)
        } else {
            crate::ops::prop_ui(tool, path)
        })
    }

    /// `value` kept inside the registry range of a schema-drawn slider, and
    /// whole for an integer one; other sliders keep their own clamps.
    pub(crate) fn registry_clamp(self, value: f32) -> f32 {
        self.registry_clamp_f64(f64::from(value)) as f32
    }

    /// [`Self::registry_clamp`] in the precision a tool argument keeps.
    pub(crate) fn registry_clamp_f64(self, value: f64) -> f64 {
        let Some(prop) = self.prop() else {
            return value;
        };
        let Some((min, max)) = prop.soft else {
            return value;
        };
        let value = value.clamp(min, max);
        if prop.integer {
            value.round()
        } else {
            value
        }
    }

    /// The tool argument a slider edits in [`Session::op_args`], for the
    /// sliders drawn by `panels/props.rs` from the schema.
    pub(crate) fn op_arg(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            SliderTarget::TubeAspect => ("fill_tube", "aspect"),
            SliderTarget::StrandWidth => ("add_strand", "width"),
            SliderTarget::StrandThickness => ("add_strand", "thickness"),
            SliderTarget::StrandStep => ("add_strand", "step"),
            SliderTarget::StrandPeriod => ("add_strand", "period"),
            SliderTarget::TubeRoll => ("fill_tube", "roll"),
            SliderTarget::TubePeriod => ("fill_tube", "period"),
            SliderTarget::TubeSectors => ("fill_tube", "sectors"),
            SliderTarget::RoundRadius => ("generate", "radius"),
            SliderTarget::TorusTube => ("generate", "torusTube"),
            SliderTarget::NoiseScale => ("generate", "scale"),
            SliderTarget::NoiseThreshold => ("generate", "threshold"),
            SliderTarget::NoiseOctaves => ("generate", "octaves"),
            SliderTarget::NoiseSeed => ("generate", "seed"),
            SliderTarget::PerlinLo => ("generate", "paletteLow"),
            SliderTarget::PerlinHi => ("generate", "paletteHigh"),
            SliderTarget::TreeDepth => ("generate", "depth"),
            SliderTarget::TreeWildness => ("generate", "wildness"),
            SliderTarget::TreeTrunk => ("generate", "treeTrunk"),
            SliderTarget::TreeLeaf => ("generate", "leaf"),
            SliderTarget::TreeSeed => ("generate", "seed"),
            SliderTarget::HelixTurns => ("generate", "turns"),
            SliderTarget::HelixRadius => ("generate", "helixRadius"),
            SliderTarget::HelixTube => ("generate", "helixTube"),
            SliderTarget::ConiferTiers => ("generate", "tiers"),
            SliderTarget::ConiferRagged => ("generate", "raggedness"),
            SliderTarget::ConiferClump => ("generate", "clump"),
            SliderTarget::ConiferTrunk => ("generate", "coniferTrunk"),
            SliderTarget::ConiferCrown => ("generate", "crown"),
            SliderTarget::ConiferNeedle => ("generate", "needle"),
            SliderTarget::ConiferSeed => ("generate", "seed"),
            _ => return None,
        })
    }
}
