//! The argument schema of every tool in [`super::OPS`], as MCP
//! `tools/list` gives it. Built once and kept: MCP lists it, and every
//! call's arguments are checked against it.

use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::remote;

/// `prop` with its JSON Schema `default`: the value the tool takes when the
/// argument is left out. `remote::apply` fills it in for every caller, and
/// the panels start from it ([`super::default`]).
fn with_default(mut prop: Value, value: Value) -> Value {
    prop["default"] = value;
    prop
}

/// `prop` as a panel shows it, Blender RNA's `name` and short
/// `description`: `label` is its JSON Schema `title` and the panel label,
/// `hint` (one clause, no full stop) the panel tooltip and the short line of
/// `froxel run <tool> --help`. The long `description` stays MCP's help.
/// [`super::prop_ui`] reads it; `docs/op-properties.md` is the design.
fn ui(mut prop: Value, label: &str, hint: &str) -> Value {
    prop["title"] = label.into();
    prop[UI]["hint"] = hint.into();
    prop
}

/// The range a number must be in, JSON Schema `minimum` and `maximum`
/// (Blender's `min` and `max`); `remote::apply` refuses a value outside it.
fn hard(mut prop: Value, min: f64, max: f64) -> Value {
    prop["minimum"] = min.into();
    prop["maximum"] = max.into();
    prop
}

/// The slider range and shown digits of a number (Blender's `soft_min`,
/// `soft_max`, and `precision`); a call may still pass a value outside it.
fn soft(mut prop: Value, min: f64, max: f64, digits: u64) -> Value {
    prop[UI]["soft"] = json!([min, max]);
    prop[UI]["digits"] = digits.into();
    prop
}

/// An enum option a panel draws as one toggle: on is `on`, off leaves the
/// option out (its default).
fn toggle(mut prop: Value, on: &str) -> Value {
    prop[UI]["on"] = on.into();
    prop
}

/// The key of a property's panel metadata. Left out of MCP `tools/list`.
pub(crate) const UI: &str = "x-ui";

/// Every tool's `tools/list` entry as MCP lists it: [`tools`] without the
/// panel metadata (`x-ui`), which no MCP client reads.
pub(crate) fn mcp_tools() -> &'static [Value] {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let mut tools = tools().to_vec();
        for tool in &mut tools {
            if let Some(properties) = tool["inputSchema"]["properties"].as_object_mut() {
                for property in properties.values_mut() {
                    if let Some(property) = property.as_object_mut() {
                        property.remove(UI);
                    }
                }
            }
        }
        tools
    })
}

/// The cross-section and end options `fill_tube` and `carve` `tube` share,
/// and so do Rhai `fill_tube` and `carve_tube`.
pub(crate) const TUBE_SHAPE_KEYS: [&str; 5] = ["curve", "caps", "aspect", "roll", "profile"];

/// The properties of [`TUBE_SHAPE_KEYS`], as `fill_tube` describes them.
fn tube_shape_props() -> [(&'static str, Value); 5] {
    let props = [
        toggle(ui(json!({"type": "string", "enum": ["polyline", "smooth"], "description": "polyline (default) or smooth."}), "Smooth", "A curve through the points instead of straight segments"), "smooth"),
        toggle(ui(json!({"type": "string", "enum": ["round", "flat"], "description": "round (default): half balls past the end points. flat: cut square at the first and last points, across the path there; a cell is in only when its centre is between those planes."}), "Flat ends", "Cut the tube square at its first and last points instead of rounding them, for pipes, discs, rims, and wheels"), "flat"),
        soft(ui(hard(with_default(num_prop("Flatten: thickness over width, 0.05..=1. 1 (the default) is round. The thickness is at least one cell and runs up for a level tube, so a flat tube lies flat."), json!(1.0)), 0.05, 1.0), "Flatten", "Thickness over width; 1 is round"), 0.05, 1.0, 2),
        soft(ui(with_default(num_prop("Degrees to turn a flat or blade cross-section about the axis. 90 stands a level shard on its edge. Defaults to 0."), json!(0.0)), "Roll", "Degrees to turn a flat or blade cross-section about the axis"), -180.0, 180.0, 0),
        toggle(ui(json!({"type": "string", "enum": ["round", "blade"], "description": "blade tapers the thickness across the width to one cell at one edge: a wedge, for splinters and broken plank ends. Defaults to round."}), "Blade", "A wedge: full thickness at one edge, one cell at the other"), "blade"),
    ];
    let mut out = TUBE_SHAPE_KEYS.map(|key| (key, Value::Null));
    for ((_, slot), prop) in out.iter_mut().zip(props) {
        *slot = prop;
    }
    out
}

/// `fill_tube` `radius`: the default radius of its points.
fn tube_radius_prop(description: &str) -> Value {
    soft(
        ui(
            hard(
                num_prop(description),
                crate::tools::MIN_TUBE_RADIUS,
                crate::tools::MAX_TUBE_RADIUS,
            ),
            "Radius",
            "Tube radius in cells; 0.5 is one cell thick",
        ),
        0.5,
        32.0,
        1,
    )
}

/// Every tool's `tools/list` entry, in [`super::OPS`] order.
pub(crate) fn tools() -> &'static [Value] {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let mut tools = build();
        with_ranges(&mut tools);
        with_panel_props(&mut tools);
        tools
    })
}

/// Merge [`super::panel::PANEL`] into the properties it names, as
/// [`ui`] and [`soft`] write them inline.
/// Merge [`super::ranges::RANGES`] into the properties as `hard` would.
fn with_ranges(tools: &mut [Value]) {
    for &(name, key, min, max) in super::ranges::RANGES {
        let tool = tools
            .iter_mut()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("ops::ranges names `{name}`, which is not a tool"));
        let property = tool["inputSchema"]["properties"]
            .get_mut(key)
            .unwrap_or_else(|| {
                panic!("ops::ranges names `{name}` `{key}`, which it does not take")
            });
        assert!(
            property.get("minimum").is_none() && property.get("maximum").is_none(),
            "`{name}` `{key}` has its range inline and in ops::ranges"
        );
        *property = hard(property.take(), min, max);
    }
}

fn with_panel_props(tools: &mut [Value]) {
    for row in super::panel::PANEL {
        let tool = tools
            .iter_mut()
            .find(|tool| tool["name"] == row.tool)
            .unwrap_or_else(|| panic!("ops::panel names `{}`, which is not a tool", row.tool));
        let property = tool["inputSchema"]["properties"]
            .get_mut(row.key)
            .unwrap_or_else(|| {
                panic!(
                    "ops::panel names `{}` `{}`, which it does not take",
                    row.tool, row.key
                )
            });
        assert!(
            property.get("title").is_none(),
            "`{}` `{}` has its panel metadata inline and in ops::panel",
            row.tool,
            row.key
        );
        let mut merged = ui(property.take(), row.label, row.hint);
        if let Some((min, max)) = row.soft {
            merged[UI]["soft"] = json!([min, max]);
        }
        if let Some(digits) = row.digits {
            merged[UI]["digits"] = digits.into();
        }
        if let Some(speed) = row.speed {
            merged[UI]["speed"] = speed.into();
        }
        *property = merged;
    }
}

/// The `tools/list` entry of one tool.
pub(crate) fn tool(name: &str) -> Option<&'static Value> {
    tools().iter().find(|tool| tool["name"] == name)
}

/// The argument properties of one tool.
pub(crate) fn properties(name: &str) -> Option<&'static serde_json::Map<String, Value>> {
    tool(name)?["inputSchema"]["properties"].as_object()
}

fn build() -> Vec<Value> {
    vec![
        define("batch", &format!("Apply a list of editing tools to the active object and the palette in order, as one undo step, instead of one call per board or cut. Stops at the first error, says which item failed (items count from 0), and leaves the document as it was with no undo step added. The whole list is checked first: an unknown option or a tool that cannot run here refuses the batch before anything changes. At most {} items; each takes what its tool takes. Replies with items, changed (the sum over the items), voxels, and results, each item's tool and counts in short form. Tools that write files are refused, as are readers, undo, and anything that changes the active object or the document: call set_active_object first and save afterwards.", remote::MAX_BATCH_ITEMS), json!({
            "operations": {"type": "array", "minItems": 1, "maxItems": remote::MAX_BATCH_ITEMS, "description": "The items in order. Each is {tool, arguments}, arguments as the tool itself takes them.", "items": {"type": "object", "properties": {
                "tool": {"type": "string", "enum": remote::batch_tool_names(), "description": "The tool to run."},
                "arguments": {"type": "object", "description": "The tool's arguments."}
            }, "required": ["tool"], "additionalProperties": false}}
        }), &["operations"]),
        define("new_document", "Replace the session with an empty one-object document and the default palette.", json!({}), &[]),
        define("open_file", "Open a .vox file, replacing the session document. By default replies with path, objectCount, voxels, world bounds, activeObject, canUndo, canRedo, bones (count), paletteSlots (used slots), motionSlots, the first 20 objectNames, moreObjects (how many names were left out), and warnings (what the file left out, such as a rig, clip, script, or marker chunk from a newer Froxel; the voxels still open, and a save drops that chunk). A Froxel file opens in the frame it was saved in, older ones with a script or markers included; a foreign file opens centred and floored. When world positions differ from the file's node positions, the reply states it as sceneShift and sceneShiftNote, as inspect_document and the exports do. detail full replies with the inspect_document result instead.", json!({
            "path": str_prop("Path of a .vox file"),
            "detail": {"type": "string", "enum": ["summary", "full"], "description": "summary (default) is the short reply; full is the inspect_document result."},
            "fullPalette": {"type": "boolean", "description": "With detail full, list all 256 palette slots, not only the used ones. Defaults to false."}
        }), &["path"]),
        define("save_file", "Write every non-empty object as a .vox scene. World voxel and marker positions survive reopening, including negative coordinates, so a reopened document exports the same glTF frame and marker values; local cells may rebase to the model box. Replies with voxels (solid cells in the file) and objects (models written).", json!({"path": str_prop("Where to write. Omit to overwrite the open file. Its folder is created.")}), &[]),
        define("inspect_document", "Path, voxel count, world bounds, objects (name, world position, local bounds), palette colours keyed by index for the slots some voxel uses (fullPalette for all 256), and motion keyed by palette index for slots that are not still at amount 1.", json!({
            "fullPalette": {"type": "boolean", "description": "List all 256 palette slots, not only the used ones. Defaults to false."}
        }), &[]),
        define("get_voxel", "Read one local cell of the active object. 0 is empty. Coordinates are local to that object.", xyz_props(), &["x", "y", "z"]),
        define("set_voxel", "Write one local cell of the active object. Coordinates are local. One undo step.", {
            let mut p = xyz_props();
            p["palette"] = int_prop("Palette index. 0 erases. 1..255 paint.");
            p
        }, &["x", "y", "z", "palette"]),
        define("set_voxels", "Write up to 200000 local cells of the active object as one undo step. Coordinates are local to that object.", json!({"cells": {"type": "array", "description": "Each item is {x,y,z,palette} or [x,y,z,palette], in the active object's local cells. Last duplicate wins."}}), &["cells"]),
        define("fill_box", "Fill an inclusive box of local cells in the active object. At most 2000000 cells. One undo step.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["palette"] = int_prop("Palette index. 0 erases. Required except with mode intersect.");
            p["mode"] = fill_mode_prop();
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("get_voxels", "List non-empty local cells of the active object inside an inclusive box. At most 100000.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("set_palette", "Replace one palette slot, or many via entries, as one undo step. The colour is color, \"#rrggbb\" or \"#rrggbbaa\" as the other tools take colours (or [r, g, b, a?]), or r, g, b (and a) as numbers; not both; leave the colour out to keep the slot's. motion is none, sway, or bob. material (diffuse, metal, emissive, or glass) and its roughness, metallic, transparency, ior, emission, and power are the slot's Render material, as the Render panel's Material section sets it; each one left out keeps the slot's.", json!({
            "index": int_prop("Palette slot 0..255"),
            "color": {"type": ["string", "array"], "items": {"type": "integer"}, "description": "\"#rrggbb\" or \"#rrggbbaa\" (the # may be left out), or [r, g, b] or [r, g, b, a] in 0..255. Instead of r, g, b."},
            "r": int_prop("Red 0..255"),
            "g": int_prop("Green 0..255"),
            "b": int_prop("Blue 0..255"),
            "a": int_prop("Alpha 0..255. Defaults to 255, or the alpha of a #rrggbbaa color."),
            "motion": str_prop("none, sway, or bob. Omit to leave the slot's motion."),
            "motionAmount": soft(ui(num_prop("Motion strength, 0..4. Omit to leave it. Default 1 when a kind is first set."), "Amount", "Motion strength"), 0.0, 4.0, 2),
            "material": ui(json!({"type": "string", "enum": ["diffuse", "metal", "emissive", "glass"], "description": "The slot's Render material: diffuse, metal, emissive, or glass. Omit to leave it."}), "Material", "The slot's material in Render mode and the tracers"),
            "roughness": soft(ui(num_prop("metal, glass: surface roughness, 0..1; 0 is a mirror or clear glass. Omit to leave it."), "Roughness", "Surface roughness; 0 is a mirror"), 0.0, 1.0, 2),
            "metallic": soft(ui(num_prop("metal: how metallic the surface is, 0..1. Omit to leave it."), "Metallic", "How metallic the surface is"), 0.0, 1.0, 2),
            "transparency": soft(ui(num_prop("glass: how much light passes through, 0..1. Omit to leave it."), "Transparency", "How much light passes through"), 0.0, 1.0, 2),
            "ior": soft(ui(num_prop("glass: index of refraction, 1..3. Omit to leave it."), "IoR", "Index of refraction"), 1.0, 3.0, 2),
            "emission": soft(ui(num_prop("emissive: light given off, 0..100. Omit to leave it."), "Emission", "Light given off"), 0.0, 100.0, 0),
            "power": soft(ui(int_prop("emissive: a whole multiplier of the emission, 1..5. Omit to leave it."), "Power", "Whole multiplier of the emission"), 1.0, 5.0, 0),
            "motion_amount": num_prop("Older name of motionAmount; still accepted."),
            "entries": {"type": "array", "description": "Batch of {index, color | r,g,b, a?, motion?, motionAmount?, material?, roughness?, metallic?, transparency?, ior?, emission?, power?}, such as {\"index\": 3, \"color\": \"#5c6838\"}. Omit when passing index."}
        }), &[]),
        define("set_active_object", "Edit a different object from inspect_document. Later voxel tools use that object's local cells.", json!({"index": int_prop("Object index")}), &["index"]),
        define("add_object", "Add an empty object at a world voxel position and return its index. It becomes active unless makeActive is false. size is a local capacity hint; the object still grows up to 256 per axis. One undo step.", json!({
            "name": str_prop("Object name. Omit for Object N."),
            "position": ivec3_prop("World position [x, y, z] in voxel units. Defaults to [0, 0, 0]."),
            "size": ivec3_prop("Optional SIZE box [x, y, z], each axis 1..=256. Does not cap later growth below the per-object limit."),
            "makeActive": {"type": "boolean", "description": "Select the new object. Defaults to true."},
            "make_active": {"type": "boolean", "description": "Older name of makeActive; still accepted."}
        }), &[]),
        define("set_object", "Rename, move, or hide an object. position is a world voxel coordinate; local cells stay put, so the voxels move with the object. One undo step.", json!({
            "index": int_prop("Object index"),
            "name": str_prop("New name"),
            "position": ivec3_prop("World position [x, y, z] in voxel units"),
            "hidden": {"type": "boolean", "description": "Hide the object. Hidden objects stay in the file and outliner."}
        }), &["index"]),
        define("remove_object", "Remove an object and its voxels. A bone bound to it stays; the binding goes. Later object indices decrease by one, and the reply says so. Refuses the last object. One undo step.", json!({
            "index": int_prop("Object index")
        }), &["index"]),
        define("fill_ellipsoid", "Fill the ellipsoid inscribed in an inclusive local box. Same limits and one undo step as fill_box. The box is checked before any cell is written.", json!({
            "x0": int_prop("Inclusive minimum x"),
            "y0": int_prop("Inclusive minimum y"),
            "z0": int_prop("Inclusive minimum z"),
            "x1": int_prop("Inclusive maximum x"),
            "y1": int_prop("Inclusive maximum y"),
            "z1": int_prop("Inclusive maximum z"),
            "palette": int_prop("Palette index. 0 erases. Required except with mode intersect."),
            "mode": fill_mode_prop()
        }), &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("fill_rounded_box", "Fill an inclusive local box with rounded edges. radius is the bevel, clamped to half the shortest side. radius 0 fills the whole box. Same limits and one undo step as fill_box.", json!({
            "x0": int_prop("Inclusive minimum x"),
            "y0": int_prop("Inclusive minimum y"),
            "z0": int_prop("Inclusive minimum z"),
            "x1": int_prop("Inclusive maximum x"),
            "y1": int_prop("Inclusive maximum y"),
            "z1": int_prop("Inclusive maximum z"),
            "palette": int_prop("Palette index. 0 erases. Required except with mode intersect."),
            "radius": num_prop("Edge bevel in voxels. 0 fills the box."),
            "mode": fill_mode_prop()
        }), &["x0", "y0", "z0", "x1", "y1", "z1", "radius"]),
        define("fill_tube", "Fill a round tube through 2..=1024 local points, for limbs, roots, branches, and twigs. Each point has its own radius, so the tube tapers; joints are round. caps round (default) ends it in half balls that run radius cells past the first and last points; caps flat cuts it square there, by the planes through those points across the path, for pipes, discs, rims, barrels, and wheels (points [0, 0, 0] and [0, 13, 0] at radius 12 fill y 0..=13 only; then the same tube at palette 0 and radius 11 hollows a pipe with a one-cell wall; two points half a cell apart are a disc one cell thick; flat caps refuse points that are all in one place). A whole-number point is the centre of that cell, and a cell is filled when its centre is within the radius of the axis. curve smooth passes a Catmull-Rom curve through the points; polyline (default) joins them straight. aspect, roll, and profile blade make it flat or a wedge, for shards, splinters, and plank ends. Colour with palette, or with palettes and a pattern: rings bands along the length every period cells, stripes divides the circumference into sectors (grooves along the tube), random picks per cell from seed. At most 2000000 cells; refused before any cell is written. One undo step.", {
            let mut p = json!({
            "points": {"type": "array", "description": "Each item is [x, y, z], [x, y, z, radius], or {x, y, z, radius?} in the active object's local cells. Numbers may be fractional."},
            "radius": tube_radius_prop("Radius for points that give none, 0.5..=128. 0.5 is one cell thick."),
            "palette": int_prop("Palette index. 0 erases. Pass this or palettes."),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "1..=64 palette indices cycled by pattern. Repeat an index to weight it."},
            "pattern": ui(json!({"type": "string", "enum": ["rings", "stripes", "random"], "description": "How palettes are laid out. Defaults to rings."}), "Colours", "How the palettes are laid out along and around the tube"),
            "period": soft(ui(with_default(num_prop("rings: band length along the tube in cells. Defaults to 2."), json!(2.0)), "Period", "Band length along the tube in cells"), 1.0, 16.0, 1),
            "sectors": soft(ui(hard(int_prop("stripes: sectors around the tube, 1..=256. Defaults to the number of palettes."), 1.0, 256.0), "Sectors", "Stripes around the tube"), 1.0, 32.0, 0),
            "seed": int_prop("random: seed. Defaults to 0."),
            "mode": fill_mode_prop()
            });
            for (key, prop) in tube_shape_props() {
                p[key] = prop;
            }
            p
        }, &["points"]),
        define("generate", "Fill a generated shape inside an inclusive local box, the editor's Generator tool: kind ellipsoid (the box's inscribed ellipsoid), rounded_box (radius), cylinder, cone, pyramid, torus (torusTube), perlin (3D noise: scale, threshold, octaves, seed, coloured paletteLow..paletteHigh by height above the threshold), tree (a branching trunk of palette with leaf clumps: depth, wildness, treeTrunk, leaf, seed), helix (turns, helixRadius, helixTube, clockwise), or conifer (tiers of needle clumps on a palette trunk: tiers, raggedness, clump, coniferTrunk, crown, needle, seed). axis is the long axis of cylinder, cone, pyramid, torus, tree, helix, and conifer; apex min puts a cone's or pyramid's point (and a tree's crown) at the low end. The shape is fitted to the box, so its size is the box's. Options of other kinds are ignored. Noise and branching follow only the cells and the seed, so the same arguments give the same shape. At most 2000000 cells; refused before any cell is written. One undo step.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["kind"] = ui(json!({"type": "string", "enum": ["ellipsoid", "rounded_box", "cylinder", "cone", "pyramid", "torus", "perlin", "tree", "helix", "conifer"], "description": "The generator: ellipsoid, rounded_box, cylinder, cone, pyramid, torus, perlin, tree, helix, or conifer."}), "Generator", "The shape generated inside the box");
            p["palette"] = with_default(int_prop("Palette index of the shape (a tree's or conifer's trunk). 0 erases. Not used by perlin. Defaults to 1."), json!(1));
            p["mode"] = fill_mode_prop();
            p["axis"] = ui(with_default(json!({"type": "string", "enum": ["x", "y", "z"], "description": "The long axis of cylinder, cone, pyramid, torus, tree, helix, and conifer. Defaults to y."}), json!("y")), "Axis", "The shape's long axis");
            p["apex"] = ui(with_default(json!({"type": "string", "enum": ["min", "max"], "description": "cone, pyramid: the end the point is at; tree: the end the crown is at. Defaults to max."}), json!("max")), "Apex at", "The end the point or crown is at");
            p["radius"] = soft(ui(hard(with_default(num_prop("rounded_box: the corner radius in cells, 0..=128. Defaults to 2."), json!(2.0)), 0.0, 128.0), "Radius", "Corner radius in cells"), 0.0, 32.0, 1);
            p["torusTube"] = soft(ui(hard(with_default(num_prop("torus: the tube's share of the radius, 0..=1. Defaults to 0.3."), json!(0.3)), 0.0, 1.0), "Tube", "Tube thickness as a share of the radius"), 0.05, 1.0, 2);
            p["scale"] = soft(ui(hard(with_default(num_prop("perlin: the noise feature size in cells, 0.5..=256. Defaults to 8."), json!(8.0)), 0.5, 256.0), "Scale", "Noise feature size in cells"), 1.0, 64.0, 1);
            p["threshold"] = soft(ui(hard(with_default(num_prop("perlin: the noise value a cell must reach to be filled, 0..=1. Defaults to 0.5."), json!(0.5)), 0.0, 1.0), "Threshold", "Noise value a cell must reach"), 0.0, 1.0, 2);
            p["octaves"] = soft(ui(hard(with_default(int_prop("perlin: noise layers, 1..=8. Defaults to 4."), json!(4)), 1.0, 8.0), "Octaves", "Noise layers"), 1.0, 6.0, 0);
            p["seed"] = soft(ui(hard(with_default(int_prop("perlin, tree, conifer: the seed, 0..=4294967295. Defaults to 1."), json!(1)), 0.0, 4294967295.0), "Seed", "Seed of the noise or branching"), 0.0, 9999.0, 0);
            p["paletteLow"] = soft(ui(hard(with_default(int_prop("perlin: the slot just above the threshold, 1..=255. Defaults to 1."), json!(1)), 1.0, 255.0), "Palette lo", "Slot just above the threshold"), 1.0, 255.0, 0);
            p["paletteHigh"] = soft(ui(hard(with_default(int_prop("perlin: the slot at the noise peak; slots between are blended in, 1..=255. Defaults to 8."), json!(8)), 1.0, 255.0), "Palette hi", "Slot at the noise peak"), 1.0, 255.0, 0);
            p["depth"] = soft(ui(hard(with_default(int_prop("tree: branching levels, 1..=5. Defaults to 3."), json!(3)), 1.0, 5.0), "Depth", "Branching levels"), 1.0, 5.0, 0);
            p["wildness"] = soft(ui(hard(with_default(num_prop("tree: how far branches bend from regular, 0..=1. Defaults to 0.35."), json!(0.35)), 0.0, 1.0), "Wildness", "How far branches bend from regular"), 0.0, 1.0, 2);
            p["treeTrunk"] = soft(ui(hard(with_default(num_prop("tree: trunk thickness, 0.05..=1.5. Defaults to 0.45."), json!(0.45)), 0.05, 1.5), "Trunk", "Trunk thickness"), 0.2, 1.5, 2);
            p["leaf"] = soft(ui(hard(with_default(int_prop("tree: the leaves' slot, 1..=255. Defaults to 1."), json!(1)), 1.0, 255.0), "Leaf color", "The leaves' slot"), 1.0, 255.0, 0);
            p["turns"] = soft(ui(hard(with_default(num_prop("helix: turns along the axis, 0.05..=64. Defaults to 2."), json!(2.0)), 0.05, 64.0), "Turns", "Turns along the axis"), 0.25, 12.0, 2);
            p["helixRadius"] = soft(ui(hard(with_default(num_prop("helix: the coil's radius as a share of the box, 0..=1. Defaults to 0.75."), json!(0.75)), 0.0, 1.0), "Radius", "Coil radius as a share of the box"), 0.0, 1.0, 2);
            p["helixTube"] = soft(ui(hard(with_default(num_prop("helix: the wire's thickness as a share of the box, 0.02..=0.9. Defaults to 0.16."), json!(0.16)), 0.02, 0.9), "Tube", "Wire thickness as a share of the box"), 0.02, 0.9, 2);
            p["clockwise"] = ui(with_default(bool_prop("helix: wind clockwise. Defaults to false."), json!(false)), "Clockwise", "Wind the helix clockwise");
            p["tiers"] = soft(ui(hard(with_default(int_prop("conifer: tiers of needles, 2..=5. Defaults to 3."), json!(3)), 2.0, 5.0), "Tiers", "Tiers of needles"), 2.0, 5.0, 0);
            p["raggedness"] = soft(ui(hard(with_default(num_prop("conifer: how ragged the tier edges are, 0..=1. Defaults to 0.25."), json!(0.25)), 0.0, 1.0), "Raggedness", "How ragged the tier edges are"), 0.0, 1.0, 2);
            p["clump"] = soft(ui(hard(with_default(num_prop("conifer: needle clump size in cells, 1..=64. Defaults to 6."), json!(6.0)), 1.0, 64.0), "Clump", "Needle clump size in cells"), 2.0, 24.0, 1);
            p["coniferTrunk"] = soft(ui(hard(with_default(num_prop("conifer: trunk thickness, 0.05..=1.5. Defaults to 0.4."), json!(0.4)), 0.05, 1.5), "Trunk", "Trunk thickness"), 0.2, 1.5, 2);
            p["crown"] = soft(ui(hard(with_default(num_prop("conifer: how much the top tier narrows to a crown, 0..=1. Defaults to 0.5."), json!(0.5)), 0.0, 1.0), "Crown", "How much the top narrows to a crown"), 0.0, 1.0, 2);
            p["needle"] = soft(ui(hard(with_default(int_prop("conifer: the needles' slot, 1..=255. Defaults to 1."), json!(1)), 1.0, 255.0), "Needle", "The needles' slot"), 1.0, 255.0, 0);
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "kind"]),
        define("recolor", "Replace palette slot from with slot to (or every pair of a map) in the active object's solid cells, in an inclusive box or the whole object, with no shape change: a blockout slot becomes the shell slot, two shades merge, a belly is re-tinted. Cells of other slots and empty cells stay. to 0 erases those cells. A slot is mapped once; the map is applied to the cells as they were, so 3 to 4 and 4 to 3 swap. One undo step. Replies with matched (cells of a mapped slot), changed, and voxels.", json!({
            "from": int_prop("Palette slot to replace, 1..255. Pass from and to, or map."),
            "to": int_prop("Palette slot it becomes, 0..255. 0 erases."),
            "map": {"type": ["object", "array"], "description": "Several at once: {\"3\": 5, \"4\": 6} or [[3, 5], [4, 6]]."},
            "x0": int_prop("Optional inclusive box corner; all six or none (the whole object)"),
            "y0": int_prop("Inclusive box corner"),
            "z0": int_prop("Inclusive box corner"),
            "x1": int_prop("Inclusive box corner"),
            "y1": int_prop("Inclusive box corner"),
            "z1": int_prop("Inclusive box corner")
        }), &[]),
        define("deterrace", "Remove the contour terraces a smooth mesh leaves when voxelized (import_mesh of an image-to-3D model): one-cell steps running in long even bands on gentle slopes, which read as stripes. mode blocks (default) re-quantizes the active object into cubes of size cells (2 to 4, default 2) on a grid through origin: a cube is solid when at least fill (default 0.5) of its cells are, and its cells take its most common slot, so every step is size cells tall and the form gets the chunky concept-cube look; a cube whose decision would move the surface more than tolerance cells (default size) stays as it is, so thin parts survive. mode stagger works on the top surface (each column's highest cell): where a column stands one cell above a face neighbour, the step's edge moves by one cell, the low side up or the high side down, in teeth run cells long (default 2) that alternate, with a seeded share jitter (default 0.3) flipped, so no step runs in a long even line; nothing moves more than one cell. An optional box x0..z1 limits it. One undo step. Replies with mode, added, removed, steps and longestRun (one-cell steps on the top surface and the longest straight run of them, before and after), kept (blocks left as they were), changed, and voxels.", json!({
            "mode": {"type": "string", "enum": ["blocks", "stagger"], "description": "blocks (default) re-quantizes into cubes; stagger breaks one-cell step edges into teeth."},
            "size": with_default(int_prop("blocks: the cube side in cells, 2..=4. Defaults to 2."), json!(2)),
            "fill": with_default(num_prop("blocks: the share of a cube's cells that makes it solid, above 0 and at most 1. Defaults to 0.5."), json!(0.5)),
            "origin": vec3_prop("blocks: a corner of the cube grid, in local cells. Defaults to [0, 0, 0]."),
            "tolerance": int_prop("blocks: how far, in cells, a cube may move the surface, 0..=16. Defaults to size."),
            "run": with_default(int_prop("stagger: tooth length along a step, 1..=16. Defaults to 2."), json!(2)),
            "jitter": with_default(num_prop("stagger: share of teeth flipped from the alternating pattern, 0..=1. Defaults to 0.3."), json!(0.3)),
            "seed": with_default(int_prop("stagger: which teeth flip. Defaults to 0."), json!(0)),
            "x0": int_prop("Inclusive box corner"),
            "y0": int_prop("Inclusive box corner"),
            "z0": int_prop("Inclusive box corner"),
            "x1": int_prop("Inclusive box corner"),
            "y1": int_prop("Inclusive box corner"),
            "z1": int_prop("Inclusive box corner")
        }), &[]),
        define("split_parts", "Cut ONE blockout (an import_mesh or carve_from_views result) into its rig parts in one call, instead of many box-based split_object calls: parts is a list of {name, seed} with seed a point [x, y, z] (a cell of the source object, or a world cell with space world), points (several seeds of one part, e.g. along a leg), bone (that bone's pivot), or marker (a point marker); each snaps to the nearest solid cell within reach (default 8). Every solid cell goes to the part whose seed is nearest along a path through the solid (metric geodesic, the default: a multi-source Dijkstra over cells joined by faces, edges, and corners, connectivity 26, or faces only, 6), so a jaw seed does not take the cheek across the mouth and a leg ends where it meets the body; metric euclidean takes the straight-line distance, and is the fallback for cells no path reaches (unreached). A part's planes [{normal, at}] keep it on the side normal points to (cells whose centre c has (c - at) . normal >= 0), to cut a neck or a tail root at a set place; at is a cell like point. Each part is written as its own object at world [0, 0, 0] with its cells in world cells, the hand-sculpting rig convention, so add_bone pivots read straight off the cells and bind_object needs no cell moves; an existing object of the part's name is replaced and moved to [0, 0, 0]. object (index or name) is the source, default the active one; it is removed unless a part has its name or removeSource is false. bind (default true) binds each part to the bone of its name when there is one. One undo step. Replies with parts (name, index, cells, seeds, bone, bounds), unreached, metric, sourceRemoved, and objects.", json!({
            "parts": {"type": "array", "items": {"type": "object"}, "description": "1..64 parts: {name, point: [x, y, z] | points: [[x, y, z], ...] | bone: name | marker: name, planes?: [{normal: [x, y, z], at: [x, y, z]}]}."},
            "object": {"type": ["integer", "string"], "description": "The source object by index or name. Defaults to the active one."},
            "metric": {"type": "string", "enum": ["geodesic", "euclidean"], "description": "geodesic (default): distance through the solid; euclidean: straight-line distance."},
            "connectivity": {"type": "integer", "enum": [6, 26], "description": "Geodesic neighbours: 26 (default) faces, edges, and corners; 6 faces only."},
            "reach": num_prop("Cells a seed may snap to the nearest solid cell, 0..64. Defaults to 8."),
            "removeSource": with_default(json!({"type": "boolean", "description": "Remove the emptied source object. Defaults to true; a part named like the source keeps it."}), json!(true)),
            "bind": with_default(json!({"type": "boolean", "description": "Bind each part to the bone of its name when there is one. Defaults to true."}), json!(true)),
            "space": remote::space_prop()
        }), &["parts"]),
        define("reduce_palette", "Clean up the colours of a voxelized mesh (import_mesh of an image-to-3D model gives a score of slots a shade apart and a per-cell ripple): first reduce, then despeckle, on the active object, with no cell added or removed. colors N clusters the object's colours into N (median cut refined by k-means in the Oklab perceptual space, over the surface cells) with an accent guard that keeps up to maxAccents (default 4) small distinct colours (eyes, a mouth, a nostril) their own slot when they hold at least minShare (default 0.002) of the surface and lie further than contrast (an Oklab distance, default 0.12) from every cluster (accents false turns it off); each cluster takes over in place the commonest of its slots that no other cell uses, or a free slot. palette snaps every colour to the nearest of a list of slots or #rrggbb colours instead (a colour not in the palette takes a free slot). despeckle (passes, 0..16, default 0; true is 1) is a majority filter on the surface: a surface cell whose slot fewer than minSame (default 2) of its 26 surface neighbours share takes the slot more than half of them hold, so lone specks and pairs go and lines and patches stay. protect lists slots both passes leave alone (the eyes). An optional box x0..z1 limits it. One undo step. Replies with slots {before, after} (distinct slots), clusters (slot, color, cells), accents, despeckled, recolored (slots that took a new colour in place), newSlots, changed, and voxels.", json!({
            "colors": int_prop("Cluster the colours into this many, 1..64."),
            "palette": {"type": "array", "items": {"type": ["integer", "string"]}, "description": "Instead of colors: snap every colour to the nearest of these slots or #rrggbb colours."},
            "accents": {"type": "boolean", "description": "colors: keep small distinct colours their own slot. Defaults to true."},
            "minShare": num_prop("Accent guard: share of the surface an accent needs, 0..1. Defaults to 0.002."),
            "contrast": num_prop("Accent guard: Oklab distance from every cluster an accent needs. Defaults to 0.12."),
            "maxAccents": int_prop("Accent guard: most accents kept beyond colors, 0..16. Defaults to 4."),
            "despeckle": with_default(json!({"type": ["integer", "boolean"], "description": "Majority filter passes over the surface, 0..16; true is 1. Defaults to 0."}), json!(0)),
            "minSame": with_default(int_prop("Despeckle: a cell keeps its slot when at least this many surface neighbours share it, 0..26. Defaults to 2."), json!(2)),
            "protect": {"type": "array", "items": {"type": "integer"}, "description": "Slots both passes leave alone."},
            "x0": int_prop("Inclusive box corner"),
            "y0": int_prop("Inclusive box corner"),
            "z0": int_prop("Inclusive box corner"),
            "x1": int_prop("Inclusive box corner"),
            "y1": int_prop("Inclusive box corner"),
            "z1": int_prop("Inclusive box corner")
        }), &[]),
        define("components", "Clean up the connected parts of the active object, for a mesh from an image-to-3D tool (import_mesh) that came with extras: a rider on a shell, moss tufts, foam splashes. Cells join by faces (connectivity 6), faces and edges (18), or faces, edges, and corners (26, the default). op list (default) reports the parts, largest first; keep_largest erases every part but the keep largest (default 1); remove_small erases parts with fewer than minCells cells (floaters); delete, move (by offset), and split (into a new object named name, or an existing one, into, keeping world positions; index picks the source) act on the part holding seed (snapped to the nearest solid cell within reach). With anchor, a cell of the part that stays, that part is first cut at its neck: the fewest cells whose removal separates seed from anchor (a minimum cut by max flow, outside a geodesic radius around each, default 0.3 of their distance); the cut cells stay with the anchor. One undo step. Replies with count, largest, components (up to 32: cells, min, max, seed), selected, seed, anchor, cut, changed, and voxels (split: target, name, created, sourceVoxels, targetVoxels).", json!({
            "op": {"type": "string", "enum": ["list", "keep_largest", "remove_small", "delete", "move", "split"], "description": "What to do. list (default) only reports."},
            "connectivity": {"type": "integer", "enum": [6, 18, 26], "description": "Neighbours that join cells: 6 faces, 18 faces and edges, 26 (default) also corners."},
            "keep": int_prop("keep_largest: how many of the largest parts stay. Defaults to 1."),
            "minCells": int_prop("remove_small: parts with fewer cells are erased."),
            "seed": {"type": "array", "items": {"type": "number"}, "description": "delete, move, split: a local cell [x, y, z] of the part to act on."},
            "anchor": {"type": "array", "items": {"type": "number"}, "description": "A local cell [x, y, z] of the part that stays; the seed's part is cut from it at the thinnest neck between them."},
            "reach": num_prop("How far seed and anchor may be from the solid cell they snap to, 0..=64. Defaults to 8."),
            "radius": num_prop("Geodesic radius in cells around seed and anchor that the neck cut stays out of. Defaults to 0.3 of their geodesic distance."),
            "offset": {"type": "array", "items": {"type": "integer"}, "description": "move: whole cells [dx, dy, dz]."},
            "index": {"type": ["integer", "string"], "description": "split: the source object (index or name). Defaults to the active one."},
            "name": str_prop("split: name of the new object. Defaults to <source>_part."),
            "into": {"type": ["integer", "string"], "description": "split: an existing object (index or name) to put the part in, instead of a new one."},
            "copy": bool_prop("split: leave the part in the source too."),
            "makeActive": bool_prop("split: make the target the active object.")
        }), &[]),
        define("surface_stroke", "Paint a stroke through points that lands on the active object's surface, for seams between shell plates, markings on a curved body, and grooves, instead of a fill_tube in paint mode at a radius that may miss the surface. The path (straight, or a smooth curve) is sampled every half cell; each sample snaps to the nearest surface cell (a solid cell with an empty face neighbour) within reach, so points may float above or sink below the surface. The solid cells within width / 2 of each snapped cell and at most depth cells into the solid (1 is the surface alone) take palette; palette 0 erases them, cutting a groove that deep. only limits it to cells of those slots. Nothing is added. One undo step. Replies with cells, changed, samples, missed (samples with no surface within reach), snap (the farthest a sample moved), length, and voxels.", json!({
            "points": {"type": "array", "description": "2..=1024 points [x, y, z] or {x, y, z} in the active object's local cells (a whole number is the centre of that cell); numbers may be fractional."},
            "palette": int_prop("Slot the stroke paints, 1..255, or 0 for a groove."),
            "curve": {"type": "string", "enum": ["polyline", "smooth"], "description": "polyline (default) or smooth (Catmull-Rom through the points)."},
            "width": num_prop("Width across the surface in cells, 0.5..=32. Defaults to 1, a one-cell line."),
            "depth": int_prop("Cells into the solid, 1..=16. Defaults to 1, the surface alone."),
            "reach": num_prop("How far a sample may be from the surface cell it snaps to, 0..=64. Defaults to 8."),
            "only": {"type": ["integer", "array"], "items": {"type": "integer"}, "description": "Only cells of these slots change, such as the shell slots, so a seam stops at the skin."}
        }), &["points", "palette"]),
        define("carve", "Remove the solid cells inside a box, ellipsoid, rounded_box, or tube of the active object, for hollows, tunnels, and pits (to cut a polygon or rounded rectangle along a path, closed or open, call sweep with palette 0; to remove everything outside a surface, call trim_outline). box, ellipsoid, and rounded_box take x0..z1 (and radius for rounded_box) exactly as their fill tools; tube takes points, radius, curve, and caps as fill_tube. Same 2000000-cell limits, checked before any cell changes; a tube also has at most 512000000 cells in the boxes around its segments, and a closed path counts like an open one. One undo step. Replies with removed.", {
            let mut p = json!({
            "shape": {"type": "string", "enum": ["box", "ellipsoid", "rounded_box", "tube"], "description": "Shape to remove."},
            "x0": int_prop("box, ellipsoid, rounded_box: inclusive corner"),
            "y0": int_prop("Inclusive corner"),
            "z0": int_prop("Inclusive corner"),
            "x1": int_prop("Inclusive corner"),
            "y1": int_prop("Inclusive corner"),
            "z1": int_prop("Inclusive corner"),
            "radius": num_prop("rounded_box: edge bevel. tube: radius for points that give none."),
            "points": {"type": "array", "description": "tube: points as in fill_tube."}
            });
            for (key, mut prop) in tube_shape_props() {
                let description = prop["description"].as_str().unwrap_or("");
                prop["description"] = format!("tube: {description}").into();
                p[key] = prop;
            }
            p
        }, &["shape"]),
        define("smooth", "Soften a blocky region of the active object with a majority filter over each cell's 26 neighbours, inside an inclusive local box of at most 2000000 cells. An empty cell with at least fillAt solid neighbours is filled with the colour most of them hold; a solid cell with at most removeAt solid neighbours is removed. The defaults fill single-cell pits and inside corners and remove single-cell spikes and outside corners, and keep flat faces and edges. It also removes features one cell thick (twigs, thin walls), so leave those out of the box. Cells outside the box are read, never changed. One undo step. Replies with filled, removed, and the passes run.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["iterations"] = with_default(int_prop("Passes, 1..=8. Defaults to 1. Stops early when a pass changes nothing."), json!(1));
            p["fillAt"] = with_default(int_prop("Fill an empty cell with at least this many solid neighbours, 1..=27. Defaults to 17; 14 also rounds inside edges; 27 never fills."), json!(17));
            p["removeAt"] = with_default(int_prop("Remove a solid cell with at most this many solid neighbours, -1..=25. Defaults to 9; 12 also rounds outside edges; -1 never removes."), json!(9));
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("break_end", "Break the active object across a plane, for a snapped trunk, branch, plank, or post. Every cell beyond a jagged surface around the plane goes: blocks of 2 by 3 cells across the face dip up to depth into the wood, a quarter of them deep pits. The cells left facing the cut take bark on the rim of the section and sapwood inside, and tapered flat splinters grow from the new face out along the normal, longer and more common near the rim with rimBias. Splinters fill only empty cells. Deterministic for a seed. At most 512 splinters and 2000000 cells. One undo step. Replies with removed, recoloured, and splinters.", json!({
            "point": vec3_prop("A point on the break plane, in local cells."),
            "normal": vec3_prop("Out of the wood, toward the part that breaks off. Need not be unit length."),
            "depth": num_prop("How far pits and the uneven face reach into the wood, 0..=32 cells. Defaults to 3."),
            "splinters": int_prop("Number of splinters, 0..=512. Pass this or density."),
            "density": num_prop("Splinters per cell of the new face, 0..=1. Defaults to 0.15."),
            "length": {"type": "array", "items": {"type": "number"}, "description": "Splinter length [min, max] in cells, 1..=64. Defaults to [4, 12]."},
            "thickness": num_prop("Splinter radius at its base, 0.5..=4. Defaults to 1."),
            "taper": num_prop("0 keeps the base radius to the tip; 1 (default) thins it to one cell."),
            "rimBias": num_prop("0..=1. How much more often and longer splinters are near the rim. Defaults to 0.5."),
            "bark": int_prop("Palette index for the rim of the new face and the base of rim splinters. Omit to keep the cells' colour."),
            "sapwood": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices mixed over the new face and the splinters. Omit to keep the cells' colour."},
            "tips": int_prop("Palette index for the outer part of each splinter, often a lighter wood. Omit to use the sapwood."),
            "seed": int_prop("Seed. Defaults to 0.")
        }), &["point", "normal"]),
        define("scatter", "Scatter small pieces of debris (shard, chip, pebble, chunk) into the active object, for a heap at a break, chips at a stump, gravel. Each piece starts inside the inclusive box's x and y at its top and falls straight down until the next step would touch a solid cell (the model or a piece before it) or pass floor, so pieces rest on what is below and pile up. Only cells, no physics; only empty cells are written. A piece that meets a solid cell at the top of the box, or would rest in an exclude box, is skipped. Deterministic for a seed. At most 2000 pieces, a box of 2000000 cells, and 200000 new cells. One undo step. Replies with pieces and skipped.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices, one picked per piece. A single integer works too."});
            p["count"] = int_prop("Pieces, 0..=2000. Defaults to 20.");
            p["kinds"] = json!({"type": "array", "items": {"type": "string", "enum": ["shard", "chip", "pebble", "chunk"]}, "description": "Piece kinds to mix. shard is a flat wedge, chip a flake one cell thick, pebble a squashed ellipsoid, chunk a block with chipped corners. Defaults to all four."});
            p["size"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Piece size [min, max] in cells, 1..=16, or one number. Defaults to [1, 3]."});
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p["floor"] = int_prop("Lowest z a piece may rest at. Defaults to the box's bottom.");
            p["clump"] = num_prop("0 spreads pieces evenly over the box; 1 piles them in its middle. Defaults to 0.5.");
            p["exclude"] = exclude_prop();
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("joints", "Cut real joint gaps into the solid cells of an inclusive box of the active object, for fractured limestone, masonry, and cracked rock. The cells are split into stones by pattern: columns (vertical columns of random widths, seen from above as a jittered grid, with a bed joint now and then), courses (horizontal courses of blocks whose vertical joints shift from course to course, a running bond), or cracks (an irregular network between jagged pieces). Cells on a boundary are removed down to depth cells under the surface; the solid cell at the bottom of each gap takes the dirt slot. exclude boxes keep walking surfaces whole. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with removed and dirtied.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["pattern"] = json!({"type": "string", "enum": ["columns", "courses", "cracks"], "description": "columns, courses (running bond), or cracks"});
            p["spacing"] = json!({"type": "array", "items": {"type": "number"}, "description": "Stone width range [min, max] in cells, 2..=128, or one number: column width, block length, or crack piece size. Defaults to [4, 9] for columns, [6, 11] for courses, [6, 11] for cracks."});
            p["course"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Course height range [min, max] in cells, 1..=128 (courses), or the spacing of bed joints across a column (columns). Defaults to [3, 5] and [10, 24]. Cracks ignore it."});
            p["width"] = int_prop("Gap width in cells, 1..=3. Defaults to 1.");
            p["depth"] = int_prop("Gap depth under the surface in cells, 1..=16. Defaults to 2.");
            p["dirt"] = int_prop("Palette index for the solid cell at the bottom of each gap, such as dark dirt. Omit to keep its colour.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Cut only cells of these palette indices (the stone slots), so moss and plants stay. Omit to cut every slot."});
            p["density"] = num_prop("cracks: the share of piece boundaries that crack, 0..=1. Defaults to 0.6.");
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "pattern"]),
        define("column_relief", "Break a cliff face of the active object into columns, for fractured limestone cliffs: along the face, columns of random width stand out of it by a random amount and end at uneven heights around the crown. The face is found in the cells of the inclusive box, so a swept, curved wall works: for each step along it, the median of the frontmost solid cell of every row up to the crown, which a ledge does not pull forward. A column fills empty cells from depth cells behind the face to out cells in front of it, from the box's bottom to its top. With lower (the default), a column that ends below the crown cuts the wall above it, depth cells back from the face, so the crown is uneven; lower false only adds cells, columns rising above the crown or ending short in front of it. Writes stay inside the box and out of exclude boxes. Run it after sweep and before joints and weather. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with columns, added, removed, and crown.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner; a few cells larger than the wall so columns can stand out of it");
            }
            p["facing"] = json!({"type": ["string", "array"], "description": "The face's outward direction, along x or y: +x, -x, +y, -y, or a vector such as [0, -1, 0]."});
            p["palette"] = int_prop("Palette index of the columns, usually the stone.");
            p["width"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Column width range [min, max] along the face in cells, 1..=64, or one number. Defaults to [6, 12]."});
            p["out"] = json!({"type": "array", "items": {"type": "integer"}, "description": "How far a column stands out of the face, [min, max] cells, 0..=16, or one number. Defaults to [1, 4]."});
            p["crown"] = int_prop("Height the column tops scatter around. Defaults to the highest solid cell in the box.");
            p["top"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Column top range [min, max] relative to the crown in cells, -256..=256: negative ends short of it, positive rises above it. Defaults to [-8, 4]."});
            p["lower"] = with_default(json!({"type": "boolean", "description": "A column ending below the crown cuts the wall above it, so the crown is uneven. false only adds cells (only rising). Defaults to true."}), json!(true));
            p["depth"] = with_default(int_prop("How far a column reaches back into the face, and how deep the cut above a short column goes, 1..=16 cells. Defaults to 3."), json!(3));
            p["exclude"] = exclude_prop();
            p["seed"] = with_default(int_prop("Seed. Defaults to 0."), json!(0));
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "facing", "palette"]),
        define("weather", "Weather the exposed cells of an inclusive box of the active object: chip edges and corners (small irregular chunks off cells exposed on two or more sides), dent flat faces with pits (real holes, not paint), and add subtle colour noise in soft patches. The faces a chip or pit uncovers take the broken slot, usually a slightly darker stone (not dirt). exclude boxes keep walking surfaces whole. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with chips, pits, removed, broken, and recoloured.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["chip"] = num_prop("Chance that an edge cell starts a chip, 0..=1; corners are likelier. Defaults to 0.15. 0 chips nothing.");
            p["chipSize"] = int_prop("Most cells one chip takes, 1..=8. Defaults to 2.");
            p["pits"] = num_prop("Chance that a flat face cell starts a pit, 0..=1. Defaults to 0.02.");
            p["pitDepth"] = int_prop("Deepest pit in cells, 1..=4. Defaults to 1.");
            p["broken"] = int_prop("Palette index for the faces a chip or pit uncovers, about 0.9 x the stone. Omit to keep their colour.");
            p["noise"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices of the colour noise, a few tones within a few units of the stone. Omit for no noise."});
            p["noiseDensity"] = num_prop("Share of exposed cells the noise recolours, 0..=1. Defaults to 0.35.");
            p["noiseScale"] = num_prop("Noise patch size in cells, 1..=32. Defaults to 3.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices (the stone slots). Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("moss", "Grow moss on the exposed faces of an inclusive box of the active object that pass the face filter. style caps (the default): cushions on tops (where the top face passes), moss on undersides when faces takes bottom, and curtains running down side faces from a top edge, each with a ragged end; coverage comes in soft patches mixed with speckle by noise. style cushions: separate irregular cushions and patches of size cells where the surface faces up (its normal within exposure of up, or of facing), kept gap cells apart until amount of those cells is covered, so moss never forms one continuous band (facing [0, 0, -1] with layer paint shades undersides, such as a tree crown's); in layer grow each cushion is domed up to thickness at its middle and drapes run from its edge down the side and hang free below an overhang. layer grow adds the moss on the empty side of each face (only empty cells are written); paint recolours the surface cells. palettes are picked in patches (per cushion), darker and lighter moss. Cut joints, bark, and weather first. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, painted, and curtains, or cushions and drapes, and warning (why) when nothing grew.", merged(growth_props(&[
            ("curtains", num_prop("caps: share of moss-capped top edges a curtain runs down from (any top edge when faces leaves out top), 0..=1. Defaults to 0.3. 0 grows caps only.")),
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Curtain length range [min, max] in cells, 1..=64 (caps), or drape length, 1..=32 (cushions), or one number. Defaults to [2, 8] for caps and [2, 6] for cushions."})),
            ("thickness", int_prop("Cushion thickness, 1..=3 on caps tops, 1..=4 at a cushion's middle (layer grow). Defaults to 1 for caps and 2 for cushions.")),
        ], "Coverage, 0..=1: of the passing faces (caps), or of the exposed cells (cushions). Defaults to 0.5 for caps and 0.45 for cushions.", "top and side"), json!({
            "style": {"type": "string", "enum": ["caps", "cushions"], "description": "caps (default): caps and curtains. cushions: separate cushions on exposed cells with drapes from overhangs."},
            "size": {"type": "array", "items": {"type": "integer"}, "description": "cushions: cushion radius range [min, max] in cells, 1..=16, or one number. Defaults to [2, 5]."},
            "gap": int_prop("cushions: fewest cells between two cushions, 1..=8. Defaults to 2."),
            "exposure": num_prop("cushions: how far a cell must face up (or facing) to take moss, the cosine of its surface normal with that direction, -1..=1. 0 takes the upper half of a log, 0.7 only its crown. Defaults to 0.35."),
            "drapes": num_prop("cushions: share of cushion edges over a drop that a drape hangs from, 0..=1 (layer grow). Defaults to 0.35. 0 hangs none.")
        })), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("algae", "Paint algae on the exposed faces of an inclusive box of the active object inside a height band, usually the waterline: dense at the bottom of the band and thinning upward in noisy clumps with stone peeking through. palettes go from the bottom of the band to its top (a darker tone low, a lighter one high). Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with added and painted.", growth_props(&[], "Coverage at the bottom of the band, 0..=1. Defaults to 0.9.", "top and side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("vines", "Hang vine strands from the top edges of an inclusive box of the active object: each strand starts beside an edge cell whose side passes the face filter and falls straight down through empty cells, wandering a little along the edge with noise, until its length or something solid below. palettes are graded from the anchor to the tip, so give them motion sway rising from the anchor (set_palette motion). blooms adds an occasional bloom cell beside a strand. Only empty cells are written. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, strands, and blooms.", growth_props(&[
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Strand length range [min, max] in cells, 1..=128, or one number. Defaults to [4, 16]."})),
            ("blooms", json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices of blooms beside the strands. Omit for none."})),
            ("bloomChance", num_prop("Chance of a bloom beside each strand cell, 0..=1. Defaults to 0.06.")),
        ], "Share of top edge cells a strand starts from, 0..=1. Defaults to 0.08.", "side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("bark", "Cut bark into the wood in an inclusive box of the active object, along the grain: the direction is an axis (x, y, z, or a vector, through the middle of the solid cells) or the local direction of points, the same points (and curve) as the fill_tube or sweep that made the trunk, branch, or root. The surface is split into long plates of varied width (around the wood) and length (along it), separated by fissures cut depth cells into the wood, the cell at the bottom of each taking the dirt slot. Plate ends sit at other heights than the next column's, as on a real trunk, and cracks share of the plates have a crack part way across. Each plate takes one of palettes, so the tone changes plate by plate instead of in stripes; end grain (the cut end of a log) and the inside of a hollow are left alone, and a hollow's inner wall is never cut through. A flat or gently curved plate (a deck, a plank, a wall; its section at least three times as wide as deep) gets columns of width across its surface from rim to rim; one too thin to cut from both faces is cut from its free face (the upper one when both are free) and never through, and one cell thick it only takes the plate tones. Use it instead of painted stripes. Then cavity_tint the box to shade the fissures, and grow moss. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with plates, removed, dirtied, painted, and cracks, and warning (why) when a shape could not be cut.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["axis"] = json!({"type": ["string", "array"], "description": "The grain along a straight axis: x, y, z, or a vector [x, y, z]. Pass this or points."});
            p["points"] = json!({"type": "array", "items": {"type": ["array", "object"]}, "description": "The grain along a path: [x, y, z] points, or the fill_tube ([x, y, z, radius]) or sweep ([x, y, z, twist, scale]) points of the wood, whose extra numbers are ignored. Pass this or axis."});
            p["curve"] = str_prop("With points: polyline (default) or smooth, as the tube used.");
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Plate slots, a few bark tones within a few units of each other; each plate takes one."});
            p["dirt"] = int_prop("Palette index for the cell at the bottom of each fissure, a dark brown. Omit to keep its colour.");
            p["width"] = json!({"type": "array", "items": {"type": "number"}, "description": "Plate width range [min, max] in cells around the wood, or across a flat plate, 1..=64, or one number. Defaults to [3, 6]."});
            p["length"] = json!({"type": "array", "items": {"type": "number"}, "description": "Plate length range [min, max] in cells along the grain, 2..=256, or one number. Defaults to [8, 20]."});
            p["depth"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Fissure depth range [min, max] in cells, 1..=4, or one number. Plate ends and cracks take the minimum. Defaults to [1, 2]."});
            p["cracks"] = with_default(num_prop("Share of plates with a crack part way across, 0..=1. Defaults to 0.25."), json!(0.25));
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices (the wood). Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["seed"] = with_default(int_prop("Seed. Defaults to 0."), json!(0));
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("planks", "Lay a deck, floor, or dock of separate boards over a rectangle of the active object at height z, along an axis, instead of dozens of fill_box and carve calls or painted stripes. Rows of random width tile the rectangle across the axis; each row is split into boards of random length, with end joints kept clear of those of the neighbouring rows. Gaps (between rows and at end joints) are empty down through the boards, with a dirt cell under them. Each board takes one tone, never that of the board before it in the row and as seldom as it can that of the boards beside it, and a row with room for a joint always has one, so no layout runs as one uniform stripe. A row's first and last boards are sawn pieces, so they may be shorter than length. Optional ragged ends, and nails and knots in the dark slot. Solid or empty, every cell of the rectangle takes a board or a gap unless only limits it, so to fit a curved hull fill its deck in one slot and pass that slot as only. The same seed gives the same boards. One undo step. Replies with rows, boards, joints, added, painted, removed, dirtied, nails, knots, changed, and voxels.", {
            let mut p = json!({});
            for key in ["x0", "y0", "x1", "y1"] {
                p[key] = int_prop("Inclusive corner of the rectangle");
            }
            p["z"] = int_prop("The top layer of the boards: they run from z down through thickness layers, and the dirt cell lies under them.");
            p["axis"] = json!({"type": "string", "enum": ["x", "y"], "description": "The way the boards run."});
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Board tones, at least two different slots, ideally three or more within a few units of each other; each board takes one."});
            p["dirt"] = int_prop("Palette index of the cell under each gap, a dark brown. Omit to leave that cell as it is, so a gap over nothing is open.");
            p["dark"] = int_prop("Palette index of nails and knots. Omit for neither.");
            p["width"] = json!({"type": ["array", "number"], "items": {"type": "integer"}, "description": "Board width range across the axis in cells [min, max], 1..=64. Defaults to [3, 6]."});
            p["length"] = json!({"type": ["array", "number"], "items": {"type": "integer"}, "description": "Board length range along the axis in cells [min, max], 2..=256. Defaults to [8, 20]."});
            p["gap"] = json!({"type": ["array", "number"], "items": {"type": "integer"}, "description": "Gap width between boards in cells, a number or [min, max], 1..=8. Defaults to 1."});
            p["thickness"] = with_default(int_prop("Layers of board, the gaps run through them all, 1..=16. Defaults to 2."), json!(2));
            p["ragged"] = with_default(num_prop("Chance per cell across a board's end that it is cut back 1 or 2 cells, 0..=1, about 0.3 for old wood. Defaults to 0: square ends."), json!(0));
            p["nails"] = num_prop("Share of boards with a dark nail near each end, 0..=1, with dark. Defaults to 0.25.");
            p["knots"] = num_prop("Share of boards with a dark knot along the middle, 0..=1, with dark. Defaults to 0.15.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on solid cells of these palette indices, such as a deck already filled to a curved outline. Omit to lay boards over the whole rectangle."});
            p["exclude"] = exclude_prop();
            p["seed"] = with_default(int_prop("Seed. Defaults to 0."), json!(0));
            p
        }, &["x0", "y0", "x1", "y1", "z", "axis", "palettes"]),
        define("cavity_tint", "Shade depth into the exposed cells of an inclusive box of the active object without hand painting: each cell counts the solid cells in a ball of radius around it, a curvature or ambient occlusion estimate. Cells in concave spots (fissures, crevices, the crook of a root, the foot of a wall) more than dark take a darker slot; with lighten, cells on convex ridges and corners more than light take a lighter one. pairs names the slots ([slot, darker] or [slot, darker, lighter]); without pairs each slot gets shades of its own colour (darken and brighten times it, with its material) in free palette slots (from 255 down, never one a cell uses), reused on later calls. Run it after bark, joints, or weather and before moss. A box of at most 2000000 cells. One undo step with the palette. Replies with darkened, lightened, and shades (the slots it made).", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["radius"] = with_default(int_prop("Radius of the ball counted around each cell, 1..=4. Larger finds broader hollows. Defaults to 2."), json!(2));
            p["dark"] = with_default(num_prop("How concave a cell must be to darken, 0..=1: 0 a flat face, 1 a closed pocket. Defaults to 0.12."), json!(0.12));
            p["lighten"] = with_default(json!({"type": "boolean", "description": "Also lighten exposed ridges and corners. Defaults to false."}), json!(false));
            p["light"] = with_default(num_prop("With lighten: how convex a cell must be to lighten, 0..=1: 0 a flat face, 1 a lone spike. Defaults to 0.25."), json!(0.25));
            p["pairs"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "integer"}}, "description": "[slot, darker] or [slot, darker, lighter] per slot (0 leaves that tint out). Slots without a pair stay. Omit to make shades."});
            p["darken"] = with_default(num_prop("Without pairs: the darker shade's colour as a factor of the slot's, 0.05..1. Defaults to 0.72."), json!(0.72));
            p["brighten"] = with_default(num_prop("Without pairs: the lighter shade's colour as a factor of the slot's, above 1 up to 4. Defaults to 1.18."), json!(1.18));
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices. Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("radial_relief", "Cut regular, shallow, sloped recesses repeated evenly round an axis into the active object, for architecture and turned or coopered pieces: kind flutes are grooves along the side of a column, pilaster, vase, urn, or barrel, spaced evenly along each slice's outline (so a square shaft gets as many per face, a round one evenly by angle) and following the outer surface the cells have; kind gores dish each segment of a dome between two meridian lines toward its middle, the ribs between them staying proud, fading to nothing at the crown and over fade cells at the foot (from). count recesses round the axis (axis z by default, through center or the middle of the solid), turned by phase degrees (for flutes a share of the outline's length; half the pitch is 180 / count). profile u (round-bottomed, the default), v, or flat (a flat floor with sloped sides) sets the section; depth (0.5..=4 cells, default 1) is the deepest point, measured in from the outer surface along the outward direction (straight out from the axis for flutes, out from the dome's centre for gores). A cell is cut when the recess floor lies a quarter cell below its centre. Flutes run along the axis from from to to (or z0 to z1 for axis z; by default the solid's extent), each width cells across (default 0.6 of the pitch), and their ends narrow and shallow out over fade cells (default the width, at least 2): ends round (a cupped end, the default), taper, or square. Because cells can only step, the slope is carried by colour too: shade (default true) tones the floor cells in the middle of each recess, light (default false) the slope cells by the ridges, each a palette slot or true for a shade made from each cell's own slot (darken and brighten times its colour, with its material, in free slots from 255 down, never one a cell uses, reused on later calls) or false for none; ribs paints the lines between recesses (a dome's ribs, a column's fillets) with a slot, ribWidth cells wide for gores (default 2), and ribHeight raises them by that many cells. Writes stay inside the box (any of x0..z1 given; a side not given is open), between from and to, out of exclude boxes, and on cells of only slots. jitter (default 0, architecture is regular) varies each recess's depth and place by that share, from seed. At most 2000000 cells in the solid's box. One undo step with the palette. Replies with cells (surface cells the recesses and ribs cover), recesses, removed, painted, added, shades, and a warning when nothing matched.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner the writes stay inside. Each is optional: a side not given is open, so z0 and z1 alone bound a column's height.");
            }
            p["kind"] = json!({"type": "string", "enum": ["flutes", "gores"], "description": "flutes: grooves along a shaft's side. gores: a dome's segments dished between meridian ribs."});
            p["count"] = int_prop("How many recesses round the axis, 1..=256.");
            p["axis"] = json!({"type": "string", "enum": ["x", "y", "z"], "description": "The axis the recesses repeat round. Defaults to z."});
            p["center"] = json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "The axis in the plane across it, [a, b] as the two other coordinates in x, y, z order (for axis z, [x, y]); cell centres are whole numbers. Defaults to the middle of the solid's extent in the box and range."});
            p["phase"] = with_default(num_prop("Turns the pattern, in degrees: for flutes a share of the outline's length (360 a whole turn), for gores an angle from the +x (axis z), +y (axis x), or +x (axis y) side. 0 puts a ridge or rib at 0. Defaults to 0."), json!(0));
            p["width"] = num_prop("Flutes: cells across each flute, 0.5..=64, at most the pitch. Defaults to 0.6 of the pitch (the outline's length over count).");
            p["from"] = int_prop("First cell along the axis: the flutes' lower end, the dome's foot. Defaults to the solid's lowest cell in the box.");
            p["to"] = int_prop("Last cell along the axis: the flutes' upper end, the dome's top. Defaults to the solid's highest cell in the box.");
            p["ends"] = json!({"type": "string", "enum": ["round", "taper", "square"], "description": "How a recess ends over fade cells: round (a cupped end, the default), taper (a straight ramp), or square (full depth to the end)."});
            p["fade"] = num_prop("Cells over which a recess fades in from its ends (flutes; default the width, at least 2) or from the foot (gores; default 6), 0..=128. Gores also fade toward the crown.");
            p["profile"] = json!({"type": "string", "enum": ["u", "v", "flat"], "description": "Section across a recess: u (round-bottomed, the default), v (straight sides), or flat (a flat floor over the middle half, sloped sides)."});
            p["depth"] = with_default(num_prop("Deepest recess depth, 0.5..=4 cells, in from the outer surface. Defaults to 1."), json!(1));
            p["shade"] = json!({"type": ["integer", "boolean"], "description": "Tone of the floor cells in the middle of each recess: a palette slot, true (the default) for a darker shade of each cell's own slot, or false for none."});
            p["light"] = json!({"type": ["integer", "boolean"], "description": "Tone of the slope cells by the ridges: a palette slot, true for a lighter shade of each cell's own slot, or false (the default) for none."});
            p["darken"] = num_prop("With shade true: the shade's colour as a factor of the slot's, 0.05..1. Defaults to 0.72.");
            p["brighten"] = num_prop("With light true: the light tone's colour as a factor of the slot's, above 1 up to 4. Defaults to 1.18.");
            p["ribs"] = int_prop("Palette slot of the lines between recesses (a dome's ribs, a column's fillets), and of raised rib cells. Omit to leave them as they are.");
            p["ribWidth"] = with_default(num_prop("Gores: width of the rib line between two gores, 0..=16 cells. Defaults to 2, which keeps a rib whole down a steep side."), json!(2));
            p["ribHeight"] = with_default(int_prop("Raise the ribs by this many cells, 0..=4, in the ribs slot (or the slot under them). Defaults to 0."), json!(0));
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices. Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["jitter"] = with_default(num_prop("Share of random change in each recess's depth and place, 0..=1. Defaults to 0: architecture is regular."), json!(0));
            p["seed"] = with_default(int_prop("Seed of the jitter. Defaults to 0."), json!(0));
            p
        }, &["kind", "count"]),
        define("foliage", "Build a clustered broadleaf tree crown on the active object, as pixel-art trees draw one: many separate leaf clumps with sunlit tops, darker undersides, and dark gaps between them, instead of one smooth blob. The crown volume is ellipsoids and boxes, or from (the existing cells of some slots, to re-cluster a crown already sculpted; those no clump keeps are removed), or both. Clump centres are packed over the volume's surface with Poisson-like spacing (no two closer than their sizes and gap allow); each clump is a flattened ellipsoid with a ragged, leafy outline that bulges out of the surface. The volume one cell under its surface fills with the core slot, so the gaps show dark and nothing is see-through. Each clump is shaded as one piece: its height in the crown and the way the crown faces the sun there set its tone, inside it the top lightens and the underside darkens along its own curve, and a darker seam marks where two clumps meet; no colour is per-cell noise. masses groups the clumps into lobes split by deeper grooves. Only empty cells and cells of the crown's own slots (palettes, core, from) are written, never protect slots: the trunk and branches stay whole and show through the gaps; exclude boxes keep a space open (in front of the trunk). Palette slots keep their motion, so give the leaf slots and the core the same sway. Writes stay inside the box, or without one inside the shapes and from cells grown by room for the clumps. Deterministic for a seed. A box of at most 2000000 cells, at most 4096 clumps. One undo step. Replies with clumps, masses, added, recoloured, removed, and core.", merged({
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner the writes stay inside. Omit all six to bound the crown by its volume.");
            }
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "1 to 8 leaf slots from the darkest to the lightest: shade, mid, light, sunlit tips."});
            p["ellipsoids"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "Crown volume: [x, y, z, radius] or [x, y, z, rx, ry, rz] per ellipsoid, up to 64 shapes with boxes. A broad middle and a few lobes read as a tree."});
            p["boxes"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "integer"}}, "description": "Crown volume: inclusive [x0, y0, z0, x1, y1, z1] boxes."});
            p["from"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Crown volume: the existing cells of these slots (in the box). Re-clusters a crown already sculpted; its cells no clump or core keeps are removed."});
            p["core"] = int_prop("Palette index of the dark core inside the crown, seen in the gaps. Defaults to the first of palettes.");
            p["size"] = json!({"type": "array", "items": {"type": "number"}, "description": "Clump radius range [min, max] in cells, 1..=32, or one number. Defaults to [3, 6]."});
            p["gap"] = with_default(num_prop("Extra cells between neighbouring clumps, 0..=8. Larger shows more dark core between them. Defaults to 1."), json!(1));
            p["flatten"] = with_default(num_prop("Clump height over width, 0.2..=1.5; below 1 clumps are wider than tall. Defaults to 0.7."), json!(0.7));
            p["ragged"] = with_default(num_prop("Leafy outline noise of each clump, 0..=1. 0 is a smooth ellipsoid. Defaults to 0.4."), json!(0.4));
            p
        }, json!({
            "light": with_default(num_prop("How strongly sunlit tops lighten toward the last palette, 0..=2. Defaults to 1."), json!(1)),
            "dark": with_default(num_prop("How strongly undersides and seams darken toward the first palette, 0..=2. Defaults to 1."), json!(1)),
            "sun": json!({"type": "array", "items": {"type": "number"}, "description": "Direction toward the sun [x, y, z], Z up. Defaults to [-0.35, -0.45, 1], high at the front left."}),
            "masses": with_default(int_prop("Lobes the clumps are grouped into, 0..=16, split by deeper grooves and each with its own tone. 0 (the default) is none."), json!(0)),
            "groove": with_default(int_prop("With masses: how many cells deeper the grooves between lobes go, 0..=8. Defaults to 2."), json!(2)),
            "protect": json!({"type": "array", "items": {"type": "integer"}, "description": "Slots never written, even when they are also crown slots. Any slot not in palettes, core, or from is already kept (the wood)."}),
            "exclude": exclude_prop(),
            "seed": with_default(int_prop("Seed. Defaults to 0."), json!(0))
        })), &["palettes"]),
        define("sweep", "Fill a 2D cross-section along a path of 2..=1024 local points, kept perpendicular to it, for curved walls, banks, ledges, and kerbs with no corners. The profile's u runs to the left of the path (seen from above, walking along it) and v up, so a wall profile stands on the path; a whole-number point is a cell centre. profile is a polygon of [u, v] points, or width (across, centred on the path), height (up from it), and radius (rounded corners) give a rounded rectangle. Each point may carry its own twist (degrees about the path) and scale, interpolated along it. curve smooth passes a Catmull-Rom curve through the points; extra slabs fill the outside of a bend. A cell is in when its centre is inside the profile at the nearest slab, so a flat top stays one flat layer. palettes lay out in bands (by height in the profile, strata), rings (along the path), or random. Writes as mode says. palette 0 erases instead: it removes the solid cells the sweep covers, for a slanted cut along a closed or curved outline (mode empty is refused with it). At most 2000000 cells. One undo step. Replies with cells and length, and removed when erasing.", json!({
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Path points: [x, y, z], [x, y, z, twist], [x, y, z, twist, scale], or {x, y, z, twist?, scale?}. twist is degrees counter-clockwise looking along the path (default 0); scale multiplies the profile, 0.05..=16 (default 1)."},
            "profile": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "Closed polygon of 3..=256 [u, v] points in cells: u to the left of the path, v up. Omit to use width, height, and radius."},
            "width": num_prop("Rounded rectangle width across the path, centred on it, 0.5..=512. Used without profile."),
            "height": num_prop("Rounded rectangle height up from the path, 0.5..=512. Used without profile."),
            "radius": num_prop("Rounded rectangle corner radius, 0 up to half the smaller side. Defaults to 0."),
            "curve": str_prop("polyline (default) or smooth"),
            "palette": int_prop("Palette index, 0 to erase the solid cells it covers, or pass palettes"),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices 1..=255 laid out by pattern"},
            "pattern": {"type": "string", "enum": ["bands", "rings", "random"], "description": "bands (default) by height v in the profile, period cells each, like strata; rings along the path; random per cell"},
            "period": num_prop("Band or ring size in cells, 0.5..=512. Defaults to 2."),
            "seed": int_prop("Seed of the random pattern. Defaults to 0."),
            "mode": mode_prop()
        }), &["points"]),
        define("trim_outline", "Reshape the active object from outside: remove every solid cell outside a surface given as a closed plan outline and an inset per height, so a finished hull narrows toward its keel in one call. The surface at height z is the outline moved inward by inset(z); a cell stays when its centre is at least that far inside the outline. Use it when everything outside goes; sweep with palette 0 cuts a band along a path instead. skin lays a new skin of that slot along the surface, thickness cells inward, because the cut exposes whatever was inside: where the cut reached, solid cells are recoloured and a hollow is sealed, while a rim, an opening, or a part the cut missed stays as it was, and the object's box never grows. x0..z1 (all six or none) and only limit what is removed or skinned. At most 2000000 solid cells. One undo step. Replies with removed, painted, and added, and a warning when nothing was outside.", json!({
            "outline": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "The closed plan outline: 3..=1024 local [x, y] points in either winding, a whole number being a cell centre. Do not repeat the first point (a repeat is ignored)."},
            "inset": {"type": ["number", "array"], "items": {"type": "array", "items": {"type": "number"}}, "description": "How far inward the surface stands: one number for every height, or [z, inset] pairs going up in z, linear between them and the end value beyond them. A negative inset stands outside the outline. Defaults to 0."},
            "curve": str_prop("polyline (default) or smooth, a closed curve through the points"),
            "x0": int_prop("Box corner (optional; with all six, nothing outside the box is removed or written)"),
            "y0": int_prop("Box corner"),
            "z0": int_prop("Box corner"),
            "x1": int_prop("Box corner"),
            "y1": int_prop("Box corner"),
            "z1": int_prop("Box corner"),
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette slots to remove or recolour; others stay. Defaults to every slot."},
            "skin": int_prop("Palette slot of a new skin along the surface. Omit for none."),
            "thickness": num_prop("The skin's thickness inward from the surface, 1..=64 cells, across a layer and into the layers above and below on a slope. Defaults to 1.")
        }), &["outline"]),
        define("deform", "Reshape the active object, or the solid cells of an inclusive box of it (x0..z1, all six or none), so a trunk, root, limb, or rock stops reading as a pipe or a box. op bend curls the cells toward direction by angle degrees over length cells of the axis from origin, and carries the rest on straight; taper scales them across the axis from start at the origin to end at the end of the length; twist turns them about the axis by angle degrees over the length; displace pushes the surface in and out with fractal 3D noise of amplitude cells, bumps about scale cells wide, octaves finer layers, and bias (-1 only dents, 0 both, 1 only bulges), for burls, lumps, and knobs; scale enlarges (or shrinks) a part about point [x, y, z] by factor (a number or [x, y, z], 0.25..4, default 1.5): the cells within radius cells are scaled as one rigid piece and over the next falloff cells (default radius) the scale eases back to none, so the rest stays where it is and the part stays attached, the exaggeration step for a small creature (a head, an eye, a fin that needs more cells to read at 20 to 60 cells long; scale the region of a box to leave other parts alone). minFeature N (2..16, any op) adds thin to the reply: the object's cells after the deform whose shortest solid run along x, y, or z is under N cells, grouped into connected parts {cells, min, max, thickness} (the 16 largest, count of all), so a fin, ear, or leg too thin for the size shows before it vanishes or reads as noise. axis is x, y, or z (default z); origin defaults to the cells' lowest along it, length to their extent, and center (the two other coordinates, in x, y, z order) to their mean. Each target cell takes the slot of the cell it came from, so palette slots are kept. Deterministic for a seed. At most 2000000 cells. One undo step. Replies with cells (read), result (left), added, removed, and thin with minFeature.", merged(json!({
            "op": {"type": "string", "enum": ["bend", "taper", "twist", "displace", "scale"], "description": "The deform."},
            "x0": int_prop("Region corner (optional; with all six only the box's cells change)"),
            "y0": int_prop("Region corner"),
            "z0": int_prop("Region corner"),
            "x1": int_prop("Region corner"),
            "y1": int_prop("Region corner"),
            "z1": int_prop("Region corner"),
            "axis": {"type": "string", "enum": ["x", "y", "z"], "description": "bend, taper, twist: the axis they run along. Defaults to z."},
            "origin": num_prop("bend, taper, twist: where along the axis they start, in local cells. Defaults to the cells' lowest."),
            "length": num_prop("bend, taper, twist: how far along the axis they run, 0.5..=4096. Defaults to the cells' extent."),
            "center": {"type": "array", "items": {"type": "number"}, "description": "bend, taper, twist: the line they work about, [a, b] as the two other coordinates in x, y, z order (for axis z, [x, y]). Defaults to the cells' mean."}
        }), json!({
            "angle": num_prop("bend: degrees, -180..=180 (negative bends away from direction). twist: degrees over the length."),
            "direction": {"type": "string", "enum": ["+x", "-x", "+y", "-y", "+z", "-z"], "description": "bend: the way the cells curl, across the axis. Defaults to +x for axis z, else +z."},
            "start": num_prop("taper: scale at the origin, 0.05..=16. Defaults to 1."),
            "end": num_prop("taper: scale at the end of the length, 0.05..=16. Defaults to 0.5."),
            "amplitude": num_prop("displace: the most the surface moves, 0.5..=32 cells. Defaults to 2."),
            "scale": num_prop("displace: bump size in cells, 1..=512. Defaults to 8."),
            "octaves": int_prop("displace: 1..=6 layers, each half the size and height. Defaults to 2."),
            "seed": with_default(int_prop("displace: noise seed. Defaults to 0."), json!(0)),
            "bias": num_prop("displace: -1 only dents, 0 dents and bulges (default), 1 only bulges."),
            "point": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "scale: the centre of the part, local cells [x, y, z]."},
            "factor": {"type": ["number", "array"], "items": {"type": "number"}, "description": "scale: how much larger, 0.25..4, or [x, y, z] per axis. Defaults to 1.5."},
            "radius": num_prop("scale: cells around point scaled as one piece, 0.5..512."),
            "falloff": num_prop("scale: cells (before scaling) over which the scale eases back to none, 0.5..512. Defaults to radius."),
            "minFeature": int_prop("Any op: report the parts of the object thinner than this many cells after the deform, 2..16.")
        })), &["op"]),
        define("blend_shapes", "Fill simple shapes blended as signed distance fields with a smooth minimum, so they flow into each other like clay instead of sticking together as separate balls and bricks: statues, creatures, topiary, roots, snowmen, any organic form built from a body, a head, eyes, and limbs. shapes is a list applied in order; each is {shape: sphere (center, radius) | ellipsoid (center, radii [rx, ry, rz] or radius) | box (center, size [sx, sy, sz] in cells, round: edge radius) | capsule (from, to, radius or radii [r0, r1] for a taper, or points [[x, y, z, r], ...] for a bent limb) | torus (center, radius to the ring's middle, thickness), rotate?: [x, y, z] degrees about the centre (x, then y, then z; not a capsule), op?: union (default) | subtract | intersect, k?: this shape's smoothing in cells, palette?}. A whole-number point is the centre of that cell. union joins the shape with a smooth minimum, subtract carves it softly out of what came before (a mouth groove, eye sockets, a hollow), intersect keeps only what is inside it. k is the fillet width in cells: 0 is the hard union of fill_ellipsoid calls, 2 to 6 melts a head into a body. A cell is filled when the blended distance at its centre is at most grow. Colour: each union shape's palette (or the call's palette); colors nearest (default) gives a filled cell the slot of the shape it is deepest inside, so seams fall halfway through the fillets, last the last listed shape whose own body holds it (eyes, buttons, and spots listed after the body), and seam dithers the boundary between two slots over that many cells. A subtract or intersect shape with a palette paints the cells along its cut (a dark mouth line). Only the shapes' box grown by k is evaluated (at most 16000000 cells; pass x0..z1 to clip it), and at most 2000000 cells are filled. Writes as mode says: replace, empty, paint, or intersect (erase the solid cells outside the blend). One undo step. Replies with cells, changed, shapes, evaluated, and bounds.", json!({
            "shapes": {"type": "array", "items": {"type": "object"}, "description": "1..=256 shapes in order: {shape: sphere | ellipsoid | box | capsule | torus, center?, radius?, radii?, size?, round?, from?, to?, points?, thickness?, rotate?, op?: union | subtract | intersect, k?, palette?}. The first is a union."},
            "k": with_default(num_prop("Default smoothing radius in cells for every shape that leaves k out, 0..=64. 0 is a hard union. Defaults to 3."), json!(3)),
            "palette": int_prop("Default palette slot of union shapes that leave palette out, 0..255 (0 erases)."),
            "grow": with_default(num_prop("Fill where the blended distance is at most this, -16..=16 cells: above 0 thickens the whole form, below 0 thins it. Defaults to 0."), json!(0)),
            "colors": {"type": "string", "enum": ["nearest", "last"], "description": "nearest (default): the slot of the union shape the cell is deepest inside. last: the last listed union shape whose own body holds the cell, else nearest."},
            "seam": with_default(num_prop("With colors nearest: dither the boundary between two shapes' slots over this many cells, 0..=32. Defaults to 0, a sharp boundary."), json!(0)),
            "seed": int_prop("Seed of the seam dither. Defaults to 0."),
            "x0": int_prop("Optional inclusive box that clips the evaluation; pass all six corners or none."),
            "y0": int_prop("Box corner"),
            "z0": int_prop("Box corner"),
            "x1": int_prop("Box corner"),
            "y1": int_prop("Box corner"),
            "z1": int_prop("Box corner"),
            "mode": {"type": "string", "enum": ["replace", "empty", "paint", "intersect"], "description": "replace (default) writes every cell of the blend; empty only empty cells; paint recolours only solid cells; intersect erases the solid cells outside the blend and keeps the rest as they are."}
        }), &["shapes"]),
        define("loft", "Fill a solid along a path of 2..=1024 local points whose cross-section changes from point to point, for trunks, roots, limbs, and branches that taper, bend, and are egg-shaped and knobbly in one call instead of a round fill_tube. Each point's section is an ellipse with rx (half-width across the path) and ry (half-height along the frame's normal, up for a level path), turned by roll degrees, cut flat by flatTop (0 none, 1 halves it at the path; negative flattens the bottom, for a log lying on the ground), with an outline noise of noise (a fraction of the radius, 0..=0.9) and about frequency lumps around. Sections run linearly between the points; a value a point leaves out takes the call's own (radius, rx, ry, roll, flatTop, noise, frequency). curve smooth passes a Catmull-Rom curve through the points; polyline (default) joins them straight. caps flat (default) cuts the ends square, round rounds them. A cell is in when its centre is inside the section at the nearest point of the path. Colour with palette, or palettes and a pattern as fill_tube: rings along the length every period cells, stripes in sectors around it, random per cell. seed picks the outline noise and the random pattern. Writes as mode says. At most 2000000 cells. One undo step. Replies with cells and length.", merged(json!({
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Path points: [x, y, z], [x, y, z, r], [x, y, z, rx, ry], [x, y, z, rx, ry, roll], or {x, y, z, radius?, rx?, ry?, roll?, flatTop?, noise?, frequency?}. Radii 0.5..=128; a whole-number point is a cell centre."},
            "radius": num_prop("Default rx and ry of points that leave them out."),
            "rx": num_prop("Default half-width across the path."),
            "ry": num_prop("Default half-height. Defaults to rx."),
            "roll": with_default(num_prop("Default roll of the section about the path, in degrees. Defaults to 0."), json!(0)),
            "flatTop": with_default(num_prop("Default flat side, -1..=1: above 0 cuts the top at ry * (1 - flatTop), below 0 the bottom. Defaults to 0."), json!(0)),
            "noise": with_default(num_prop("Default outline noise as a fraction of the radius, 0..=0.9. Defaults to 0."), json!(0)),
            "frequency": with_default(num_prop("Default lumps around the outline, 0..=64. Defaults to 5."), json!(5))
        }), json!({
            "curve": str_prop("polyline (default) or smooth"),
            "caps": {"type": "string", "enum": ["flat", "round"], "description": "flat (default) or round ends."},
            "palette": int_prop("Palette index, or pass palettes"),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices laid out by pattern"},
            "pattern": {"type": "string", "enum": ["rings", "stripes", "random"], "description": "rings (default) along the length, stripes around it, random per cell"},
            "period": with_default(num_prop("Ring length in cells, 0.5..=512. Defaults to 2."), json!(2)),
            "sectors": int_prop("Stripes around, 1..=256. Defaults to the number of palettes."),
            "seed": with_default(int_prop("Seed of the outline noise and the random pattern. Defaults to 0."), json!(0)),
            "mode": mode_prop()
        })), &["points"]),
        define("brush", "Sculpt the active object's surface with brush dabs along a path of points, like Blender's clay and grab brushes, to refine a silhouette after the rough form. op inflate pushes the surface out by up to strength x radius / 2 cells at each dab's centre; deflate (or dig) pushes it in the same way; smooth blurs solid and empty cells so bumps and pits melt (strength sets the passes); flatten cuts bumps and fills pits toward the mean plane of the surface under the dab; grab (or drag) moves the cells under each dab by vector cells times strength, with the falloff, so it pulls a branch stub out or bends a lip over. Each point is [x, y, z], [x, y, z, radius], or [x, y, z, radius, strength]; points leave out radius (default 3, 0.5..=64) and strength (0..=1, default 0.5, grab 1). Dabs are laid between points every spacing cells (default half the radius, 0 none; grab lays one per point), and each reads what the dabs before it left. falloff is smooth (default), linear, or constant. only limits the brush to cells of those slots: other cells never change and do not grow. Added cells take the nearest solid cell's slot, or palette. One undo step. Replies with dabs, added, and removed.", json!({
            "op": {"type": "string", "enum": ["inflate", "deflate", "dig", "smooth", "flatten", "grab", "drag"], "description": "The brush."},
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Dab centres in local cells: [x, y, z], [x, y, z, radius], [x, y, z, radius, strength], or {x, y, z, radius?, strength?}."},
            "radius": with_default(num_prop("Default dab radius in cells, 0.5..=64. Defaults to 3."), json!(3)),
            "strength": with_default(num_prop("Default strength, 0..=1. Defaults to 0.5, or 1 for grab."), json!(0.5)),
            "falloff": {"type": "string", "enum": ["smooth", "linear", "constant"], "description": "How a dab fades to its radius. Defaults to smooth."},
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices the brush may change; other cells stay. Omit for all."},
            "palette": int_prop("Slot of added cells. Defaults to the nearest solid cell's."),
            "vector": vec3_prop("grab: [dx, dy, dz] cells a dab's centre moves at strength 1; at most the radius."),
            "spacing": num_prop("Most cells between dabs, 0..=256; 0 lays one per point. Defaults to half the radius.")
        }), &["op", "points"]),
        define("carve_from_views", "Fill the visual hull of 2 to 6 orthographic views of a concept's turnaround sheet into the active object: each view's silhouette is extruded through a box along its depth axis and the cells every view covers are kept, so the rough form of a front, side, and top view becomes 3D in one call. With a depth map per view (depth, depthPath) the cells in front of each view's depth surface are carved away too (space carving with depth), so a round head or a dome comes out round instead of a box; one view with depth is enough for a relief. A view's depth is a grey depth map of the same crop (white near, black far; a PNG path, true for the crop of the top-level depthPath, or {path | values (rows of numbers 0..1, null background), crop (defaults to the view's), background (with a colour image every pixel holds a value and the silhouette is the colour image's; without one the map keys its background as a view does; none takes every pixel), invert (black near), gamma (value^gamma first; auto picks the curve whose calibration meets the silhouette contour, where a round body turns away at the hull's middle, for a map not linear in depth such as Depth Anything's), near and far (cells from the camera side of the box where white and black land), calibrate (hull, the default: each image row's nearest pixel is placed where the silhouette hull is nearest in that row, measured by the view looking along the row, so a sphere's rim lands on its middle plane; box: the map's range spans the box, for a relief, and what hull falls back to when no view measures the depth), back (mirror the surface about the hull's middle for the far side, when the opposite view has no map), regions (calibrate each part of the map on its own: one region per peak, apart where the grey jumps or dips between peaks by more than edge, default 0.06; for a map that normalises each part like shading, as an image generator draws it), detail (a second map, a path or {path | values, crop, invert, gamma, edge}: its regions give the curvature inside each part and this map their order and offsets, so a monocular model's map, right in order but flat inside each part, and a generated map, round but wrong in order, make one surface)}). Coloured pixels (a magenta background and its fringe) hold no depth, and a ray reads the median of its footprint, so outlines and specks do not move it. Depth is relative, from an image generator's depth pass or a monocular model such as Depth Anything V2 (grayscale output, white near): leave calibrate at hull and give the views the colour sheet's crops; gamma auto for Depth Anything, regions for a generated map, or Depth Anything as the map and the generated one as its detail; pass near and far only when the scale is known. A cell stays when its centre is at or behind every depth surface, less depthTolerance cells (default 0). Then shape it with carve, smooth, and set_voxels, and check it with compare_silhouette. Views are seen as model_view names them, for a model facing -Y with +Z up: front from -Y (image right is +X), back from +Y, left (also side) from +X (image right is +Y, so the model's front is on the image's left), right from -X, top from +Z (image down is -Y), bottom from -Z; flip mirrors an image. A view is a PNG path with a crop [x, y, width, height] in pixels (several views can share one sheet), or rows of characters with a legend. Its background rule: auto (default: transparent pixels if the crop has any, else the commonest colour of the image border within tolerance), alpha, or a key colour; then the mask edits of compare_silhouette clean it (excludeColors, excludeTolerance, flood, mask, erase, keepLargest; top level for every view except mask and erase; flood and mask need a path). cellsPerPixel (per view or for all) or size [across, down] maps a view to cells; size [x, y, z] sets the box and stretches any view without its own scale. A view smaller than the box is centred across and stands on the box's bottom. A cell is in a view when at least coverage (default 0.5) of its footprint is inked. Colour: each cell takes the pixel of the view whose visible surface is nearest, quantised to the nearest of slots (default every slot), or palette gives one slot. shell keeps only cells within that many cells of the outside (a hollow log); smooth runs that many smooth passes. Writes as mode says; mode intersect instead keeps only the object's solid cells inside the hull, as they are, and erases the rest, so a blockout is cut to the views without refilling it. At most 4000000 cells in the box. One undo step. A top-level path, background, tolerance, and alphaThreshold apply to every view without its own; a view with depth and no path or rows takes its silhouette from the depth map (then pass palette, or give another view a colour image). Replies with cells, bounds {min, max}, size, and each view's grid, covered cells, and depth {calibrate, near, far, gamma, rim, pairs and residual (cells) for hull, regions and regionResidual with regions or detail, carved}.", merged(merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "2 to 6 views (1 with depth), one per side: {view: front | back | left | side | right | top | bottom, path (a PNG) | rows (strings; . and space are empty) + legend ({char: palette index or #rrggbb}), crop?: [x, y, width, height], cellsPerPixel? | size?: [across, down], background?: auto | alpha | #rrggbb | [r, g, b], tolerance? (RGB distance, default 40), alphaThreshold? (default 128), excludeColors?, excludeTolerance?, flood?, mask?, erase?, keepLargest? (as compare_silhouette), flip?, depth?: true | path | {path | values, crop, background, invert, gamma (number or auto), near, far, calibrate: hull | box, back, regions, edge, detail}}"},
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth from this image at the view's crop."),
            "depthTolerance": num_prop("Cells in front of a depth surface that still stay, 0..64. Defaults to 0."),
            "size": {"type": "array", "items": {"type": ["integer", "null"]}, "minItems": 3, "maxItems": 3, "description": "Box [x, y, z] in cells, 1..2048 each; null takes the largest view along that axis."},
            "origin": ivec3_prop("Local cell of the box's minimum corner. Defaults to [0, 0, 0]."),
            "cellsPerPixel": num_prop("Cells per image pixel for every view without its own size or cellsPerPixel, such as 0.25 for 4 pixels a cell."),
            "palette": int_prop("One palette index for every cell, instead of the views' colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices image colours quantise to. Defaults to all of 1..255. A legend's palette index is used as it is."}
        }), json!({
            "downscale": {"type": ["boolean", "object"], "description": "Pixelate every view first at the cell grid, as pixelate_views does (true, or its options {colors, coverage, thin, features, lines, outline, mergeLines, accents, minShare, contrast, maxAccents}), so a small creature keeps its eyes, teeth, spots, and thin legs instead of averaging them away; the box is size (at least one axis; the others follow the views' proportions). Not with depth maps. The reply adds downscale (size, colors, legend, and per view grid, inked, thin, lines, and features)."},
            "coverage": num_prop("Share of a cell's footprint in a view that must be inked, 0..1. Defaults to 0.5."),
            "shell": int_prop("Keep only cells within this many cells of the outside, 1..64: a hollow shell. Omit for solid."),
            "smooth": int_prop("Passes of smooth over the hull, 0..64 (fillAt 17, removeAt 9). Defaults to 0."),
            "mode": fill_mode_prop()
        })), mask_edit_props(true)), &["views"]),
        define("project_paint", "Recolour the active object's existing surface from ONE image projected along a view direction, such as a shell's plate pattern from a top view or face markings from the front: cells are never added or removed. The image is a PNG path with a crop [x, y, width, height], or rows of characters with a legend, with the background rule of carve_from_views (auto, alpha, or a key colour; alpha on an opaque image inks every pixel) and its mask edits (excludeColors, flood, mask, erase, keepLargest); background pixels leave the surface alone. view is front, back, left (also side), right, top, or bottom, seen as model_view and carve_from_views name them (front from -Y with image right +X, left from +X with image right +Y, top from +Z with image down -Y); or yaw and pitch in degrees as preview takes them (yaw 0 looks from +X, -90 from the front, pitch 90 from above) for an oblique projection. flip mirrors the image. The box (origin and size [x, y, z], defaulting to the object's bounds) limits which cells are painted, and the image is placed on the box's projection as a carve_from_views view is placed in its box: stretched to it, or at cellsPerPixel or imageSize [across, down] cells, centred across and standing on the bottom (an oblique view is centred both ways). So the same sheet crop and cellsPerPixel used to carve lands on the same cells. Each inked grid cell casts parallel rays: depth (default 1) paints the solid cells within that many cells of the first one each ray meets, so nothing behind the visible surface changes; cells outside the box or the only slots still hide what is behind them. normalLimit (degrees, 0..90) skips a surface whose normal is further than that from the view direction, so steep sides get no streaks. Colours quantise to the nearest of slots (default every slot), a legend's palette index is used as it is, or palette gives one slot to every inked pixel (a mask). only limits repainting to cells of those slots. A cell under several pixels takes the commonest. One undo step. Replies with cells (painted), changed, bounds, origin, size, grid, covered (inked grid cells), rays (inked rays that met the object), and steep (rays skipped by normalLimit).", merged(merged(json!({
            "view": {"type": "string", "enum": ["front", "back", "left", "side", "right", "top", "bottom"], "description": "The side the image looks from. Or pass yaw and pitch."},
            "yaw": num_prop("Degrees about +Z, as preview takes it: 0 looks from +X, -90 from the front (-Y). Instead of view."),
            "pitch": num_prop("Degrees above the horizon, -90..90; 90 looks straight down. Instead of view."),
            "path": str_prop("The image PNG."),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height] of the image in pixels. Defaults to the whole image."},
            "rows": {"type": "array", "items": {"type": "string"}, "description": "The image as rows of characters instead of path; . and space are background."},
            "legend": {"type": "object", "description": "rows: {char: palette index or #rrggbb}."},
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "flip": {"type": "boolean", "description": "Mirror the image left to right."},
            "origin": ivec3_prop("Local cell of the box's minimum corner. Defaults to the object's."),
            "size": {"type": "array", "items": {"type": ["integer", "null"]}, "minItems": 3, "maxItems": 3, "description": "Box [x, y, z] in cells, 1..2048 each; null (or leaving it out) reaches the object's far side."},
            "cellsPerPixel": num_prop("Cells per image pixel, such as 0.25 for 4 pixels a cell. Defaults to stretching the image over the box."),
            "imageSize": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2, "description": "[across, down] cells the image spans, instead of cellsPerPixel."}
        }), json!({
            "palette": int_prop("One palette index for every inked pixel, instead of the image's colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices image colours quantise to. Defaults to all of 1..255."},
            "coverage": num_prop("Share of a grid cell's footprint that must be inked, 0..1. Defaults to 0.5."),
            "depth": int_prop("Solid cells painted per ray from the visible surface, 1..64. Defaults to 1."),
            "normalLimit": num_prop("Degrees, 0..90: leave a surface whose normal is further than this from the view direction. Omit for no limit."),
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices that may be repainted; other cells keep their slot. Omit for all."}
        })), mask_edit_props(false)), &[]),
        define("paint_from_views", "Recolour EVERY surface cell of the active object (or of the objects in objects, together, each hiding what is behind it from the others) from a whole orthographic views sheet in one call, so an image-to-3D shape that is good from the front and smeared on its hidden sides takes the approved sheet's colours on every side. Cells are never added or removed. views is a list of {view, path | rows, crop, legend, flip, background, ...} as carve_from_views takes them (front, back, left or side, right, top, bottom; sheet-wide path, background, tolerance, and mask edits at the top level); over MCP, a top-level path with no views reads the sheet's 2 x 2 layout as measure_sheet does. Each view's ink bounds (fit ink, the default; fit crop uses the whole crop) are stretched over the painted objects' box as that view sees it, so the sheet and the shape need the same proportions, not the same size. A surface cell is painted only from views that see it: it is the first solid cell of its column from that side, so a flipper never paints the shell behind it. It takes the colour of its footprint in the view its outward normal faces most; blend nearest (default) blends the two best views linearly within seam degrees (default 12, 0 for hard seams) of the boundary between them, near 45 degree normals; blend soft weights every view that sees it by max(0, normal . view) to the power sharpness (default 4). Colours are read inset pixels (default 2, 0 for rows) inside each silhouette's edge, so anti-aliased background fringes never land on the model, and a footprint outside that ink takes the nearest ink within reach cells (default 3). mirror (default true) makes a missing right view from the left one mirrored (and the other way). Colours are mixed in Oklab, then quantised: into slots (palette indices), into newSlots N new colours clustered from the painted ones (median cut and k-means in Oklab, in free slots; an existing slot of exactly the colour is reused) with an accent guard that keeps up to maxAccents (default 4) small distinct colours (eyes, a mouth) their own slot when they hold at least minShare (default 0.002) of the cells and lie further than contrast (Oklab, default 0.12) from every cluster (accents false turns it off), or by default into the slots the object already uses. A box x0..z1 and only (slots) limit which cells are painted. Cells no view sees with ink (a belly with no bottom view) keep their slot. One undo step. Replies with painted, surface, unseen, blended, views (per view: visible, used, snapped, missed, mirrored), slots (slot and cells), newSlots (slot and rgba), accents, changed, objects, and voxels.", merged(json!({
            "views": {"type": "array", "items": {"type": "object"}, "description": "1..6 views: {view, path | rows, crop?, legend?, flip?, background?, tolerance?, alphaThreshold?, mask edits}. Leave out with a top-level path to use the sheet's measure_sheet layout."},
            "path": str_prop("The sheet PNG every view reads unless it has its own path."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "objects": {"type": "array", "items": {"type": ["string", "integer"]}, "description": "Objects to paint together, by name or index. Defaults to the active object."},
            "blend": {"type": "string", "enum": ["nearest", "soft"], "description": "nearest (default): the view the normal faces most, blended within seam of a boundary; soft: every view that sees the cell, by facing."},
            "seam": num_prop("nearest: degrees around the boundary between the two best views where they blend, 0..45. Defaults to 12."),
            "sharpness": num_prop("soft: the power of the facing weight, 0..64. Defaults to 4."),
            "fit": {"type": "string", "enum": ["ink", "crop"], "description": "What of each view maps onto the box: ink (default) its ink bounds, crop the whole crop."},
            "reach": num_prop("Cells: a footprint off a view's ink takes the nearest ink within this, 0..64. Defaults to 3."),
            "inset": int_prop("Pixels: colours are read this far inside each silhouette's edge, so anti-aliased background fringes never land on the model, 0..16. Defaults to 2 (0 when every view is rows)."),
            "mirror": {"type": "boolean", "description": "Make a missing right view from the left one mirrored, and the other way. Defaults to true."},
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices colours quantise to. Defaults to the slots the painted objects use."},
            "newSlots": int_prop("Instead of slots: up to this many new colours (1..64) clustered from the painted ones, in free slots."),
            "accents": {"type": "boolean", "description": "newSlots: keep small distinct colours their own slot. Defaults to true."},
            "minShare": num_prop("Accent guard: share of the painted cells an accent needs, 0..1. Defaults to 0.002."),
            "contrast": num_prop("Accent guard: Oklab distance from every cluster an accent needs. Defaults to 0.12."),
            "maxAccents": int_prop("Accent guard: most accents kept beyond newSlots, 0..16. Defaults to 4."),
            "downscale": {"type": ["boolean", "object"], "description": "Pixelate every view first at the cell grid, as pixelate_views does (true, or its options {colors, coverage, thin, features, lines, outline, mergeLines, accents, minShare, contrast, maxAccents}), so a small creature keeps its eyes, teeth, spots, and thin legs instead of averaging them away; the grid is the painted objects' box, and each view is then read whole (fit crop, inset 0). The reply adds downscale (size, colors, legend, and per view grid, inked, thin, lines, and features)."},
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices that may be repainted. Omit for all."},
            "x0": int_prop("Box corner of the painted cells (all six or none)."), "y0": int_prop("Box corner."), "z0": int_prop("Box corner."),
            "x1": int_prop("Opposite box corner."), "y1": int_prop("Opposite box corner."), "z1": int_prop("Opposite box corner.")
        }), mask_edit_props(true)), &[]),
        define("shell_plates", "Paint a turtle-shell-like plate pattern over a dome-shaped surface in one call, instead of drawing every seam with surface_stroke: a central plate, rings of plates (rings: plates per ring, from the centre out), an outer band of plates (band: its share of the radius, bandCount plates, 0 for one unbroken band), and a rim (rim: cells at the edge), each kind with its slots (palettes {central, ring, band, rim}: one slot or a list, each plate picking one by seed), split by seams seamWidth cells wide in the seam slot, or cut seamDepth cells deep with seam 0. It works on the surface, not a view: the cells taken are the surface cells whose normal is within facing degrees of straight up (default 80), optionally in a box x0..z1, a zRange [z0, z1], and the only slots; each one's place is its distance along the surface from the apex (the highest cell near center, default the middle of the taken cells in plan) over that distance out to the dome's edge in its direction, so rings keep their width down the steep sides of a tall or oval dome and nothing streaks as a top projection does. layout rings cuts each ring into sectors with jittered boundaries, staggered ring to ring; layout voronoi gives each cell the nearest of seeds laid out in the rings, for rounder, irregular plates. jitter (0..1, default 0.25) moves boundaries and seeds; symmetric (default true) mirrors them across the plane through the centre along y, so a shell facing -Y has matching left and right plates; seed changes them. depth paints plates that many cells in along the normal. Nothing is added; a groove erases. One undo step. Replies with cells, changed, taken, unreached (taken cells not joined to the apex along the surface, left alone), plates, seams, apex, kinds {central, ring, band, rim} (surface cells of each), and voxels.", json!({
            "palettes": {"type": "object", "description": "{ring: slot or [slots], seam: slot (0 cuts a groove), central?: defaults to ring, band?: defaults to ring, rim?: defaults to band}."},
            "layout": {"type": "string", "enum": ["rings", "voronoi"], "description": "rings (default): sectors per ring; voronoi: nearest seed."},
            "rings": {"type": ["array", "integer"], "items": {"type": "integer"}, "description": "Plates per ring from the centre out, 1..64 each, at most 8 rings. Defaults to [6]."},
            "central": num_prop("Share of the radius the central plate spans, 0..0.9; 0 for none. Defaults to 0.22."),
            "band": num_prop("Share of the radius the outer band spans, 0..0.6; 0 for none. Defaults to 0.15."),
            "bandCount": int_prop("Plates in the band, 0..64; 0 is one unbroken band. Defaults to twice the last ring's."),
            "rim": num_prop("Rim width in cells along the surface at the edge, 0..32. Defaults to 1."),
            "center": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "Plan centre [x, y]. Defaults to the middle of the taken cells."},
            "seamWidth": num_prop("Seam width in cells, 0.5..6. Defaults to 1."),
            "seamDepth": int_prop("Cells a seam reaches in (the groove's depth with seam 0), 1..8. Defaults to 1."),
            "depth": int_prop("Cells a plate's slot reaches in along the normal, 1..8. Defaults to 1."),
            "facing": num_prop("Degrees from straight up a taken cell's normal may be, 0..180. Defaults to 80."),
            "zRange": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2, "description": "Inclusive heights [z0, z1] of the taken cells."},
            "x0": int_prop("Box corner of the taken cells (all six or none)."), "y0": int_prop("Box corner."), "z0": int_prop("Box corner."),
            "x1": int_prop("Opposite box corner."), "y1": int_prop("Opposite box corner."), "z1": int_prop("Opposite box corner."),
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Only cells of these slots are taken."},
            "jitter": num_prop("How far boundaries and seeds move, 0..1. Defaults to 0.25."),
            "symmetric": {"type": "boolean", "description": "Mirror the layout across the plane through the centre along y. Defaults to true."},
            "seed": int_prop("Seed of the jitter and of each plate's slot pick. Defaults to 1.")
        }), &["palettes"]),
        define("decal", "Lay ONE picture (an eye, a mouth, a badge, a marking) onto the active object's curved surface around a point, so it keeps its size and shape on a round head instead of stretching on the slopes as project_paint's parallel projection does: the surface is unrolled around the point (a discrete exponential map, following the surface's normal and the distance along it), and each surface cell takes the picture's colour under it. Cells are never added or removed. The picture is a PNG path with a crop or rows with a legend, with the background rule of carve_from_views; background pixels leave the surface alone; flip mirrors it. point [x, y, z] (a whole number is a cell's centre) snaps to the nearest surface cell within reach (default 8), or ray {origin, direction} takes the first solid cell it meets (aim from where the camera is). size is the width in cells along the surface (the height follows the picture's aspect) or [across, down]. up (default [0, 0, 1]) is the picture's up, laid onto the surface at the point; image right is then up x outward normal, so a face looking along -Y with up +Z has image right +X, as the front view. depth (default 1, at most 16) paints that many cells into the solid along the normal. normalLimit (degrees, default 90) stops the decal where the surface turns further than that from the point's normal, so it never wraps round a thin part. Colours quantise to the nearest of slots (default every slot), a legend's palette index is used as it is, or palette gives one slot to every inked pixel. only limits repainting to cells of those slots. mirror paints the mirrored picture on the far side of a plane too (a left eye for a right one). One undo step. Replies with cells, changed, bounds, centre (the surface cell), normal, grid, covered (inked grid cells), missed (inked grid cells no surface cell landed on: the decal ran off the surface or past normalLimit), reached (surface cells unrolled), and voxels.", json!({
            "point": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "Local [x, y, z] near the surface; the decal centres on the nearest surface cell within reach."},
            "ray": {"type": "object", "description": "{origin: [x, y, z], direction: [x, y, z]}: the decal centres on the first solid cell the ray meets. Instead of point."},
            "size": {"type": ["number", "array"], "description": "Cells across along the surface, 1..256 (the height follows the picture), or [across, down]."},
            "up": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "The picture's up direction. Defaults to [0, 0, 1]."},
            "path": str_prop("The picture PNG."),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height] of the image in pixels. Defaults to the whole image."},
            "rows": {"type": "array", "items": {"type": "string"}, "description": "The picture as rows of characters instead of path; . and space are background."},
            "legend": {"type": "object", "description": "rows: {char: palette index or #rrggbb}."},
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "flip": {"type": "boolean", "description": "Mirror the picture left to right."},
            "palette": int_prop("One palette index for every inked pixel, instead of the picture's colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices picture colours quantise to. Defaults to all of 1..255."},
            "coverage": num_prop("Share of a grid cell's footprint that must be inked, 0..1. Defaults to 0.5."),
            "depth": int_prop("Cells painted into the solid along the normal, 1..16. Defaults to 1."),
            "reach": num_prop("How far point may be from the surface, 0..64 cells. Defaults to 8."),
            "normalLimit": num_prop("Degrees, 1..180: the decal stops where the surface turns further than this from the point's normal. Defaults to 90."),
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices that may be repainted; other cells keep their slot. Omit for all."}
        }), &["size"]),
        define("orient_to_views", "Turn the active object, such as an image-to-3D mesh fresh from import_mesh that came in lying on its side or facing anywhere, to the turn whose silhouettes best match a views sheet, and scale it, instead of guessing rotation and size by eye. views are 1 to 6 views, one per side, as fit_to_views takes them (path + crop + background rule, or rows + legend; flip mirrors; weight, default 1); a top-level path, background, tolerance, and alphaThreshold apply to every view without its own, and a top-level path with no views reads the sheet's 2 x 2 layout as measure_sheet does. Each candidate turn projects the object's surface cells onto each view's image axes (as model_view draws them, the model facing -Y with +Z up), fits the view's mask over the projection with its own scale and offset (as compare_silhouette's fit), and scores the weighted mean IoU. The 24 axis-aligned turns are tried (turns upright: only the 4 about up), then, with yawStep, fine steps about up (and about x and y with fineAxes xyz, for a mesh tilted off its axes) within yawRange degrees (default 45) of the best, one axis at a time. The object is then resampled to that turn (each new cell takes the nearest source cell's slot), scaled to size cells on its longest axis, height cells tall, or by scale (default 1), with its bottom centre kept; apply false only reports. Check margin (the best score minus the second's): a near tie between turns means the views do not tell them apart (a front and back that look alike), so add a view or check the reply's candidates. import_mesh's orient option runs the same search on the mesh before it is voxelized (with upright: true, after standing it level by its principal axes). One undo step. Replies with before and after {score, views [{view, iou}]}, rotation {euler (degrees about x, then y, then z, as import_mesh rotation takes them), axes (where the object's x, y, and z went), yaw, fine (the fine degrees about x, y, and z), matrix}, candidates (the 6 best axis-aligned turns), margin, scale, applied, cells, bounds, changed, and voxels.", json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "1 to 6 views: {view: front | back | left | side | right | top | bottom, path (a PNG) | rows + legend, crop [x, y, width, height], background, tolerance, alphaThreshold, flip, weight}."},
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "turns": {"type": "string", "enum": ["all", "upright"], "description": "all (default): the 24 axis-aligned turns; upright: the 4 about up, for an object already standing on its base."},
            "yawStep": with_default(num_prop("Degrees between fine yaw steps about up around the best turn, 0..=90. Defaults to 0, none."), json!(0)),
            "yawRange": with_default(num_prop("Fine steps go this many degrees each way, 0..=180. Defaults to 45."), json!(45)),
            "fineAxes": str_prop("Axes of the fine steps: z (default, yaw about up), or xyz to also tilt about x and y, for a mesh tilted off its axes."),
            "resolution": int_prop("Longest side of the projected model masks in pixels, 8..=512. Defaults to 64."),
            "size": num_prop("Cells along the longest axis after the turn, 1..=256. Instead of height or scale."),
            "height": num_prop("Cells along up after the turn, 1..=256. Instead of size or scale."),
            "scale": num_prop("Scale factor, above 0 and at most 16. Defaults to 1, the turn alone."),
            "apply": bool_prop("Turn and scale the object (default true); false only reports the best turn.")
        }), &[]),
        define("fit_to_views", "Refine the active object so its orthographic views match an approved views sheet, with colours from a palette: the finishing pass after a carve_from_views or import_mesh blockout (inspired by Voxify3D, CVPR 2026; a CPU discrete search, no neural network). views are 1 to 6 views, one per side, given as carve_from_views takes them (path + crop + background rule, or rows + legend; flip mirrors); a top-level path, background, tolerance, and alphaThreshold apply to every view without its own. Rays are orthographic and parallel: one view pixel is one column of cells. Each crop is placed once over the object's current projection with compare_silhouette's align (fit by default: the scale and offset with the most overlap; height, width, or {scale, offset}, top level or per view), then sampled to one target per cell column (inside when coverage of its footprint is inked, and the mean colour). Per view pixel the energy is maskWeight when the target is inside and nothing covers it, or the target is background and a cell does; depthWeight * min(|d - dRef|, 8) / 8 against a reference object's front surface; colorWeight * min(RGB distance / 255, 1) between the first visible cell's slot and the target; plus smoothWeight per exposed cell face. Shape moves (shape, default on) run first on mask, depth, and smoothness only: excess rays are cleared, missing rays filled with cells every other view sees inside (a run, or one cell touching the body), a covered ray's front moved to the reference's, and rough single cells removed or filled; each kept only if it lowers the energy, in sweeps shuffled by seed until none is kept or iterations (default 8) run out. Then (recolor, default on) every cell a view sees takes the slot of slots (default: the object's own slots) nearest the colours of the views that see it and face it within normalLimit (default 60 degrees); each column's colour is the majority slot of its pixels, so thin outlines and cube bevels do not tint it (and lines under half a cell drop out). Views placed with fit share one scale (sharedScale). A view's color: false uses it for the shape only, and keep protects slots. Views of one feature drawn at different places on a boxy part (a face's eyes in the front and side views) give that feature twice: fit such a part with recolor false, or keep its face slots. Other visible objects cover and hide but never change unless isolate. reference (an object index or name, such as a mesh imported with import_mesh newObject) gives the depth target; a view's depth map (depth and depthPath as carve_from_views takes them, calibrated the same way against the hull of the views' targets) gives that view's depth target instead, so a blockout's front follows the sheet's depth pass. Deterministic, one undo step. Three axis views cannot see a round form's diagonal corners, so the cells they leave ambiguous may follow a rounding prior (roundWeight, off by default; 0.3 for a round head; roundness; roundAxes): superellipses inscribed in each cross-section, or the reference object when one is given, and cells outside it go wherever no silhouette needs them. Replies with views [{view, grid, align {scale, offset} (reuse it to keep the placement), before and after {iou, missing, excess, colorError, depthError with a reference or depth, pixels}, depth (its calibration)}], energy before and after, added, removed, recolored, changed, sweeps, moves, rounded, palette [{slot, cells}] after, origin and size of the box worked in, and voxels.", merged(merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "1 to 6 views, one per side: {view: front | back | left | side | right | top | bottom, path (a PNG) | rows + legend, crop [x, y, width, height], background (auto, alpha, #rrggbb, [r, g, b]), tolerance, alphaThreshold, excludeColors, excludeTolerance, flood, mask, erase, keepLargest (as compare_silhouette), flip, align, color (false: shape only, its colours ignored), depth (as carve_from_views: true | path | {path | values, crop, background, invert, gamma, near, far, calibrate, back, regions, edge, detail})}."},
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth target from it at the view's crop."),
            "depthTolerance": num_prop("Cells in front of a depth surface that still count as on it, 0..64. Defaults to 0."),
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "align": {"type": ["string", "object"], "description": "fit (default), height, width, or {scale, offset}: crop pixel p lands on cell column p * scale + offset. A view's own align overrides it."},
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices the colours may take. Defaults to the slots the object holds."},
            "reference": {"type": ["integer", "string"], "description": "Another object (index or name) whose front surface is the depth target, such as a mesh imported with import_mesh newObject. Omit for none."},
            "isolate": {"type": "boolean", "description": "Fit the active object alone; by default other visible objects cover and hide but never change."}
        }), json!({
            "coverage": num_prop("Share of a cell column's footprint that must be inked, 0..1. Defaults to 0.5."),
            "iterations": int_prop("Most shape sweeps, 0..64; 0 only recolours. Defaults to 8."),
            "seed": int_prop("Shuffles the order rays are visited in. Defaults to 1."),
            "margin": int_prop("Empty cells around the bounds the fit may fill, 0..64. Defaults to 15% of the largest extent, at least 4."),
            "maskWeight": num_prop("Cost of a missing or excess view pixel, 0..100. Defaults to 1."),
            "colorWeight": num_prop("Cost of a colour 255 RGB apart, 0..100. Defaults to 1."),
            "depthWeight": num_prop("Cost of a first cell 8 or more cells from the reference's or the depth map's, 0..100. Defaults to 0.5."),
            "smoothWeight": num_prop("Cost of each exposed cell face, 0..100; keeps lone spikes and pits out. Defaults to 0.1."),
            "shape": {"type": "boolean", "description": "Add and remove cells. Defaults to true."},
            "recolor": {"type": "boolean", "description": "Recolour the cells the views see. Defaults to true. Off for a part with a hand-set face, so only its shape is fitted."},
            "normalLimit": num_prop("Degrees, 0..90: a view recolours only cells whose surface faces it within this angle (a seen cell no view faces takes the view facing it most), so a feature does not wrap round a corner. Defaults to 60."),
            "keep": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices whose cells are never removed or recoloured (hand-set eyes); they still cover and hide."},
            "roundWeight": num_prop("Cost of keeping a cell outside the rounding prior, 0..100; 0 keeps the maximal hull. Each cross-section along each axis gets the superellipse inscribed in its bounds (with a reference: the reference itself), and cells outside it that no axis view needs (a box's diagonal corners, which three axis silhouettes cannot see) are removed, so a round head comes out round while every silhouette stays. Defaults to 0 (the maximal hull); 0.3 suits a round part such as a head, not a flat-bottomed dome or a plate, which lose their rim."),
            "roundAxes": str_prop("Axes whose cross-sections the rounding prior rounds, letters of xyz. Defaults to yz: front sections and the plan; an x section runs along a creature facing -Y, through its neck."),
            "roundness": num_prop("Superellipse exponent of the rounding prior, 1..16: 2 is an ellipse, larger is squarer. Defaults to 2.5."),
            "sharedScale": {"type": "boolean", "description": "Views placed with fit share one scale (the median), offsets refitted at it, as a sheet is drawn at one scale. Defaults to true."}
        })), mask_edit_props(true)), &["views"]),
        define("optimize_to_views", "Iterate a small creature (20 to 60 cells long) until its orthographic views match its pixel-art views, in the spirit of Voxify3D (CVPR 2026; an independent CPU solver, no neural network): the finishing pass after a carve_from_views, import_mesh, or hand-sculpted blockout, where one pass of paint_from_views gives streaky colours and lost eyes. The views are stretched over the active object's box, so a view pixel is exactly one column of cells: pass the rows and legend of pixelate_views (at the object's box) as views, or crops of a sheet (path + crop + background, or path alone for measure_sheet's layout), which are pixelated at the box first (downscale, default on for any view that is not rows; true or pixelate_views' options; false only for pixel art crops). A missing right view is the left one mirrored (mirror). Shape (shape, default on; up to iterations passes, default 8): an excess ray (the view empty, a cell covering it) has its column cleared, a missing ray gains one cell touching the body that every other view sees inside (nearest the neighbouring rays' depth, or a depth map's surface), and with a depth map per view (depth, depthPath, calibrated against the views' hull as carve_from_views does) cells in front of its surface go and a front behind it is filled; each move is kept only if it lowers the energy of the rays it touches (1 per missing or excess pixel, depthWeight * min(|front - depth| - depthTolerance, 4) / 4, and surfaceWeight per exposed face), stays within band cells (default 2) of the starting surface, keeps the object connected (connected), and leaves no cell thinner than minFeature (default 1, off) that was not already; colour never carves. Colour (recolor, default on): every surface cell takes the view colour's slot that best agrees with the views that see it (the first solid cell of a ray), each weighted by how squarely it faces the cell (max(0, n . view) ^ sharpness, default 2), plus smooth (default 0.6) times the share of its surface neighbours with another slot, solved by iterated conditional modes, so colour regions stay patches and a disagreeing speck joins its neighbours; a colour under accentShare (default 0.04) of the views' coloured pixels (eyes, pupils, nostrils) is an accent (accents false turns the guard off): a cell a view squarely paints with it keeps it, and no cell takes one no view paints there. Colours go into slots of exactly the views' colours (new ones made in free slots) or, with slots, the nearest of those. keep protects slots from removal and recolouring. Deterministic, one undo step. Replies with views [{view, grid, mirrored, before and after {agreement (pixels right in shape and colour over pixels either covers), iou, color (right colour over overlap), colorError (mean Oklab distance), depthError with a depth map, pixels}, depth (its calibration)}], agreement {before, after} (the mean over views), iterations, moves, added, removed, recolored, colorSweeps, surface, unseen (surface cells no view sees), accentCells, blocked {band, keep, connected, thin, gain, unreachable}, energy before and after, colours [{colour, slot, accent, pixels}], newSlots, downscale (the pixelation, without rows), origin, size, millis, changed, and voxels.", merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "1 to 6 views, one per side: {view: front | back | left | side | right | top | bottom, rows + legend (pixelate_views' reply) | path + crop + background rule, flip, color (false: shape only), depth (as carve_from_views)}. Leave out with a top-level path to use the sheet's measure_sheet layout."},
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "downscale": {"type": ["boolean", "object"], "description": "Pixelate the views at the object's box first, with pixelate_views' options (colors, accents, coverage, thin, features, lines, outline, mergeLines, despeckle). Defaults to true unless every view is rows."},
            "fit": str_prop("ink (default): each view's ink bounds map onto the box; crop: the whole rows grid. A downscaled view is always its grid."),
            "mirror": bool_prop("A missing right view is the left one mirrored, and the other way. Defaults to true."),
            "iterations": int_prop("Most shape passes, 0..64; 0 only recolours. Defaults to 8."),
            "shape": bool_prop("Add and remove cells. Defaults to true."),
            "recolor": bool_prop("Recolour the surface. Defaults to true."),
            "band": int_prop("Cells from the starting surface that may be added (outside) or removed (inside), 0..8. Defaults to 2."),
            "minFeature": int_prop("No removal leaves a cell thinner than this many cells (its shortest solid run along x, y, or z) unless it already was, 1..16. Defaults to 1 (off); 2 keeps legs and fins at least two cells."),
            "connected": bool_prop("No removal splits the object into more parts. Defaults to true."),
            "smooth": num_prop("Weight of the smoothness term against the views' vote, 0..4: higher merges colour patches more. Defaults to 0.6."),
            "sharpness": num_prop("Exponent of a view's facing weight, 0..16. Defaults to 2."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices the views' colours land in (nearest in Oklab). Defaults to a slot per view colour: an existing slot of exactly that colour, or a new one."},
            "keep": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices whose cells are never removed or recoloured (hand-set eyes)."},
            "accents": bool_prop("The accent guard (default true): small distinct colours keep their cells."),
            "accentShare": num_prop("Share of the views' coloured pixels under which a colour is an accent, 0..0.5. Defaults to 0.04."),
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth target from it at the view's crop."),
            "depthWeight": num_prop("Cost of a front 4 or more cells off the depth map's surface, 0..16. Defaults to 0.5."),
            "depthTolerance": num_prop("Cells off a depth surface that still count as on it, 0..16. Defaults to 0.5."),
            "surfaceWeight": num_prop("Cost of each exposed cell face, 0..4; keeps lone spikes out. Defaults to 0.05."),
            "colorCarve": bool_prop("Remove an edge cell two views see first and paint differently when every ray that sees it still meets the body and more of them then agree. Defaults to true (with shape and recolor)."),
            "semantic": {"type": ["boolean", "object"], "description": "The CLIP semantic term (a froxel built with the clip or clip-metal feature, and the CLIP ViT-B/32 weights, about 605 MB, at models/clip-vit-base-patch32.safetensors in Froxel's support folder or FROXEL_CLIP_MODEL): true or {weight (default 1), rounds (default 2, 0..8), patches (default 8), model (the weights file)}. After the colour step each view and the model's view are compared patch by patch (7 x 7 patches of CLIP's vision tower); per round the views' votes in the least similar patches weigh up to 1 + weight more for a recolouring, and in the patches least alike each view in turn shows its own colours there; each change is kept only when the mean similarity rises, so faces, eyes, and legs read as in the views. The reply adds semantic {scorer, start, before, after (mean similarity), views, rounds, kept, tried, patches, recolored, loadMs}."}
        }), mask_edit_props(true)), &[]),
        define("pixelate_views", "Turn an orthographic views sheet into pixel art at the cell grid a small creature (20 to 60 cells long) will be carved or painted at, before carve_from_views or paint_from_views read it, so its eyes, pupils, teeth, nostrils, spots, and thin legs and fins keep cells instead of averaging away into smears; writes no voxels. Content-adaptive downscaling (after Kopf, Shamir and Peers 2013 and Gerstner et al. 2012): one small palette shared by every view, colors clusters (default 6) by median cut and k-means in Oklab with the accent guard of paint_from_views (accents, minShare, contrast, maxAccents) so a small distinct colour such as an eye keeps its own; each cell takes the colour holding most of a centre-weighted kernel over its footprint (never a mean, so edges stay crisp and no in-between colour appears), with anti-aliased fringe pixels out of the vote; a cell is inked when coverage (default 0.5) of its footprint is, or, with thin (a share, default 0.25; false turns it off), when it lies on the midline of a part thinner than a cell; and features (default on; false, or {minArea (cells, default 0.15), maxArea (6), contrast (Oklab from the mean colour around it, 0.1), minCells (1)}) gives every small region of one colour that stands out from its surroundings at least minCells cells (about its area, up to 4), smallest first so a pupil keeps its place inside the eye; and lines (default on; false, or {length (cells a stroke's midline must run through a cell, default 0.6), contrast (Oklab from the cell's vote, 0.08)}) gives a drawn line thinner than a cell (an eye's ring, a mouth, a belly's edge), which loses every cell's vote to the fill around it, a line of cells in its colour; a patch of one colour that lines would take whole (a small mouth between its strokes) stays. The outline along the silhouette is kept as a line only with outline (default false: a dark rim on a model's edge cells is seldom wanted); without it the outline's pixels vote only where nothing else does, so no stray dark cell is left on the rim. With lines, mergeLines (an Oklab distance, default 0.2; false keeps them apart) merges the colours that live mostly in strokes and lie that close to the commonest stroke colour into it (an outline blended into a shadow clusters apart from it), so a view has one outline colour; the reply's mergedLines counts them. A white eye or tooth on a white sheet is keyed out as background by the background rule: fillHoles (true is 0.1, or a share of the view's ink) inks every enclosed background region up to that share again in its own colour. views are {view, path | rows, crop, legend, flip, background, ...} as carve_from_views takes them, or a top-level path alone reads the sheet's 2 x 2 layout as measure_sheet does. Each view's ink bounds become its grid: size [x, y, z] gives the box in cells (at least one axis; an axis left out or null follows the views' ink proportions), so size [null, 20, null] makes a creature 20 cells long. Replies with size, colors, accents, mergedLines, legend ({char: #rrggbb}), and views, each {view, rows, legend, grid, inked, thin (cells the thin rule kept), lines (cells a drawn line holds), source (the ink crop in pixels), stretch (the grid's aspect over the ink's: far from 1 the views disagree, such as a top view turned 90 degrees), features [{colour, area, cells, added, at [column, row]}]}: pass views straight to carve_from_views (with the same size) or paint_from_views as its views. png writes the views side by side at pngScale pixels a cell (default 8) for checking. carve_from_views and paint_from_views take the same options as downscale to do it in one call.", merged(json!({
            "views": {"type": "array", "items": {"type": "object"}, "description": "1..6 views: {view, path | rows, crop?, legend?, flip?, background?, tolerance?, alphaThreshold?, mask edits}. Leave out with a top-level path to use the sheet's measure_sheet layout."},
            "path": str_prop("The sheet PNG every view reads unless it has its own path."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "size": {"type": "array", "items": {"type": ["integer", "null"]}, "minItems": 3, "maxItems": 3, "description": "Box [x, y, z] in cells, 1..512 each; at least one axis, null follows the views' proportions."},
            "colors": int_prop("Clusters of the shared palette, 1..32. Defaults to 6."),
            "accents": {"type": "boolean", "description": "Keep small distinct colours (eyes) their own colour beyond colors. Defaults to true."},
            "minShare": num_prop("Accent guard: share of the ink an accent needs, 0..1. Defaults to 0.002."),
            "contrast": num_prop("Accent guard: Oklab distance from every cluster an accent needs. Defaults to 0.12."),
            "maxAccents": int_prop("Accent guard: most accents beyond colors, 0..16. Defaults to 4."),
            "coverage": num_prop("Share of a cell's footprint that must be inked, 0..1. Defaults to 0.5."),
            "thin": {"type": ["number", "boolean"], "description": "Least inked share for a cell on the midline of a part thinner than a cell (a fin, a leg), 0..1; false keeps only coverage. Defaults to 0.25."},
            "despeckle": {"type": "integer", "description": "Passes of a majority filter over each view's grid: a cell whose colour fewer than two of its eight inked neighbours share takes their commonest colour (feature cells are kept), 0..16; 0 turns it off. Defaults to 1."},
            "features": {"type": ["boolean", "object"], "description": "Keep small salient features: true (default), false, or {minArea, maxArea (cells of area, defaults 0.15 and 6), contrast (Oklab, default 0.1), minCells (1..16, default 1)}."},
            "lines": {"type": ["boolean", "object"], "description": "Keep drawn lines thinner than a cell (an eye's ring, a mouth) as a line of cells in their colour: true (default), false, or {length (cells a stroke's midline must run through a cell, 0..4, default 0.6), contrast (Oklab from the cell's vote, default 0.08)}."},
            "outline": {"type": "boolean", "description": "With lines, keep the outline along the silhouette too, as a rim of cells in its colour. Defaults to false: the outline's pixels then vote only where nothing else does."},
            "mergeLines": {"type": ["number", "boolean"], "description": "With lines, merge the colours that live mostly in strokes and lie within this Oklab distance of the commonest stroke colour into it, so an outline blended into a shadow is one colour, 0..1.5. Defaults to 0.2; false keeps them apart."},
            "png": str_prop("Write the pixelated views side by side to this PNG for checking."),
            "pngScale": int_prop("Pixels a cell in png, 1..64. Defaults to 8.")
        }), mask_edit_props(true)), &[]),
        define("measure_sheet", "Read a views sheet image before carving or fitting it, instead of measuring it with scripts; writes no file and changes nothing. The inked pixels (under the background rule and the mask edits of compare_silhouette) split into connected regions; regions whose boxes come within gap pixels merge, and regions under minShare of the largest are left out (listed in ignored). Each view's crop is its tight box [x, y, width, height], in image pixels. Four views in two rows and two columns take the layout names in reading order (front, side, top, back by default), and fitViews is then the views list carve_from_views and fit_to_views take with path. pitch is each view's cube size in pixels, from the autocorrelation of the brightness steps across cube seams (pitchX across, pitchY down, strength 0..1 each), refined to a fraction of a pixel from its multiples; cubes is the crop in cubes. checks compare front height with side height, front width with top width, and side length (width) with top length (height), in pixels and in cubes, each ok within checkTolerance percent (default 3); ok is all of them. parts (true, or {view, radius, minShare}) counts the pieces of one view (top by default) outside its body, the largest region left after an opening by radius pixels (default half the view's thickest point), each bigger than minShare and reaching more than 0.42 radius past it (a boxy body's rounded-off corners do not): flippers, legs, a head, and a tail each count once, with bounds and a compass direction from the body (n is image up), so a top view with six flippers shows.", merged(json!({
            "path": str_prop("The views sheet PNG."),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height]: read only this part of the image. Crops in the reply stay in image pixels."},
            "background": {"type": ["string", "array"], "description": "auto (default: transparent pixels, else the commonest border colour), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "gap": num_prop("Pixels between two regions' boxes that still merge them into one view. Defaults to 12."),
            "minShare": num_prop("A region under this share of the largest one's pixels is left out (labels, specks). Defaults to 0.02."),
            "layout": {"type": "array", "items": {"type": "string"}, "minItems": 4, "maxItems": 4, "description": "View names of a 2 x 2 sheet in reading order. Defaults to [front, side, top, back]."},
            "pitchRange": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "[low, high] cube pitch searched, in pixels. Defaults to [4, 96]."},
            "parts": {"type": ["boolean", "object"], "description": "Count the parts sticking out of a view's body: true (the top view) or {view (a layout name or an index), radius (opening radius in pixels), minShare (smallest part's share of the view, default 0.004)}."},
            "checkTolerance": num_prop("Largest difference, in percent, a check passes with. Defaults to 3.")
        }), mask_edit_props(true)), &["path"]),
        define("compare_silhouette", "Measure how closely the model reads like its concept from one camera, as numbers and a picture: draws the preview silhouette (the camera of preview: preset game, presets, placementYaw, view, yaw, pitch, orthographic, fov, margin, object, isolate, pose) and compares it with a mask cut from a concept PNG (crop and background rule as in carve_from_views). The concept is placed over the preview with one uniform scale and an offset, so its proportions are kept: align fit (default) finds the placement with the most overlap, height or width match that extent with bottoms and centres lined up, and {scale, offset} gives it (concept pixel p lands on output pixel p * scale + offset; reuse a reply's align to hold the placement still between edits). Replies with iou, missing (share of the concept the model does not cover), excess (share of the model outside the concept), pixel counts, align, both bounds and aspect ratios, and writes an overlay PNG to path: the preview with the concept's outline in cyan, missing areas in magenta, excess in orange, and the numbers in the corner. A hero view or a photo with water, splashes, or a rider in it scores nothing until its background is masked: excludeColors (with excludeTolerance) drops pixels near those colours, flood fills the background from the crop's border in small colour steps (a gradient or a photo), mask keeps only the pixels under a mask PNG's ink (invert drops them), erase drops rectangles in image pixels, and keepLargest keeps the largest connected region; the reply's maskEdits counts the pixels each removed. width and height default to 512 tall at the concept crop's aspect. Writes a file, so Rhai scripts do not have it.", merged(merged(json!({
            "path": str_prop("Overlay PNG to write. Its folder is created."),
            "concept": str_prop("Concept PNG path"),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height] of the concept view in pixels. Defaults to the whole image."},
            "background": {"type": ["string", "array"], "description": "The concept's background: auto (default), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance of a key colour, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below which a pixel is background, 1..255. Defaults to 128."),
            "flip": json!({"type": "boolean", "description": "Mirror the concept left to right."}),
            "align": {"type": ["string", "object"], "description": "fit (default), height, width, or {scale, offset: [x, y]}."}
        }), json!({
            "preset": str_prop("Camera preset: game (orthographic, yaw -135, pitch 32: frog-world's gameplay camera, unless presets or the user's camera_presets.json redefines it), default, or one from presets."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "view": view_prop(),
            "yaw": num_prop("Degrees. 0 puts the camera on +X looking toward -X."),
            "pitch": num_prop("Degrees, -90..90."),
            "orthographic": json!({"type": "boolean", "description": "Orthographic, as a turnaround sheet is drawn."}),
            "fov": num_prop("Perspective field of view in degrees. Defaults to 50."),
            "margin": num_prop("Empty border on each side, 0..0.45. Defaults to 0.06."),
            "object": json!({"type": ["integer", "string"], "description": "Frame this object (index or name)."}),
            "isolate": json!({"type": "boolean", "description": "Draw only object."}),
            "width": int_prop("16..4096"),
            "height": int_prop("16..4096"),
            "antialias": json!({"type": "boolean", "description": "Defaults to true."}),
            "lighting": lighting_prop(),
            "lightingPresets": lighting_presets_prop(),
            "pose": json!({"type": "object", "description": "Bone name to quaternion, as in preview."})
        })), mask_edit_props(false)), &["path", "concept"]),
        define("set_marker", "Name a point, a collision box, or a connector (a piece's joining end) on an object, for a game to read: a ledge top, a spawn, a collision volume, where two pieces join. It is saved in the .vox, listed in the export_mesh and export_models replies, and exported as a glTF node named after the marker with extras.froxel (kind, plus size, min, and max for a box and direction for a connector, whose node rotation turns -Z toward it) and in the scene's extras.froxelMarkers. Coordinates are the object's local mesh units: cell (x, y, z) spans x..x+1, so the top centre of cell (4, 4, 10) is [4.5, 4.5, 11]. The marker moves with its object. A marker of the same name is replaced. One undo step. Replies with the marker in local, world, and glTF coordinates.", json!({
            "name": str_prop("Marker name, 1 to 64 characters, such as ledge_top_4m. The glTF node takes it."),
            "kind": {"type": "string", "enum": ["point", "box", "connector"], "description": "point (default), box (collision), or connector"},
            "point": vec3_prop("point and connector: [x, y, z] in local mesh units"),
            "direction": vec3_prop("connector: the way the joining end faces, [x, y, z], normalised"),
            "x0": int_prop("box: inclusive cell corner; the box spans x0..x1+1"),
            "y0": int_prop("box: inclusive cell corner"),
            "z0": int_prop("box: inclusive cell corner"),
            "x1": int_prop("box: inclusive cell corner"),
            "y1": int_prop("box: inclusive cell corner"),
            "z1": int_prop("box: inclusive cell corner"),
            "object": {"type": ["integer", "string"], "description": "Object index or name it belongs to. Defaults to the active object."}
        }), &["name"]),
        define("remove_marker", "Remove the marker called name, or every marker with all: true. One undo step. Replies with removed.", json!({
            "name": str_prop("Marker name"),
            "all": {"type": "boolean", "description": "Remove every marker"}
        }), &[]),
        define("collision_from_voxels", "Build collision box markers from an object's solid cells, for a game to read, instead of writing boxes by hand: a walkable deck, a hollow hold, and open holes, which one convex hull cannot give. Boxes are named <prefix>_1, <prefix>_2, ... and are ordinary box markers: the .vox saves them, export writes them as glTF nodes, list_markers lists them, and preview with overlay: \"boxes\" draws them. A rerun with the same prefix replaces that prefix's boxes on the object, so run it again whenever the shape changes. Hollows stay open: no box ever holds an empty block holeSize wide on each axis (clipped to the box, so a hatch through a one-cell deck counts). Surface dents and gaps smaller than that may be filled, and the reply says how much. Set holeSize a little under the smallest opening the player must use. A box's faces sit on its cells, so walking surfaces stay exact. When maxBoxes cannot be met without closing a hollow, the boxes with the least solid are dropped and the reply says so (dropped, warning). Coordinates are the object's local mesh units, and the boxes move with it. One undo step. Replies with boxes (the count), solidCells, ignoredBySlot, ignoredThin, targetCells (what the boxes aim to cover), coveredCells, missedCells, coveredPercent, boxCells, emptyFilledCells and emptyFilledPercent (wrongly filled empty cells, as a share of the boxes' volume), merged, dropped, replaced, prefix, and markers.", json!({
            "prefix": str_prop("Marker names are <prefix>_<number>. Default <object name>_collider. An existing marker with one of those names that this tool would not replace is an error."),
            "layer": {"type": "integer", "minimum": 1, "maximum": 64, "description": "Horizontal cuts between boxes lie on planes this many cells apart, from the lowest cell, unless only another height opens a hollow. Default 1, any height. Not with zBands."},
            "zBands": {"type": "array", "items": {"type": "integer"}, "description": "Horizontal cuts by hand: ascending z planes in local mesh units, at least two, such as [0, 3, 8, 12]. Put one on each deck and walking level. Not with layer."},
            "minFeature": {"type": "integer", "minimum": 1, "maximum": 32, "description": "Ignore details thin on two axes, such as vines and rope: a cell is covered only if it lies in a full square this wide in an axis plane. A deck or a wall thin on one axis stays; a one-cell diagonal does not hold such a square. Default 1, keep everything."},
            "holeSize": {"type": "integer", "minimum": 1, "maximum": 64, "description": "The tolerance: the smallest empty block that stays open. Default 3. Larger means fewer boxes and more empty volume filled; 1 fills nothing, an exact cover."},
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Cover only cells of these palette indices. Omit for every slot."},
            "exclude": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices to leave out, such as moss, vines, and rope. These are slots, not the boxes the stone tools take under this name."},
            "maxBoxes": {"type": "integer", "minimum": 1, "maximum": 4096, "description": "Most boxes. With boxes to spare the emptiest boxes split further; with too many, neighbours merge, cheapest added empty volume first, as far as holeSize allows, and then the boxes with the least solid are dropped. Omit to split only until no box holds a hollow."},
            "object": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."}
        }), &[]),
        define("list_markers", "Markers with their kind, object, and local, world, and glTF (Y-up) coordinates: a point, a box's min, max, centre, and size, a connector's point, direction, and node rotation.", json!({}), &[]),
        define("import_mesh", "Voxelize a .glb, .gltf, or .obj mesh (with its .mtl and textures) into the active object, or a new object with newObject, to bring a form sculpted in Blender or made by an image-to-3D tool into the voxel workflow. Every triangle primitive of the default glTF scene is read under its node transforms; embedded and external buffers and PNG, JPEG, and WebP textures (EXT_texture_webp, as image-to-3D tools write) are read; KTX2/Basis textures are skipped with a warning; Draco and meshopt compression are refused. The file's Y-up becomes Z-up (upAxis z keeps it). Size by size (cells along the longest axis, default 64), scale (cells per file unit), or voxelSize (metres per cell, so 0.08 keeps a glTF's real size at 0.08 m voxels). fill solid (default) fills every cell a flood from the box border cannot reach, so a hollow or double-walled mesh comes out solid; an open mesh (openEdges above 0: holes, seams, an open underside) first has its openings sealed by cells enclosed in at least 17 of 26 directions, so a turtle shell open underneath is filled up to its rim; parity is the older majority vote of ray parity along x, y, and z; shell writes only the cells the surface touches. Each surface cell takes the base-colour texture times the factor times the vertex colours at the nearest point of its nearest triangle; inside cells take the nearest surface colour. Colours within tolerance of a palette slot reuse it; the rest are reduced to at most maxNewSlots colours written into slots no cell uses, from 255 down; slots maps only to the slots given, and palette paints one slot. Every cell the surface touches is kept, which keeps every part but fattens it; coverage (a share, such as 0.5) keeps a cell only when that share of it is solid on a voxelization 2 to 4 times finer, so a creature 20 to 60 cells long stays as tight as its mesh and its legs and fins do not merge, and keepThin (default true) keeps a part thinner than a cell (a fin, an ear, a leg) as the one cell its midline crosses, as a voxel remesher keeps thin sheets, instead of losing it under half a cell; minFeature N reports the parts thinner than N cells (thin). clean (true, or reduce_palette's options) reduces the imported colours to a few clusters in Oklab with the eyes kept and despeckles them before anything is written, so an image-to-3D texture's score of noisy slots never reaches the palette. At most 256 cells per axis and 2000000 cells, refused before anything changes. One undo step. Replies with cells, shell, interior, openEdges (edges of one triangle once equal positions are welded; 0 is watertight), written, changed, slots, newSlots, bounds (min, max, size in local cells), scale, triangles, primitives, textures, warnings, clean (with clean), coverage {share, keepThin, fine, touched, kept, thin} (with coverage), and thin (with minFeature).", json!({
            "path": str_prop("Path of the .glb, .gltf, or .obj"),
            "size": num_prop("Cells along the longest axis, 1..=256. Defaults to 64 when neither scale nor voxelSize is given."),
            "scale": num_prop("Cells per file unit (per metre for a glTF). Instead of size."),
            "voxelSize": num_prop("Metres per cell, such as 0.08: the mesh keeps its real size. Instead of size."),
            "upAxis": {"type": "string", "enum": ["y", "z"], "description": "The file's up axis. y (default, glTF and most OBJ) turns it to Z-up; z keeps the axes."},
            "rotation": vec3_prop("Degrees about x, then y, then z (cell axes), after the up axis. Defaults to [0, 0, 0]."),
            "position": vec3_prop("Where the anchor lands, in the active object's local cells (a cell corner). With newObject, the new object's world position, and the anchor is its local origin. Defaults to [0, 0, 0]."),
            "anchor": {"type": "string", "enum": ["min", "center", "bottom"], "description": "Point of the mesh's box placed at position: min (default) the low corner, center, or bottom (the centre of the bottom face)."},
            "fill": {"type": "string", "enum": ["solid", "parity", "shell"], "description": "solid (default) fills all a flood from the box border cannot reach, sealing an open mesh's openings at their rims; parity is the older ray-parity vote; shell writes only cells the surface touches."},
            "colors": {"type": "string", "enum": ["auto", "vertex"], "description": "auto (default): base-colour texture times factor times vertex colours; vertex: vertex colours only."},
            "palette": int_prop("Paint every cell this palette index instead of sampling colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Map colours only to these palette indices (the nearest of them); the palette is unchanged."},
            "maxNewSlots": int_prop("Most palette slots written with new colours, 0..=255. Defaults to 32; 0 maps to the nearest existing slots."),
            "tolerance": num_prop("sRGB distance (0..=442) under which an existing slot is reused. Defaults to 12."),
            "mode": mode_prop(),
            "orient": {"type": ["object", "string"], "description": "Turn the mesh before voxelizing it. \"upright\" stands a tilted image-to-3D mesh level from its shape alone: up is the thinnest principal axis of its vertices, signed so the form narrows upward, at most maxTilt degrees (default 40) from the file's up (a fish, whose thinnest axis lies flat, keeps its up), and the end of its long level axis whose cross-section stands taller turns to -Y (round or square shapes keep their up or yaw: keptUp, keptYaw). An object runs orient_to_views' search to match a views sheet: {views (as orient_to_views takes them) or only the sheet path (its measure_sheet layout), background, tolerance, turns, yawStep, yawRange, fineAxes, resolution, weight}; with upright: true (and maxTilt) in it the mesh is stood up first and only its yaw is searched. The reply adds orientation (and its upright)."},
            "clean": {"type": ["boolean", "object"], "description": "Run reduce_palette over the imported cells before they are written, in the same undo step: true is {colors: 8, despeckle: 1}, or its options (colors | palette, accents, minShare, contrast, maxAccents, despeckle, minSame, protect). The reply adds clean."},
            "coverage": num_prop("Keep a cell when this share of it (0..1, such as 0.5) is solid, measured on a voxelization 2 to 4 times finer, instead of every cell the surface touches: the form stays as tight as the mesh, so a small creature's legs and fins do not merge into lumps. At most 256 cells on the longest axis. The reply adds coverage."),
            "keepThin": {"type": "boolean", "description": "With coverage: a part thinner than a cell (a fin, an ear, a leg) keeps the one cell its midline crosses instead of vanishing under the coverage. Defaults to true."},
            "minFeature": int_prop("Report the imported parts thinner than this many cells, 2..16 (thin in the reply)."),
            "newObject": {"type": "boolean", "description": "Create a new object for the mesh and make it active. Defaults to false."},
            "name": str_prop("Name of the new object (with newObject). Omit for Object N.")
        }), &["path"]),
        define("mirror_object", "Copy an object mirrored across a world axis through the origin. Cells use -coord-1 on that axis. One undo step. mirrorBones also copies the bound bone and its descendants, swapping an _l/_r suffix. The copy is unbound unless mirrorBones is true.", json!({
            "index": int_prop("Object index to copy"),
            "axis": str_prop("x, y, or z"),
            "name": str_prop("Name of the copy"),
            "position": ivec3_prop("World cell of the copy's local (0, 0, 0). Omit to use the minimum corner of the mirrored cells."),
            "mirrorBones": {"type": "boolean", "description": "Copy the bound bone subtree and swap _l/_r. Default false."}
        }), &["index", "axis", "name"]),
        define("clear_object", "Empty an object of all its cells at once, whatever its size: fill_box with palette 0 stops at 2000000 cells, this has no limit. index is the object (an index or a name), the active one when omitted. Its name, position, bone, script, and markers stay; remove_object deletes it instead. One undo step; an empty object adds none. Replies with index, name, removed, and voxels.", json!({
            "index": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."}
        }), &[]),
        define("split_object", "Move (or copy) the cells of an object inside one or more boxes, or of some palette slots, or both, into a new or an existing object, keeping each cell's world position: cut one carve_from_views blockout of a creature into its rig parts (head, flippers, tail) instead of one carve per part plus complement erases. A new object takes the source's position, so its local cells are the source's; an existing target (into) takes them at its own offset, replacing what it holds there. invert takes every cell the boxes and slots do not select. One undo step. Replies with target, name, created, cells (taken), replaced, position, sourceVoxels, targetVoxels, and bounds (target local).", json!({
            "index": {"type": ["integer", "string"], "description": "Source object index or name. Defaults to the active object."},
            "boxes": {"type": "array", "description": "One inclusive box [x0, y0, z0, x1, y1, z1] in the source's local cells, or a list of up to 64. A cell inside any of them is selected. Omit to select by slots alone."},
            "slots": {"type": ["integer", "array"], "items": {"type": "integer"}, "description": "Only cells of these palette slots (the mask). Omit for every slot."},
            "invert": {"type": "boolean", "description": "Take every cell the boxes and slots do not select. Default false."},
            "copy": {"type": "boolean", "description": "Leave the cells in the source too. Default false (move)."},
            "into": {"type": ["integer", "string"], "description": "Existing target object, by index or name. Omit to create a new object."},
            "name": str_prop("Name of the new object. Defaults to the source's name with _part."),
            "makeActive": {"type": "boolean", "description": "Make the target the active object. Default false."}
        }), &[]),
        define("add_bone", "Add a bone. One undo step. parent is omitted for a root. pivot is a fractional world coordinate, so a joint can sit between cells.", json!({
            "name": str_prop("Unique bone name"),
            "parent": {"type": ["string", "null"], "description": "Parent bone name. Omit or null for a root."},
            "pivot": vec3_prop("World pivot [x, y, z]. Fractional values sit between cells."),
            "rotation": vec4_prop("Quaternion [x, y, z, w] in voxel space. Defaults to identity."),
            "rest": vec4_prop("Rest turn [x, y, z, w], local, on top of rotation: the angle the bone's parts are drawn, edited, saved, and exported at, while their cells stay flat on their own grid. Defaults to identity.")
        }), &["name", "pivot"]),
        define("set_bone", "Change a bone's parent, pivot, rotation, rest turn, or name. rest (or restFromPose, from the preview pose) is the saved angle the bone's parts are drawn, edited, and exported at: model a fin or a seat flat as its own object, bind it to a bone pivoted where it meets the body, and turn it there; the cells stay on the part's own grid and the glTF joint node rests at that turn. One undo step. Omit a field to leave it. parent null makes a root. joint true puts the pivot where the bone's part meets its parent's part instead of a given pivot, as place_joints does for one bone (with the new parent when parent is given too); the reply then has the joint's method, samples, gap, and previous pivot.", json!({
            "name": str_prop("Bone to edit"),
            "parent": {"type": ["string", "null"], "description": "Parent bone. null makes a root. Omit to leave the parent."},
            "pivot": vec3_prop("World pivot [x, y, z]"),
            "joint": {"type": "boolean", "description": "Measure the pivot at the joint with the parent's part (see place_joints). Not with pivot."},
            "snap": {"type": "string", "enum": ["none", "corner", "center", "half"], "description": "With joint: round the pivot to cell corners, cell centres, or half cells. Default none."},
            "band": num_prop("With joint, parts apart: cells past the closest gap that still count. Default 0.5."),
            "rotation": vec4_prop("Quaternion [x, y, z, w]"),
            "rest": vec4_prop("Rest turn [x, y, z, w], local, on top of rotation: the saved angle the bone's parts sit at (a fin sculpted flat, turned onto the body). Identity clears it."),
            "restFromPose": {"type": "boolean", "description": "Keep the bone's preview turn (set_pose) as its rest turn and drop the turn from the pose, so the part stays where it is drawn. Not with rest."},
            "rename": str_prop("New unique name")
        }), &["name"]),
        define("place_joints", "Put bone pivots at the joints between rig parts, after split_object and bind_object, instead of placing them by eye. A bone's part is the cells of the objects bound to it; its parent part is the cells bound to the nearest ancestor bone that has any. The pivot is the centroid of the cells both parts hold (method overlap), else of the faces where the parts touch (contact), else of the midpoints between the child's surface cells and the parent's nearest surface cells within band cells of the closest gap (nearest; gap is the empty cells between them). snap rounds it to cell corners, cell centres, or half cells. Every bone by default: roots and bones with no part are skipped with the reason; a named bone that cannot be placed is an error. One undo step. Replies with placed [{name, parent, pivot, previous, method, samples, gap}] and skipped [{name, reason}].", json!({
            "bones": {"type": "array", "items": {"type": "string"}, "description": "Bone names. Default every bone."},
            "snap": {"type": "string", "enum": ["none", "corner", "center", "half"], "description": "Round the pivots: none (default), corner (integer), center (n + 0.5), or half."},
            "band": num_prop("Parts apart: cells past the closest gap that still count toward the pivot. Default 0.5.")
        }), &[]),
        define("remove_bone", "Delete a bone. Objects bound to it become unbound. Children keep its parent. One undo step.", json!({
            "name": str_prop("Bone to delete")
        }), &["name"]),
        define("bind_object", "Bind an object to one bone, or unbind it. One undo step. An unbound object follows the skeleton root at export.", json!({
            "index": int_prop("Object index from inspect_document"),
            "bone": {"type": ["string", "null"], "description": "Bone name. null unbinds."}
        }), &["index", "bone"]),
        define("list_bones", "Bones with parent, pivot, rotation, rest (the saved turn their parts sit at), and the object indices bound to each. Pivots are fractional world coordinates.", json!({}), &[]),
        define("set_pose", "Preview a pose in the window. Each quaternion is local and multiplied on the right of the bind rotation and the bone's rest turn, so a parent orbits its children. Omitted bones stay at rest. Identity is a no-op. Does not change the bind pose. The window draws a posed part turned and its tools edit it where it is drawn, on its own grid; local cell coordinates in the tools do not change. One undo step.", json!({
            "pose": {"type": "object", "description": "Map of bone name to a quaternion [x, y, z, w] or {rotation, scale}; scale is local per axis, 0 to 16, and children inherit it. Replaces the preview.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}}
        }), &["pose"]),
        define("clear_pose", "Remove the preview pose. The bind pose stays. One undo step.", json!({}), &[]),
        define("add_clip", "Add an empty named clip. Keys are local TRS in voxel space: translation is the full offset from the parent, not a delta. One undo step. Saved in the .vox MVAN chunk and written to glTF as an animation.", json!({
            "name": str_prop("Unique clip name")
        }), &["name"]),
        define("set_clip_keys", "Replace one bone's keys in a clip. Times must be finite, >= 0, and increasing. rotation defaults to identity, translation to [0, 0, 0], scale to [1, 1, 1]. One undo step.", json!({
            "name": str_prop("Clip name"),
            "bone": str_prop("Bone name"),
            "keys": {"type": "array", "description": "Keyframes. time is required. rotation is [x, y, z, w]. translation and scale are [x, y, z].", "items": {"type": "object"}}
        }), &["name", "bone", "keys"]),
        define("list_clips", "Named clips and their bone keys.", json!({}), &[]),
        define("remove_clip", "Remove a clip. One undo step.", json!({
            "name": str_prop("Clip name")
        }), &["name"]),
        define("undo", "Undo the last edit.", json!({}), &[]),
        define("redo", "Redo the last undone edit.", json!({}), &[]),
        define("render", "Path-trace the whole scene to a PNG. Blocks until the file is written. The camera orbits at yaw and pitch and fits the framed box (every visible posed cell, or target) inside the frame with margin on each side, at every angle of the call; zoom and distance scale that fit. Give only width or only height and the other side follows the model, so a long model gets a wide frame. Replies framed (the world box) and distance.", {
            let denoise = if crate::denoise::available() {
                "Open Image Denoise on the finished frame, using the first-hit albedo and normal. Default true."
            } else {
                "Edge-aware filter before writing. Default true. Build with --features oidn for Open Image Denoise."
            };
            merged(json!({
            "path": str_prop("PNG file to write. Its folder is created."),
            "width": int_prop("Pixels, 1..4096, at most 4096 x 2048 in all. Default 256, or from height and the model's shape."),
            "height": int_prop("Pixels, 1..4096. Default 256, or from width and the model's shape."),
            "samples": int_prop("Path-trace samples, 1..8192. Default 16. The CPU tracer takes about a second per 16 samples at 256 x 256; gpu is far faster."),
            "bounces": int_prop("Max bounces, 0..8. Default 4."),
            "seed": int_prop("Sampling seed. Default 1."),
            "denoise": {"type": "boolean", "description": denoise},
            "atmospheric": {"type": "boolean", "description": "Nishita sky. Omit to keep the document setting."},
            "skyIntensity": num_prop("Sky intensity. Omit to keep the document setting."),
            "sunIntensity": num_prop("Sun intensity. Omit to keep the document setting."),
            "glassMode": str_prop("probabilistic or absorption. Omit to keep the document setting."),
            "sky-intensity": num_prop("Older name of skyIntensity; still accepted."),
            "sun-intensity": num_prop("Older name of sunIntensity; still accepted."),
            "glass-mode": str_prop("Older name of glassMode; still accepted."),
            "aces": {"type": "boolean", "description": "ACES filmic tonemap. Default true."},
            "stochasticGi": {"type": "boolean", "description": "Stochastic area lights for the indirect bounces. Omit to keep the document setting."},
            "gpu": {"type": "boolean", "description": "Trace on the GPU (wgpu) instead of the CPU; an error when no adapter is available. Default false."},
            "albedoPath": str_prop("Also write the first-hit albedo of the main view as a PNG here."),
            "normalPath": str_prop("Also write the first-hit normal of the main view as a PNG here."),
            "aperture": num_prop("Lens aperture. Omit to keep the document setting."),
            "focus": num_prop("Focus distance. Omit to keep the document setting."),
            "nee": ui(bool_prop("Power-weighted next-event estimation of emissive faces, with stochasticGi. Omit to keep the document setting."), "Power-weighted NEE", "Pick emissive faces by power for direct light"),
            "ris": soft(ui(hard(int_prop("Candidates per light sample (resampled importance sampling), with stochasticGi, 1..32. Omit to keep the document setting."), 1.0, 32.0), "RIS samples", "Candidates per light sample"), 1.0, 32.0, 0),
            "indirectClamp": soft(ui(hard(num_prop("Largest radiance an indirect bounce may carry, against fireflies, 0..32; 0 is no clamp. Omit to keep the document setting."), 0.0, 32.0), "Indirect clamp", "Largest radiance a bounce may carry; 0 is none"), 0.0, 32.0, 1),
            "horizon": soft(ui(hard(num_prop("Flat sky: degrees the horizon gradient blends over, 0..15. Omit to keep the document setting."), 0.0, 15.0), "Horizon", "Degrees the flat sky's horizon blends over"), 0.0, 15.0, 1),
            "sunElevation": soft(ui(hard(num_prop("Sun height above the horizon in degrees, -90..90. Omit to keep the document setting."), -90.0, 90.0), "Elevation", "Sun height above the horizon in degrees"), -90.0, 90.0, 0),
            "sunAzimuth": soft(ui(hard(num_prop("Sun direction round up in degrees, 0..360. Omit to keep the document setting."), 0.0, 360.0), "Azimuth", "Sun direction round up in degrees"), 0.0, 360.0, 0),
            "sunDisc": soft(ui(hard(num_prop("Sun disc size in degrees, 0.1..30; larger gives softer shadows. Omit to keep the document setting."), 0.1, 30.0), "Disc size", "Sun disc size in degrees"), 0.1, 30.0, 1),
            "sunColor": ui(json!({"type": ["string", "array"], "items": {"type": "integer"}, "description": "Sun colour, \"#rrggbb\" or [r, g, b] in 0..255. Omit to keep the document setting."}), "Color", "Sun colour"),
            }), json!({
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. A model facing -Y is face-on at yaw -90. Omit to keep the framing angle (30)."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Omit to keep the framing angle (25)."),
            "distance": num_prop("Multiple of the fitted distance, greater than 0 and at most 100. Below 1 is closer. Default 1."),
            "zoom": num_prop("Magnification of the fitted frame, 0.01..100. 2 is twice as large (crops the box), 0.5 half. Default 1."),
            "margin": num_prop("Empty border on each side of the fitted box as a fraction of the frame, 0..0.45. Default 0.05."),
            "target": target_prop(),
            "orthographic": {"type": "boolean", "description": "Parallel rays, fitted to the box like perspective. Default false."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. A zero scale hides that bone's parts. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}},
            "showBones": {"type": "boolean", "description": "Draw joint points and parent links on the PNG. Default false."},
            "view": view_prop(),
            "views": {"type": "array", "description": "Extra PNGs. The required path is still written with the top-level yaw and pitch. Each item is {yaw, pitch, path} or {view, path}.", "items": {"type": "object"}}
            }))
        }, &["path"]),
        define("model_view", "Flat model view to a PNG, like the editor's model view without shading: each pixel is the palette colour of the first visible cell under its centre, with no light, no tonemap, and no denoise, so it compares directly with a turnaround sheet. No path tracer. The camera fits the framed box like render; every view of the call shares one centre and one scale. Orthographic replies pixelsPerVoxel.", json!({
            "path": str_prop("PNG file to write. Its folder is created."),
            "width": int_prop("Pixels, 1..4096. Default 512, or from height, pixelsPerVoxel, and the model's shape."),
            "height": int_prop("Pixels, 1..4096. Default 512, or from width, pixelsPerVoxel, and the model's shape."),
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. A model facing -Y is face-on at yaw -90. Default 30."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Default 25."),
            "distance": num_prop("Multiple of the fitted distance, greater than 0 and at most 100. Below 1 is closer. Default 1."),
            "zoom": num_prop("Magnification of the fitted frame, 0.01..100. 2 is twice as large (crops the box), 0.5 half. Default 1."),
            "margin": num_prop("Empty border on each side of the fitted box as a fraction of the frame, 0..0.45. Default 0.05."),
            "target": target_prop(),
            "pixelsPerVoxel": num_prop("Orthographic scale instead of the fit: pixels per voxel edge, 0.05..256. Omitted width and height then follow the framed box. Not with zoom or distance."),
            "orthographic": {"type": "boolean", "description": "Parallel rays. Default true."},
            "background": {"type": ["string", "array"], "description": "#rrggbb, #rrggbbaa, or [r, g, b, a?] for pixels that hit no cell. Default transparent."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. A zero scale hides that bone's parts. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}},
            "showBones": {"type": "boolean", "description": "Draw joint points and parent links on the PNG. Default false."},
            "views": {"type": "array", "description": "Extra PNGs with the same camera settings. Each item is {view, path} or {yaw, pitch, path}.", "items": {"type": "object"}}
        }), &["path"]),
        define("preview", "Fast lit preview to a PNG in well under a second: the editor viewport's mesh and light (sun plus ambient), rasterised on the CPU with no path tracer and no window. The camera orbits at yaw and pitch and fits the model, or one object, inside the image with a margin. Use it to check shape and framing often; use render for lighting and materials and model_view for palette-exact colours. lighting lights it as a game does instead (old_pond is frog-world's Old Pond: warm sun, blue ambient, its background), to judge colour under the game's light; lightingPresets adds a project's presets. preset names a camera: game is the gameplay camera (orthographic, yaw -135, pitch 32), and presets (an object or a JSON file) defines or overrides presets, so a project's own game camera is one file. placementYaw shows an asset as it stands when placed turned that many degrees about up in a level, from that same camera, with no angles to work out. inputs (.vox files, or {path?, label?, params?}), variants (parameter sets of the open scripted document, or a variants JSON path), or reference (a concept PNG placed as the first tile) make a contact sheet instead: one tile per model in a grid with its name and parameters under it; width and height are then each tile's (default 512). The open document and its undo history are left as they were. overlay draws the markers over the model, collision boxes as outlines, with their names. Two triangle counts come back and they are different meshes: previewTriangles is the unmerged viewport mesh the image draws; export.triangles and export.quads are what export_mesh or export_models writes with the export options given in export (merge, colorAtlas, ...; all off by default), so pass the game's export options to see the game's count. countsLabel writes both in the corner. Replies with tiles (label, x, y, width, height, previewTriangles, export, voxels, params).", merged(json!({
            "path": str_prop("PNG file to write. Its folder is created."),
            "glb": str_prop("Draw this written .glb instead of the document, as an unlit material does: nearest texel times COLOR_0, no light, so a baked export looks as it will unshaded in a game. Takes the camera, size, background and antialias arguments only."),
            "width": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "height": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "preset": str_prop("Named camera: game (orthographic, yaw -135, pitch 32: frog-world's gameplay camera, unless presets or the user's camera_presets.json redefines it), default (perspective, yaw 30, pitch 25), or one from presets. yaw, pitch, view, orthographic, fov, and margin given here win over it."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "inputs": {"type": "array", "items": {"type": ["string", "object"]}, "description": "Contact sheet: .vox paths, or {path?, label?, params?} (no path is the open document; params are set_params values applied before the tile, so a scripted file is baked with them). At most 64."},
            "variants": {"type": ["array", "string"], "description": "Contact sheet of the open scripted document: parameter sets [{name?, params}] as export_variants takes, or the path of a variants JSON file. Each is baked in a scratch run; the document is restored."},
            "reference": {"type": ["string", "object"], "description": "Contact sheet: a PNG (a concept or turnaround) or {path, label?}, scaled to fit the first tile with its aspect kept. Alone, it sits beside the open document."},
            "columns": int_prop("Contact sheet columns, 1..65. Default about square."),
            "labels": {"type": "boolean", "description": "Contact sheet: each tile's name and parameters under it. Default true."},
            "overlay": {"type": ["boolean", "string", "array"], "description": "Draw the markers (set_marker, Rhai marker, collision_box, connector) over the model: true or all, or boxes (collision boxes as outlines), points, connectors (an arrow toward their direction), markers, or a list. What the model hides is drawn faded and dashed, so a collision box can be checked against the stone before export. Replies with overlay: each drawn marker's name, kind, object, anchor pixel, and whether it is hidden. Works on contact sheet tiles too."},
            "overlayLabels": {"type": "boolean", "description": "Each overlay marker's name beside it. Default true."},
            "export": {"type": ["object", "boolean"], "description": "The export options the reply's export counts use, as export_mesh takes them: merge, colorAtlas, vertexColors, exportHidden, splitObjects, aoStrength, aoRadius, aoTarget, bake, gameView (all off when absent, like export_mesh). Replies export {triangles, quads, and these options}, or error when that export would fail. The counts are of the whole document whatever object frames, and match the glb. Unmerged counting is quick; merge costs about what the export does. false skips it."},
}), json!({
            "lighting": lighting_prop(),
            "lightingPresets": lighting_presets_prop(),
            "countsLabel": {"type": "boolean", "description": "Write previewTriangles (the unmerged mesh drawn) and the export counts in the image's bottom-left corner, and in each contact sheet tile. Default false."},
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. A model facing -Y is face-on at yaw -90. Default 30."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Default 25."),
            "orthographic": {"type": "boolean", "description": "Parallel projection fitted tightly to the silhouette; replies pixelsPerVoxel. Default false (the viewport's 50 degree perspective)."},
            "fov": num_prop("Vertical field of view in degrees for perspective, 10..120. Default 50, like the viewport; lower flattens a long model."),
            "margin": num_prop("Empty border on each side as a fraction of the image, 0..0.45. Default 0.06."),
            "object": {"type": ["integer", "string"], "description": "Frame this object (index or name) instead of the whole model. Other visible objects are still drawn unless isolate is true. A hidden object named here is drawn."},
            "isolate": {"type": "boolean", "description": "Draw only object. Default false."},
            "background": {"type": ["string", "array"], "description": "#rrggbb, #rrggbbaa, or [r, g, b, a?]. Default the viewport grey #16181c; #00000000 is transparent."},
            "antialias": {"type": "boolean", "description": "2x2 supersampling up to 2048x2048 output pixels. Default true."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}}
        })), &["path"]),
        define("reload", "Restart this MCP server on the froxel binary now on disk, so tools added by a rebuild appear without the user restarting the server: the server re-executes itself in place (same process, same stdin and stdout), after asking the new binary whether it can resume (froxel mcp --reload-check; a broken or older build is refused and this server keeps running). The open document is kept: written to a temporary .vox and reopened, with its path, active object, and unsaved state (undo history and the preview pose are not kept); connected to an editor (--connect), the document stays in the window. After this reply the new server sends notifications/tools/list_changed; a client that honours listChanged (this server declares it) asks tools/list again, and one that does not keeps its old list until it reconnects. Without force nothing happens when the binary has not changed since the server started. Started with --watch, the server does this by itself before the first request after a rebuild. Unix only. Replies with reloaded, binary, changed, document, and next, or reloaded false and the reason.", json!({
            "force": {"type": "boolean", "description": "Reload even when the binary has not changed. Default false."}
        }), &[]),
        define("set_view", "Move the session camera. Connected to a window, that is the editor's camera, so the next screenshot_viewport shows an exact view. Not an edit and not undoable. The edit viewport stays lit; use model_view for palette-exact colours.", json!({
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. Omit to keep the current yaw."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Omit to keep the current pitch."),
            "frame": {"type": "boolean", "description": "Centre on every object at the framing distance first. Default true."},
            "distance": num_prop("Multiple of the framing distance, greater than 0 and at most 100. Needs frame. Default 1."),
            "orthographic": {"type": "boolean", "description": "Parallel projection in the edit viewport and the CPU trace. Omit to keep it."},
            "preset": str_prop("Named camera: game (orthographic, yaw -135, pitch 32: frog-world's gameplay camera, unless presets or the user's camera_presets.json redefines it), default, or one from presets; yaw, pitch, view, and orthographic given here win over it."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "bones": {"type": "boolean", "description": "Draw the skeleton overlay in the viewport. false hides it for a screenshot. Omit to keep it."},
            "oneFrame": ui(bool_prop("The window's Render mode traces one frame and starts a new one once the view holds still, instead of starting over on every step of an orbit or edit. Omit to keep it."), "One frame", "Do not start over on every orbit or edit step; trace again once the view holds still"),
            "temporalSmooth": soft(ui(hard(num_prop("The window's live Render mode: the least weight a new frame gets against the frames before it, 0..1; lower is smoother and slower to follow a change. Omit to keep it."), 0.0, 1.0), "Temporal smooth", "Least weight a new live frame gets"), 0.0, 1.0, 3)
        }), &[]),
        define("set_tool", "Pick the window's tool and its options, as the tool strip, the mode buttons, the palette, and the tool options row do, so a ui_click on the viewport then stamps that tool. Not an edit and not undoable. Replies with the tool, mode, slot, and options as they now are.", json!({
            "tool": ui(json!({"type": "string", "enum": ["brush", "line", "circle", "square", "box", "face", "generator", "tube", "sculpt", "mutator", "marquee", "box_select", "flood_select"], "description": "The tool: brush, line, circle, square, box, face, generator, tube, sculpt, mutator, marquee, box_select, or flood_select. Omit to keep it."}), "Tool", "The window's active tool"),
            "mode": ui(json!({"type": "string", "enum": ["add", "remove", "paint"], "description": "add, remove, or paint. Omit to keep it."}), "Mode", "Add, remove, or paint"),
            "slot": ui(int_prop("The active palette slot, 0..255. Omit to keep it."), "Slot", "The active palette slot"),
            "size": soft(ui(int_prop("Brush, Line, Circle, Square, Box, Mutator: brush size in cells, 1..63. Omit to keep it."), "Size", "Brush size in cells"), 1.0, 63.0, 0),
            "shape": ui(json!({"type": "string", "enum": ["square", "circle"], "description": "The brush's shape: square or circle. Omit to keep it."}), "Shape", "The brush's shape"),
            "seedColor": soft(ui(int_prop("Mutator: the slot foliage grows from, 1..255. Omit to keep it."), "Slot", "The slot foliage grows from"), 1.0, 255.0, 0),
            "density": soft(ui(num_prop("Mutator: the share of free cells beside the seed colour that grow, 0..1. Omit to keep it."), "Density", "Share of free cells that grow"), 0.0, 1.0, 2)
        }), &[]),
        define("screenshot", "PNG of the whole editor window. Only when connected to a window.", json!({"path": str_prop("Optional file to write. Its folder is created.")}), &[]),
        define("screenshot_viewport", "PNG of the 3D viewport. Only when connected to a window.", json!({"path": str_prop("Optional file to write. Its folder is created.")}), &[]),
        define("screenshot_region", "PNG of a rectangle in window points, origin at the top-left of the client area. Only when connected to a window.", json!({
            "x": num_prop("Left edge, window points"),
            "y": num_prop("Top edge, window points"),
            "width": num_prop("Width in window points"),
            "height": num_prop("Height in window points"),
            "path": str_prop("Optional file to write. Its folder is created.")
        }), &["x", "y", "width", "height"]),
        define("ui_inspect", "Controls from the previous frame: tool buttons, mode, outliner, sliders, palette cells, viewport chips, and the viewport rectangle, in window points. Only when connected to a window.", json!({}), &[]),
        define("ui_click", "Click a control by its ui_inspect id, or the topmost registered control at a window point. An id needs no coordinates and survives a redraw that moved the control: the click lands at its centre, at x, y if given, or for a slider at value along its track. A viewport hit stamps the active tool. A slider hit sets that slider from the point along its track. A palette cell selects that slot. An item of an open drop-down is clicked before what lies under it, and picking it closes the drop-down. A point on no registered control gets a real pointer press and release, so any widget there (a checkbox, a combo box, a button) is clicked; the reply is then pointer [x, y]. Only when connected to a window.", json!({
            "id": str_prop("A control's id from ui_inspect, such as tool.generator or op.generate.kind.conifer. Instead of a point, or with one inside the control."),
            "value": num_prop("With the id of a slider: the value to set, as a point that far along its track."),
            "x": num_prop("X in window points, origin at the top-left of the client area"),
            "y": num_prop("Y in window points, origin at the top-left of the client area")
        }), &[]),
        define("set_script", "Store a Rhai script on an object and read its parameters. One undo step. Voxels change only when regenerate runs. A failure keeps the source and reports the script line. Scripts have the sculpt tools as functions (fill_box with a mode, fill_ellipsoid, fill_rounded_box, fill_tube, generate, blend_shapes, carve_box, carve_ellipsoid, carve_rounded_box, carve_tube, recolor, surface_stroke, shell_plates, smooth, break_end, scatter, joints, column_relief, weather, bark, cavity_tint, radial_relief, foliage, moss, algae, vines, sweep, trim_outline, loft, deform, brush, voxelize (import_mesh's voxelizer on an inline mesh; scripts read no files), carve_from_views with views given as rows and a legend, pixelate_views with views as rows (returns pixel art rows at the cell grid), project_paint and decal with the image as rows and a legend, fit_to_views with views as rows and a legend (no other objects in view; reference names an object the run builds), and marker, collision_box, connector, collision_from_voxels for markers) with the MCP options in a trailing map such as #{mode: \"empty\"}, get_voxels, set_voxels, and the document tools (set_palette, set_object, remove_object, clear_object, mirror_object, split_object, add_bone, set_bone, place_joints, remove_bone, bind_object, list_bones, set_pose, clear_pose, add_clip, set_clip_keys, list_clips, remove_clip), which apply after the script in the same undo step, plus sqrt, pow, PI, min, max, clamp, push on arrays, and strings (\"ledge_\" + 4, `ledge_${i}`, to_string); docs/scripting.md lists them.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "source": str_prop("Rhai source. Empty clears the script.")
        }), &["source"]),
        define("get_script", "Script source, auto flag, and declared parameters.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        define("set_params", "Set script parameters by name. One undo step. Does not run the script.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "values": {"type": "object", "description": "Map of parameter name to a number, bool, [r, g, b] (whole 0..255, or 0..1 when every channel is at most 1 and one has a decimal point: [1, 1, 1] is near black, [1.0, 1.0, 1.0] white), a list of numbers (floats), or a list of [x, y] or [x, y, z] points (vec2s, vec3s). Out of range or too long is an error."}
        }), &["values"]),
        define("list_params", "Parameters from the last successful run of the object's script.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        define("regenerate", "Run the object's script and replace the voxels it writes. One undo step. There is no operation or voxel cap; the run stops at timeoutSeconds. A timeout or runtime error leaves the document unchanged and includes the script line.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "timeoutSeconds": {"type": "number", "description": "Stop the run after this many seconds. Default 300. 0 waits as long as it takes."}
        }), &[]),
        define("export_mesh", "Write a glTF 2.0 .glb of every object. Exposed faces only. The default material is a 256-wide palette texture. A swaying or bobbing slot adds TEXCOORD_1 (sway, bob) on every vertex. A skeleton writes one rigid skin: each vertex belongs to one bone with weight 1. The reply includes the file's node, mesh, skin and animation structure, quads, and triangles (every triangle in the glb), and the markers counted by kind ({\"box\": 22, \"connector\": 2}); reply: \"full\" lists each marker (local, world, glTF), as list_markers does.", json!({
            "path": str_prop("Where to write the .glb. Its folder is created."),
            "merge": {"type": "boolean", "description": "Greedy-merge coplanar faces of one palette slot. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "Store a linear COLOR_0 on each vertex instead of the palette texture. Defaults to false."},
            "exportHidden": {"type": "boolean", "description": "Include hidden objects. Default false, which omits them."},
            "splitObjects": {"type": "boolean", "description": "One mesh node per object, still sharing one skin, so a game can hide a part. Default false is a single mesh."},
            "colorAtlas": {"type": "boolean", "description": "Merge coplanar faces whatever their palette slot and paint each cell into a per-mesh colour atlas texture (nearest filtering) instead of the palette, for far fewer triangles on noisy palettes. Faces of different motion or opacity stay apart. One rigid mesh only: not with a skeleton, splitObjects or vertexColors. Defaults to false."},
            "aoStrength": {"type": "number", "minimum": 0, "maximum": 1, "description": "Bake ambient occlusion into COLOR_0 so crevices, joints and the undersides of overhangs export darker, at no runtime cost. 0..1, how much light a fully occluded corner loses. Multiplied into vertexColors, otherwise a grey COLOR_0 that glTF (and Godot's StandardMaterial3D) multiplies with the texture. aoTarget picks vertex colours or the colour atlas texels. Defaults to 0, off."},
            "aoRadius": {"type": "integer", "minimum": 0, "maximum": 8, "description": "Reach of the occlusion in cells beyond the four that touch a corner, as a cosine-weighted hemisphere, so deep crevices and overhangs darken more. 0 or 1 is the corner term alone. Defaults to 3."},
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "Where aoStrength goes. vertex: a per-corner COLOR_0; faces whose corners shade differently are not merged, which costs triangles. texture: each cell's colour atlas texel is darkened by the mean of its corners and faces merge exactly as without occlusion; needs colorAtlas. both: the two at once, for a shader that uses one (a material multiplying texture by COLOR_0 darkens twice). auto (default): texture with colorAtlas, else vertex."},
            "bake": bake_prop(),
            "gameView": game_view_prop(),
            "reply": reply_prop()
        }), &["path"]),
        define("export_variants", "Bake a scripted document once per parameter set and write <name>.vox and <name>.glb for each into dir, with the export_mesh mesh options, then a manifest JSON (default dir/variants.json) with each set's requested and resolved parameters, markers (local, world, glTF), bounds (voxel and glTF), quads, voxels, and mesh options. An existing manifest is updated by variant name: the sets exported now replace their entries in place, the other entries are kept in order, and new names are appended, so a subset can be redone; replace: true writes a fresh manifest with only these sets. The top-level options are present while every entry shares them. An entry records sceneShift when the document opened with one (see open_file). Parameters are keyed by name, or object/name when two objects share one, and take the values set_params does (a colour is whole 0..255, or 0..1 with a decimal point), so a manifest entry's params can be passed back unchanged. Every script runs per set, as regenerate does. The open document and its undo history are left as they were. One failing set stops the call with its name, before the manifest is written. Replies with the manifest path, the names it added, replaced, and kept, the shared options, and per set exported now its name, glb and vox paths, bounds, voxels, quads, sceneShift, and markers counted by kind ({\"box\": 230}); parameters and marker detail are in the manifest file. reply: \"full\" returns the whole manifest instead.", json!({
            "variants": {"type": ["array", "string"], "minItems": 1, "maxItems": 64, "items": {"type": "object"}, "description": "1 to 64 sets: {name?, params}, or the path of a JSON file of them. name is a file stem of letters, digits, _, -, . (default variant_1, ...); params maps parameter names to values, as set_params takes them."},
            "dir": str_prop("Folder for the .vox, .glb, and manifest files. It is created."),
            "manifest": str_prop("Manifest path. Defaults to dir/variants.json. Its folder is created."),
            "replace": {"type": "boolean", "description": "Write a fresh manifest with only these sets instead of merging into the existing one by name. Defaults to false."},
            "vox": {"type": "boolean", "description": "Write a .vox per set too. Defaults to true."},
            "reply": {"type": "string", "enum": ["compact", "full"], "description": "compact (the default): per set exported now its files, bounds, voxels, quads, and markers counted by kind. full: the whole manifest, every entry with its parameters and markers (local, world, glTF). The manifest file is complete either way."},
            "timeoutSeconds": num_prop("Script deadline per object and set, up to 3600. Defaults to 300."),
            "merge": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "colorAtlas": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "aoStrength": num_prop("As export_mesh, 0..1. Defaults to 0, off."),
            "aoRadius": int_prop("As export_mesh, 0..8. Defaults to 3."),
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "As export_mesh. Defaults to auto."},
            "bake": {"type": ["object", "boolean"], "description": "As export_mesh. Defaults to off."},
            "gameView": {"type": ["object", "boolean"], "description": "As export_mesh: leave out the faces a fixed game camera never sees. Defaults to off."},
            "exportHidden": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "splitObjects": {"type": "boolean", "description": "As export_mesh. Defaults to false."}
        }), &["variants", "dir"]),
        define("export_models", "Write 1 to 16 .glb files and report each file's quads, triangles, and structure: nodes, meshes (vertices, indices, bounds), skins, and animations. A warning names an empty mesh, a skin with no joints, a nameless or duplicated node, or an animation with no channels. source opens that .vox in its own session, so the open document is not replaced. Omit source to export the open document. One model's failure is reported on that model and does not skip the others. Each model's markers are counted by kind; reply: \"full\" lists each one.", json!({
            "models": {"type": "array", "minItems": 1, "maxItems": 16, "description": "Each item is {path, source?, merge?, vertexColors?, exportHidden?, splitObjects?, colorAtlas?, aoStrength?, aoRadius?, aoTarget?, bake?, gameView?}, with the export_mesh meanings. path ends in .glb. source, when set, is a .vox file.", "items": {"type": "object"}},
            "reply": reply_prop()
        }), &["models"]),
        bake_scene_tool(),
    ]
}

/// The `gameView` option of `export_mesh`: faces a fixed game camera never sees.
fn game_view_prop() -> Value {
    json!({"type": ["object", "boolean"], "description": "Leave out the faces a fixed game camera never sees, for fewer triangles and a smaller colour atlas with the look from that camera unchanged: {yaw, pitch, spread?, occlusion?, solid?}. yaw and pitch (degrees) are the camera's direction in the asset's own frame, as preview's (a level's turn of the asset taken off: a camera at yaw -135 over an asset turned 45 degrees is yaw -180 here). spread (degrees, a number or [about up, up or down], 0..80, defaults to [25, 14]) widens it to every direction the lens and the camera's pan can see. A face is kept when one of those directions sees its front (back faces and undersides go) and, with occlusion (defaults to true), when a ray from it toward the camera along one of them leaves the model without meeting a solid cell or a solid box. solid: up to 256 inclusive boxes [x0, y0, z0, x1, y1, z1] of cells in the document's frame that also hide, such as a neighbour piece standing against this one; they are not written. Occlusion (aoStrength) and the bake still read every cell, so the kept faces shade as before. Defaults to off: every exposed face."})
}

/// The `reply` option of `export_mesh` and `export_models`.
fn reply_prop() -> Value {
    json!({"type": "string", "enum": ["compact", "full"], "description": "compact (the default): the markers counted by kind, {\"box\": 22}. full: every marker with its local, world, and glTF place, as list_markers gives them."})
}

/// MCP `bake_scene`. Its own function keeps `json!` within the recursion limit.
fn bake_scene_tool() -> Value {
    define("bake_scene", "Bake one placement of many .vox assets together, so each is shaded by its neighbours and by ground, into one merged .glb with one colour atlas (colorAtlas faces, one texel per cell face, material KHR_materials_unlit, to draw unshaded). The open document is not touched. A placement is the frame of the glb export_mesh writes, as a game places it: world = translation + Lean x scale x Ry(yaw) x (x, z, -y) of a cell corner, Y up, so a level's translation, rotation.y in degrees, and uniform scale copy across. An optional lean {degrees, toward} tilts a placement by degrees (-90..90) about its base so its top moves toward the yaw toward (0 is -Z, 90 is -X, as yaw); without it the placement is upright. An instance's optional sway (0..4, default 1) multiplies the sway amount (TEXCOORD_1 x) of its moving slots in the written mesh, so kinds of plant sway apart in one baked mesh; bob is kept. Any yaw or lean works, and repeats of one .vox share one grid. Occluders stop light and are not written. Costs: triangles are the sum of each placement's colorAtlas triangles, as nothing merges across placements, and the atlas holds one RGBA texel per exposed cell face of every placement. Rays read one bit grid per distinct source; occluder triangles take about 112 bytes each. The reply gives memory in bytes. Replies with triangles, vertices, atlas, texels, bytes, memory, bake, and structure.", json!({
        "scene": {"type": ["object", "string"], "description": "The scene, or the path of a JSON file of it (relative paths inside start at that file's folder; for an object, at the server's working directory): {instances: [{source: a .vox path, translation?: [x, y, z], yaw?: degrees about +Y, scale?: uniform, lean?: {degrees: tilt, toward: yaw the top moves toward}, sway?: multiplier of its slots' sway, 0..4}], occluders?: [{mesh: a .glb, .gltf or .obj path, translation?, yaw?, scale?, lean?} | {plane: height, solid below it} | {height: {origin?: [x, z], cell?: spacing, rows: [[y at x...] per z...]}}], bake?, exportHidden?}. 1 to 32768 instances."},
        "path": str_prop(".glb file to write. Its folder is created."),
        "bake": {"type": ["object", "boolean"], "description": "The export_mesh bake option, over the scene's own; distance is in scene units here. Defaults to occlusion alone."}
    }), &["scene", "path"])
}

fn define(name: &str, description: &str, mut properties: Value, required: &[&str]) -> Value {
    let op = super::by_tool(name).unwrap_or_else(|| panic!("`{name}` is not in ops::OPS"));
    let [read_only, destructive, idempotent] = op.access.hints();
    if op.space() {
        properties["space"] = remote::space_prop();
    }
    if op.mirror() {
        properties["mirror"] = remote::mirror_prop();
    }
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": idempotent,
            "openWorldHint": false
        }
    })
}

/// `exclude`: boxes a stone tool leaves alone.
fn exclude_prop() -> Value {
    json!({"type": "array", "items": {"type": ["integer", "array"]}, "description": "Boxes left alone: one inclusive box [x0, y0, z0, x1, y1, z1] or a list of up to 64, such as a ledge top, a cave floor, or a river bed (the solid top layer, the air above it, or both). No cell inside one is changed, and nothing grows from or lands in one, so a walking surface needs no refill afterwards."})
}

/// The arguments `moss`, `algae`, and `vines` share, plus `extra`.
fn growth_props(extra: &[(&str, Value)], amount: &str, faces: &str) -> Value {
    let mut p = json!({});
    for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
        p[key] = int_prop("Inclusive box corner");
    }
    p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Plant palette indices. A single integer works too."});
    p["amount"] = num_prop(amount);
    p["noise"] = num_prop("0 grows in smooth patches, 1 in ragged speckle. Defaults to 0.5.");
    p["faces"] = json!({"type": ["string", "array"], "description": format!("Faces it grows on: top, side, bottom, all, or a list of them. Defaults to {faces}.")});
    p["facing"] = vec3_prop("Only faces whose outward normal points within 60 degrees of this direction, such as [0, -1, 0] for the front or [0, 0, -1] for undersides. Without faces, facing considers every face (vines: side); a facing that no face in faces points along is an error.");
    p["band"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Only cells with z in [low, high], such as a waterline band. algae thins from its bottom to its top and uses the box's height without one."});
    p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Grow only on cells of these palette indices (the stone slots). Omit for every slot."});
    p["layer"] = json!({"type": "string", "enum": ["grow", "paint"], "description": "grow adds cells on the empty side of each face; paint recolours the surface. Defaults to grow, paint for algae. vines always grow."});
    p["exclude"] = exclude_prop();
    p["seed"] = int_prop("Seed. Defaults to 0.");
    for (key, value) in extra {
        p[*key] = value.clone();
    }
    p
}

/// `mode` of the fills and `carve_from_views`, which also intersect.
fn fill_mode_prop() -> Value {
    json!({"type": "string", "enum": ["replace", "empty", "paint", "intersect"], "description": "replace (default) writes every cell of the shape; empty writes only empty cells, so existing detail stays; paint recolours only solid cells; intersect keeps only the object's solid cells inside the shape, as they are, and erases the rest (a boxy hull cut to an ellipsoid dome), so palette is not used."})
}

fn mode_prop() -> Value {
    json!({"type": "string", "enum": ["replace", "empty", "paint"], "description": "replace (default) writes every cell of the shape; empty writes only empty cells, so existing detail stays; paint recolours only solid cells."})
}

fn bool_prop(description: &str) -> Value {
    json!({"type": "boolean", "description": description})
}

fn str_prop(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

fn int_prop(description: &str) -> Value {
    json!({"type": "integer", "description": description})
}

fn num_prop(description: &str) -> Value {
    json!({"type": "number", "description": description})
}

fn vec3_prop(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "number"},
        "minItems": 3,
        "maxItems": 3,
        "description": description
    })
}

fn ivec3_prop(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "integer"},
        "minItems": 3,
        "maxItems": 3,
        "description": description
    })
}

fn vec4_prop(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "number"},
        "minItems": 4,
        "maxItems": 4,
        "description": description
    })
}

fn view_prop() -> Value {
    json!({
        "type": "string",
        "enum": ["front", "back", "left", "right", "top", "bottom"],
        "description": "Named view of a model facing -Y with +Z up, instead of yaw and pitch. front is yaw -90, back 90, left 0, right 180 (the model's own sides), top pitch 90, bottom pitch -90."
    })
}

fn target_prop() -> Value {
    json!({
        "anyOf": [
            {"type": "string", "enum": ["all", "active"]},
            {"type": "integer"},
            {"type": "object", "properties": {"min": {"type": "array", "items": {"type": "integer"}, "minItems": 3, "maxItems": 3}, "max": {"type": "array", "items": {"type": "integer"}, "minItems": 3, "maxItems": 3}}, "required": ["min", "max"], "additionalProperties": false}
        ],
        "description": "What the camera fits: all (default, every visible posed cell), active (the active object), an object index, or {min, max}, an inclusive box of world cells. Other objects are still drawn."
    })
}

fn xyz_props() -> Value {
    json!({
        "x": int_prop("Local x of the active object"),
        "y": int_prop("Local y"),
        "z": int_prop("Local z (up)")
    })
}

/// The properties of two `json!` objects in one, for a tool with more
/// properties than one `json!` expands within the recursion limit.
fn merged(mut first: Value, second: Value) -> Value {
    if let (Some(map), Value::Object(more)) = (first.as_object_mut(), second) {
        map.extend(more);
    }
    first
}

/// The mask edits a concept or a view takes (`compare_silhouette`,
/// `project_paint`, and every view of `carve_from_views` and
/// `fit_to_views`). `sheet` leaves out `mask` and `erase`, which only a view
/// or a single image has, for the sheet-wide keys.
fn mask_edit_props(sheet: bool) -> Value {
    let suffix = if sheet {
        ", for every view without its own"
    } else {
        ""
    };
    let mut props = json!({
        "excludeColors": {"type": "array", "items": {"type": ["string", "array"]}, "description": format!("Colours (#rrggbb or [r, g, b]) whose pixels are background too, such as water and splashes{suffix}.")},
        "excludeTolerance": num_prop(&format!("RGB distance from an excludeColors colour that still goes, 0..442. Defaults to 40{suffix}.")),
        "flood": {"type": ["boolean", "number", "object"], "description": format!("Flood fill from every pixel of the crop's border, stepping to a neighbour within this RGB distance of the pixel it comes from, and drop what it reaches: a photo-like or gradient background. true is 24; a number or {{tolerance}} sets it{suffix}.")},
        "keepLargest": {"type": "boolean", "description": format!("Keep only the largest connected inked region, dropping specks and a separate splash{suffix}.")},
        "fillHoles": {"type": ["boolean", "number"], "description": format!("Ink again, in the image's own colour, every background region the ink encloses (not reaching the crop's border) that holds at most this share of the view's inked pixels: white eyes and teeth on a white sheet, which the background rule keys out. true is 0.1. Needs a path{suffix}.")}
    });
    if !sheet {
        props["mask"] = json!({"type": ["string", "object"], "description": "A mask image: a PNG path or {path, crop, background, tolerance, alphaThreshold, invert}. It is cut as a view is (auto background: transparent pixels, else the border colour, so white on black works), stretched over the crop, and only the pixels under its ink stay (invert: they go). A mask as large as the image shares its crop."});
        props["erase"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "description": "Rectangles [x, y, width, height] in image pixels (as crop is) whose pixels go: a rider, a label."});
    }
    props
}

/// The export option `bake`. Its own function keeps `json!` within the
/// recursion limit, like [`lighting_prop`].
fn bake_prop() -> Value {
    json!({"type": ["object", "boolean"], "description": "Paint light into the colour atlas by real rays, one texel per cell face, to draw unshaded; needs colorAtlas. The glb material gets KHR_materials_unlit and extras.froxelBake. A texel is albedo x (ambient x ambientEnergy x (1 - occlusion x blocked) + sun x sunEnergy x max(n.l, 0) x open) in linear light, clamped, encoded sRGB: blocked is the share of cosine-weighted rays that hit an opaque cell within distance (a hit at t counts 1 - (t/distance)^2), open the share of sun rays that get out. Fields, all optional: occlusion 0..1 (default 1), distance in cells up to 4096 (default 16), rays 1..1024 per cell face (default 64), preset (a lighting preset such as old_pond) or sunDirection [x, y, z] toward the sun, Z up, or godotSunBasis; sunColor and ambientColor ([r, g, b] 0..16 sRGB, or #rrggbb); sunEnergy; ambientEnergy; lightingPresets (as preview takes it); sunSoftness, the sun disc's half angle in degrees 0..45 (default 0, a hard shadow), and sunRays 1..256 (default 1 hard, 16 soft). With no preset and no sun or ambient field it is occlusion alone under a white ambient of 1 (sun off); named lighting starts from sun 1, ambient 0.3. A preset's exposure, background and lut are not baked, because a game's tonemapper and grade reach unshaded surfaces too. Deterministic: no noise, and the same input gives the same bytes. Triangle counts do not change. true is every default. Absent or false is off and leaves the file byte for byte."})
}

/// MCP `preview` `lighting`. Its own function keeps `json!` within the
/// macro recursion limit.
fn lighting_prop() -> Value {
    json!({"type": ["string", "object"], "description": "A game's light instead of the viewport's: a preset name (viewport, the default; old_pond, frog-world's Old Pond region; or one from lightingPresets or the user's lighting_presets.json), or an object of lighting fields: sunDirection [x, y, z] toward the sun, Z up, or godotSunBasis (the 9 or 12 numbers of a DirectionalLight3D's Transform3D as the .tscn writes it); sunColor and ambientColor ([r, g, b] 0..16 sRGB, or #rrggbb); sunEnergy and ambientEnergy; exposure; background; lut (a 3D LUT PNG, N*N by N, applied last as Godot's colour correction); preset (the preset these fields go over). Shading is albedo x (sun x energy x max(n.l, 0) + ambient x energy) x exposure in linear light, clamped, with no shadows. Replies lighting with the values used."})
}

/// MCP `preview`, `compare_silhouette`, and `set_view` `presets`.
fn camera_presets_prop() -> Value {
    json!({"type": ["object", "string"], "description": "A project's camera presets: an object of name to {yaw, pitch, orthographic, fov, margin}, or the path of a JSON file of them (or with them under presets). godotLook [x, y, z], a Godot camera's look direction, replaces yaw and pitch. A name that exists further down (game, default, or one in the user's camera_presets.json or the file FROXEL_CAMERA_PRESETS names) is overridden field by field; another name is a new preset over default."})
}

/// MCP `preview`, `compare_silhouette`, and `set_view` `placementYaw`.
fn placement_yaw_prop() -> Value {
    num_prop("Degrees: the asset's turn about up where it is placed in a level, positive counter-clockwise from above, the same number as a Godot node's rotation_degrees.y. The camera (and a lighting sun) turn the other way, so the image is the asset as it stands in the level from the same camera. yaw in the reply is the angle used, yaw minus this.")
}

/// MCP `preview` `lightingPresets`.
fn lighting_presets_prop() -> Value {
    json!({"type": ["object", "string"], "description": "Lighting presets for this call: name to lighting fields, or the path of a JSON file of them (also under lighting). A built-in name is overridden field by field; a relative lut path is relative to the file."})
}
