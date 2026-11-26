# Operation properties: one declaration for MCP, Rhai, the CLI, and the panels

A tool option is declared once, in `src/ops/schema.rs`, and every frontend
reads it from there. This note is the design and the migration state.

## The model: Blender's RNA

Blender declares an operator's properties once (`bpy.props.FloatProperty(name=,
description=, default=, min=, max=, soft_min=, soft_max=, precision=)`,
`EnumProperty(items=)`). The UI never repeats that data: `layout.prop(op,
"name")` picks the widget from the property type (bool → checkbox, enum →
buttons or menu, number → slider), labels it with `name`, gives it
`description` as the tooltip, clamps to `min`/`max`, and drags inside
`soft_min`/`soft_max`. The redo panel is that loop run over every property.
Python's `bpy.ops.x(...)` keywords are the same properties. A panel decides
only *which* properties go *where*.

Froxel already had half of this: `OPS` (`src/ops/mod.rs`) and the schema feed
MCP `tools/list`, `control.methods`, `froxel run` and its `--help`, the
argument check at the top of `remote::apply`, and the defaults (`with_default`,
`ops::default_*`). Missing were the Rhai option lists (hand-written consts,
only checked to be a subset of the schema) and the panels (hand-written labels,
ranges, and tooltips). Adding `caps` to `fill_tube` took edits in the schema,
two Rhai lists, the session, the panel, and the docs.

## What a property carries

A schema property is JSON Schema plus a Froxel `x-ui` object, written with
builders next to the property:

| Data | Where | Blender | Who reads it |
|---|---|---|---|
| type, `enum` | JSON Schema | property type, `items` | MCP, argument check, widget choice |
| `description` | JSON Schema | `description` | MCP, `froxel run --help` |
| `default` | `with_default` | `default` | `remote::apply`, Rhai, panel start values |
| `minimum`, `maximum` | `hard(prop, lo, hi)` | `min`, `max` | MCP clients, the tests (refused just outside) |
| `title` | `ui(prop, label, hint)` | `name` | panel label, MCP (standard JSON Schema) |
| `x-ui.hint` | `ui(...)` | short `description` | panel tooltip, `froxel run <tool> --help` short line |
| `x-ui.soft`, `x-ui.digits` | `soft(prop, lo, hi, digits)` | `soft_min`, `soft_max`, `precision` | slider track and shown digits |
| `x-ui.on` | `toggle(prop, on)` | a two-item enum drawn as a checkbox | toggle widget: on is that item, off leaves it out |

`x-ui` is stripped from the MCP `tools/list` (`schema::mcp_tools`), so the MCP
reply does not grow; `title` and the hard range stay, since they are standard.
The hint is the short help; `description` stays the long one MCP needs.

`ops::prop_ui(tool, key)` reads it all into a `PropUi` (label, hint, soft
range, digits, `on`, integer or not, enum items) and panics on a property with
no `x-ui`, as `ops::default` does on a missing default, so a panel cannot draw
an undeclared option.

## Rhai

`ops::rhai_options(tool, positional)` is the option list of a Rhai function
that maps to an MCP tool: every schema property except the ones the function
takes positionally. A new schema property is a new Rhai option with no edit in
`src/script`. Where a Rhai-only function shares part of a tool (`carve_tube`
is `carve` `shape: "tube"`), the shared keys are one const in the schema
(`schema::TUBE_SHAPE_KEYS`) that builds both tools' properties and the Rhai
list. `tests/ops.rs` keeps checking the remaining hand-written lists.

Positional bindings are not derived yet. Most `register_fn` closures build a
map from a few positional values plus the options map, but the positional
part is irregular (`fill_tube` takes `palette` or `palettes` by the value's
type; the fills take six corners then a slot). A later step can declare the
positional keys once next to the `op(...)` row and generate the two
`register_fn` overloads (with and without the options map) from them; the
irregular ones keep a hand closure.

## Panels

A tool's panel options are that tool's MCP arguments: `Session::op_args`
(`src/state/op_args.rs`) maps a tool name to an argument map, as a Blender
operator keeps its properties. A key left out takes its schema default
(`Session::op_arg`). When the tool runs, `Session::op_call` adds the values
the drag supplies (points, palette), then runs the same `ops::check_arguments`
and `ops::fill_defaults` as `remote::apply`, and the result goes to the same
parser as MCP (`remote::tube_writes` for the Tube tool). The panel cannot send
an option MCP would refuse, and a new schema option needs no new session
field.

`src/panels/props.rs` draws widgets over that map:

- `op_toggle(ui, session, tool, key)`: a toggle labelled `title`, with the
  hint and `(MCP tool key on)` as its tooltip. For a boolean it sets
  `true`; for an enum with `x-ui.on` it sets that item. Off leaves the key
  out.
- `op_choice(ui, session, tool, key, none)`: one button per enum item, plus
  an optional first button that leaves the key out (the Tube tool's Solid).
- `op_slider(ui, session, tool, key, target)` and `op_slider_as(..., label)`:
  a slider over the soft range with the schema's digits, integer for an
  integer property, hint on the label. `SliderTarget::op_arg` names the
  argument a slider edits, so `ui_inspect` and `ui_click` drive it as before,
  and `write_slider` keeps the value inside the soft range.
  `op_slider_as` relabels a property a tool shows twice (the Tube tool's
  Start and End radius, which become each point's radius, so they stay
  session fields).

The helpers go through `row`, so `tests/panels.rs` holds.

## Migrated

- `fill_tube` (and `carve` `tube`): the Tube tool's Flatten, Roll, Blade,
  Flat ends, Smooth, the colour buttons, Period, and Sectors are
  `op_args["fill_tube"]`, drawn from the schema, and a drag runs them
  through the MCP argument check and `remote::tube_writes`. Start, End,
  Chain, Finish, and Second slot are the drag's own (point radii, the chain,
  the second of `palettes`) and stay session fields. Rhai `fill_tube` and
  `carve_tube` options are derived.

## Still to migrate

Rhai option lists (`const ...: &[&str]` in `src/script`) to `rhai_options`,
and panels to `op_toggle`/`op_slider`, tool by tool:

- `src/script/shapes.rs`: `FILL`/`CARVE` (fill_box, fill_ellipsoid,
  fill_rounded_box, carve_*), `TERRACE` (deterrace), `REDUCE`
  (reduce_palette), `STROKE` (surface_stroke), `PLATES` (shell_plates),
  `RECOLOR`, `SMOOTH`, `BREAK` (break_end), `SCATTER`.
- `src/script/organic.rs`: loft, deform, brush, blend_shapes, sweep, foliage.
- `src/script/stone.rs`: column_relief, weather, moss, algae, vines, bark,
  planks, cavity_tint, radial_relief, trim_outline, joints.
- `src/script/silhouette.rs`: carve_from_views, paint_from_views,
  project_paint, decal, orient/fit/optimize_to_views, compare_silhouette.
- `src/script/markers.rs`, `src/script/import.rs`: set_marker,
  collision_from_voxels, import_mesh.
- Panels: Smooth (`SliderTarget::Smooth*`), Sculpt brush
  (`panels/tools.rs`, plain `egui::Slider`s with their own hints), Foliage,
  Organic (loft/deform/blend), Stone, Silhouette (views tools), Import,
  Export and Bake (`panels/render.rs`, `SliderTarget::Export*`/`Bake*`).
- Each migration moves the panel's session fields into `op_args[tool]`,
  maps its sliders in `SliderTarget::op_arg`, and runs the tool through
  `Session::op_call`.
- Then: generate the positional Rhai bindings.
