//! The MCP shape tools in Rhai: `fill_ellipsoid`, `fill_rounded_box`,
//! `fill_tube`, `fill_box` with a mode, `carve_*`, `recolor`,
//! `surface_stroke`, `deterrace`, `reduce_palette`, `smooth`,
//! `break_end`, and `scatter`.
//!
//! Each call turns its arguments into the MCP tool's arguments and runs the
//! same parser and shape code (`remote::fill_writes`, `remote::carve_writes`,
//! `remote::smooth_args`, `remote::break_spec`, `remote::scatter_spec`) on
//! the object being written. Options use the MCP names, so a trailing map
//! such as `#{mode: "empty", aspect: 0.5}` reads like the tool call.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, INT};
use serde_json::Value;

use super::bind::{bind, Binding, Pos::*};
use super::{axis, coord, runtime, slot_index, Scratch};
use crate::remote::{self, Sculpt};

/// `carve` keys a Rhai `carve_box`, `carve_ellipsoid`, and
/// `carve_rounded_box` do not take as options: the shape, its corners and
/// radius (positional), and the tube's.
const CARVE_BOX: &[&str] = &[
    "shape", "x0", "y0", "z0", "x1", "y1", "z1", "radius", "points", "curve", "caps", "aspect",
    "roll", "profile",
];
/// `carve` keys `carve_tube` does not take as options.
const CARVE_TUBE: &[&str] = &["shape", "x0", "y0", "z0", "x1", "y1", "z1", "points"];

pub(super) type Fallible<T> = Result<T, Box<EvalAltResult>>;
pub(super) type Args = serde_json::Map<String, Value>;

/// The shape tools' bindings: positional arguments, then an optional map
/// of every other `tool` argument ([`super::bind`]).
pub(super) const BINDINGS: &[Binding] = &[
    Binding::new("fill_box", "fill_box", &[Corners, Int("palette")]).with_options_only(),
    Binding::new(
        "fill_ellipsoid",
        "fill_ellipsoid",
        &[Corners, Int("palette")],
    ),
    Binding::new(
        "fill_rounded_box",
        "fill_rounded_box",
        &[Corners, Number("radius"), Int("palette")],
    ),
    Binding::new(
        "fill_tube",
        "fill_tube",
        &[List("points"), Slots("palette", "palettes")],
    ),
    Binding::new("recolor", "recolor", &[Int("from"), Int("to")]).excluded(&["map"]),
    Binding::new("recolor", "recolor", &[List("map")]).excluded(&["from", "to"]),
    Binding::new("reduce_palette", "reduce_palette", &[]).with_options_only(),
    Binding::new("deterrace", "deterrace", &[]),
    Binding::new(
        "surface_stroke",
        "surface_stroke",
        &[List("points"), Int("palette")],
    ),
    Binding::new("shell_plates", "shell_plates", &[]).with_options_only(),
    Binding::new("carve_box", "carve", &[Corners])
        .fixed(&[("shape", "box")])
        .excluded(CARVE_BOX),
    Binding::new("carve_ellipsoid", "carve", &[Corners])
        .fixed(&[("shape", "ellipsoid")])
        .excluded(CARVE_BOX),
    Binding::new("carve_rounded_box", "carve", &[Corners, Number("radius")])
        .fixed(&[("shape", "rounded_box")])
        .excluded(CARVE_BOX),
    Binding::new("carve_tube", "carve", &[List("points")])
        .fixed(&[("shape", "tube")])
        .excluded(CARVE_TUBE),
    Binding::new("smooth", "smooth", &[Corners]),
    Binding::new("break_end", "break_end", &[List("point"), List("normal")]),
    Binding::new("scatter", "scatter", &[Corners, Any("palettes")]),
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for binding in BINDINGS {
        let state = scratch.clone();
        let name = binding.name;
        bind(engine, *binding, move |args| {
            let mut state = state.borrow_mut();
            let int = |count: Result<INT, String>| count.map(Dynamic::from_int);
            let map = |reply: Result<Map, String>| reply.map(Dynamic::from_map);
            match name {
                "fill_box" => int(state.fill(args, Sculpt::Box)),
                "fill_ellipsoid" => int(state.fill(args, Sculpt::Ellipsoid)),
                "fill_rounded_box" => int(state.fill(args, Sculpt::RoundedBox)),
                "fill_tube" => int(state.fill(args, Sculpt::Tube)),
                "recolor" => map(state.recolor(args)),
                "reduce_palette" => map(state.reduce_colors(args)),
                "deterrace" => map(state.deterrace(args)),
                "surface_stroke" => map(state.stroke(args)),
                "shell_plates" => map(state.plates(args)),
                "carve_box" | "carve_ellipsoid" | "carve_rounded_box" | "carve_tube" => {
                    int(state.carve(args))
                }
                "smooth" => map(state.smooth(args)),
                "break_end" => map(state.break_end(args)),
                "scatter" => map(state.scatter(args)),
                other => unreachable!("no runner for `{other}`"),
            }
        });
    }
}

impl Scratch {
    fn fill(&mut self, args: Args, sculpt: Sculpt) -> Result<INT, String> {
        self.keep_going()?;
        let method = match sculpt {
            Sculpt::Box => "voxel.fill",
            Sculpt::Ellipsoid => "voxel.ellipsoid",
            Sculpt::RoundedBox => "voxel.rounded",
            Sculpt::Tube => "voxel.tube",
        };
        let (args, _) = self.localize(method, args)?;
        let (writes, _) = remote::fill_writes_mirrored(self.cells(), &args, sculpt)?;
        self.apply(&writes)
    }

    fn carve(&mut self, args: Args) -> Result<INT, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.carve", args)?;
        let writes = remote::carve_writes(self.cells(), &args)?;
        self.apply(&writes)
    }

    /// The MCP arguments of `method` in local cells of the object being
    /// written: with `space: "world"`, its position when the run started is
    /// subtracted (`remote::to_local`) and returned. An object the run
    /// creates has no position yet, so it takes local cells only.
    pub(super) fn localize(
        &self,
        method: &str,
        args: Args,
    ) -> Result<(Value, Option<crate::state::Cell>), String> {
        let mut args = Value::Object(args);
        if !remote::is_world(&args)? {
            if let Some(map) = args.as_object_mut() {
                map.remove("space");
            }
            return Ok((args, None));
        }
        let offset = match &self.built[self.current].key {
            super::BuiltKey::Index(index) => self.view.positions.get(*index).copied(),
            super::BuiltKey::Name(_) => None,
        }
        .ok_or("space world needs an object of the document; an object this run creates takes local cells")?;
        Ok((remote::to_local(method, &args, offset)?, Some(offset)))
    }

    /// MCP `reduce_palette` on the object being written. Clusters take free
    /// slots of the palette as the run leaves it (an existing slot of
    /// exactly the colour is reused), never another slot in place.
    fn reduce_colors(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let palette = self.run_palette();
        let mut shades = self.run_shades();
        let (writes, _, counts) = remote::reduce_writes(
            self.cells(),
            &palette,
            &Value::Object(args),
            &mut shades,
            &[false; 256],
        )?;
        self.shades.extend(shades.created.iter().copied());
        let changed = self.apply(&writes)?;
        let mut out = super::document::dynamic(&counts)
            .try_cast::<Map>()
            .unwrap_or_default();
        out.insert("changed".into(), Dynamic::from_int(changed));
        Ok(out)
    }

    fn deterrace(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (writes, counts) = remote::terrace_writes(self.cells(), &Value::Object(args))?;
        let changed = self.apply(&writes)?;
        let mut out = super::document::dynamic(&counts)
            .try_cast::<Map>()
            .unwrap_or_default();
        out.insert("changed".into(), Dynamic::from_int(changed));
        Ok(out)
    }

    fn stroke(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.surface_stroke", args)?;
        let (writes, counts) = remote::stroke_writes(self.cells(), &args)?;
        let changed = self.apply(&writes)?;
        let mut out = reply([
            ("changed", changed),
            ("cells", counts["cells"].as_i64().unwrap_or(0)),
            ("samples", counts["samples"].as_i64().unwrap_or(0)),
            ("missed", counts["missed"].as_i64().unwrap_or(0)),
        ]);
        out.insert(
            "snap".into(),
            Dynamic::from_float(counts["snap"].as_f64().unwrap_or(0.0)),
        );
        if let Some(mirrored) = counts.get("mirrored").and_then(Value::as_i64) {
            out.insert("mirrored".into(), Dynamic::from_int(mirrored));
        }
        Ok(out)
    }

    fn plates(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.shell_plates", args)?;
        let (writes, counts) = remote::plates_writes(self.cells(), &args)?;
        let changed = self.apply(&writes)?;
        Ok(reply([
            ("changed", changed),
            ("cells", counts["cells"].as_i64().unwrap_or(0)),
            ("taken", counts["taken"].as_i64().unwrap_or(0)),
            ("unreached", counts["unreached"].as_i64().unwrap_or(0)),
            ("plates", counts["plates"].as_i64().unwrap_or(0)),
            ("seams", counts["seams"].as_i64().unwrap_or(0)),
        ]))
    }

    fn recolor(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.recolor", args)?;
        let (writes, matched, _) = remote::recolor_writes(self.cells(), &args)?;
        let changed = self.apply(&writes)?;
        Ok(reply([("changed", changed), ("matched", matched as INT)]))
    }

    fn smooth(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.smooth", args)?;
        let (min, max, iterations, fill_at, remove_at) = remote::smooth_args(&args)?;
        let report =
            crate::tools::smooth_checked(self.cells(), min, max, iterations, fill_at, remove_at)?;
        let changed = self.apply(&report.writes)?;
        Ok(reply([
            ("changed", changed),
            ("filled", report.filled as INT),
            ("removed", report.removed as INT),
            ("iterations", INT::from(report.iterations)),
        ]))
    }

    fn break_end(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let spec = remote::break_spec(&Value::Object(args))?;
        let broken = crate::tools::break_end(self.cells(), &spec)?;
        let changed = self.apply(&broken.writes)?;
        Ok(reply([
            ("changed", changed),
            ("removed", broken.removed as INT),
            ("recoloured", broken.recoloured as INT),
            ("splinters", broken.splinters as INT),
        ]))
    }

    fn scatter(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let spec = remote::scatter_spec(&Value::Object(args))?;
        let done = crate::tools::scatter(self.cells(), &spec)?;
        let changed = self.apply(&done.writes)?;
        Ok(reply([
            ("changed", changed),
            ("pieces", done.placed as INT),
            ("skipped", done.skipped as INT),
        ]))
    }

    pub(super) fn keep_going(&self) -> Result<(), String> {
        match self.stop.reason() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    pub(super) fn cells(&self) -> &std::collections::HashMap<crate::state::Cell, u8> {
        &self.built[self.current].cells
    }

    /// Write `(x, y, z, palette)` cells, 0 erasing, after checking every one
    /// is inside the coordinate range. Returns how many cells changed.
    pub(super) fn apply(&mut self, writes: &[(i32, i32, i32, i32)]) -> Result<INT, String> {
        let mut checked = Vec::with_capacity(writes.len());
        for &(x, y, z, slot) in writes {
            checked.push((
                coord(x.into(), y.into(), z.into())?,
                slot_index(slot.into())?,
            ));
        }
        let cells = self.cells_mut();
        let mut changed = 0;
        for (cell, slot) in checked {
            let before = if slot == 0 {
                cells.remove(&cell)
            } else {
                cells.insert(cell, slot)
            };
            if before != (slot != 0).then_some(slot) {
                changed += 1;
            }
        }
        Ok(changed)
    }
}

/// The box corners, each inside the coordinate range, as `x0..z1`.
pub(super) fn box_args(corners: [INT; 6]) -> Fallible<Args> {
    let mut args = Args::new();
    for (name, value) in ["x0", "y0", "z0", "x1", "y1", "z1"].iter().zip(corners) {
        axis(value).map_err(runtime)?;
        args.insert((*name).into(), value.into());
    }
    Ok(args)
}

/// The options map of a Rhai function that runs MCP `tool`: every argument
/// of its schema but `excluded` (the ones the function takes positionally,
/// and the few a script does not take), so a new schema argument is a new
/// Rhai option with no edit here. Errors name `function` (`carve_tube` runs
/// `carve`), and its schema defaults are filled.
pub(super) fn tool_options_as(
    opts: &Map,
    tool: &str,
    excluded: &[&str],
    function: &str,
) -> Fallible<Args> {
    options(opts, &crate::ops::rhai_options(tool, excluded), function)
}

/// An options map as MCP arguments. A key the function does not take is an
/// error, so a misspelt option does not pass unnoticed.
pub(super) fn options(opts: &Map, allowed: &[&str], function: &str) -> Fallible<Args> {
    let mut args = Args::new();
    for (key, value) in opts {
        if !allowed.contains(&key.as_str()) {
            return Err(runtime(format!(
                "{function} has no option `{key}`; it takes {}",
                allowed.join(", ")
            )));
        }
        args.insert(key.to_string(), json(value)?);
    }
    // A left-out option takes its MCP schema default, as from MCP.
    crate::ops::fill_tool_defaults(function, &mut args);
    Ok(args)
}

/// A Rhai value as JSON: numbers, strings, bools, arrays, and maps.
pub(super) fn json(value: &Dynamic) -> Fallible<Value> {
    if let Ok(int) = value.as_int() {
        return Ok(int.into());
    }
    if let Ok(float) = value.as_float() {
        return serde_json::Number::from_f64(float)
            .map(Value::Number)
            .ok_or_else(|| runtime("numbers must be finite"));
    }
    if let Ok(flag) = value.as_bool() {
        return Ok(flag.into());
    }
    if value.is_unit() {
        return Ok(Value::Null);
    }
    if value.is_string() {
        return Ok(value.clone().into_string().map_err(runtime)?.into());
    }
    if value.is_array() {
        let items = value
            .read_lock::<Array>()
            .ok_or_else(|| runtime("bad list"))?;
        return items
            .iter()
            .map(json)
            .collect::<Fallible<_>>()
            .map(Value::Array);
    }
    if value.is_map() {
        let map = value.read_lock::<Map>().ok_or_else(|| runtime("bad map"))?;
        let mut out = Args::new();
        for (key, item) in map.iter() {
            out.insert(key.to_string(), json(item)?);
        }
        return Ok(Value::Object(out));
    }
    Err(runtime(format!(
        "a {} cannot be a tool argument",
        value.type_name()
    )))
}

fn reply<const N: usize>(fields: [(&str, INT); N]) -> Map {
    fields
        .into_iter()
        .map(|(key, value)| (key.into(), Dynamic::from_int(value)))
        .collect()
}
