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
        toggle(ui(json!({"type": "string", "enum": ["polyline", "smooth"], "description": "polyline (default) or smooth (Catmull-Rom through the points)."}), "Smooth", "A curve through the points instead of straight segments"), "smooth"),
        toggle(ui(json!({"type": "string", "enum": ["round", "flat"], "description": "round (default): half balls of the radius past the ends. flat: cut square by the planes across the path at the first and last points; needs points apart."}), "Flat ends", "Cut the tube square at its first and last points instead of rounding them, for pipes, discs, rims, and wheels"), "flat"),
        soft(ui(hard(with_default(num_prop("Thickness over width, 0.05..=1. 1 (the default) is round. At least one cell; a level tube flattens vertically, lying flat."), json!(1.0)), 0.05, 1.0), "Flatten", "Thickness over width; 1 is round"), 0.05, 1.0, 2),
        soft(ui(with_default(num_prop("Degrees to turn a flat or blade cross-section about the axis; 90 stands a level shard on edge. Defaults to 0."), json!(0.0)), "Roll", "Degrees to turn a flat or blade cross-section about the axis"), -180.0, 180.0, 0),
        toggle(ui(json!({"type": "string", "enum": ["round", "blade"], "description": "blade: a wedge, full thickness at one edge and one cell at the other, for splinters and broken plank ends. Defaults to round."}), "Blade", "A wedge: full thickness at one edge, one cell at the other"), "blade"),
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
        super::bounds::with_box_forms(&mut tools);
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
        define("batch", &format!("Apply editing tools to the active object and the palette in order as one undo step, instead of one call per board or cut. The list is checked first: an unknown option or a tool that cannot run here refuses it before anything changes. The first error stops it, names the failing item (from 0), and leaves the document as it was with no undo step. At most {} items, each taking its tool's arguments. Replies with items, changed (summed over items), voxels, and results (each item's tool and counts in short form). Refused: tools that write files, readers, undo, and anything that changes the active object or the document (call set_active_object first and save afterwards).", remote::MAX_BATCH_ITEMS), json!({
            "operations": {"type": "array", "minItems": 1, "maxItems": remote::MAX_BATCH_ITEMS, "description": "The items in order. Each is {tool, arguments}, arguments as the tool itself takes them.", "items": {"type": "object", "properties": {
                "tool": {"type": "string", "enum": remote::batch_tool_names(), "description": "The tool to run."},
                "arguments": {"type": "object", "description": "The tool's arguments."}
            }, "required": ["tool"], "additionalProperties": false}}
        }), &["operations"]),
        define("new_document", "Replace the session with an empty one-object document and default palette.", json!({}), &[]),
        define("open_file", "Open a .vox file, replacing the session document. Replies with path, objectCount, voxels, world bounds, activeObject, canUndo, canRedo, bones (count), paletteSlots (used), motionSlots, the first 20 objectNames, moreObjects (names left out), and warnings (chunks not read, such as a newer Froxel's rig, clip, script, or markers; the voxels open, a save drops them). Froxel files open in their saved frame (older ones with a script or markers too); foreign files open centred and floored. World positions differing from the file's node positions are stated as sceneShift and sceneShiftNote, as in inspect_document and the exports.", json!({
            "path": str_prop("Path of a .vox file"),
            "detail": {"type": "string", "enum": ["summary", "full"], "description": "summary (default) is the short reply; full is the inspect_document result."},
            "fullPalette": {"type": "boolean", "description": "With detail full, all 256 palette slots, not only used ones. Defaults to false."}
        }), &["path"]),
        define("save_file", "Write every non-empty object as a .vox scene. World voxel and marker positions, negative too, survive reopening, so the reopened document exports the same glTF frame and marker values; local cells may rebase to the model box. Replies with voxels (solid cells in the file) and objects (models written).", json!({"path": str_prop("Where to write. Omit to overwrite the open file. Its folder is created.")}), &[]),
        define("inspect_document", "Path, voxel count, world bounds, objects (name, world position, local bounds, skin and bricks), palette colours by index for the slots some voxel uses (fullPalette: all 256), and motion by palette index for slots not still at amount 1.", json!({
            "fullPalette": {"type": "boolean", "description": "List all 256 palette slots, not only the used ones. Defaults to false."}
        }), &[]),
        define("get_voxel", "Read one local cell of the active object; 0 is empty.", xyz_props(), &["x", "y", "z"]),
        define("set_voxel", "Write one local cell of the active object. One undo step.", {
            let mut p = xyz_props();
            p["palette"] = int_prop("Palette index. 0 erases. 1..255 paint.");
            p
        }, &["x", "y", "z", "palette"]),
        define("set_voxels", "Write up to 200000 local cells of the active object as one undo step.", json!({"cells": {"type": "array", "description": "{x,y,z,palette} or [x,y,z,palette] per cell. Last duplicate wins."}}), &["cells"]),
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
        define("set_palette", "Replace one palette slot, or many via entries, as one undo step. Give color or r, g, b (and a), not both. Every field left out keeps the slot's value. material and its roughness, metallic, transparency, ior, emission, and power are the slot's Render material, as the Render panel's Material section sets it.", json!({
            "index": int_prop("Palette slot 0..255"),
            "color": {"type": ["string", "array"], "items": {"type": "integer"}, "description": "\"#rrggbb\" or \"#rrggbbaa\" (# optional), or [r, g, b] or [r, g, b, a] in 0..255."},
            "r": int_prop("Red 0..255"),
            "g": int_prop("Green 0..255"),
            "b": int_prop("Blue 0..255"),
            "a": int_prop("Alpha 0..255. Defaults to 255, or the alpha of a #rrggbbaa color."),
            "motion": str_prop("none, sway, or bob."),
            "motionAmount": soft(ui(num_prop("Motion strength, 0..4. Default 1 when a kind is first set."), "Amount", "Motion strength"), 0.0, 4.0, 2),
            "material": ui(json!({"type": "string", "enum": ["diffuse", "metal", "emissive", "glass"], "description": "diffuse, metal, emissive, or glass."}), "Material", "The slot's material in Render mode and the tracers"),
            "roughness": soft(ui(num_prop("metal, glass: surface roughness, 0..1; 0 is a mirror or clear glass."), "Roughness", "Surface roughness; 0 is a mirror"), 0.0, 1.0, 2),
            "metallic": soft(ui(num_prop("metal: how metallic, 0..1."), "Metallic", "How metallic the surface is"), 0.0, 1.0, 2),
            "transparency": soft(ui(num_prop("glass: how much light passes through, 0..1."), "Transparency", "How much light passes through"), 0.0, 1.0, 2),
            "ior": soft(ui(num_prop("glass: index of refraction, 1..3."), "IoR", "Index of refraction"), 1.0, 3.0, 2),
            "emission": soft(ui(num_prop("emissive: light given off, 0..100."), "Emission", "Light given off"), 0.0, 100.0, 0),
            "power": soft(ui(int_prop("emissive: a whole multiplier of the emission, 1..5."), "Power", "Whole multiplier of the emission"), 1.0, 5.0, 0),
            "motion_amount": num_prop("Older name of motionAmount; still accepted."),
            "entries": {"type": "array", "description": "Many slots: {index} plus any of the fields above, such as {\"index\": 3, \"color\": \"#5c6838\"}. Omit when passing index."}
        }), &[]),
        define("set_active_object", "Edit a different object from inspect_document. Later voxel tools use that object's local cells.", json!({"index": int_prop("Object index")}), &["index"]),
        define("add_object", "Add an empty object at a world voxel position; returns its index. One undo step.", json!({
            "name": str_prop("Object name. Omit for Object N."),
            "position": ivec3_prop("World position [x, y, z] in voxel units. Defaults to [0, 0, 0]."),
            "size": ivec3_prop("Optional SIZE box [x, y, z], a local capacity hint, each axis 1..=256; the object still grows up to 256 per axis."),
            "makeActive": {"type": "boolean", "description": "Select the new object. Defaults to true."},
            "make_active": {"type": "boolean", "description": "Older name of makeActive; still accepted."}
        }), &[]),
        define("set_object", "Rename, move, or hide an object. position is in world voxels; local cells stay put, so the voxels move with it. One undo step.", json!({
            "index": int_prop("Object index"),
            "name": str_prop("New name"),
            "position": ivec3_prop("World position [x, y, z] in voxel units"),
            "hidden": {"type": "boolean", "description": "Hide the object. Hidden objects stay in the file and outliner."}
        }), &["index"]),
        define("remove_object", "Remove an object and its voxels. A bone bound to it stays, unbound. Later object indices drop by one, as the reply says. Refuses the last object. One undo step.", json!({
            "index": int_prop("Object index")
        }), &["index"]),
        define("fill_ellipsoid", "Fill the ellipsoid inscribed in an inclusive local box. Limits as fill_box, checked before any cell is written. One undo step.", json!({
            "x0": int_prop("Inclusive minimum x"),
            "y0": int_prop("Inclusive minimum y"),
            "z0": int_prop("Inclusive minimum z"),
            "x1": int_prop("Inclusive maximum x"),
            "y1": int_prop("Inclusive maximum y"),
            "z1": int_prop("Inclusive maximum z"),
            "palette": int_prop("Palette index. 0 erases. Required except with mode intersect."),
            "mode": fill_mode_prop()
        }), &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("fill_rounded_box", "Fill an inclusive local box with rounded edges. radius is clamped to half the shortest side. Limits as fill_box. One undo step.", json!({
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
        define("fill_tube", "Fill a round tube through 2..=1024 local points, for limbs, roots, branches, and twigs; each point's radius tapers it; joints are round. A whole-number point is a cell centre; a cell fills when its centre is within the radius of the axis. caps flat makes pipes, discs, rims, barrels, wheels: points [0, 0, 0] and [0, 13, 0] at radius 12 fill y 0..=13; the same at palette 0, radius 11 then hollows a one-cell wall; points half a cell apart make a one-cell disc. aspect, roll, and profile make shards, splinters, and plank ends. At most 2000000 cells; refused before any write. One undo step.", {
            let mut p = json!({
            "points": {"type": "array", "description": "[x, y, z], [x, y, z, radius], or {x, y, z, radius?}; fractions allowed."},
            "radius": tube_radius_prop("Radius for points that give none, 0.5..=128. 0.5 is one cell thick."),
            "palette": int_prop("Palette index. 0 erases. Pass this or palettes."),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "1..=64 palette indices cycled by pattern. Repeat an index to weight it."},
            "pattern": ui(json!({"type": "string", "enum": ["rings", "stripes", "random"], "description": "rings: bands every period cells along the length; stripes: sectors around it (grooves); random: per cell by seed. Defaults to rings."}), "Colours", "How the palettes are laid out along and around the tube"),
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
        define("generate", "Fill a generated shape fitted to an inclusive local box (its size is the box's), the editor's Generator tool: kind ellipsoid (inscribed), rounded_box, cylinder, cone, pyramid, torus, perlin (3D noise coloured paletteLow..paletteHigh by height above threshold), tree (a branching palette trunk with leaf clumps), helix, or conifer (tiers of needle clumps on a palette trunk). Each option names its kinds; other kinds ignore it. Deterministic: noise and branching follow only the cells and seed. At most 2000000 cells, refused before any is written. One undo step.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["kind"] = ui(json!({"type": "string", "enum": ["ellipsoid", "rounded_box", "cylinder", "cone", "pyramid", "torus", "perlin", "tree", "helix", "conifer"], "description": "The shape to generate."}), "Generator", "The shape generated inside the box");
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
        define("recolor", "Replace slot from with slot to (or every pair of map) in the active object's solid cells, in an inclusive box or the whole object, with no shape change: a blockout slot becomes the shell slot, two shades merge, a belly is re-tinted. Other cells stay. A slot is mapped once, from the cells as they were, so 3 to 4 and 4 to 3 swap. One undo step. Replies with matched (cells of a mapped slot), changed, and voxels.", json!({
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
        define("deterrace", "Remove contour terraces from a voxelized smooth mesh (import_mesh of an image-to-3D model): one-cell steps in long even bands on gentle slopes that read as stripes. blocks re-quantizes the active object into size-cell cubes on a grid through origin: a cube is solid when fill of its cells are and takes their commonest slot, so steps are size cells tall (the chunky concept-cube look); a cube that would move the surface over tolerance cells stays, so thin parts survive. stagger works on the top surface (each column's highest cell): where a column is one cell above a face neighbour, the step edge moves one cell (low side up or high side down) in alternating teeth run cells long, jitter of them flipped by seed, so no step runs long and even; nothing moves over one cell. Optional box x0..z1. One undo step. Replies with mode, added, removed, steps and longestRun (top-surface one-cell steps and their longest straight run, before and after), kept (blocks left as they were), changed, and voxels.", json!({
            "mode": {"type": "string", "enum": ["blocks", "stagger"], "description": "blocks (default) or stagger."},
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
        define("split_parts", "Cut ONE blockout (from import_mesh or carve_from_views) into rig parts in one call, instead of many box-based split_object calls. Seeds (point: a source cell, or a world cell with space world; points: several, e.g. along a leg; bone: its pivot; marker: a point marker) snap to the nearest solid cell within reach. Each solid cell joins the part with the nearest seed through the solid (geodesic: multi-source Dijkstra), so a jaw seed does not take the cheek across the mouth and a leg ends at the body; euclidean is the fallback for cells no path reaches (unreached). planes [{normal, at}] keep a part where (c - at) . normal >= 0, c a cell centre and at a cell like point, to cut a neck or tail root at a set place. Each part becomes an object at world [0, 0, 0] holding world cells (the hand-sculpting rig convention), so add_bone pivots read straight off the cells and bind_object needs no cell moves; an existing object of that name is replaced and moved there. The source is removed unless a part takes its name or removeSource is false. One undo step. Replies with parts (name, index, cells, seeds, bone, bounds), unreached, metric, sourceRemoved, and objects.", json!({
            "parts": {"type": "array", "items": {"type": "object"}, "description": "1..64 parts: {name, point: [x, y, z] | points: [[x, y, z], ...] | bone: name | marker: name, planes?: [{normal: [x, y, z], at: [x, y, z]}]}."},
            "object": {"type": ["integer", "string"], "description": "The source object by index or name. Defaults to the active one."},
            "metric": {"type": "string", "enum": ["geodesic", "euclidean"], "description": "geodesic (default): distance through the solid; euclidean: straight-line distance."},
            "connectivity": {"type": "integer", "enum": [6, 26], "description": "Geodesic neighbours: 26 (default) faces, edges, and corners; 6 faces only."},
            "reach": num_prop("Cells a seed may snap to the nearest solid cell, 0..64. Defaults to 8."),
            "removeSource": with_default(json!({"type": "boolean", "description": "Remove the emptied source object. Defaults to true; a part named like the source keeps it."}), json!(true)),
            "bind": with_default(json!({"type": "boolean", "description": "Bind each part to the bone of its name when there is one. Defaults to true."}), json!(true)),
            "space": remote::space_prop()
        }), &["parts"]),
        define("reduce_palette", "Clean up a voxelized mesh's colours on the active object (import_mesh of an image-to-3D model leaves many near-equal slots and a per-cell ripple): reduce, then despeckle; no cell is added or removed. colors N clusters the surface colours into N (median cut and k-means in Oklab), an accent guard keeping small distinct colours (eyes, mouth, nostril) their own slot; each cluster takes over in place its commonest slot no other cell uses, or a free slot. palette instead snaps each colour to the nearest listed slot or #rrggbb (one not in the palette takes a free slot). despeckle: a surface cell whose slot fewer than minSame of its 26 surface neighbours share takes the slot over half of them hold; specks and pairs go, lines and patches stay. Both passes skip protect slots. An optional box x0..z1 limits it. One undo step. Replies with slots {before, after} (distinct slots), clusters (slot, color, cells), accents, despeckled, recolored (slots that took a new colour in place), newSlots, changed, and voxels.", json!({
            "colors": int_prop("Cluster the colours into this many, 1..64."),
            "palette": {"type": "array", "items": {"type": ["integer", "string"]}, "description": "Instead of colors: snap each colour to the nearest of these slots or #rrggbb colours."},
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
        define("components", "Clean up the connected parts of the active object, such as an image-to-3D mesh (import_mesh) with extras: a rider on a shell, moss tufts, foam splashes. op list reports the parts by connectivity, largest first; keep_largest erases all but the keep largest; remove_small erases parts under minCells cells (floaters); delete, move (by offset), and split (to a new object name, or into an existing one, keeping world positions; index is the source) act on the part holding seed (snapped to the nearest solid cell within reach). With anchor (a cell of the part that stays), that part is first cut at its neck: the fewest cells separating seed from anchor (a minimum cut by max flow, outside radius around each); the cut cells stay with the anchor. One undo step. Replies with count, largest, components (up to 32: cells, min, max, seed), selected, seed, anchor, cut, changed, and voxels (split: target, name, created, sourceVoxels, targetVoxels).", json!({
            "op": {"type": "string", "enum": ["list", "keep_largest", "remove_small", "delete", "move", "split"], "description": "What to do. list (default) only reports."},
            "connectivity": {"type": "integer", "enum": [6, 18, 26], "description": "Neighbours that join cells: 6 faces, 18 faces and edges, 26 (default) also corners."},
            "keep": int_prop("keep_largest: how many of the largest parts stay. Defaults to 1."),
            "minCells": int_prop("remove_small: parts with fewer cells are erased."),
            "seed": {"type": "array", "items": {"type": "number"}, "description": "delete, move, split: a local cell [x, y, z] of the part to act on."},
            "anchor": {"type": "array", "items": {"type": "number"}, "description": "A local cell [x, y, z] of the part that stays; seed's part is cut from it at the thinnest neck between them."},
            "reach": num_prop("How far seed and anchor may be from the solid cell they snap to, 0..=64. Defaults to 8."),
            "radius": num_prop("Geodesic radius in cells around seed and anchor that the neck cut stays out of. Defaults to 0.3 of their geodesic distance."),
            "offset": {"type": "array", "items": {"type": "integer"}, "description": "move: whole cells [dx, dy, dz]."},
            "index": {"type": ["integer", "string"], "description": "split: the source object (index or name). Defaults to the active one."},
            "name": str_prop("split: name of the new object. Defaults to <source>_part."),
            "into": {"type": ["integer", "string"], "description": "split: an existing object (index or name) to put the part in, instead of a new one."},
            "copy": bool_prop("split: leave the part in the source too."),
            "makeActive": bool_prop("split: make the target the active object.")
        }), &[]),
        define("surface_stroke", "Paint a stroke through points onto the active object's surface: plate seams, markings on a curved body, grooves (a paint-mode fill_tube may miss the surface). The path is sampled every half cell; each sample snaps to the nearest surface cell (solid, with an empty face neighbour) within reach, so points may float or sink. Solid cells within width / 2 of a snapped cell, at most depth deep, take palette (0 erases: a groove). Nothing is added. One undo step. Replies with cells, changed, samples, missed (samples with no surface in reach), snap (farthest a sample moved), length, and voxels.", json!({
            "points": {"type": "array", "description": "2..=1024 points [x, y, z] or {x, y, z} in local cells (a whole number is a cell centre; fractions allowed)."},
            "palette": int_prop("Slot the stroke paints, 1..255, or 0 for a groove."),
            "curve": {"type": "string", "enum": ["polyline", "smooth"], "description": "polyline (default) or smooth (Catmull-Rom through the points)."},
            "width": num_prop("Width across the surface in cells, 0.5..=32. Defaults to 1, a one-cell line."),
            "depth": int_prop("Cells into the solid, 1..=16. Defaults to 1, the surface alone."),
            "reach": num_prop("How far a sample may be from the surface cell it snaps to, 0..=64. Defaults to 8."),
            "only": {"type": ["integer", "array"], "items": {"type": "integer"}, "description": "Only cells of these slots change, such as the shell slots, so a seam stops at the skin."}
        }), &["points", "palette"]),
        define("carve", "Remove the active object's solid cells inside a box, ellipsoid, rounded_box, or tube: hollows, tunnels, pits (a polygon or rounded rectangle along a closed or open path: sweep with palette 0; everything outside a surface: trim_outline). box, ellipsoid, and rounded_box take x0..z1 (and radius) as their fill tools; tube takes points, radius, curve, and caps as fill_tube. At most 2000000 cells, checked before any cell changes; a tube also at most 512000000 cells in the boxes around its segments, a closed path counting as an open one. One undo step. Replies with removed.", {
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
        define("smooth", "Soften a blocky region of the active object with a 26-neighbour majority filter in an inclusive local box of at most 2000000 cells: an empty cell with at least fillAt solid neighbours takes their commonest colour; a solid one with at most removeAt goes. The defaults fill one-cell pits and inside corners, remove one-cell spikes and outside corners, keep flat faces and edges, and also remove one-cell-thick features (twigs, thin walls): keep those out of the box. Cells outside are read, never changed. One undo step. Replies with filled, removed, and the passes run.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["iterations"] = with_default(int_prop("Passes, 1..=8. Defaults to 1. Stops early when a pass changes nothing."), json!(1));
            p["fillAt"] = with_default(int_prop("Fill an empty cell with at least this many solid neighbours, 1..=27. Defaults to 17; 14 also rounds inside edges; 27 never fills."), json!(17));
            p["removeAt"] = with_default(int_prop("Remove a solid cell with at most this many solid neighbours, -1..=25. Defaults to 9; 12 also rounds outside edges; -1 never removes."), json!(9));
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("break_end", "Break the active object across a plane: a snapped trunk, branch, plank, or post. Cells beyond a jagged surface round the plane go: 2 by 3 cell blocks across the face dip up to depth in, a quarter of them deep pits. Cells facing the cut take bark on the rim, sapwood inside; tapered flat splinters grow from the new face along the normal into empty cells only, longer and commoner near the rim by rimBias. Deterministic for a seed. At most 512 splinters and 2000000 cells. One undo step. Replies with removed, recoloured, and splinters.", json!({
            "point": vec3_prop("A point on the break plane, in local cells."),
            "normal": vec3_prop("Out of the wood, toward the part that breaks off. Need not be unit length."),
            "depth": num_prop("How far pits and the uneven face reach into the wood, 0..=32 cells. Defaults to 3."),
            "splinters": int_prop("Number of splinters, 0..=512. Pass this or density."),
            "density": num_prop("Splinters per cell of the new face, 0..=1. Defaults to 0.15."),
            "length": {"type": "array", "items": {"type": "number"}, "description": "Splinter length [min, max] in cells, 1..=64. Defaults to [4, 12]."},
            "thickness": num_prop("Splinter radius at its base, 0.5..=4. Defaults to 1."),
            "taper": num_prop("0 keeps the base radius to the tip; 1 (default) thins it to one cell."),
            "rimBias": num_prop("0..=1. How much more often and longer splinters are near the rim. Defaults to 0.5."),
            "bark": int_prop("Palette index of the new face's rim and the base of rim splinters. Omit to keep the cells' colour."),
            "sapwood": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices mixed over the new face and splinters. Omit to keep the cells' colour."},
            "tips": int_prop("Palette index of each splinter's outer part, often a lighter wood. Omit to use the sapwood."),
            "seed": int_prop("Seed. Defaults to 0.")
        }), &["point", "normal"]),
        define("scatter", "Scatter debris (shard, chip, pebble, chunk) into the active object: a heap at a break, chips at a stump, gravel. Each piece starts at the inclusive box's top, within its x and y, and drops until the next step would touch a solid cell (model or earlier piece) or pass floor, so pieces pile up. Cells only, no physics; only empty cells are written. A piece blocked at the box's top or resting in an exclude box is skipped. Deterministic for a seed. At most 2000 pieces, a box of 2000000 cells, and 200000 new cells. One undo step. Replies with pieces and skipped.", {
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
        define("joints", "Cut joint gaps into the solid cells of an inclusive box of the active object: fractured limestone, masonry, cracked rock. pattern: columns (random-width vertical columns, a jittered grid from above, an occasional bed joint), courses (a running bond: block courses whose vertical joints shift each course), or cracks (an irregular network of jagged pieces). Boundary cells are removed depth cells deep; the solid cell at each gap's bottom takes dirt. exclude boxes keep walking surfaces whole. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with removed and dirtied.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["pattern"] = json!({"type": "string", "enum": ["columns", "courses", "cracks"], "description": "Stone layout."});
            p["spacing"] = json!({"type": "array", "items": {"type": "number"}, "description": "Stone width range [min, max] in cells, 2..=128, or one number: column width, block length, or crack piece size. Defaults to [4, 9] for columns, [6, 11] for courses, [6, 11] for cracks."});
            p["course"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Course height range [min, max] in cells, 1..=128 (courses), or the spacing of bed joints across a column (columns). Defaults to [3, 5] and [10, 24]. Cracks ignore it."});
            p["width"] = int_prop("Gap width in cells, 1..=3. Defaults to 1.");
            p["depth"] = int_prop("Gap depth under the surface in cells, 1..=16. Defaults to 2.");
            p["dirt"] = int_prop("Palette index of the solid cell at each gap's bottom, such as dark dirt. Omit to keep its colour.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Cut only cells of these palette indices (the stone slots), so moss and plants stay. Omit for every slot."});
            p["density"] = num_prop("cracks: the share of piece boundaries that crack, 0..=1. Defaults to 0.6.");
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "pattern"]),
        define("column_relief", "Break a cliff face of the active object into columns (fractured limestone): random widths standing out by random amounts, ending at uneven heights around the crown. The face is found in the inclusive box, so a curved swept wall works: per step along it, the median of the rows' frontmost solid cells up to the crown, which a ledge does not pull forward. A column fills empty cells from depth behind the face to out in front, box bottom to top. With lower, a column ending below the crown cuts the wall above it depth cells back; without, cells are only added and short columns end in front of the wall. Writes stay in the box and out of exclude boxes. Run after sweep, before joints and weather. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with columns, added, removed, and crown.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner, a few cells beyond the wall so columns can stand out");
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
        define("weather", "Weather the exposed cells of an inclusive box of the active object: chip edges and corners (small irregular chunks off cells exposed on two or more sides), dent flat faces with pits (real holes, not paint), and add subtle colour noise in soft patches. Faces a chip or pit uncovers take the broken slot. exclude boxes keep walking surfaces whole. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with chips, pits, removed, broken, and recoloured.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["chip"] = num_prop("Chance an edge cell starts a chip, 0..=1; corners are likelier. Defaults to 0.15. 0 chips nothing.");
            p["chipSize"] = int_prop("Most cells one chip takes, 1..=8. Defaults to 2.");
            p["pits"] = num_prop("Chance that a flat face cell starts a pit, 0..=1. Defaults to 0.02.");
            p["pitDepth"] = int_prop("Deepest pit in cells, 1..=4. Defaults to 1.");
            p["broken"] = int_prop("Palette index for the faces a chip or pit uncovers: usually a slightly darker stone (about 0.9 x), not dirt. Omit to keep their colour.");
            p["noise"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices of the colour noise: a few tones within a few units of the stone. Omit for no noise."});
            p["noiseDensity"] = num_prop("Share of exposed cells the noise recolours, 0..=1. Defaults to 0.35.");
            p["noiseScale"] = num_prop("Noise patch size in cells, 1..=32. Defaults to 3.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices (the stone slots). Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        define("moss", "Grow moss on the exposed faces of an inclusive box of the active object that pass the face filter. style caps (default): cushions on tops, undersides when faces takes bottom, and ragged-ended curtains down side faces from top edges, in soft noise patches mixed with speckle. style cushions: separate irregular cushions of size cells where the surface faces up (or facing) within exposure, gap cells apart until amount is covered, never one continuous band (facing [0, 0, -1] with layer paint shades undersides, such as a tree crown's); with layer grow each domes up to thickness at its middle, and drapes run from its edge down the side and hang free below overhangs. layer grow writes only empty cells, on each face's empty side; paint recolours surface cells. palettes are picked per patch or cushion (darker and lighter moss). Cut joints, bark, and weather first. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, painted, and curtains, or cushions and drapes, and warning (why) when nothing grew.", merged(growth_props(&[
            ("curtains", num_prop("caps: share of moss-capped top edges a curtain runs down from (any top edge when faces leaves out top), 0..=1. Defaults to 0.3. 0 grows caps only.")),
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Curtain length range [min, max] in cells, 1..=64 (caps), or drape length, 1..=32 (cushions), or one number. Defaults to [2, 8] for caps and [2, 6] for cushions."})),
            ("thickness", int_prop("Cushion thickness, 1..=3 on caps tops, 1..=4 at a cushion's middle (layer grow). Defaults to 1 for caps and 2 for cushions.")),
        ], "Coverage, 0..=1: of the passing faces (caps), or of the exposed cells (cushions). Defaults to 0.5 for caps and 0.45 for cushions.", "top and side"), json!({
            "style": {"type": "string", "enum": ["caps", "cushions"], "description": "caps (default): caps and curtains. cushions: separate cushions with drapes from overhangs."},
            "size": {"type": "array", "items": {"type": "integer"}, "description": "cushions: cushion radius range [min, max] in cells, 1..=16, or one number. Defaults to [2, 5]."},
            "gap": int_prop("cushions: fewest cells between two cushions, 1..=8. Defaults to 2."),
            "exposure": num_prop("cushions: how far a cell must face up (or facing): the cosine of its surface normal with that direction, -1..=1. 0 takes a log's upper half, 0.7 only its crown. Defaults to 0.35."),
            "drapes": num_prop("cushions: share of cushion edges over a drop that a drape hangs from, 0..=1 (layer grow). Defaults to 0.35. 0 hangs none.")
        })), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("algae", "Paint algae on the exposed faces of an inclusive box of the active object within a height band (band), usually the waterline: dense at its bottom, thinning upward in noisy clumps with stone peeking through. palettes run bottom to top (darker low, lighter high). Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with added and painted.", growth_props(&[], "Coverage at the bottom of the band, 0..=1. Defaults to 0.9.", "top and side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("vines", "Hang vine strands from the top edges of an inclusive box of the active object: each starts beside an edge cell whose side passes the face filter and falls straight down through empty cells, wandering a little along the edge, until its length or something solid. palettes grade from anchor to tip, so give them motion sway rising from the anchor (set_palette motion). blooms adds occasional bloom cells beside strands. Only empty cells are written. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, strands, and blooms.", growth_props(&[
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Strand length range [min, max] in cells, 1..=128, or one number. Defaults to [4, 16]."})),
            ("blooms", json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices of blooms beside the strands. Omit for none."})),
            ("bloomChance", num_prop("Chance of a bloom beside each strand cell, 0..=1. Defaults to 0.06.")),
        ], "Share of top edge cells a strand starts from, 0..=1. Defaults to 0.08.", "side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("bark", "Cut bark into the wood of an inclusive box of the active object along the grain: axis (through the solid's middle) or points and curve, the path of the fill_tube or sweep that made the trunk, branch, or root. Use it instead of painted stripes. The surface splits into long plates (width around, length along) between fissures depth cells deep, their bottom cell taking dirt; plate ends stagger between columns as on a real trunk, and a cracks share of plates crack part way across. Each plate takes one of palettes. End grain and a hollow's inside are left alone; a hollow's inner wall is never cut through. A flat or gently curved plate (deck, plank, wall: at least three times as wide as deep) gets width columns rim to rim; one too thin for both faces is cut from its free face (the upper if both), never through; one cell thick, it only takes tones. Then cavity_tint the box to shade the fissures, and grow moss. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with plates, removed, dirtied, painted, cracks, and warning (why) when a shape could not be cut.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["axis"] = json!({"type": ["string", "array"], "description": "The grain along a straight axis: x, y, z, or a vector [x, y, z]. Pass this or points."});
            p["points"] = json!({"type": "array", "items": {"type": ["array", "object"]}, "description": "The grain along a path: [x, y, z] points, or the wood's fill_tube ([x, y, z, radius]) or sweep ([x, y, z, twist, scale]) points, extra numbers ignored. Pass this or axis."});
            p["curve"] = str_prop("With points: polyline (default) or smooth, as the tube used.");
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Plate slots, a few bark tones within a few units of each other; one per plate."});
            p["dirt"] = int_prop("Palette index of the cell at each fissure's bottom, a dark brown. Omit to keep its colour.");
            p["width"] = json!({"type": "array", "items": {"type": "number"}, "description": "Plate width range [min, max] in cells around the wood, or across a flat plate, 1..=64, or one number. Defaults to [3, 6]."});
            p["length"] = json!({"type": "array", "items": {"type": "number"}, "description": "Plate length range [min, max] in cells along the grain, 2..=256, or one number. Defaults to [8, 20]."});
            p["depth"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Fissure depth range [min, max] in cells, 1..=4, or one number. Plate ends and cracks take the minimum. Defaults to [1, 2]."});
            p["cracks"] = with_default(num_prop("Share of plates with a crack part way across, 0..=1. Defaults to 0.25."), json!(0.25));
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices (the wood). Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["seed"] = with_default(int_prop("Seed. Defaults to 0."), json!(0));
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        define("planks", "Lay a deck, floor, or dock of separate boards over a rectangle of the active object at height z along axis, instead of dozens of fill_box and carve calls or painted stripes. Rows of random width tile across the axis, split into boards of random length with end joints clear of the neighbouring rows'. Gaps (between rows, at joints) are empty through the boards with a dirt cell below. A board's tone never repeats the previous one in its row and as seldom as possible its neighbours', and every row with room has a joint, so nothing runs as one uniform stripe. A row's end boards are sawn, so may be shorter than length. Optional ragged ends, nails, and knots. Every rectangle cell, solid or empty, takes a board or gap unless only limits it: for a curved hull, fill its deck in one slot and pass that as only. Same seed, same boards. One undo step. Replies with rows, boards, joints, added, painted, removed, dirtied, nails, knots, changed, and voxels.", {
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
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on solid cells of these palette indices. Omit for the whole rectangle."});
            p["exclude"] = exclude_prop();
            p["seed"] = with_default(int_prop("Seed. Defaults to 0."), json!(0));
            p
        }, &["x0", "y0", "x1", "y1", "z", "axis", "palettes"]),
        define("cavity_tint", "Shade depth into the exposed cells of an inclusive box of the active object without hand painting: each cell counts the solid cells in a ball of radius around it (a curvature or ambient occlusion estimate). Concave cells (fissures, crevices, a root's crook, a wall's foot) past dark take a darker slot; with lighten, convex ridges and corners past light take a lighter one. pairs names the slots; without it each slot gets shades of its own colour and material (darken, brighten) in free slots from 255 down, never one a cell uses, reused on later calls. Run it after bark, joints, or weather and before moss. A box of at most 2000000 cells. One undo step with the palette. Replies with darkened, lightened, and shades (the slots it made).", {
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
        define("radial_relief", "Cut regular, shallow, sloped recesses evenly round an axis into the active object (architecture, turned or coopered pieces). flutes: grooves along a column, pilaster, vase, urn, or barrel, spaced evenly along each slice's outline (as many per face of a square shaft, by angle on a round one), on the outer surface from from to to. gores: each dome segment between two meridians dished toward its middle, ribs proud, fading out at the crown and over fade cells at the foot. count recesses round axis through center, turned by phase (half the pitch is 180 / count). depth is in from the outer surface, out from the axis (flutes) or the dome's centre (gores); a cell is cut when the floor lies a quarter cell below its centre. Colour carries the slope cells cannot: shade tones floor cells, light slope cells, each a slot or true for a tone of each cell's own slot (darken or brighten, with its material, in free slots from 255 down, never one in use, reused by later calls); ribs paints the lines between recesses, ribHeight raises them. Writes stay in the given sides of x0..z1, between from and to, outside exclude, on only slots. jitter varies depth and place by seed. At most 2000000 cells in the solid's box. One undo step with the palette. Replies with cells (surface cells the recesses and ribs cover), recesses, removed, painted, added, shades, and a warning when nothing matched.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner the writes stay inside. Each is optional: a side not given is open, so z0 and z1 alone bound a column's height.");
            }
            p["kind"] = json!({"type": "string", "enum": ["flutes", "gores"], "description": "flutes (grooves along a shaft) or gores (a dome's dished segments)."});
            p["count"] = int_prop("How many recesses round the axis, 1..=256.");
            p["axis"] = json!({"type": "string", "enum": ["x", "y", "z"], "description": "The axis the recesses repeat round. Defaults to z."});
            p["center"] = json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "Where the axis crosses the plane across it: the two other coordinates in x, y, z order ([x, y] for axis z); cell centres are whole numbers. Defaults to the middle of the solid's extent in the box and range."});
            p["phase"] = with_default(num_prop("Turns the pattern in degrees: flutes, a share of the outline's length (360 a whole turn); gores, an angle from +x (axis z), +y (axis x), or +x (axis y). 0 puts a ridge or rib at 0. Defaults to 0."), json!(0));
            p["width"] = num_prop("Flutes: cells across each flute, 0.5..=64, at most the pitch. Defaults to 0.6 of the pitch (the outline's length over count).");
            p["from"] = int_prop("First cell along the axis: the flutes' lower end, the dome's foot. Defaults to the solid's lowest cell in the box.");
            p["to"] = int_prop("Last cell along the axis: the flutes' upper end, the dome's top. Defaults to the solid's highest cell in the box.");
            p["ends"] = json!({"type": "string", "enum": ["round", "taper", "square"], "description": "How a recess ends over fade cells: round (a cupped end, the default), taper (a straight ramp), or square (full depth to the end)."});
            p["fade"] = num_prop("Cells over which a recess fades in from its ends (flutes; default the width, at least 2) or from the foot (gores; default 6), 0..=128. Gores also fade toward the crown.");
            p["profile"] = json!({"type": "string", "enum": ["u", "v", "flat"], "description": "Section across a recess: u (round-bottomed, the default), v (straight sides), or flat (a flat floor over the middle half, sloped sides)."});
            p["depth"] = with_default(num_prop("Deepest recess depth, 0.5..=4 cells, in from the outer surface. Defaults to 1."), json!(1));
            p["shade"] = json!({"type": ["integer", "boolean"], "description": "Tone of the floor cells mid-recess: a palette slot, true (the default) for a darker shade of each cell's own slot, or false for none."});
            p["light"] = json!({"type": ["integer", "boolean"], "description": "Tone of the slope cells by the ridges: a palette slot, true for a lighter shade of each cell's own slot, or false (the default) for none."});
            p["darken"] = num_prop("With shade true: the shade's colour as a factor of the slot's, 0.05..1. Defaults to 0.72.");
            p["brighten"] = num_prop("With light true: the light tone's colour as a factor of the slot's, above 1 up to 4. Defaults to 1.18.");
            p["ribs"] = int_prop("Palette slot of the lines between recesses (a dome's ribs, a column's fillets) and of raised rib cells. Omit to leave them.");
            p["ribWidth"] = with_default(num_prop("Gores: width of the rib line between two gores, 0..=16 cells. Defaults to 2, which keeps a rib whole down a steep side."), json!(2));
            p["ribHeight"] = with_default(int_prop("Raise the ribs by this many cells, 0..=4, in the ribs slot (or the slot under them). Defaults to 0."), json!(0));
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices. Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["jitter"] = with_default(num_prop("Share of random change in each recess's depth and place, 0..=1. Defaults to 0: architecture is regular."), json!(0));
            p["seed"] = with_default(int_prop("Seed of the jitter. Defaults to 0."), json!(0));
            p
        }, &["kind", "count"]),
        define("foliage", "Build a clustered broadleaf crown on the active object, as pixel-art trees draw it: separate leaf clumps with sunlit tops, darker undersides, and dark gaps, not one smooth blob. Volume: ellipsoids, boxes, from, or a mix. Clumps are packed over its surface with Poisson-like spacing (size, gap); each is a flattened ellipsoid with a ragged, leafy outline bulging out. One cell under the surface fills with core, so gaps show dark and nothing is see-through. Each clump is shaded as one piece: its height and its facing to the sun set the tone, its top lightens and underside darkens along its curve, and a darker seam marks where clumps meet; no per-cell noise. Only empty cells and the crown's slots (palettes, core, from) are written, never protect: trunk and branches stay whole and show through gaps; exclude keeps a space open (in front of the trunk). Slots keep their motion: give leaves and core the same sway. Writes stay in the box, or without one in the volume grown by room for the clumps. Deterministic for a seed. A box of at most 2000000 cells, at most 4096 clumps. One undo step. Replies with clumps, masses, added, recoloured, removed, and core.", merged({
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner the writes stay inside. Omit all six to bound the crown by its volume.");
            }
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "1 to 8 leaf slots, darkest to lightest: shade, mid, light, sunlit tips."});
            p["ellipsoids"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "Volume: [x, y, z, radius] or [x, y, z, rx, ry, rz] each; up to 64 shapes with boxes. A broad middle and a few lobes read as a tree."});
            p["boxes"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "integer"}}, "description": "Volume: inclusive [x0, y0, z0, x1, y1, z1] boxes."});
            p["from"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Volume: the existing cells of these slots (in the box), re-clustering a sculpted crown; cells no clump or core keeps are removed."});
            p["core"] = int_prop("Slot of the dark core seen in the gaps. Defaults to the first of palettes.");
            p["size"] = json!({"type": "array", "items": {"type": "number"}, "description": "Clump radius range [min, max] in cells, 1..=32, or one number. Defaults to [3, 6]."});
            p["gap"] = with_default(num_prop("Extra cells between neighbouring clumps, 0..=8; more shows more core. Defaults to 1."), json!(1));
            p["flatten"] = with_default(num_prop("Clump height over width, 0.2..=1.5; below 1 is wider than tall. Defaults to 0.7."), json!(0.7));
            p["ragged"] = with_default(num_prop("Outline noise of each clump, 0..=1; 0 is a smooth ellipsoid. Defaults to 0.4."), json!(0.4));
            p
        }, json!({
            "light": with_default(num_prop("How strongly sunlit tops lighten toward the last palette, 0..=2. Defaults to 1."), json!(1)),
            "dark": with_default(num_prop("How strongly undersides and seams darken toward the first palette, 0..=2. Defaults to 1."), json!(1)),
            "sun": json!({"type": "array", "items": {"type": "number"}, "description": "Direction toward the sun [x, y, z], Z up. Defaults to [-0.35, -0.45, 1], high at the front left."}),
            "masses": with_default(int_prop("Lobes the clumps group into, 0..=16, split by deeper grooves, each with its own tone. 0 (the default) is none."), json!(0)),
            "groove": with_default(int_prop("With masses: extra depth of the grooves between lobes in cells, 0..=8. Defaults to 2."), json!(2)),
            "protect": json!({"type": "array", "items": {"type": "integer"}, "description": "Slots never written, even crown slots. Slots outside palettes, core, and from are kept anyway (the wood)."}),
            "exclude": exclude_prop(),
            "seed": with_default(int_prop("Seed. Defaults to 0."), json!(0))
        })), &["palettes"]),
        define("sweep", "Fill a 2D cross-section along a path of 2..=1024 local points, perpendicular to it: curved walls, banks, ledges, and kerbs with no corners. Profile u runs left of the path (seen from above, walking it), v up, so a wall profile stands on the path; whole-number points are cell centres. profile is a [u, v] polygon, or width, height, and radius make a rounded rectangle; per-point twist and scale interpolate along it. curve smooth is Catmull-Rom; extra slabs fill a bend's outside. A cell is in when its centre is inside the profile at the nearest slab, so a flat top stays one layer. palettes follow pattern; writes follow mode. palette 0 removes the covered solid cells instead: a slanted cut along a closed or curved outline (mode empty is refused with it). At most 2000000 cells. One undo step. Replies with cells and length, and removed when erasing.", json!({
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
        define("trim_outline", "Reshape the active object from outside: remove every solid cell outside a surface given as a closed plan outline and an inset per height, so a hull narrows toward its keel in one call. At height z the surface is the outline moved inward by inset(z); a cell stays when its centre is at least that far inside. Use it when everything outside goes; sweep with palette 0 cuts a band along a path instead. skin lays a new skin of that slot thickness cells inward along the surface, since the cut exposes the inside: where the cut reached, solid cells are recoloured and hollows sealed; a rim, an opening, or a part the cut missed stays, and the box never grows. x0..z1 (all six or none) and only limit what is removed or skinned. At most 2000000 solid cells. One undo step. Replies with removed, painted, and added, and a warning when nothing was outside.", json!({
            "outline": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "The closed plan outline: 3..=1024 local [x, y] points in either winding, a whole number a cell centre. A repeated first point is ignored."},
            "inset": {"type": ["number", "array"], "items": {"type": "array", "items": {"type": "number"}}, "description": "How far inward the surface stands: one number, or [z, inset] pairs rising in z, linear between and the end value beyond. A negative inset stands outside the outline. Defaults to 0."},
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
        define("deform", "Reshape the active object, or the solid cells of an inclusive box x0..z1 (all six or none), so a trunk, root, limb, or rock stops reading as a pipe or a box. bend curls the cells toward direction by angle over length cells of the axis from origin, the rest carried on straight; taper scales across the axis from start to end over the length; twist turns about the axis by angle over the length; displace pushes the surface in and out with fractal 3D noise for burls, lumps, and knobs; scale enlarges or shrinks a part about point by factor: cells within radius move as one rigid piece and the scale eases to none over falloff, so the part stays attached and the rest stays put. scale is the exaggeration step for a small creature (20 to 60 cells long) whose head, eye, or fin needs more cells to read (a box leaves other parts alone). minFeature N adds thin to the reply: the cells after the deform whose shortest solid run along x, y, or z is under N, as connected parts {cells, min, max, thickness} (the 16 largest, count of all), so a fin, ear, or leg too thin shows before it vanishes or reads as noise. Each target cell keeps the slot of the cell it came from. Deterministic for a seed. At most 2000000 cells. One undo step. Replies with cells (read), result (left), added, removed, and thin with minFeature.", merged(json!({
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
            "center": {"type": "array", "items": {"type": "number"}, "description": "bend, taper, twist: the line they work about, [a, b], the two other coordinates in x, y, z order (axis z: [x, y]). Defaults to the cells' mean."}
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
            "radius": num_prop("scale: cells around point scaled as one rigid piece, 0.5..512."),
            "falloff": num_prop("scale: cells (before scaling) over which the scale eases back to none, 0.5..512. Defaults to radius."),
            "minFeature": int_prop("Any op: report the parts of the object thinner than this many cells after the deform, 2..16.")
        })), &["op"]),
        define("blend_shapes", "Fill simple shapes blended as signed distance fields with a smooth minimum, so they flow together like clay instead of separate balls and bricks: statues, creatures, topiary, roots, snowmen, any organic body, head, eyes, and limbs. A whole-number point is a cell centre. union melts a shape in, subtract carves it softly out of what came before (a mouth groove, eye sockets, a hollow), intersect keeps only what is inside it. k is the fillet width in cells: 0 is a hard union like fill_ellipsoid calls, 2 to 6 melts a head into a body. A cell fills when the blended distance at its centre is at most grow. Each union shape's palette (or the call's) colours it, placed by colors and seam; a subtract or intersect shape with a palette paints the cells along its cut (a dark mouth line). Only the shapes' box grown by k is evaluated (at most 16000000 cells; x0..z1 clips it); at most 2000000 cells fill. Writes as mode says. One undo step. Replies with cells, changed, shapes, evaluated, and bounds.", json!({
            "shapes": {"type": "array", "items": {"type": "object"}, "description": "1..=256 shapes in order, the first a union: {shape: sphere (center, radius) | ellipsoid (center, radii [rx, ry, rz] or radius) | box (center, size [sx, sy, sz] in cells, round: edge radius) | capsule (from, to, radius or radii [r0, r1] for a taper, or points [[x, y, z, r], ...] for a bent limb) | torus (center, radius to the ring's middle, thickness), rotate?: [x, y, z] degrees about the centre (x, then y, then z; not a capsule), op?: union (default) | subtract | intersect, k?: this shape's smoothing in cells, palette?}."},
            "k": with_default(num_prop("Default smoothing radius in cells for shapes without k, 0..=64; 0 is a hard union. Defaults to 3."), json!(3)),
            "palette": int_prop("Default slot of union shapes without palette, 0..255 (0 erases)."),
            "grow": with_default(num_prop("Fill where the blended distance is at most this, -16..=16 cells: above 0 thickens, below 0 thins. Defaults to 0."), json!(0)),
            "colors": {"type": "string", "enum": ["nearest", "last"], "description": "nearest (default): the union shape the cell is deepest inside, so seams fall mid-fillet. last: the last listed union shape whose own body holds it (eyes, buttons, spots listed after the body), else nearest."},
            "seam": with_default(num_prop("With colors nearest: dither the boundary between two slots over this many cells, 0..=32. Defaults to 0, a sharp boundary."), json!(0)),
            "seed": int_prop("Seed of the seam dither. Defaults to 0."),
            "x0": int_prop("Optional inclusive box clipping the evaluation; all six corners or none."),
            "y0": int_prop("Box corner"),
            "z0": int_prop("Box corner"),
            "x1": int_prop("Box corner"),
            "y1": int_prop("Box corner"),
            "z1": int_prop("Box corner"),
            "mode": {"type": "string", "enum": ["replace", "empty", "paint", "intersect"], "description": "replace (default) writes every cell of the blend; empty only empty cells; paint only solid cells; intersect erases solid cells outside the blend, keeping the rest."}
        }), &["shapes"]),
        define("loft", "Fill a solid along 2..=1024 local points whose cross-section changes per point: trunks, roots, limbs, and branches that taper, bend, and are egg-shaped and knobbly, in one call instead of a round fill_tube. Each section is an ellipse of rx (across the path) and ry (along the frame's normal, up for a level path), turned by roll, cut by flatTop, with noise outline lumps; values run linearly between points, and one a point leaves out takes the call's. curve smooth is a Catmull-Rom curve through the points. A cell is in when its centre is inside the section at the nearest path point. Colour as fill_tube: palette, or palettes and pattern. Writes as mode says. At most 2000000 cells. One undo step. Replies with cells and length.", merged(json!({
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Path points: [x, y, z], [x, y, z, r], [x, y, z, rx, ry], [x, y, z, rx, ry, roll], or {x, y, z, radius?, rx?, ry?, roll?, flatTop?, noise?, frequency?}. Radii 0.5..=128; a whole-number point is a cell centre."},
            "radius": num_prop("Default rx and ry of points that leave them out."),
            "rx": num_prop("Default half-width across the path."),
            "ry": num_prop("Default half-height. Defaults to rx."),
            "roll": with_default(num_prop("Default roll of the section about the path, in degrees. Defaults to 0."), json!(0)),
            "flatTop": with_default(num_prop("Default flat side, -1..=1: above 0 cuts the top at ry * (1 - flatTop) (1 halves it at the path), below 0 the bottom, for a log lying on the ground. Defaults to 0."), json!(0)),
            "noise": with_default(num_prop("Default outline noise as a fraction of the radius, 0..=0.9. Defaults to 0."), json!(0)),
            "frequency": with_default(num_prop("Default count of outline lumps, 0..=64. Defaults to 5."), json!(5))
        }), json!({
            "curve": str_prop("polyline (default) or smooth"),
            "caps": {"type": "string", "enum": ["flat", "round"], "description": "flat (default, cut square) or round ends."},
            "palette": int_prop("Palette index, or pass palettes"),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices laid out by pattern"},
            "pattern": {"type": "string", "enum": ["rings", "stripes", "random"], "description": "rings (default) every period cells along the length, stripes in sectors around it, random per cell"},
            "period": with_default(num_prop("Ring length in cells, 0.5..=512. Defaults to 2."), json!(2)),
            "sectors": int_prop("Stripes around, 1..=256. Defaults to the number of palettes."),
            "seed": with_default(int_prop("Seed of the outline noise and the random pattern. Defaults to 0."), json!(0)),
            "mode": mode_prop()
        })), &["points"]),
        define("brush", "Sculpt the active object's surface with dabs along a path (like Blender's clay and grab brushes) to refine a silhouette after the rough form. op inflate pushes out up to strength x radius / 2 cells at a dab's centre; deflate (dig) pushes in; smooth blurs solid and empty cells so bumps and pits melt (strength sets passes); flatten cuts bumps and fills pits toward the mean plane under the dab; grab (drag) moves a dab's cells by vector x strength with the falloff, to pull a stub out or bend a lip over. Points without radius or strength use the defaults. Dabs fall every spacing cells (grab: one per point), each reading what earlier ones left. only restricts it to those slots: other cells never change or grow. Added cells take the nearest solid cell's slot, or palette. One undo step. Replies with dabs, added, and removed.", json!({
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
        define("carve_from_views", "Fill into the active object the visual hull of 2 to 6 orthographic views of a turnaround sheet: each silhouette is extruded through a box along its depth axis, and cells every view covers stay. A depth map per view (depth, depthPath) also carves cells in front of its surface, so a head or dome comes out round; one depth view makes a relief. Coloured pixels (a magenta background and fringe) hold no depth; a ray reads its footprint's median, so outlines and specks do not move it. For relative depth (a generator's depth pass, or Depth Anything V2, grayscale, white near) keep calibrate hull and the colour sheet's crops; gamma auto for Depth Anything, regions for a generated map, or Depth Anything as the map and the generated one as detail; near and far only for a known scale. Views as model_view names them, model facing -Y, +Z up: front from -Y (image right +X), back from +Y, left (also side) from +X (image right +Y: the model's front is image left), right from -X, top from +Z (image down -Y), bottom from -Z. A view is a PNG path with a pixel crop [x, y, width, height] (views may share a sheet), or rows with a legend; background auto (default: transparent pixels if the crop has any, else the commonest image-border colour within tolerance), alpha, or a key colour, then compare_silhouette's mask edits (top level for all views, except mask and erase; flood and mask need a path). cellsPerPixel or size [across, down] scales a view; size [x, y, z] sets the box and stretches unscaled views; a smaller view is centred across, on the box's bottom. Each cell takes the colour of the view whose visible surface is nearest, quantised to slots, or palette. Top-level path, background, tolerance, and alphaThreshold apply to views without their own; a depth view without path or rows takes its silhouette from the map (then pass palette or give another view a colour image). Refine with carve, smooth, and set_voxels; check with compare_silhouette. Writes as mode says (intersect cuts a blockout to the views). At most 4000000 cells in the box. One undo step. Replies with cells, bounds {min, max}, size, and per view grid, covered cells, and depth {calibrate, near, far, gamma, rim, pairs and residual (cells) for hull, regions and regionResidual with regions or detail, carved}.", merged(merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "2 to 6 (1 with depth), one per side: {view: front | back | left | side | right | top | bottom, path | rows (strings; . and space empty) + legend ({char: palette index or #rrggbb}), crop?, cellsPerPixel? | size?: [across, down], background?, tolerance? (default 40), alphaThreshold? (default 128), excludeColors?, excludeTolerance?, flood?, mask?, erase?, keepLargest?, flip? (mirror), depth?}. depth: a grey map of the same crop (white near): a PNG path, true for this crop of depthPath, or {path | values (rows of 0..1, null background), crop (default the view's), background (with a colour image all pixels count and the silhouette is the image's; without, keyed as a view; none: all pixels), invert (black near), gamma (value^gamma first; auto fits it so the calibration meets the silhouette contour where a round body turns away, for maps not linear in depth such as Depth Anything's), near, far (cells from the box's camera side for white and black), calibrate (hull, default: each image row's nearest pixel goes where the hull is nearest in that row, seen along the row, so a sphere's rim is on its middle plane; box: the map spans the box, for a relief, and hull's fallback when no view measures depth), back (mirror the surface about the hull's middle when the opposite view has no map), regions (calibrate each part alone: a region per peak, split where the grey jumps or dips over edge, default 0.06; for maps normalised per part like shading, as image generators make), detail (a second map, path or {path | values, crop, invert, gamma, edge}: its regions give each part's curvature, this map their order and offsets, so a monocular map (right order, flat parts) and a generated one (round, wrong order) make one surface)}."},
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth from this image at the view's crop."),
            "depthTolerance": num_prop("A cell stays when its centre is at most this many cells in front of every depth surface, 0..64. Defaults to 0."),
            "size": {"type": "array", "items": {"type": ["integer", "null"]}, "minItems": 3, "maxItems": 3, "description": "Box [x, y, z] in cells, 1..2048 each; null takes the largest view along that axis."},
            "origin": ivec3_prop("Local cell of the box's minimum corner. Defaults to [0, 0, 0]."),
            "cellsPerPixel": num_prop("Cells per image pixel for every view without its own size or cellsPerPixel, such as 0.25 for 4 pixels a cell."),
            "palette": int_prop("One palette index for every cell, instead of the views' colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices image colours quantise to. Defaults to all of 1..255. A legend's palette index is used as it is."}
        }), json!({
            "downscale": {"type": ["boolean", "object"], "description": "Pixelate every view first at the cell grid, as pixelate_views does (true, or its options {colors, coverage, thin, features, lines, outline, mergeLines, accents, minShare, contrast, maxAccents}), so a small creature keeps its eyes, teeth, spots, and thin legs instead of averaging them away; the box is size (at least one axis; the others follow the views' proportions). Not with depth maps. The reply adds downscale (size, colors, legend, and per view grid, inked, thin, lines, and features)."},
            "coverage": num_prop("Share of a cell's footprint that must be inked for the cell to be in a view, 0..1. Defaults to 0.5."),
            "shell": int_prop("Keep only cells within this many cells of the outside, 1..64: a hollow shell. Omit for solid."),
            "smooth": int_prop("Passes of smooth over the hull, 0..64 (fillAt 17, removeAt 9). Defaults to 0."),
            "mode": fill_mode_prop()
        })), mask_edit_props(true)), &["views"]),
        define("project_paint", "Recolour the active object's existing surface from ONE image projected along a view (a shell's plates from the top, face markings from the front); cells are never added or removed. Image: PNG path with crop, or rows with legend; background rule and mask edits as in carve_from_views (alpha on an opaque image inks every pixel); background leaves the surface alone. Sides as in model_view and carve_from_views (front from -Y, image right +X; left from +X, image right +Y; top from +Z, image down -Y); yaw and pitch, as preview takes them, project obliquely. The box (origin, size) limits painted cells, and the image sits on its projection as a carve_from_views view in its box (stretched, or by cellsPerPixel or imageSize, centred across and on the bottom; oblique: centred both ways), so a carve's crop and cellsPerPixel hit the same cells. Each inked grid cell casts parallel rays; depth paints solid cells that far from the first hit, so nothing behind the visible surface changes; cells outside the box or only still occlude. normalLimit spares steep sides from streaks. Colours quantise to the nearest of slots, a legend index is used as is, or palette paints all ink one slot (a mask). A cell under several pixels takes the commonest. One undo step. Replies with cells (painted), changed, bounds, origin, size, grid, covered (inked grid cells), rays (inked rays that met the object), and steep (rays skipped by normalLimit).", merged(merged(json!({
            "view": {"type": "string", "enum": ["front", "back", "left", "side", "right", "top", "bottom"], "description": "The side the image looks from; side is left. Or pass yaw and pitch."},
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
        define("paint_from_views", "Recolour every surface cell of the active object, or of objects together (each hiding what is behind it from the others), from a whole orthographic views sheet, so an image-to-3D shape smeared on its hidden sides takes the sheet's colours all round. Cells are never added or removed. views are as carve_from_views takes them (a top-level path, background, tolerance, and mask edits apply to all); over MCP a top-level path alone reads the sheet's 2 x 2 layout as measure_sheet does. Each view (see fit) stretches over the objects' box as it sees it, so sheet and shape need the same proportions, not size. A cell is painted only by views where it is the first solid cell of its column (a flipper never paints the shell behind it), mixed by blend from its footprint inset pixels inside the silhouette, or from the nearest ink within reach. Colours mix in Oklab and quantise into slots, into newSlots new colours, or by default into the slots the objects use. x0..z1 and only limit the painted cells. Cells no view sees with ink (a belly with no bottom view) keep their slot. One undo step. Replies with painted, surface, unseen, blended, views (per view: visible, used, snapped, missed, mirrored), slots (slot and cells), newSlots (slot and rgba), accents, changed, objects, and voxels.", merged(json!({
            "views": {"type": "array", "items": {"type": "object"}, "description": "1..6 views: {view, path | rows, crop?, legend?, flip?, background?, tolerance?, alphaThreshold?, mask edits}. Leave out with a top-level path to use the sheet's measure_sheet layout."},
            "path": str_prop("The sheet PNG every view reads unless it has its own path."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "objects": {"type": "array", "items": {"type": ["string", "integer"]}, "description": "Objects to paint together, by name or index. Defaults to the active object."},
            "blend": {"type": "string", "enum": ["nearest", "soft"], "description": "nearest (default): the view the normal faces most, blended within seam of a boundary; soft: every view that sees the cell, by facing."},
            "seam": num_prop("nearest: degrees around the boundary between the two best views (near 45 degree normals) where they blend linearly, 0..45; 0 gives hard seams. Defaults to 12."),
            "sharpness": num_prop("soft: each view's weight is max(0, normal . view) to this power, 0..64. Defaults to 4."),
            "fit": {"type": "string", "enum": ["ink", "crop"], "description": "What of each view maps onto the box: ink (default) its ink bounds, crop the whole crop."},
            "reach": num_prop("Cells: a footprint off a view's ink takes the nearest ink within this, 0..64. Defaults to 3."),
            "inset": int_prop("Pixels: colours are read this far inside each silhouette's edge, so anti-aliased fringes never land on the model, 0..16. Defaults to 2 (0 when every view is rows)."),
            "mirror": {"type": "boolean", "description": "Make a missing right view by mirroring the left one, and the other way. Defaults to true."},
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices colours quantise to. Defaults to the slots the painted objects use."},
            "newSlots": int_prop("Instead of slots: up to this many new colours (1..64) clustered from the painted ones by median cut and k-means in Oklab, in free slots; an existing slot of exactly the colour is reused, and the accent guard keeps small distinct colours (eyes, a mouth) their own slot."),
            "accents": {"type": "boolean", "description": "newSlots: keep small distinct colours their own slot. Defaults to true."},
            "minShare": num_prop("Accent guard: share of the painted cells an accent needs, 0..1. Defaults to 0.002."),
            "contrast": num_prop("Accent guard: Oklab distance from every cluster an accent needs. Defaults to 0.12."),
            "maxAccents": int_prop("Accent guard: most accents kept beyond newSlots, 0..16. Defaults to 4."),
            "downscale": {"type": ["boolean", "object"], "description": "Pixelate every view first at the cell grid, as pixelate_views does (true, or its options {colors, coverage, thin, features, lines, outline, mergeLines, accents, minShare, contrast, maxAccents}), so a small creature keeps eyes, teeth, spots, and thin legs; the grid is the painted objects' box and each view is then read whole (fit crop, inset 0). The reply adds downscale (size, colors, legend, and per view grid, inked, thin, lines, and features)."},
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices that may be repainted. Omit for all."},
            "x0": int_prop("Box corner of the painted cells (all six or none)."), "y0": int_prop("Box corner."), "z0": int_prop("Box corner."),
            "x1": int_prop("Opposite box corner."), "y1": int_prop("Opposite box corner."), "z1": int_prop("Opposite box corner.")
        }), mask_edit_props(true)), &[]),
        define("shell_plates", "Paint turtle-shell plates over a dome-shaped surface in one call, instead of drawing seams with surface_stroke: a central plate, rings, an outer band, and a rim, each kind from its palettes slots (picked per plate by seed), split by seamWidth seams in the seam slot, or seamDepth grooves with seam 0. It works on the surface, not a view: surface cells whose normal is within facing of straight up, optionally limited by the box x0..z1, zRange, and only. A cell's place is its surface distance from the apex (the highest cell near center) over the distance to the dome's edge that way, so rings keep their width down a tall or oval dome's steep sides and nothing streaks as in a top projection. layout rings: jittered sectors per ring, staggered ring to ring; voronoi: the nearest of seeds laid in the rings, rounder and irregular. symmetric mirrors across the plane through the centre along y, so a shell facing -Y matches left and right. Nothing is added; a groove erases. One undo step. Replies with cells, changed, taken, unreached (taken cells not joined to the apex along the surface, left alone), plates, seams, apex, kinds {central, ring, band, rim} (surface cells of each), and voxels.", json!({
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
        define("decal", "Lay ONE picture (eye, mouth, badge, marking) on the active object's curved surface around a point, keeping its size and shape on a round head where project_paint's parallel projection stretches on slopes: the surface is unrolled around the point (a discrete exponential map along the normal and surface distance) and each surface cell takes the picture colour over it. Cells are never added or removed. Picture: PNG path with crop, or rows with legend, background rule as in carve_from_views; background leaves the surface alone. Place by point (whole numbers are cell centres) or ray (aim from the camera); size runs along the surface. up is laid on the surface at the point and image right is up x outward normal, so a face looking along -Y with up +Z has image right +X, as the front view. depth paints into the solid along the normal; normalLimit stops it where the surface turns away from the point's normal, so it never wraps round a thin part. Colours quantise to the nearest of slots, a legend index is used as is, or palette paints all ink one slot. mirror also paints the mirrored picture across a plane (a left eye for a right one). One undo step. Replies with cells, changed, bounds, centre (the surface cell), normal, grid, covered (inked grid cells), missed (inked grid cells landing on no surface cell: off the surface or past normalLimit), reached (surface cells unrolled), and voxels.", json!({
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
        define("orient_to_views", "Turn the active object (an import_mesh image-to-3D mesh lying on its side or facing anywhere) to the turn whose silhouettes best match a views sheet, and scale it, instead of guessing by eye. views as fit_to_views takes them; top-level path, background, tolerance, and alphaThreshold apply to views without their own; a path with no views reads the sheet's 2 x 2 layout as measure_sheet does. Each turn projects the surface cells onto each view's image axes (as model_view, model facing -Y, +Z up), fits the view's mask at its own scale and offset (compare_silhouette's fit), and scores the weighted mean IoU; yawStep then refines the best within yawRange, one axis at a time. The object is resampled (each cell takes the nearest source cell's slot) and scaled by size, height, or scale about its bottom centre. A small margin (best score minus second) means the views cannot tell turns apart (front and back alike): add a view or check candidates. import_mesh's orient runs the same search before voxelizing (upright: true stands it level by its principal axes first). One undo step. Replies with before and after {score, views [{view, iou}]}, rotation {euler (degrees about x, then y, then z, as import_mesh rotation), axes (where the object's x, y, and z went), yaw, fine (fine degrees about x, y, and z), matrix}, candidates (the 6 best axis-aligned turns), margin, scale, applied, cells, bounds, changed, and voxels.", json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "1 to 6, one per side: {view: front | back | left | side | right | top | bottom, path | rows + legend, crop [x, y, width, height], background, tolerance, alphaThreshold, flip (mirror), weight (default 1)}."},
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "turns": {"type": "string", "enum": ["all", "upright"], "description": "all (default): the 24 axis-aligned turns; upright: the 4 about up, for an object already standing."},
            "yawStep": with_default(num_prop("Degrees between fine yaw steps about up around the best turn, 0..=90. Defaults to 0, none."), json!(0)),
            "yawRange": with_default(num_prop("Fine steps go this many degrees each way, 0..=180. Defaults to 45."), json!(45)),
            "fineAxes": str_prop("z (default, yaw about up), or xyz to also tilt about x and y, for a mesh tilted off its axes."),
            "resolution": int_prop("Longest side of the projected model masks in pixels, 8..=512. Defaults to 64."),
            "size": num_prop("Cells along the longest axis after the turn, 1..=256. Instead of height or scale."),
            "height": num_prop("Cells along up after the turn, 1..=256. Instead of size or scale."),
            "scale": num_prop("Scale factor, above 0 and at most 16. Defaults to 1, the turn alone."),
            "apply": bool_prop("Turn and scale the object (default true); false only reports the best turn.")
        }), &[]),
        define("fit_to_views", "Refine the active object so its orthographic views match an approved views sheet, colours from a palette: the finishing pass after a carve_from_views or import_mesh blockout (inspired by Voxify3D, CVPR 2026; a CPU discrete search, no neural network). views as carve_from_views; a view pixel is a column of cells. Each crop is placed once on the object's projection by compare_silhouette's align and sampled to one target per column (inside when coverage of its footprint is inked; the mean colour). Pixel energy: maskWeight if missing or excess, depthWeight * min(|d - dRef|, 8) / 8 against the depth target, colorWeight * min(RGB distance / 255, 1) of the first visible slot; plus smoothWeight per exposed face. shape runs first (mask, depth, smoothness): clear excess rays, fill missing ones with cells every other view sees inside (a run, or one cell touching the body), move a covered ray's front to the depth target, remove or fill rough single cells; a move stays only if it lowers the energy, in seed-shuffled sweeps until none stays or iterations run out. Then recolor: each seen cell takes the slot of slots nearest the colours of the views seeing it and facing it within normalLimit; a column takes its pixels' majority slot, so thin outlines, cube bevels, and lines under half a cell do not tint it. A view's color false: shape only; keep protects slots. A feature drawn at different places in two views of a boxy part (eyes in front and side) comes out twice: use recolor false or keep its face slots. Other visible objects cover and hide but never change unless isolate. Depth target: reference's front surface, or a view's depth map (depth, depthPath as carve_from_views, calibrated against the hull of the views' targets). roundWeight (off by default) rounds the diagonal corners three axis views cannot see. Deterministic, one undo step. Replies with views [{view, grid, align {scale, offset} (reuse it to keep the placement), before and after {iou, missing, excess, colorError, depthError with a reference or depth, pixels}, depth (its calibration)}], energy before and after, added, removed, recolored, changed, sweeps, moves, rounded, palette [{slot, cells}] after, origin and size of the box worked in, and voxels.", merged(merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "1 to 6 views, one per side: {view: front | back | left | side | right | top | bottom, path (a PNG) | rows + legend, crop [x, y, width, height], background (auto, alpha, #rrggbb, [r, g, b]), tolerance, alphaThreshold, excludeColors, excludeTolerance, flood, mask, erase, keepLargest (as compare_silhouette), flip, align, color (false: shape only, its colours ignored), depth (as carve_from_views: true | path | {path | values, crop, background, invert, gamma, near, far, calibrate, back, regions, edge, detail})}."},
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth target from it at the view's crop."),
            "depthTolerance": num_prop("Cells in front of a depth surface that still count as on it, 0..64. Defaults to 0."),
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "align": {"type": ["string", "object"], "description": "fit (default), height, width, or {scale, offset}: crop pixel p lands on cell column p * scale + offset. A view's own align overrides it."},
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices the colours may take. Defaults to the slots the object holds."},
            "reference": {"type": ["integer", "string"], "description": "Object (index or name) whose front surface is the depth target, such as an import_mesh newObject. Omit for none."},
            "isolate": {"type": "boolean", "description": "Fit the active object alone."}
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
            "recolor": {"type": "boolean", "description": "Recolour the cells the views see. Defaults to true; false for a part with a hand-set face."},
            "normalLimit": num_prop("Degrees, 0..90: a view recolours only cells facing it within this angle (a seen cell no view faces takes the view facing it most), so a feature does not wrap round a corner. Defaults to 60."),
            "keep": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices whose cells are never removed or recoloured (hand-set eyes); they still cover and hide."},
            "roundWeight": num_prop("Cost of keeping a cell outside the rounding prior, 0..100: the superellipse inscribed in each roundAxes cross-section's bounds (with a reference, the reference itself). Cells outside it that no axis view needs are removed, so a round head comes out round and every silhouette stays. Defaults to 0 (the maximal hull); 0.3 suits a round part such as a head, not a flat-bottomed dome or a plate, which lose their rim."),
            "roundAxes": str_prop("Axes whose cross-sections the rounding prior rounds, letters of xyz. Defaults to yz: front sections and the plan; an x section runs along a creature facing -Y, through its neck."),
            "roundness": num_prop("Superellipse exponent of the rounding prior, 1..16: 2 is an ellipse, larger is squarer. Defaults to 2.5."),
            "sharedScale": {"type": "boolean", "description": "Views placed with fit share one scale (the median), offsets refitted at it, as a sheet is drawn at one scale. Defaults to true."}
        })), mask_edit_props(true)), &["views"]),
        define("optimize_to_views", "Iterate a small creature (20 to 60 cells long) until its orthographic views match its pixel-art views (after Voxify3D, CVPR 2026; an independent CPU solver, no neural network): the finishing pass after carve_from_views, import_mesh, or a hand blockout, where one paint_from_views pass leaves streaky colours and lost eyes. Views stretch over the active object's box, a pixel per column of cells: pixelate_views' rows and legend (at that box), or sheet crops (see downscale). Shape passes clear an excess ray's column and give a missing ray one cell touching the body where every other view sees inside (at the neighbouring rays' depth, or a depth map's surface); a depth map (calibrated against the views' hull as carve_from_views does) also cuts cells in front of its surface and fills a front behind it. A move stays only if it lowers its rays' energy (1 per missing or excess pixel, depthWeight * min(|front - depth| - depthTolerance, 4) / 4, surfaceWeight per exposed face) within band, connected, and minFeature; colour never carves. Recolouring gives each surface cell the slot best agreeing with the views that see it (as a ray's first solid cell), weighted by max(0, n . view) ^ sharpness, plus smooth times its share of neighbours with another slot (iterated conditional modes), so colours stay patches and specks join their neighbours. An accent (a colour under accentShare of the coloured pixels: eyes, pupils, nostrils) stays where a view squarely paints it and nowhere else. Deterministic, one undo step. Replies with views [{view, grid, mirrored, before and after {agreement (right shape and colour over either's pixels), iou, color (right colour over overlap), colorError (mean Oklab distance), depthError (with a depth map), pixels}, depth (its calibration)}], agreement {before, after} (mean over views), iterations, moves, added, removed, recolored, colorSweeps, surface, unseen (surface cells no view sees), accentCells, blocked {band, keep, connected, thin, gain, unreachable}, energy before and after, colours [{colour, slot, accent, pixels}], newSlots, downscale (the pixelation, without rows), origin, size, millis, changed, and voxels.", merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "1 to 6 views, one per side: {view: front | back | left | side | right | top | bottom, rows + legend (pixelate_views' reply) | path + crop + background rule, flip, color (false: shape only), depth (as carve_from_views)}. Leave out with a top-level path for the sheet's measure_sheet layout."},
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "downscale": {"type": ["boolean", "object"], "description": "Pixelate the views at the object's box first, with pixelate_views' options (colors, accents, coverage, thin, features, lines, outline, mergeLines, despeckle); false only for pixel-art crops. Defaults to true unless every view is rows."},
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
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices the views' colours land in (nearest in Oklab). Defaults to a slot per view colour: an existing slot of exactly that colour, or a new one in a free slot."},
            "keep": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices whose cells are never removed or recoloured (hand-set eyes)."},
            "accents": bool_prop("The accent guard (default true): small distinct colours keep their cells."),
            "accentShare": num_prop("Share of the views' coloured pixels under which a colour is an accent, 0..0.5. Defaults to 0.04."),
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth target from it at the view's crop."),
            "depthWeight": num_prop("Cost of a front 4 or more cells off the depth map's surface, 0..16. Defaults to 0.5."),
            "depthTolerance": num_prop("Cells off a depth surface that still count as on it, 0..16. Defaults to 0.5."),
            "surfaceWeight": num_prop("Cost of each exposed cell face, 0..4; keeps lone spikes out. Defaults to 0.05."),
            "colorCarve": bool_prop("Remove an edge cell two views see first and paint differently when every ray seeing it still meets the body and more then agree. Defaults to true (with shape and recolor)."),
            "semantic": {"type": ["boolean", "object"], "description": "The CLIP semantic term (needs froxel built with the clip or clip-metal feature and the CLIP ViT-B/32 weights, about 605 MB, at models/clip-vit-base-patch32.safetensors in Froxel's support folder or FROXEL_CLIP_MODEL): true or {weight (default 1), rounds (default 2, 0..8), patches (default 8), model (the weights file)}. After recolouring, each view is compared with the model's view on 7 x 7 patches of CLIP's vision tower; each round the views' votes in the least similar patches weigh up to 1 + weight more, and in the least alike each view in turn shows its own colours; a change stays only if the mean similarity rises, so faces, eyes, and legs read as in the views. The reply adds semantic {scorer, start, before, after (mean similarity), views, rounds, kept, tried, patches, recolored, loadMs}."}
        }), mask_edit_props(true)), &[]),
        define("pixelate_views", "Turn an orthographic views sheet into pixel art at the cell grid a small creature (20 to 60 cells long) is carved or painted at, before carve_from_views or paint_from_views, so eyes, pupils, teeth, nostrils, spots, and thin legs and fins keep cells instead of smearing; writes no voxels. Content-adaptive downscaling (after Kopf, Shamir and Peers 2013 and Gerstner et al. 2012): one small palette for all views (colors, by median cut and k-means in Oklab, with paint_from_views' accent guard); each cell takes the colour holding most of a centre-weighted kernel over its footprint, never a mean (crisp edges, no in-between colours; anti-aliased fringe pixels do not vote); coverage, thin, features, and lines decide which cells keep ink and colour. views are as carve_from_views takes them, or a top-level path alone reads the sheet's 2 x 2 layout as measure_sheet does. Each view's ink bounds become its grid; size sets the box. Replies with size, colors, accents, mergedLines, legend ({char: #rrggbb}), and views, each {view, rows, legend, grid, inked, thin (cells the thin rule kept), lines (cells a drawn line holds), source (the ink crop in pixels), stretch (grid aspect over ink aspect; far from 1 the views disagree, such as a top view turned 90 degrees), features [{colour, area, cells, added, at [column, row]}]}: pass views straight to carve_from_views (same size) or paint_from_views. png writes the views side by side for checking. carve_from_views and paint_from_views do it in one call with downscale.", merged(json!({
            "views": {"type": "array", "items": {"type": "object"}, "description": "1..6 views: {view, path | rows, crop?, legend?, flip?, background?, tolerance?, alphaThreshold?, mask edits}. Leave out with a top-level path to use the sheet's measure_sheet layout."},
            "path": str_prop("The sheet PNG every view reads unless it has its own path."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "size": {"type": "array", "items": {"type": ["integer", "null"]}, "minItems": 3, "maxItems": 3, "description": "Box [x, y, z] in cells, 1..512 each; at least one axis, null follows the views' proportions ([null, 20, null] makes a creature 20 cells long)."},
            "colors": int_prop("Clusters of the shared palette, 1..32. Defaults to 6."),
            "accents": {"type": "boolean", "description": "Keep small distinct colours (eyes) their own colour beyond colors. Defaults to true."},
            "minShare": num_prop("Accent guard: share of the ink an accent needs, 0..1. Defaults to 0.002."),
            "contrast": num_prop("Accent guard: Oklab distance from every cluster an accent needs. Defaults to 0.12."),
            "maxAccents": int_prop("Accent guard: most accents beyond colors, 0..16. Defaults to 4."),
            "coverage": num_prop("Share of a cell's footprint that must be inked, 0..1. Defaults to 0.5."),
            "thin": {"type": ["number", "boolean"], "description": "Least inked share for a cell on the midline of a part thinner than a cell (a fin, a leg), 0..1; false keeps only coverage. Defaults to 0.25."},
            "despeckle": {"type": "integer", "description": "Majority filter passes over each view's grid: a cell whose colour fewer than two of its eight inked neighbours share takes their commonest colour (feature cells are kept), 0..16; 0 turns it off. Defaults to 1."},
            "features": {"type": ["boolean", "object"], "description": "Keep small salient features: true (default), false, or {minArea, maxArea (cells of area, defaults 0.15 and 6), contrast (Oklab from the mean colour around it, default 0.1), minCells (1..16, default 1)}. Each small one-colour region that stands out gets at least minCells cells (about its area, up to 4), smallest first, so a pupil keeps its place inside the eye."},
            "lines": {"type": ["boolean", "object"], "description": "Keep drawn lines thinner than a cell (an eye's ring, a mouth), which lose every cell's vote to the fill, as lines of cells in their colour: true (default), false, or {length (cells a stroke's midline must run through a cell, 0..4, default 0.6), contrast (Oklab from the cell's vote, default 0.08)}. A one-colour patch lines would take whole (a small mouth between its strokes) stays."},
            "outline": {"type": "boolean", "description": "With lines, keep the outline along the silhouette too, as a rim of cells in its colour. Defaults to false (a dark rim on a model's edge cells is seldom wanted): the outline's pixels then vote only where nothing else does, so no stray dark cell is left on the rim."},
            "mergeLines": {"type": ["number", "boolean"], "description": "With lines, merge colours living mostly in strokes within this Oklab distance of the commonest stroke colour into it, so an outline blended into a shadow is one colour and a view has one outline colour, 0..1.5. Defaults to 0.2; false keeps them apart. The reply's mergedLines counts them."},
            "png": str_prop("Write the pixelated views side by side to this PNG for checking."),
            "pngScale": int_prop("Pixels a cell in png, 1..64. Defaults to 8.")
        }), mask_edit_props(true)), &[]),
        define("measure_sheet", "Measure a views sheet image before carving or fitting it, instead of scripts; writes no file and changes nothing. Inked pixels (by the background rule and compare_silhouette's mask edits) split into connected regions; regions whose boxes come within gap pixels merge, and those under minShare of the largest go to ignored. Each view's crop is its tight box [x, y, width, height] in image pixels. A 2 x 2 sheet takes the layout names in reading order, and fitViews is then the views list carve_from_views and fit_to_views take with path. pitch is each view's cube size in pixels, from the autocorrelation of brightness steps across cube seams (pitchX across, pitchY down, strength 0..1 each), refined below a pixel from its multiples; cubes is the crop in cubes. checks compare front and side height, front and top width, and side width with top height (the length), in pixels and cubes, each ok within checkTolerance percent; ok is all of them. parts counts one view's pieces outside its body (the largest region left after an opening by radius pixels, default half the view's thickest point), each over minShare and reaching more than 0.42 radius past it (a boxy body's rounded corners do not): flippers, legs, a head, and a tail count once each, with bounds and a compass direction from the body (n is image up).", merged(json!({
            "path": str_prop("The views sheet PNG."),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height]: read only this part; crops in the reply stay in image pixels."},
            "background": {"type": ["string", "array"], "description": "auto (default: transparent pixels, else the commonest border colour), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "gap": num_prop("Pixels between two regions' boxes that still merge them into one view. Defaults to 12."),
            "minShare": num_prop("Regions under this share of the largest one's pixels are left out (labels, specks). Defaults to 0.02."),
            "layout": {"type": "array", "items": {"type": "string"}, "minItems": 4, "maxItems": 4, "description": "View names of a 2 x 2 sheet in reading order. Defaults to [front, side, top, back]."},
            "pitchRange": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "[low, high] cube pitch searched, in pixels. Defaults to [4, 96]."},
            "parts": {"type": ["boolean", "object"], "description": "Count the parts sticking out of a view's body: true (the top view) or {view (a layout name or an index), radius (opening radius in pixels), minShare (smallest part's share of the view, default 0.004)}."},
            "checkTolerance": num_prop("Largest difference, in percent, a check passes with. Defaults to 3.")
        }), mask_edit_props(true)), &["path"]),
        define("compare_silhouette", "Measure how closely the model reads like its concept from one camera, as numbers and a picture: the preview silhouette (preview's camera arguments) against a mask cut from a concept PNG (crop and background rule as in carve_from_views), placed by align at one uniform scale and offset, keeping proportions. Replies with iou, missing (share of the concept not covered), excess (share of the model outside it), pixel counts, align, both bounds and aspect ratios, and maskEdits (pixels each mask edit removed); writes an overlay PNG to path: the preview, the concept's outline in cyan, missing in magenta, excess in orange, numbers in the corner. A hero view or photo with water, splashes, or a rider scores nothing until masked (excludeColors, flood, mask, erase, keepLargest). width and height default to 512 tall at the crop's aspect. Writes a file, so Rhai scripts do not have it.", merged(merged(json!({
            "path": str_prop("Overlay PNG to write. Its folder is created."),
            "concept": str_prop("Concept PNG path"),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height] of the concept view in pixels. Defaults to the whole image."},
            "background": {"type": ["string", "array"], "description": "The concept's background: auto (default), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance of a key colour, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below which a pixel is background, 1..255. Defaults to 128."),
            "flip": json!({"type": "boolean", "description": "Mirror the concept left to right."}),
            "align": {"type": ["string", "object"], "description": "fit (default): most overlap; height or width: match that extent, bottoms and centres lined up; {scale, offset: [x, y]}: concept pixel p lands on p * scale + offset (reuse a reply's align to hold it still between edits)."}
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
        define("set_marker", "Name a point, a collision box, or a connector (a piece's joining end) on an object for a game: a ledge top, a spawn, a collision volume, where two pieces join. Saved in the .vox, listed in the export_mesh and export_models replies, and exported as a glTF node of its name with extras.froxel (kind; size, min, and max for a box; direction for a connector, whose node rotation turns -Z toward it) and in the scene's extras.froxelMarkers. Coordinates are local mesh units: cell (x, y, z) spans x..x+1, so the top centre of cell (4, 4, 10) is [4.5, 4.5, 11]. It moves with its object; a marker of the same name is replaced. One undo step. Replies with the marker in local, world, and glTF coordinates.", json!({
            "name": str_prop("Marker name, 1 to 64 characters, such as ledge_top_4m; the glTF node takes it."),
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
        define("collision_from_voxels", "Build collision box markers from an object's solid cells for a game, instead of boxes by hand: decks, hollow holds, and open holes, which one convex hull cannot give. Boxes are box markers <prefix>_1, <prefix>_2, ...: saved in the .vox, exported as glTF nodes, listed by list_markers, drawn by preview overlay: \"boxes\". Rerunning with a prefix replaces its boxes on the object; rerun when the shape changes. No box holds an empty block holeSize wide on each axis (clipped to the box: a hatch through a one-cell deck counts), so hollows stay open; smaller dents and gaps may be filled (reported). Set holeSize a little under the smallest opening the player uses. Box faces sit on cells, so walking surfaces are exact. If maxBoxes means closing a hollow, the least solid boxes are dropped (dropped, warning). Local mesh units; boxes move with the object. One undo step. Replies with boxes (the count), solidCells, ignoredBySlot, ignoredThin, targetCells (what the boxes aim to cover), coveredCells, missedCells, coveredPercent, boxCells, emptyFilledCells and emptyFilledPercent (wrongly filled empty cells, as a share of the boxes' volume), merged, dropped, replaced, prefix, and markers.", json!({
            "prefix": str_prop("Marker names are <prefix>_<number>. Default <object name>_collider. An existing marker of such a name that this tool would not replace is an error."),
            "layer": {"type": "integer", "minimum": 1, "maximum": 64, "description": "Horizontal cuts between boxes lie on planes this many cells apart, from the lowest cell, unless only another height opens a hollow. Default 1, any height. Not with zBands."},
            "zBands": {"type": "array", "items": {"type": "integer"}, "description": "Horizontal cuts by hand: ascending z planes in local mesh units, at least two, such as [0, 3, 8, 12]. Put one on each deck and walking level. Not with layer."},
            "minFeature": {"type": "integer", "minimum": 1, "maximum": 32, "description": "Ignore details thin on two axes (vines, rope): a cell is covered only inside a full square this wide in an axis plane, so a deck or wall thin on one axis stays and a one-cell diagonal goes. Default 1, keep everything."},
            "holeSize": {"type": "integer", "minimum": 1, "maximum": 64, "description": "The smallest empty block that stays open. Default 3. Larger: fewer boxes, more empty volume filled; 1 fills nothing, an exact cover."},
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Cover only cells of these palette indices. Omit for every slot."},
            "exclude": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices to leave out (moss, vines, rope): slots, not the boxes the stone tools take under this name."},
            "maxBoxes": {"type": "integer", "minimum": 1, "maximum": 4096, "description": "Most boxes. With boxes to spare the emptiest split further; with too many, neighbours merge, cheapest added empty volume first, as far as holeSize allows, then the boxes with the least solid are dropped. Omit to split only until no box holds a hollow."},
            "object": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."}
        }), &[]),
        define("list_markers", "Markers with their kind, object, and local, world, and glTF (Y-up) coordinates: a point, a box's min, max, centre, and size, a connector's point, direction, and node rotation.", json!({}), &[]),
        define("import_mesh", "Voxelize a .glb, .gltf, or .obj mesh (with .mtl and textures) into the active object, or a new one (newObject): a Blender sculpt or an image-to-3D result. Reads every triangle primitive of the default glTF scene under its node transforms, embedded and external buffers, and PNG, JPEG, and WebP (EXT_texture_webp) textures; KTX2/Basis textures are skipped with a warning; Draco and meshopt compression are refused. Size by size, scale, or voxelSize. fill solid first seals an open mesh's openings (openEdges above 0) with cells enclosed in at least 17 of 26 directions, so a hollow, double-walled, or open-bottomed shell comes out solid to its rim. Surface cells take texture x factor x vertex colour at the nearest point of the nearest triangle; inside cells the nearest surface colour. Colours within tolerance of a slot reuse it; the rest reduce to at most maxNewSlots colours in unused slots, from 255 down. Without coverage every touched cell is kept (no part lost, but fattened); coverage keeps a 20 to 60 cell creature as tight as its mesh, and keepThin keeps parts thinner than a cell. clean runs reduce_palette before writing, so noisy texture slots never reach the palette. At most 256 cells per axis and 2000000 cells, refused before any change. One undo step. Replies with cells, shell, interior, openEdges (one-triangle edges after welding equal positions; 0 is watertight), written, changed, slots, newSlots, bounds (min, max, size in local cells), scale, triangles, primitives, textures, warnings, clean, coverage {share, keepThin, fine, touched, kept, thin}, and thin (with minFeature).", json!({
            "path": str_prop("Path of the .glb, .gltf, or .obj"),
            "size": num_prop("Cells along the longest axis, 1..=256. Defaults to 64 when neither scale nor voxelSize is given."),
            "scale": num_prop("Cells per file unit (per metre for a glTF). Instead of size."),
            "voxelSize": num_prop("Metres per cell, such as 0.08: the mesh keeps its real size. Instead of size."),
            "upAxis": {"type": "string", "enum": ["y", "z"], "description": "The file's up axis. y (default, glTF and most OBJ) turns it to Z-up; z keeps the axes."},
            "rotation": vec3_prop("Degrees about x, then y, then z (cell axes), after the up axis. Defaults to [0, 0, 0]."),
            "position": vec3_prop("Where the anchor lands, in the active object's local cells (a cell corner); with newObject, the new object's world position, the anchor at its local origin. Defaults to [0, 0, 0]."),
            "anchor": {"type": "string", "enum": ["min", "center", "bottom"], "description": "Point of the mesh's box placed at position: min (default) the low corner, center, or bottom (the centre of the bottom face)."},
            "fill": {"type": "string", "enum": ["solid", "parity", "shell"], "description": "solid (default) fills all a flood from the box border cannot reach, after sealing an open mesh's openings; parity is the older majority vote of ray parity along x, y, and z; shell writes only cells the surface touches."},
            "colors": {"type": "string", "enum": ["auto", "vertex"], "description": "auto (default): base-colour texture times factor times vertex colours; vertex: vertex colours only."},
            "palette": int_prop("Paint every cell this palette index instead of sampling colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Map colours only to these palette indices (the nearest of them); the palette is unchanged."},
            "maxNewSlots": int_prop("Most palette slots written with new colours, 0..=255. Defaults to 32; 0 maps to the nearest existing slots."),
            "tolerance": num_prop("sRGB distance (0..=442) under which an existing slot is reused. Defaults to 12."),
            "mode": mode_prop(),
            "orient": {"type": ["object", "string"], "description": "Turn the mesh before voxelizing. \"upright\" levels a tilted image-to-3D mesh by its shape alone: up is the vertices' thinnest principal axis, signed so the form narrows upward, at most maxTilt degrees (default 40) from the file's up (a fish, whose thinnest axis lies flat, keeps its up); the end of the long level axis with the taller cross-section turns to -Y (round or square shapes keep up or yaw: keptUp, keptYaw). An object runs orient_to_views' search against a views sheet: {views (as orient_to_views takes them) or the sheet path alone (its measure_sheet layout), background, tolerance, turns, yawStep, yawRange, fineAxes, resolution, weight}; upright: true (and maxTilt) in it stands the mesh up first and searches only yaw. The reply adds orientation (and its upright)."},
            "clean": {"type": ["boolean", "object"], "description": "Run reduce_palette over the imported cells before they are written, in the same undo step: true is {colors: 8, despeckle: 1}, or its options (colors | palette, accents, minShare, contrast, maxAccents, despeckle, minSame, protect). The reply adds clean."},
            "coverage": num_prop("Keep a cell when this share of it (0..1, such as 0.5) is solid on a voxelization 2 to 4 times finer, instead of every touched cell, so a small creature's legs and fins do not merge into lumps. At most 256 cells on the longest axis. The reply adds coverage."),
            "keepThin": {"type": "boolean", "description": "With coverage: a part thinner than a cell (a fin, an ear, a leg) keeps the one cell its midline crosses, as a voxel remesher keeps thin sheets, instead of vanishing. Defaults to true."},
            "minFeature": int_prop("Report the imported parts thinner than this many cells, 2..16 (thin in the reply)."),
            "newObject": {"type": "boolean", "description": "Create a new object for the mesh and make it active. Defaults to false."},
            "name": str_prop("Name of the new object (with newObject). Omit for Object N.")
        }), &["path"]),
        define("mirror_object", "Copy an object, with its strands, mirrored across a world axis through the origin: cells use -coord-1 on that axis. The copy is unbound unless mirrorBones. One undo step.", json!({
            "index": int_prop("Object index to copy"),
            "axis": str_prop("x, y, or z"),
            "name": str_prop("Name of the copy"),
            "position": ivec3_prop("World cell of the copy's local (0, 0, 0). Omit to use the minimum corner of the mirrored cells."),
            "mirrorBones": {"type": "boolean", "description": "Copy the bound bone and its descendants, swapping an _l/_r suffix. Default false."}
        }), &["index", "axis", "name"]),
        define("clear_object", "Empty an object of all cells and strands at once, at any size (fill_box with palette 0 stops at 2000000 cells). Name, position, bone, script, and markers stay; remove_object deletes it instead. One undo step; an empty object adds none. Replies with index, name, removed, and voxels.", json!({
            "index": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."}
        }), &[]),
        define("add_strand", "Add a strand to an object: a chain of cubes along a curve, each cube turned along the path (its length along the tangent, width to the path's left, thickness up), so a sedge leaf, a reed, a water weed, a barb, or a fin ray bends smoothly instead of stepping on the grid, and a section under one cell gives detail finer than the grid. The strand stays editable: it is stored as its definition (points, section, step, colours) in the object's local cells, moves with the object, its bone, and the pose, and is drawn, traced, and exported as the same turned boxes everywhere (viewport, preview, model_view, render, export_mesh), never rasterized into cells; bake_strands writes cells from it when a grid copy is wanted (MagicaVoxel shows only cells). Consecutive cubes of one frame and slot merge into one box, so a straight run costs one box. At most 4096 strands and 20000 boxes per object. One undo step. Replies with object, name, strands (count), boxes (the object's), and strand (its index, definition, boxes, length).", merged(strand_props(), json!({})), &["points"]),
        define("edit_strand", "Change a strand of an object (index, from add_strand or list_strands): any of add_strand's fields replaces that field (points replaces the path); point with at moves one point (a position alone keeps its twist and scale); insert with at adds a point before that index (the count appends); remove deletes a point; move [dx, dy, dz] shifts the whole strand by cells. One undo step. Replies as add_strand.", merged(strand_props(), json!({
            "index": int_prop("The strand's index on the object"),
            "point": int_prop("Index of the point to move to at"),
            "insert": int_prop("Index to insert a new point at (the point count appends)"),
            "remove": int_prop("Index of the point to delete; a strand keeps at least 2"),
            "at": {"type": ["array", "object"], "description": "The moved or inserted point: [x, y, z, twist?, scale?] or {x, y, z, twist?, scale?}"},
            "move": vec3_prop("Shift every point by [dx, dy, dz] cells")
        })), &["index"]),
        define("remove_strand", "Remove a strand of an object by index, or every strand with all: true. One undo step. Replies with object, name, strands, boxes, and removed.", json!({
            "object": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."},
            "index": int_prop("The strand's index on the object"),
            "all": bool_prop("Remove every strand of the object")
        }), &[]),
        define("list_strands", "The strands of an object: each one's index, points ([x, y, z, twist, scale]), curve, width, thickness, step, palette or palettes with pattern, boxes, and length, plus the object's box total.", json!({
            "object": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."}
        }), &[]),
        define("bake_strands", "Write a grid copy of strands into their object's cells: every cell whose centre lies inside a strand box takes its slot, over what is there, for MagicaVoxel or for grid tools. strands picks strand indices (default all); skin adds the skin's bricks; keep leaves the strands in place, otherwise they are removed so the object does not hold both. One undo step. Replies with object, name, strands (left), boxes, cells (covered), changed, baked, kept, and voxels.", json!({
            "object": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."},
            "strands": {"type": "array", "items": {"type": "integer"}, "description": "Strand indices to bake. Defaults to all."},
            "keep": bool_prop("Keep the strands (and skin) after baking. Defaults to false."),
            "skin": bool_prop("Bake the object's skin bricks too, and remove the skin unless keep. Defaults to false.")
        }), &[]),
        define("skin", "Cover an object's surface with irregular bricks instead of grid steps, the look of the Frog World creatures: bricks of uneven length in staggered rows along a flow axis, each with its own relief, slight tilt, and the commonest colour of the cells under it. The skin is stored as these settings and its bricks are derived from the cells, so a later edit reshapes it; they are drawn, traced, and exported as turned boxes everywhere (viewport, preview, model_view, render, export_mesh), and the cells stay as the core under them. Thin parts (fins, whiskers) and bare slots (eyes, teeth) keep their cells. Each call sets the whole skin (an omitted field takes its default); off removes it. For rig parts give every part the same axis: the rows of parts that meet continue each other. bake_strands with skin writes a grid copy for MagicaVoxel, which shows only cells. At most 50000 bricks per object. One undo step. Replies with object, name, skin, and bricks.", json!({
            "object": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."},
            "off": bool_prop("Remove the object's skin. Defaults to false."),
            "axis": ui(with_default(json!({"type": "string", "enum": ["auto", "x", "y", "z"], "description": "The flow the rows run along: a body's length. auto takes the longest spread of the object's cells. Defaults to auto."}), json!("auto")), "Axis", "The flow the rows run along"),
            "length": ui(json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "Shortest and longest brick along the flow, in cells, 0.25..=64: [2, 4] when omitted."}), "Length", "Shortest and longest brick, in cells"),
            "width": soft(ui(hard(with_default(num_prop("Row spacing around the body, in cells, 0.25..=64. Defaults to 1.5."), json!(1.5)), 0.25, 64.0), "Width", "Row spacing around the body, in cells"), 0.5, 8.0, 2),
            "height": soft(ui(hard(with_default(num_prop("How far a brick stands out past the cells under it, in cells, 0..=8. Defaults to 0.4."), json!(0.4)), 0.0, 8.0), "Height", "How far a brick stands out, in cells"), 0.0, 2.0, 2),
            "depth": soft(ui(hard(with_default(num_prop("How deep a brick reaches into the body, in cells, 0..=8. Defaults to 0.75."), json!(0.75)), 0.0, 8.0), "Depth", "How deep a brick reaches in, in cells"), 0.0, 2.0, 2),
            "stagger": soft(ui(hard(with_default(num_prop("How far each row is offset from the next, as a share of a brick, 0..=1. Defaults to 0.5."), json!(0.5)), 0.0, 1.0), "Stagger", "Offset of each row from the next"), 0.0, 1.0, 2),
            "relief": soft(ui(hard(with_default(num_prop("Random change of each brick's height, in cells, 0..=8. Defaults to 0.3."), json!(0.3)), 0.0, 8.0), "Relief", "Random change of each brick's height"), 0.0, 1.0, 2),
            "tilt": soft(ui(hard(with_default(num_prop("Random tilt and turn of each brick, in degrees, 0..=45. Defaults to 2."), json!(2.0)), 0.0, 45.0), "Tilt", "Random tilt of each brick, in degrees"), 0.0, 15.0, 1),
            "follow": soft(ui(hard(with_default(num_prop("How far bricks turn from the grid toward the smoothed surface, 0..=1: 0 keeps faces square, so a curve reads as steps of rows; 1 lays them on the surface like shingles. Defaults to 0."), json!(0.0)), 0.0, 1.0), "Follow", "Turn bricks toward the surface"), 0.0, 1.0, 2),
            "gap": soft(ui(hard(with_default(num_prop("Groove between bricks, in cells, 0..=1. Defaults to 0.12."), json!(0.12)), 0.0, 1.0), "Gap", "Groove between bricks, in cells"), 0.0, 0.5, 2),
            "smooth": soft(ui(hard(with_default(num_prop("Blur radius of the surface the bricks follow, in cells, 0.5..=8. Defaults to 2."), json!(2.0)), 0.5, 8.0), "Smooth", "Blur of the surface the bricks follow"), 0.5, 4.0, 1),
            "thin": soft(ui(hard(with_default(num_prop("Parts thinner than about this many cells keep their cells bare (fins, whiskers), 0..=64; 0 covers everything. Defaults to 3."), json!(3.0)), 0.0, 64.0), "Thin", "Thinner parts keep their cells"), 0.0, 8.0, 1),
            "bare": ui(json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices never covered: eyes, teeth, markings to keep. Omit for none."}), "Bare", "Slots the skin never covers"),
            "seed": soft(ui(with_default(int_prop("The layout's seed. Defaults to 0."), json!(0)), "Seed", "Another seed lays the bricks out anew"), 0.0, 999.0, 0)
        }), &[]),
        define("split_object", "Move (or copy) an object's cells in boxes, of some slots, or both, into a new or existing object at the same world positions: cut a carve_from_views blockout into rig parts (head, flippers, tail) instead of a carve per part plus complement erases. A new object takes the source's position (same local cells); an existing target (into) takes them at its own offset, replacing what it holds there. One undo step. Replies with target, name, created, cells (taken), replaced, position, sourceVoxels, targetVoxels, and bounds (target local).", json!({
            "index": {"type": ["integer", "string"], "description": "Source object index or name. Defaults to the active object."},
            "boxes": {"type": "array", "description": "One inclusive box [x0, y0, z0, x1, y1, z1] in the source's local cells, or a list of up to 64. A cell inside any of them is selected. Omit to select by slots alone."},
            "slots": {"type": ["integer", "array"], "items": {"type": "integer"}, "description": "Only cells of these palette slots (the mask). Omit for every slot."},
            "invert": {"type": "boolean", "description": "Take every cell the boxes and slots do not select. Default false."},
            "copy": {"type": "boolean", "description": "Leave the cells in the source too. Default false (move)."},
            "into": {"type": ["integer", "string"], "description": "Existing target object, by index or name. Omit to create a new object."},
            "name": str_prop("Name of the new object. Defaults to the source's name with _part."),
            "makeActive": {"type": "boolean", "description": "Make the target the active object. Default false."}
        }), &[]),
        define("add_bone", "Add a bone. One undo step.", json!({
            "name": str_prop("Unique bone name"),
            "parent": {"type": ["string", "null"], "description": "Parent bone name. Omit or null for a root."},
            "pivot": vec3_prop("World pivot [x, y, z]. Fractional values sit between cells."),
            "rotation": vec4_prop("Quaternion [x, y, z, w] in voxel space. Defaults to identity."),
            "rest": vec4_prop("Rest turn [x, y, z, w], local, on top of rotation: the angle the bone's parts are drawn, edited, saved, and exported at, their cells flat on their own grid. Defaults to identity.")
        }), &["name", "pivot"]),
        define("set_bone", "Change a bone's parent, pivot, rotation, rest turn, or name; omitted fields stay. rest is the saved angle the bone's parts are drawn, edited, and exported at: model a fin or a seat flat as its own object, bind it to a bone pivoted where it meets the body, and turn it there; the cells stay on the part's grid and the glTF joint node rests at that turn. joint true puts the pivot where the bone's part meets its parent's part (the new parent, when given), as place_joints does for one bone; the reply then has the joint's method, samples, gap, and previous pivot. One undo step.", json!({
            "name": str_prop("Bone to edit"),
            "parent": {"type": ["string", "null"], "description": "Parent bone; null makes a root. Omit to leave it."},
            "pivot": vec3_prop("World pivot [x, y, z]"),
            "joint": {"type": "boolean", "description": "Measure the pivot at the joint with the parent's part (see place_joints). Not with pivot."},
            "snap": {"type": "string", "enum": ["none", "corner", "center", "half"], "description": "With joint: round the pivot to cell corners, cell centres, or half cells. Default none."},
            "band": num_prop("With joint, parts apart: cells past the closest gap that still count. Default 0.5."),
            "rotation": vec4_prop("Quaternion [x, y, z, w]"),
            "rest": vec4_prop("Rest turn [x, y, z, w], local, on top of rotation (a fin sculpted flat, turned onto the body). Identity clears it."),
            "restFromPose": {"type": "boolean", "description": "Keep the bone's preview turn (set_pose) as its rest turn and drop it from the pose, so the part stays where it is drawn. Not with rest."},
            "rename": str_prop("New unique name")
        }), &["name"]),
        define("place_joints", "Put bone pivots at the joints between rig parts, after split_object and bind_object, instead of by eye. A bone's part is the cells of the objects bound to it; the parent part is that of the nearest ancestor bone with one. The pivot is the centroid of the cells both parts hold (method overlap), else of their touching faces (contact), else of midpoints between the child's surface cells and the parent's nearest surface cells within band cells of the closest gap (nearest; gap is the empty cells between). Every bone by default; roots and bones with no part are skipped with the reason, and a named bone that cannot be placed is an error. One undo step. Replies with placed [{name, parent, pivot, previous, method, samples, gap}] and skipped [{name, reason}].", json!({
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
        define("set_pose", "Preview a pose in the window; the bind pose does not change. Each quaternion is local, multiplied on the right of the bind rotation and the bone's rest turn, so a parent orbits its children. Omitted bones stay at rest; identity is a no-op. A posed part is drawn turned and its tools edit it there, on its own grid; local cell coordinates in the tools do not change. One undo step.", json!({
            "pose": {"type": "object", "description": "Bone name to a quaternion [x, y, z, w] or {rotation, scale}; scale is local per axis, 0 to 16, inherited by children. Replaces the preview.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}}
        }), &["pose"]),
        define("clear_pose", "Remove the preview pose. The bind pose stays. One undo step.", json!({}), &[]),
        define("add_clip", "Add an empty named clip. Keys are local TRS in voxel space: translation is the full offset from the parent, not a delta. One undo step. Saved in the .vox MVAN chunk and written to glTF as an animation.", json!({
            "name": str_prop("Unique clip name")
        }), &["name"]),
        define("set_clip_keys", "Replace one bone's keys in a clip. Times must be finite, >= 0, and increasing. One undo step.", json!({
            "name": str_prop("Clip name"),
            "bone": str_prop("Bone name"),
            "keys": {"type": "array", "description": "Keyframes {time, rotation, translation, scale}; only time is required. rotation is [x, y, z, w], identity if omitted; translation [x, y, z], [0, 0, 0] if omitted; scale [x, y, z], [1, 1, 1] if omitted.", "items": {"type": "object"}}
        }), &["name", "bone", "keys"]),
        define("list_clips", "Named clips and their bone keys.", json!({}), &[]),
        define("remove_clip", "Remove a clip. One undo step.", json!({
            "name": str_prop("Clip name")
        }), &["name"]),
        define("undo", "Undo the last edit.", json!({}), &[]),
        define("redo", "Redo the last undone edit.", json!({}), &[]),
        define("render", "Path-trace the whole scene to a PNG; blocks until it is written. The camera orbits at yaw and pitch and fits the framed box (every visible posed cell, or target) inside the frame with margin, at every angle of the call; zoom and distance scale that fit. Given only width or only height, the other side follows the model's shape. atmospheric, skyIntensity, glassMode, stochasticGi, nee, ris, indirectClamp, horizon, aperture, focus, and the sun options left out keep the document setting. Replies framed (the world box) and distance.", {
            let denoise = if crate::denoise::available() {
                "Open Image Denoise on the finished frame, using the first-hit albedo and normal. Default true."
            } else {
                "Edge-aware filter before writing. Default true. Build with --features oidn for Open Image Denoise."
            };
            merged(json!({
            "path": str_prop("PNG file to write. Its folder is created."),
            "width": int_prop("Pixels, 1..4096, at most 4096 x 2048 in all. Default 256, or from height and the model's shape."),
            "height": int_prop("Pixels, 1..4096. Default 256, or from width and the model's shape."),
            "samples": int_prop("Path-trace samples, 1..8192. Default 16. CPU: about a second per 16 samples at 256 x 256; gpu is far faster."),
            "bounces": int_prop("Max bounces, 0..8. Default 4."),
            "seed": int_prop("Sampling seed. Default 1."),
            "denoise": {"type": "boolean", "description": denoise},
            "atmospheric": {"type": "boolean", "description": "Nishita sky."},
            "skyIntensity": num_prop("Sky intensity."),
            "sunIntensity": num_prop("Sun intensity."),
            "glassMode": str_prop("probabilistic or absorption."),
            "sky-intensity": num_prop("Older name of skyIntensity; still accepted."),
            "sun-intensity": num_prop("Older name of sunIntensity; still accepted."),
            "glass-mode": str_prop("Older name of glassMode; still accepted."),
            "aces": {"type": "boolean", "description": "ACES filmic tonemap. Default true."},
            "stochasticGi": {"type": "boolean", "description": "Stochastic area lights for the indirect bounces."},
            "gpu": {"type": "boolean", "description": "Trace with wgpu on the GPU instead of the CPU; an error with no adapter. Default false."},
            "albedoPath": str_prop("Also write the first-hit albedo of the main view as a PNG here."),
            "normalPath": str_prop("Also write the first-hit normal of the main view as a PNG here."),
            "aperture": num_prop("Lens aperture."),
            "focus": num_prop("Focus distance."),
            "nee": ui(bool_prop("Power-weighted next-event estimation of emissive faces, with stochasticGi."), "Power-weighted NEE", "Pick emissive faces by power for direct light"),
            "ris": soft(ui(hard(int_prop("Candidates per light sample (resampled importance sampling), with stochasticGi, 1..32."), 1.0, 32.0), "RIS samples", "Candidates per light sample"), 1.0, 32.0, 0),
            "indirectClamp": soft(ui(hard(num_prop("Largest radiance an indirect bounce may carry, against fireflies, 0..32; 0 is no clamp."), 0.0, 32.0), "Indirect clamp", "Largest radiance a bounce may carry; 0 is none"), 0.0, 32.0, 1),
            "horizon": soft(ui(hard(num_prop("Flat sky: degrees the horizon gradient blends over, 0..15."), 0.0, 15.0), "Horizon", "Degrees the flat sky's horizon blends over"), 0.0, 15.0, 1),
            "sunElevation": soft(ui(hard(num_prop("Sun height above the horizon in degrees, -90..90."), -90.0, 90.0), "Elevation", "Sun height above the horizon in degrees"), -90.0, 90.0, 0),
            "sunAzimuth": soft(ui(hard(num_prop("Sun direction round up in degrees, 0..360."), 0.0, 360.0), "Azimuth", "Sun direction round up in degrees"), 0.0, 360.0, 0),
            "sunDisc": soft(ui(hard(num_prop("Sun disc size in degrees, 0.1..30; larger gives softer shadows."), 0.1, 30.0), "Disc size", "Sun disc size in degrees"), 0.1, 30.0, 1),
            "sunColor": ui(json!({"type": ["string", "array"], "items": {"type": "integer"}, "description": "Sun colour, \"#rrggbb\" or [r, g, b] in 0..255."}), "Color", "Sun colour"),
            }), json!({
            "yaw": num_prop("Orbit yaw in degrees: 0 puts the camera on +X looking toward -X; a model facing -Y is face-on at -90. Default 30."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90; 90 looks straight down. Omit for the framing angle (25)."),
            "distance": num_prop("Multiple of the fitted distance, greater than 0 and at most 100. Below 1 is closer. Default 1."),
            "zoom": num_prop("Magnification of the fitted frame, 0.01..100. 2 is twice as large (crops the box), 0.5 half. Default 1."),
            "margin": num_prop("Empty border on each side of the fitted box as a fraction of the frame, 0..0.45. Default 0.05."),
            "target": target_prop(),
            "orthographic": {"type": "boolean", "description": "Parallel rays, fitted like perspective. Default false."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. A zero scale hides that bone's parts. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}},
            "showBones": {"type": "boolean", "description": "Draw joint points and parent links on the PNG. Default false."},
            "view": view_prop(),
            "views": {"type": "array", "description": "Extra PNGs, each {yaw, pitch, path} or {view, path}; path is still written at the top-level yaw and pitch.", "items": {"type": "object"}}
            }))
        }, &["path"]),
        define("model_view", "Flat model view to a PNG, like the editor's: each pixel is the palette colour of the first visible cell under its centre, with no light, tonemap, denoise, or path tracer, so it compares directly with a turnaround sheet. The camera fits the framed box like render; all views of a call share one centre and scale. Orthographic replies pixelsPerVoxel.", json!({
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
        define("preview", "Fast lit PNG in under a second, the check to run after every step: the viewport's mesh and light (sun plus ambient), rasterised on the CPU, with no path tracer or window. The camera orbits at yaw and pitch, fitting the model or one object. render shows lighting and materials, model_view palette-exact colours. lighting uses a game's light (old_pond: frog-world's Old Pond, warm sun, blue ambient, its background) to judge colour under it; preset names a camera (game: the gameplay camera); placementYaw shows an asset as placed in a level, turned about up. inputs, variants, or reference make a contact sheet, one labelled tile per model; width and height are then per tile. The open document and its undo history are left as they were. overlay draws markers and collision boxes. previewTriangles counts the unmerged mesh drawn; export.triangles and export.quads count what export_mesh or export_models writes with the options in export (all off by default), so pass the game's options for its count. countsLabel prints both. Replies with tiles (label, x, y, width, height, previewTriangles, export, voxels, params).", merged(json!({
            "path": str_prop("PNG file to write. Its folder is created."),
            "glb": str_prop("Draw this written .glb instead, unlit (nearest texel times COLOR_0, no light), as a baked export looks unshaded in a game. Only the camera, size, background, and antialias arguments apply."),
            "width": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "height": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "preset": str_prop("game (orthographic, yaw -135, pitch 32; frog-world's gameplay camera unless presets or the user's camera_presets.json redefines it), default (perspective, yaw 30, pitch 25), or one from presets. yaw, pitch, view, orthographic, fov, and margin given win over it."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "inputs": {"type": "array", "items": {"type": ["string", "object"]}, "description": "Contact sheet: .vox paths or {path?, label?, params?} (no path: the open document; params: set_params values baked in first). At most 64."},
            "variants": {"type": ["array", "string"], "description": "Contact sheet of the open scripted document: parameter sets [{name?, params}] as export_variants takes, or a variants JSON path, each baked in a scratch run; the document is restored."},
            "reference": {"type": ["string", "object"], "description": "Contact sheet: a PNG (concept or turnaround) or {path, label?}, fitted into the first tile, aspect kept. Alone, it sits beside the open document."},
            "columns": int_prop("Contact sheet columns, 1..65. Default about square."),
            "labels": {"type": "boolean", "description": "Contact sheet: each tile's name and parameters under it. Default true."},
            "overlay": {"type": ["boolean", "string", "array"], "description": "Draw markers (set_marker, Rhai marker, collision_box, connector): true or all, boxes (outlines), points, connectors (an arrow along their direction), markers, or a list. What the model hides is faded and dashed, to check a collision box against the stone. Replies with overlay: each marker's name, kind, object, anchor pixel, and whether it is hidden. Also on contact sheet tiles."},
            "overlayLabels": {"type": "boolean", "description": "Each overlay marker's name beside it. Default true."},
            "export": {"type": ["object", "boolean"], "description": "With bevel the image draws the cells and strands bevelled as that export writes them. Options for the reply's export counts, as export_mesh takes them (merge, colorAtlas, vertexColors, exportHidden, splitObjects, bevel, bevelMode, aoStrength, aoRadius, aoTarget, bake, gameView; all off when absent). Replies export {triangles, quads, and these options}, or error when that export would fail. Counts cover the whole document whatever object frames and match the glb; unmerged is quick, merge costs about an export. false skips it."},
}), json!({
            "lighting": lighting_prop(),
            "lightingPresets": lighting_presets_prop(),
            "countsLabel": {"type": "boolean", "description": "Write previewTriangles and the export counts in the image's bottom-left corner, and in each contact sheet tile. Default false."},
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. A model facing -Y is face-on at yaw -90. Default 30."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Default 25."),
            "orthographic": {"type": "boolean", "description": "Parallel projection fitted tightly to the silhouette; replies pixelsPerVoxel. Default false (the viewport's 50 degree perspective)."},
            "fov": num_prop("Vertical field of view in degrees for perspective, 10..120. Default 50, like the viewport; lower flattens a long model."),
            "margin": num_prop("Empty border on each side as a fraction of the image, 0..0.45. Default 0.06."),
            "object": {"type": ["integer", "string"], "description": "Frame this object (index or name) instead of the whole model; other visible objects are still drawn unless isolate. A hidden object named here is drawn."},
            "isolate": {"type": "boolean", "description": "Draw only object. Default false."},
            "background": {"type": ["string", "array"], "description": "#rrggbb, #rrggbbaa, or [r, g, b, a?]. Default the viewport grey #16181c; #00000000 is transparent."},
            "antialias": {"type": "boolean", "description": "2x2 supersampling up to 2048x2048 output pixels. Default true."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}}
        })), &["path"]),
        define("reload", "Restart this MCP server on the froxel binary now on disk, so a rebuild's new tools appear without the user restarting it. It re-executes in place (same process, stdin, and stdout) once the new binary passes froxel mcp --reload-check; a broken or older build is refused and this server keeps running. The document survives via a temporary .vox (path, active object, unsaved state; not undo history or the preview pose); with --connect it stays in the editor window. Then notifications/tools/list_changed is sent: a client honouring listChanged (declared here) re-reads tools/list, others keep the old list until they reconnect. Without force nothing happens if the binary is unchanged since the server started. With --watch this runs by itself before the first request after a rebuild. Unix only. Replies with reloaded, binary, changed, document, and next, or reloaded false and the reason.", json!({
            "force": {"type": "boolean", "description": "Reload even when the binary has not changed. Default false."}
        }), &[]),
        define("set_view", "Move the session camera (with a window, the editor's camera, so the next screenshot with region viewport shows that exact view). Arguments left out keep their value. Not an edit and not undoable. The edit viewport stays lit; use model_view for palette-exact colours.", json!({
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees; 0 puts the camera on +X, looking toward -X."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90; 90 looks straight down."),
            "frame": {"type": "boolean", "description": "Centre on every object at the framing distance first. Default true."},
            "distance": num_prop("Multiple of the framing distance, greater than 0 and at most 100. Needs frame. Default 1."),
            "orthographic": {"type": "boolean", "description": "Parallel projection in the edit viewport and the CPU trace."},
            "preset": str_prop("Named camera: game (orthographic, yaw -135, pitch 32: frog-world's gameplay camera unless presets or the user's camera_presets.json redefine it), default, or one from presets; yaw, pitch, view, and orthographic here win over it."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "bones": {"type": "boolean", "description": "Draw the skeleton overlay; false hides it for a screenshot."},
            "oneFrame": ui(bool_prop("Render mode traces one frame, starting a new one once the view holds still instead of on every orbit or edit step."), "One frame", "Do not start over on every orbit or edit step; trace again once the view holds still"),
            "temporalSmooth": soft(ui(hard(num_prop("Live Render mode: the least weight a new frame gets against earlier ones, 0..1; lower is smoother and slower to follow."), 0.0, 1.0), "Temporal smooth", "Least weight a new live frame gets"), 0.0, 1.0, 3)
        }), &[]),
        define("set_tool", "Pick the window's tool and its options, as the tool strip, mode buttons, palette, and tool options row do, so a ui_click on the viewport then stamps that tool. Every argument left out is kept. Not an edit and not undoable. Replies with the tool, mode, slot, and options as they now are.", json!({
            "tool": ui(json!({"type": "string", "enum": ["brush", "line", "circle", "square", "box", "face", "generator", "tube", "strand", "sculpt", "mutator", "marquee", "box_select", "flood_select"], "description": "The window's tool."}), "Tool", "The window's active tool"),
            "mode": ui(json!({"type": "string", "enum": ["add", "remove", "paint"], "description": "Edit mode."}), "Mode", "Add, remove, or paint"),
            "slot": ui(int_prop("The active palette slot, 0..255."), "Slot", "The active palette slot"),
            "size": soft(ui(int_prop("Brush, Line, Circle, Square, Box, Mutator: brush size in cells, 1..63."), "Size", "Brush size in cells"), 1.0, 63.0, 0),
            "shape": ui(json!({"type": "string", "enum": ["square", "circle"], "description": "The brush's shape."}), "Shape", "The brush's shape"),
            "seedColor": soft(ui(int_prop("Mutator: the slot foliage grows from, 1..255."), "Slot", "The slot foliage grows from"), 1.0, 255.0, 0),
            "density": soft(ui(num_prop("Mutator: the share of free cells beside the seed colour that grow, 0..1."), "Density", "Share of free cells that grow"), 0.0, 1.0, 2)
        }), &[]),
        define("screenshot", "PNG of the editor window, its 3D viewport (region viewport), or a rectangle (x, y, width, height in window points from the top-left of the client area). Only when connected to a window.", json!({
            "region": {"type": "string", "enum": ["window", "viewport"], "description": "window (default) or viewport. Ignored with a rectangle."},
            "x": num_prop("Rectangle left edge, window points; give all four of x, y, width, height."),
            "y": num_prop("Rectangle top edge, window points."),
            "width": num_prop("Rectangle width, window points."),
            "height": num_prop("Rectangle height, window points."),
            "path": str_prop("Optional file to write. Its folder is created.")
        }), &[]),
        define("ui_inspect", "Controls from the previous frame: tool buttons, mode, outliner, sliders, palette cells, viewport chips, and the viewport rectangle, in window points. Only when connected to a window.", json!({}), &[]),
        define("ui_click", "Click a control by its ui_inspect id or the topmost registered control at a window point. An id needs no coordinates and survives a redraw that moved the control; the click lands at its centre, at x, y if given, or at value along a slider's track. A viewport hit stamps the active tool, a slider hit sets the slider from the point, a palette cell selects that slot. An open drop-down's items take the click first, and picking one closes it. A point on no registered control gets a real pointer press and release, clicking any widget there (checkbox, combo box, button); the reply is then pointer [x, y]. Only when connected to a window.", json!({
            "id": str_prop("A control's id from ui_inspect, such as tool.generator or op.generate.kind.conifer: instead of a point, or with one inside the control."),
            "value": num_prop("With the id of a slider: the value to set, as a point that far along its track."),
            "x": num_prop("X in window points, origin at the top-left of the client area"),
            "y": num_prop("Y in window points, origin at the top-left of the client area")
        }), &[]),
        define("set_script", "Store a Rhai script on an object and read its parameters. One undo step. Voxels change only when regenerate runs. A failure keeps the source and reports the script line. Script functions: the sculpt tools (fill_box with a mode, fill_ellipsoid, fill_rounded_box, fill_tube, generate, blend_shapes, carve_box, carve_ellipsoid, carve_rounded_box, carve_tube, recolor, surface_stroke, shell_plates, smooth, break_end, scatter, joints, column_relief, weather, bark, cavity_tint, radial_relief, foliage, moss, algae, vines, sweep, trim_outline, loft, deform, brush, voxelize (import_mesh's voxelizer on an inline mesh; scripts read no files), carve_from_views with views as rows and a legend, pixelate_views with views as rows (returns pixel art rows at the cell grid), project_paint and decal with the image as rows and a legend, fit_to_views with views as rows and a legend (no other objects in view; reference names an object the run builds), and marker, collision_box, connector, collision_from_voxels) with the MCP options in a trailing map such as #{mode: \"empty\"}; get_voxels, set_voxels; the document tools (set_palette, set_object, remove_object, clear_object, mirror_object, split_object, add_bone, set_bone, place_joints, remove_bone, bind_object, list_bones, set_pose, clear_pose, add_clip, set_clip_keys, list_clips, remove_clip), applied after the script in the same undo step; sqrt, pow, PI, min, max, clamp, push on arrays, and strings (\"ledge_\" + 4, `ledge_${i}`, to_string). docs/scripting.md lists them.", json!({
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
        define("regenerate", "Run the object's script and replace the voxels it writes. One undo step. No operation or voxel cap; the run stops at timeoutSeconds. A timeout or runtime error leaves the document unchanged and includes the script line.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "timeoutSeconds": {"type": "number", "description": "Stop the run after this many seconds. Default 300. 0 waits as long as it takes."}
        }), &[]),
        define("export_mesh", "Write a glTF 2.0 .glb of every object, exposed faces only. The default material is a 256-wide palette texture. A swaying or bobbing slot adds TEXCOORD_1 (sway, bob) on every vertex. A skeleton writes one rigid skin: each vertex one bone, weight 1. The reply has the file's node, mesh, skin and animation structure, quads, triangles (every triangle in the glb), and the markers counted by kind ({\"box\": 22, \"connector\": 2}); reply: \"full\" lists each marker (local, world, glTF) as list_markers does.", json!({
            "path": str_prop("Where to write the .glb. Its folder is created."),
            "merge": {"type": "boolean", "description": "Greedy-merge coplanar faces of one palette slot. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "Store a linear COLOR_0 on each vertex instead of the palette texture. Defaults to false."},
            "exportHidden": {"type": "boolean", "description": "Include hidden objects. Default false, which omits them."},
            "splitObjects": {"type": "boolean", "description": "One mesh node per object, sharing one skin, so a game can hide a part. Default false is a single mesh."},
            "colorAtlas": {"type": "boolean", "description": "Merge coplanar faces across palette slots and paint each cell into a per-mesh colour atlas texture (nearest filtering) instead of the palette: far fewer triangles on noisy palettes. Faces of different motion or opacity stay apart. One rigid mesh only: not with a skeleton, splitObjects or vertexColors. Defaults to false."},
            "bevel": soft(ui(hard(with_default(num_prop("Bevel every cell instead of exporting flat faces: each cube is cut along its edges this many cells back on both faces, 0..=0.4, for a toy-block look at low resolution. Faces are not merged (merge and the colour atlas's merge do not apply), so expect three to four times the triangles. Strand boxes are bevelled too. preview with export {bevel} draws it. Defaults to 0, off."), json!(0.0)), 0.0, 0.4), "Bevel", "Cut every cube's edges this many cells back"), 0.0, 0.4, 2),
            "bevelMode": ui(with_default(json!({"type": "string", "enum": ["cubes", "edges"], "description": "With bevel: cubes rounds every cube where it meets open space, so shallow grooves run between neighbours on the surface; edges rounds only the shape's outward edges, so flat runs stay whole and the silhouette softens. Defaults to cubes."}), json!("cubes")), "Bevel mode", "Every cube, or only the shape's outer edges"),
            "aoStrength": {"type": "number", "minimum": 0, "maximum": 1, "description": "Bake ambient occlusion so crevices, joints and overhang undersides export darker, at no runtime cost. 0..1, the light a fully occluded corner loses. Multiplied into vertexColors, else a grey COLOR_0 that glTF (and Godot's StandardMaterial3D) multiplies with the texture; aoTarget picks vertex colours or the colour atlas texels. Defaults to 0, off."},
            "aoRadius": {"type": "integer", "minimum": 0, "maximum": 8, "description": "Occlusion reach in cells beyond the four touching a corner, a cosine-weighted hemisphere, so deep crevices and overhangs darken more. 0 or 1 is the corner term alone. Defaults to 3."},
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "Where aoStrength goes. vertex: a per-corner COLOR_0; faces whose corners shade differently do not merge, costing triangles. texture (needs colorAtlas): each cell's atlas texel darkened by its corners' mean, faces merging as without occlusion. both: the two, for a shader that uses one (a material multiplying texture by COLOR_0 darkens twice). auto (default): texture with colorAtlas, else vertex."},
            "bake": bake_prop(),
            "gameView": game_view_prop(),
            "reply": reply_prop()
        }), &["path"]),
        define("export_variants", "Bake a scripted document once per parameter set, writing <name>.vox and <name>.glb into dir with the export_mesh mesh options, then a manifest JSON of each set's requested and resolved parameters, markers (local, world, glTF), bounds (voxel and glTF), quads, voxels, and mesh options. An existing manifest is merged by variant name (these sets replace their entries in place, others stay in order, new names are appended), so a subset can be redone. Top-level options are present while every entry shares them. An entry records sceneShift when the document opened with one (see open_file). Parameters are keyed by name, or object/name when two objects share one, with set_params values (a colour is whole 0..255, or 0..1 with a decimal point), so an entry's params pass back unchanged. Every script runs per set, as regenerate does. The open document and its undo history are left as they were. A failing set stops the call with its name before the manifest is written. Replies with the manifest path, the names added, replaced, and kept, the shared options, and per set exported now its name, glb and vox paths, bounds, voxels, quads, sceneShift, and markers counted by kind ({\"box\": 230}); parameters and marker detail are in the manifest file, or in the reply with reply \"full\".", json!({
            "variants": {"type": ["array", "string"], "minItems": 1, "maxItems": 64, "items": {"type": "object"}, "description": "1 to 64 sets: {name?, params}, or a JSON file of them. name is a file stem of letters, digits, _, -, . (default variant_1, ...); params maps parameter names to values, as set_params takes them."},
            "dir": str_prop("Folder for the .vox, .glb, and manifest files; created."),
            "manifest": str_prop("Manifest path. Defaults to dir/variants.json. Its folder is created."),
            "replace": {"type": "boolean", "description": "Write a fresh manifest with only these sets instead of merging into the existing one by name. Defaults to false."},
            "vox": {"type": "boolean", "description": "Write a .vox per set too. Defaults to true."},
            "reply": {"type": "string", "enum": ["compact", "full"], "description": "compact (the default): per set exported now its files, bounds, voxels, quads, and markers counted by kind. full: the whole manifest, every entry with its parameters and markers. The manifest file is complete either way."},
            "timeoutSeconds": num_prop("Script deadline per object and set, up to 3600. Defaults to 300."),
            "merge": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "colorAtlas": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "bevel": hard(num_prop("As export_mesh, 0..=0.4 cells. Defaults to 0, off."), 0.0, 0.4),
            "bevelMode": {"type": "string", "enum": ["cubes", "edges"], "description": "As export_mesh. Defaults to cubes."},
            "aoStrength": num_prop("As export_mesh, 0..1. Defaults to 0, off."),
            "aoRadius": int_prop("As export_mesh, 0..8. Defaults to 3."),
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "As export_mesh. Defaults to auto."},
            "bake": {"type": ["object", "boolean"], "description": "As export_mesh. Defaults to off."},
            "gameView": {"type": ["object", "boolean"], "description": "As export_mesh: leave out the faces a fixed game camera never sees. Defaults to off."},
            "exportHidden": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "splitObjects": {"type": "boolean", "description": "As export_mesh. Defaults to false."}
        }), &["variants", "dir"]),
        define("export_models", "Write 1 to 16 .glb files and report each one's quads, triangles, and structure (nodes; meshes with vertices, indices, and bounds; skins; animations), warning on an empty mesh, a skin with no joints, a nameless or duplicated node, or an animation with no channels. source opens that .vox in its own session, leaving the open document alone; omit it to export the open document. A model's failure is reported on that model and skips no others. Markers are counted by kind; reply: \"full\" lists each one.", json!({
            "models": {"type": "array", "minItems": 1, "maxItems": 16, "description": "Each item is {path, source?, merge?, vertexColors?, exportHidden?, splitObjects?, colorAtlas?, bevel?, bevelMode?, aoStrength?, aoRadius?, aoTarget?, bake?, gameView?}, with the export_mesh meanings. path ends in .glb. source, when set, is a .vox file.", "items": {"type": "object"}},
            "reply": reply_prop()
        }), &["models"]),
        bake_scene_tool(),
    ]
}

/// The `gameView` option of `export_mesh`: faces a fixed game camera never sees.
fn game_view_prop() -> Value {
    json!({"type": ["object", "boolean"], "description": "Drop faces a fixed game camera never sees, for fewer triangles and a smaller atlas, unchanged from that camera: {yaw, pitch, spread?, occlusion?, solid?}. yaw, pitch: degrees, the camera's direction in the asset's own frame as in preview, minus the level's turn of the asset (yaw -135 over an asset turned 45 degrees is -180 here). spread (degrees, a number or [about up, up or down], 0..80, defaults to [25, 14]) widens it to all directions the lens and pan see. A face stays if one of them sees its front (back faces and undersides go) and, with occlusion (defaults to true), a ray from it toward the camera along one escapes without meeting a solid cell or box. solid: up to 256 inclusive boxes [x0, y0, z0, x1, y1, z1] of document cells that also hide, such as a neighbour piece; not written. aoStrength and the bake still read every cell, so kept faces shade as before. Defaults to off: every exposed face."})
}

/// The `reply` option of `export_mesh` and `export_models`.
fn reply_prop() -> Value {
    json!({"type": "string", "enum": ["compact", "full"], "description": "compact (the default): markers counted by kind, {\"box\": 22}. full: every marker's local, world, and glTF place, as list_markers gives them."})
}

/// MCP `bake_scene`. Its own function keeps `json!` within the recursion limit.
fn bake_scene_tool() -> Value {
    define("bake_scene", "Bake one placement of many .vox assets together, each shaded by its neighbours and the ground, into one merged .glb with one colour atlas (colorAtlas faces, one texel per cell face, KHR_materials_unlit, drawn unshaded). The open document is not touched. A placement is in the frame of export_mesh's glb, as a game places it: world = translation + Lean x scale x Ry(yaw) x (x, z, -y) of a cell corner, Y up, so a level's translation, rotation.y in degrees, and uniform scale copy across. lean tilts it about its base, its top toward the yaw toward (0 is -Z, 90 is -X); without it, upright. sway multiplies its moving slots' sway amount (TEXCOORD_1 x), so plant kinds sway apart in one mesh; bob is kept. Any yaw or lean works; repeats of one .vox share a grid. Occluders stop light and are not written. Costs: triangles are the sum of each placement's colorAtlas triangles (no merging across placements); the atlas has one RGBA texel per exposed cell face of every placement; rays read one bit grid per distinct source; occluder triangles take about 112 bytes each. Replies with triangles, vertices, atlas, texels, bytes, memory (bytes), bake, and structure.", json!({
        "scene": {"type": ["object", "string"], "description": "The scene, or a JSON file of it (relative paths inside start at that file's folder; for an object, at the server's working directory): {instances: [{source: a .vox path, translation?: [x, y, z], yaw?: degrees about +Y, scale?: uniform, lean?: {degrees: tilt -90..90, toward: yaw the top moves toward}, sway?: its slots' sway multiplier, 0..4, default 1}], occluders?: [{mesh: a .glb, .gltf or .obj path, translation?, yaw?, scale?, lean?} | {plane: height, solid below it} | {height: {origin?: [x, z], cell?: spacing, rows: [[y at x...] per z...]}}], bake?, exportHidden?}. 1 to 32768 instances."},
        "path": str_prop(".glb file to write. Its folder is created."),
        "bake": {"type": ["object", "boolean"], "description": "The export_mesh bake option, over the scene's own; distance is in scene units here. Defaults to occlusion alone."}
    }), &["scene", "path"])
}

fn strand_props() -> Value {
    json!({
        "object": {"type": ["integer", "string"], "description": "Object index or name the strand belongs to. Defaults to the active object."},
        "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Path points in the object's local cells, 2..=256: [x, y, z], [x, y, z, twist], [x, y, z, twist, scale], or {x, y, z, twist?, scale?}; whole numbers are cell centres. twist: degrees counter-clockwise looking along the path (default 0); scale multiplies the section, 0.05..=16 (default 1), so a leaf tapers to its tip."},
        "curve": toggle(ui(with_default(json!({"type": "string", "enum": ["smooth", "polyline"], "description": "smooth (Catmull-Rom through the points) or polyline (straight spans). Defaults to smooth."}), json!("smooth")), "Straight", "Straight spans between the points instead of a smooth curve"), "polyline"),
        "width": soft(ui(hard(with_default(num_prop("Section across the path, to its left, in cells, 0.125..=64. Defaults to 1."), json!(1.0)), 0.125, 64.0), "Width", "Section across the path, in cells"), 0.125, 8.0, 3),
        "thickness": soft(ui(hard(with_default(num_prop("Section up from the path, in cells, 0.125..=64. Defaults to 1."), json!(1.0)), 0.125, 64.0), "Thickness", "Section up from the path, in cells"), 0.125, 8.0, 3),
        "step": soft(ui(hard(with_default(num_prop("Cube length along the path in cells, 0.125..=64; the cube count is the whole number nearest length / step. Defaults to 1."), json!(1.0)), 0.125, 64.0), "Step", "Cube length along the path, in cells"), 0.125, 8.0, 3),
        "palette": hard(int_prop("Palette index 1..=255 of every cube. Defaults to the active slot."), 1.0, 255.0),
        "palettes": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices 1..=255 (up to 64) laid out by pattern"},
        "pattern": ui(json!({"type": "string", "enum": ["gradient", "rings", "random"], "description": "gradient (default): even runs from the first point to the last (a leaf dark at the base, pale at the tip); rings: in turn every period cells; random: per cube by seed"}), "Pattern", "How the slots lay out along the strand"),
        "period": soft(ui(hard(with_default(num_prop("rings: cells of path per slot, 0.25..=512. Defaults to 2."), json!(2.0)), 0.25, 512.0), "Period", "Cells of path per slot of the rings"), 0.25, 32.0, 2),
        "seed": int_prop("random: the seed. Defaults to 0.")
    })
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
    json!({"type": "array", "items": {"type": ["integer", "array"]}, "description": "Boxes left alone: one inclusive [x0, y0, z0, x1, y1, z1] or up to 64, such as a ledge top, cave floor, or river bed (solid top layer, air above, or both). No cell inside changes and nothing grows from or lands in them, so walking surfaces need no refill."})
}

/// The arguments `moss`, `algae`, and `vines` share, plus `extra`.
fn growth_props(extra: &[(&str, Value)], amount: &str, faces: &str) -> Value {
    let mut p = json!({});
    for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
        p[key] = int_prop("Inclusive box corner");
    }
    p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Plant palette indices, or one integer."});
    p["amount"] = num_prop(amount);
    p["noise"] = num_prop("0 grows in smooth patches, 1 in ragged speckle. Defaults to 0.5.");
    p["faces"] = json!({"type": ["string", "array"], "description": format!("top, side, bottom, all, or a list. Defaults to {faces}.")});
    p["facing"] = vec3_prop("Only faces whose outward normal is within 60 degrees of this, such as [0, -1, 0] (front) or [0, 0, -1] (undersides). Without faces it considers every face (vines: side); a facing no face in faces points along is an error.");
    p["band"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Only cells with z in [low, high], such as a waterline. algae thins from its bottom to its top, and without band uses the box's height."});
    p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Grow only on these slots (the stone). Omit for all."});
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
    json!({"type": "string", "enum": ["replace", "empty", "paint", "intersect"], "description": "replace (default) writes every cell of the shape; empty only empty cells, keeping detail; paint recolours solid cells only; intersect keeps the solid cells inside the shape as they are and erases the rest (a boxy hull cut to a dome), so palette is unused."})
}

fn mode_prop() -> Value {
    json!({"type": "string", "enum": ["replace", "empty", "paint"], "description": "replace (default) writes every cell of the shape; empty only empty cells, keeping detail; paint recolours solid cells only."})
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
        "description": "Named view instead of yaw and pitch, model facing -Y, +Z up: front yaw -90, back 90, left 0, right 180 (the model's own sides), top pitch 90, bottom pitch -90."
    })
}

fn target_prop() -> Value {
    json!({
        "anyOf": [
            {"type": "string", "enum": ["all", "active"]},
            {"type": "integer"},
            {"type": "object", "properties": {"min": {"type": "array", "items": {"type": "integer"}, "minItems": 3, "maxItems": 3}, "max": {"type": "array", "items": {"type": "integer"}, "minItems": 3, "maxItems": 3}}, "required": ["min", "max"], "additionalProperties": false}
        ],
        "description": "What the camera fits: all (default: every visible posed cell), active, an object index, or {min, max} (inclusive world cells). Other objects still draw."
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
        "excludeColors": {"type": "array", "items": {"type": ["string", "array"]}, "description": format!("Colours (#rrggbb or [r, g, b]) that are background too, such as water and splashes{suffix}.")},
        "excludeTolerance": num_prop(&format!("RGB distance from an excludeColors colour that still goes, 0..442. Defaults to 40{suffix}.")),
        "flood": {"type": ["boolean", "number", "object"], "description": format!("Drop the background a flood fill from the crop's border reaches, stepping to neighbours within this RGB distance: a photo or gradient. true is 24; a number or {{tolerance}} sets it{suffix}.")},
        "keepLargest": {"type": "boolean", "description": format!("Keep only the largest connected inked region (drops specks and a separate splash){suffix}.")},
        "fillHoles": {"type": ["boolean", "number"], "description": format!("Re-ink, in the image's colour, enclosed background regions (not touching the crop's border) of at most this share of the inked pixels: white eyes and teeth on a white sheet, which the background rule keys out. true is 0.1. Needs a path{suffix}.")}
    });
    if !sheet {
        props["mask"] = json!({"type": ["string", "object"], "description": "PNG path or {path, crop, background, tolerance, alphaThreshold, invert}, keyed as a view (auto: transparent pixels, else the border colour, so white on black works) and stretched over the crop; only pixels under its ink stay (invert: they go). A mask the image's size shares its crop."});
        props["erase"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "description": "Rectangles [x, y, width, height] in image pixels to drop: a rider, a label."});
    }
    props
}

/// The export option `bake`. Its own function keeps `json!` within the
/// recursion limit, like [`lighting_prop`].
fn bake_prop() -> Value {
    json!({"type": ["object", "boolean"], "description": "Paint light into the colour atlas by real rays (one texel per cell face, drawn unshaded); needs colorAtlas. The material gets KHR_materials_unlit and extras.froxelBake. texel = albedo x (ambient x ambientEnergy x (1 - occlusion x blocked) + sun x sunEnergy x max(n.l, 0) x open), linear, clamped, sRGB-encoded; blocked: the share of cosine-weighted rays hitting an opaque cell within distance (a hit at t counts 1 - (t/distance)^2); open: the share of sun rays that escape. Optional fields: occlusion 0..1 (default 1); distance in cells, up to 4096 (default 16); rays per face 1..1024 (default 64); preset (a lighting preset such as old_pond) or sunDirection [x, y, z] toward the sun, Z up, or godotSunBasis; sunColor, ambientColor ([r, g, b] 0..16 sRGB, or #rrggbb); sunEnergy; ambientEnergy; lightingPresets (as preview); sunSoftness, the sun disc's half angle 0..45 degrees (default 0, hard); sunRays 1..256 (default 1 hard, 16 soft). No preset, sun, or ambient field: occlusion alone under white ambient 1, no sun; named lighting starts at sun 1, ambient 0.3. A preset's exposure, background and lut are not baked (a game's tonemapper and grade reach unshaded surfaces too). Deterministic: same input, same bytes. Triangle counts are unchanged. true: every default; absent or false: off, the file byte for byte unchanged."})
}

/// MCP `preview` `lighting`. Its own function keeps `json!` within the
/// macro recursion limit.
fn lighting_prop() -> Value {
    json!({"type": ["string", "object"], "description": "A game's light instead of the viewport's: a preset name (viewport, the default; old_pond, frog-world's Old Pond; one from lightingPresets or the user's lighting_presets.json), or fields: sunDirection [x, y, z] toward the sun, Z up, or godotSunBasis (the 9 or 12 numbers of a DirectionalLight3D's Transform3D in a .tscn); sunColor, ambientColor ([r, g, b] 0..16 sRGB, or #rrggbb); sunEnergy, ambientEnergy; exposure; background; lut (a 3D LUT PNG, N*N by N, applied last as Godot's colour correction); preset (the base for these fields). Shading: albedo x (sun x energy x max(n.l, 0) + ambient x energy) x exposure, linear, clamped, no shadows. Replies lighting with the values used."})
}

/// MCP `preview`, `compare_silhouette`, and `set_view` `presets`.
fn camera_presets_prop() -> Value {
    json!({"type": ["object", "string"], "description": "Camera presets: name to {yaw, pitch, orthographic, fov, margin}, or a JSON file of them (or of them under presets). godotLook [x, y, z], a Godot camera's look direction, replaces yaw and pitch. An existing name (game, default, or one in the user's camera_presets.json or FROXEL_CAMERA_PRESETS's file) is overridden field by field; a new name is a preset over default."})
}

/// MCP `preview`, `compare_silhouette`, and `set_view` `placementYaw`.
fn placement_yaw_prop() -> Value {
    num_prop("Degrees the asset is turned about up where a level places it, counter-clockwise from above (a Godot node's rotation_degrees.y). The camera and a lighting sun turn the other way, showing the asset as it stands in the level from the same camera. The reply's yaw is yaw minus this.")
}

/// MCP `preview` `lightingPresets`.
fn lighting_presets_prop() -> Value {
    json!({"type": ["object", "string"], "description": "Lighting presets for this call: name to lighting fields, or a JSON file of them (also under lighting). Built-in names are overridden field by field; a relative lut path is relative to the file."})
}
