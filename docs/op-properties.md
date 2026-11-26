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

Every Rhai function that runs an MCP tool takes its options map through
`tool_options(opts, tool, excluded)` (`src/script/shapes.rs`), which allows
every schema argument of `tool` but `excluded` (`ops::rhai_options`). A new
schema argument is a new Rhai option with no edit in `src/script`. `excluded`
is what the function takes positionally (corners, points, the slot) plus the
few a script does not take: `collision_from_voxels` `object` (a script edits
its own object), `vines` `layer` (vines always grow), and the views tools'
single-image keys (`path`, `background`, `crop`, ...) and `png`/`pngScale`
(scripts never write files). `tool_options_as` serves a function named
otherwise (`carve_tube`, `carve_box`, and `voxelize` run `carve` and
`import_mesh`). `tests/ops.rs` checks every excluded key is an argument of its
tool and that no `src/script` file calls `options` with a hand-written list.

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

## Panel structs

The other sections (Foliage, Stone, Organic, Silhouette, Import, the edit
sections, Break, Scatter, Smooth, and the Sculpt brush) already ran their tool
through `remote::apply`: a typed panel struct (`FoliagePanel`, ...) builds the
arguments in `args()`, and the MCP handler checks and runs them. Their storage
stays typed; what moved is the metadata. `src/ops/panel.rs` is a table of rows
`(tool, key, label, hint, soft range, digits, speed)` that
`schema::tools` merges into the properties as `ui` and `soft` would inline,
and the panel draws:

- `prop_label(ui, tool, key)`: a grid row's label with the hint as tooltip;
- `prop_slider(ui, tool, key, &mut value)` and `prop_drag(...)`: over the soft
  range with the schema's decimals and drag speed, for any numeric field;
- `prop_check(ui, tool, key, &mut bool)`: a checkbox labelled by the argument.

The rows were generated from the panels as they were, so labels, tooltips,
and ranges are the same (tooltips now end with the MCP argument). A panel may
start a soft range at 0 below a positive minimum where 0 leaves the argument
out (Markers' max boxes). Rows whose field is not one argument (a parsed slot
list, an enum index, an `_on` flag for an optional value, a pair of numbers
for one argument) keep their hand-written label.

## Migrated

- `fill_tube` (and `carve` `tube`): the Tube tool's Flatten, Roll, Blade,
  Flat ends, Smooth, the colour buttons, Period, and Sectors are
  `op_args["fill_tube"]`, drawn from the schema, and a drag runs them
  through the MCP argument check and `remote::tube_writes`. Start, End,
  Chain, Finish, and Second slot are the drag's own (point radii, the chain,
  the second of `palettes`) and stay session fields. Rhai `fill_tube` and
  `carve_tube` options are derived.

## Still hand-written

Rhai option lists and the panel metadata of every tool panel are derived.
What is left:

- Panel rows that are not one argument (see Panel structs): parsed slot
  lists, enum segments over panel indices, optional values behind an `_on`
  flag, pairs. `op_choice` can draw an enum once its panel stores the item.
- Rows whose label or tooltip differ between two panels for the same argument
  (Growth and Moss `thickness`, the two Deform `angle` rows): the first panel's
  wording is in the table, the second stays hand-written.
- The Render, Export, and Bake sliders (`panels/render.rs`), which set render
  and export settings, not tool arguments, and the generator form.
- Positional Rhai bindings, written by hand around `tool_options`.
