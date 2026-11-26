//! The MCP stone and surface tools in Rhai: `joints`, `column_relief`,
//! `weather`, `moss`, `algae`, `vines`, `sweep`, `trim_outline`, `bark`, `planks`,
//! `cavity_tint`, `radial_relief`, and `foliage`.
//!
//! Each call turns its arguments into the MCP tool's arguments and runs
//! `remote::stone_writes` on the object being written, as the MCP handler
//! does on the active object. Options use the MCP names in a trailing map.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Dynamic, Engine, Map, INT};
use serde_json::Value;

use super::bind::{bind, Binding, Pos::*};
use super::shapes::Args;
use super::Scratch;
use crate::remote::{self, Stone};

const BOX: &[&str] = &["x0", "y0", "z0", "x1", "y1", "z1"];

/// The stone tools' bindings ([`super::bind`]). `trim_outline`,
/// `foliage`, and `radial_relief` also have a form over a box; `planks` takes
/// a rectangle at a height.
pub(super) const BINDINGS: &[Binding] = &[
    // The profile is a list of [u, v] points or #{width, height, radius},
    // which `run` spreads into those arguments.
    Binding::new(
        "sweep",
        "sweep",
        &[List("points"), Any("profile"), Slots("palette", "palettes")],
    )
    .excluded(&["width", "height", "radius"]),
    Binding::new(
        "trim_outline",
        "trim_outline",
        &[List("outline"), Any("inset")],
    )
    .excluded(BOX),
    Binding::new(
        "trim_outline",
        "trim_outline",
        &[Corners, List("outline"), Any("inset")],
    ),
    Binding::new("moss", "moss", &[Corners, Any("palettes")]),
    Binding::new("algae", "algae", &[Corners, Any("palettes")]),
    // Vines always grow, so they take no layer.
    Binding::new("vines", "vines", &[Corners, Any("palettes")]).excluded(&["layer"]),
    Binding::new("bark", "bark", &[Corners, Any("palettes")]),
    Binding::new(
        "column_relief",
        "column_relief",
        &[Corners, Any("facing"), Int("palette")],
    ),
    Binding::new(
        "planks",
        "planks",
        &[
            Int("x0"),
            Int("y0"),
            Int("x1"),
            Int("y1"),
            Int("z"),
            Str("axis"),
            Any("palettes"),
        ],
    ),
    // The options name the volume, so there is no form without them.
    Binding::new("foliage", "foliage", &[Any("palettes")])
        .excluded(BOX)
        .with_options_only(),
    Binding::new("foliage", "foliage", &[Corners, Any("palettes")]).with_options_only(),
    Binding::new("cavity_tint", "cavity_tint", &[Corners]),
    Binding::new(
        "radial_relief",
        "radial_relief",
        &[Str("kind"), Int("count")],
    ),
    Binding::new(
        "radial_relief",
        "radial_relief",
        &[Corners, Str("kind"), Int("count")],
    )
    .with_options_only(),
    Binding::new("weather", "weather", &[Corners]),
    Binding::new("joints", "joints", &[Corners, Str("pattern")]),
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for binding in BINDINGS {
        let state = scratch.clone();
        let name = binding.name;
        bind(engine, *binding, move |mut args| {
            let mut state = state.borrow_mut();
            let tool = match name {
                "sweep" => {
                    spread_profile(&mut args)?;
                    Stone::Sweep
                }
                "trim_outline" => Stone::Trim,
                "moss" => Stone::Moss,
                "algae" => Stone::Algae,
                "vines" => Stone::Vines,
                "bark" => Stone::Bark,
                "column_relief" => Stone::Relief,
                "planks" => Stone::Planks,
                "foliage" => Stone::Foliage,
                "weather" => Stone::Weather,
                "joints" => Stone::Joints,
                "cavity_tint" => return state.cavity(args).map(Dynamic::from_map),
                "radial_relief" => return state.radial(args).map(Dynamic::from_map),
                other => unreachable!("no runner for `{other}`"),
            };
            state.stone(args, tool).map(Dynamic::from_map)
        });
    }
}

/// `sweep`'s profile as a map: its width, height, and radius as arguments.
fn spread_profile(args: &mut Args) -> Result<(), String> {
    match args.remove("profile") {
        Some(Value::Array(points)) => {
            args.insert("profile".into(), Value::Array(points));
        }
        Some(Value::Object(rect)) => {
            for (key, value) in rect {
                if !["width", "height", "radius"].contains(&key.as_str()) {
                    return Err(format!(
                        "sweep profile map takes width, height, and radius, not `{key}`"
                    ));
                }
                args.insert(key, value);
            }
        }
        _ => {
            return Err(
                "sweep profile must be a list of [u, v] points or #{width, height, radius}".into(),
            )
        }
    }
    Ok(())
}

impl Scratch {
    /// MCP `cavity_tint` on the object being written. Shades it makes go
    /// into free slots of the palette as the run leaves it: not one a cell
    /// of the document or of this run uses, nor one `set_palette` named.
    fn cavity(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        self.shaded(Value::Object(args), remote::cavity_writes)
    }

    /// MCP `radial_relief` on the object being written, in local cells or
    /// (`space: "world"`) world cells. Shades it makes go into free slots
    /// as `cavity_tint`'s do.
    fn radial(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.radial_relief", args)?;
        self.shaded(args, remote::radial_writes)
    }

    /// A tool that may make shades, on the object being written.
    fn shaded(&mut self, args: Value, tool: remote::ShadedWrites) -> Result<Map, String> {
        let mut shades = self.run_shades();
        let (writes, counts) = tool(self.cells(), &args, &mut shades)?;
        self.shades.extend(shades.created.iter().copied());
        let changed = self.apply(&writes)?;
        let mut reply: Map = counts
            .iter()
            .map(|(key, value)| (key.as_str().into(), super::document::dynamic(value)))
            .collect();
        reply.insert("changed".into(), Dynamic::from_int(changed));
        Ok(reply)
    }

    /// The palette as the run leaves it, for a tool that makes slots
    /// (`cavity_tint`, `paint_from_views`, `reduce_palette`): a slot a cell
    /// of the document or of this run uses, or one `set_palette` named or an
    /// earlier tool made, is taken. The caller adds `created` to
    /// `self.shades`.
    pub(super) fn run_shades(&self) -> crate::tools::Shades {
        let mut shades = match &self.view.shades {
            Some(shades) => (**shades).clone(),
            None => crate::state::Session::blank_shades(),
        };
        for (slot, rgb) in &self.palette {
            let color = &mut shades.colors[*slot as usize];
            color[..3].copy_from_slice(rgb);
            if color[3] == 0 {
                color[3] = 255;
            }
            shades.used[*slot as usize] = true;
        }
        for built in &self.built {
            for slot in built.cells.values() {
                shades.used[*slot as usize] = true;
            }
        }
        for shade in &self.shades {
            shades.colors[shade.slot as usize] = shade.rgba;
            shades.materials[shade.slot as usize] = shades.materials[shade.from as usize];
            shades.used[shade.slot as usize] = true;
        }
        shades.created.clear();
        shades
    }

    /// Run a stone tool on the object being written. The reply holds
    /// `changed` and the tool's own counts.
    fn stone(&mut self, args: Args, tool: Stone) -> Result<Map, String> {
        self.keep_going()?;
        let (writes, counts) = remote::stone_writes(self.cells(), &Value::Object(args), tool)?;
        let changed = self.apply(&writes)?;
        Ok(reply(counts, changed))
    }
}

/// A tool's counts and `changed` as a Rhai map.
pub(super) fn reply(counts: serde_json::Map<String, Value>, changed: INT) -> Map {
    let mut reply: Map = counts
        .into_iter()
        .map(|(key, value)| {
            let value = match (value.as_i64(), value.as_str()) {
                (Some(int), _) => Dynamic::from_int(int),
                (None, Some(text)) => Dynamic::from(text.to_string()),
                // A map or a list (deform's `thin`) keeps its shape.
                _ if value.is_object() || value.is_array() => super::document::dynamic(&value),
                (None, None) => Dynamic::from_float(value.as_f64().unwrap_or(0.0)),
            };
            (key.into(), value)
        })
        .collect();
    reply.insert("changed".into(), Dynamic::from_int(changed));
    reply
}
