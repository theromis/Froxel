//! Rhai functions declared from their MCP tool: a [`Binding`] names the
//! function, the tool whose schema gives its options, and its positional
//! arguments, and [`bind`] registers the overloads (with and without the
//! trailing options map) and builds the tool's arguments from the call. The
//! options are every schema argument but the positional ones
//! ([`super::shapes::tool_options_as`]), so a new schema argument is a new
//! Rhai option and a binding never lists keys. `docs/op-properties.md` is the
//! design.

use std::any::TypeId;

use rhai::{Array, Dynamic, Engine, ImmutableString, Map, NativeCallContext, INT};
use serde_json::Value;

use super::runtime;
use super::shapes::{box_args, json, tool_options_as, Args, Fallible};

/// One positional argument of a binding, in call order.
#[derive(Clone, Copy, Debug)]
pub(super) enum Pos {
    /// The six corners `x0, y0, z0, x1, y1, z1`, each in the coordinate range.
    Corners,
    /// A whole number.
    Int(&'static str),
    /// A number, whole or fractional.
    Number(&'static str),
    /// A string.
    Str(&'static str),
    /// A list, as JSON.
    List(&'static str),
    /// Any value, as JSON.
    Any(&'static str),
    /// A slot (`.0`) or a list of slots (`.1`), by the value's type.
    Slots(&'static str, &'static str),
}

impl Pos {
    fn keys(self) -> Vec<&'static str> {
        match self {
            Pos::Corners => vec!["x0", "y0", "z0", "x1", "y1", "z1"],
            Pos::Int(key) | Pos::Number(key) | Pos::Str(key) | Pos::List(key) | Pos::Any(key) => {
                vec![key]
            }
            Pos::Slots(one, many) => vec![one, many],
        }
    }

    fn types(self) -> Vec<TypeId> {
        match self {
            Pos::Corners => vec![TypeId::of::<INT>(); 6],
            Pos::Int(_) => vec![TypeId::of::<INT>()],
            Pos::List(_) => vec![TypeId::of::<Array>()],
            Pos::Str(_) => vec![TypeId::of::<ImmutableString>()],
            Pos::Number(_) | Pos::Any(_) | Pos::Slots(..) => vec![TypeId::of::<Dynamic>()],
        }
    }
}

/// Which overloads a binding registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Forms {
    /// With and without the trailing options map.
    Both,
    /// Only with it (`fill_box`, whose mode is usually given).
    WithOptions,
    /// Only without it: every argument is positional (`add_clip`).
    WithoutOptions,
}

/// A Rhai function that runs MCP `tool`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Binding {
    /// The Rhai name: the tool's, or its own (`carve_tube` runs `carve`).
    pub name: &'static str,
    pub tool: &'static str,
    pub positional: &'static [Pos],
    /// Arguments the function always sets (`shape: "tube"`).
    pub fixed: &'static [(&'static str, &'static str)],
    /// Schema arguments it takes neither positionally nor as options.
    pub excluded: &'static [&'static str],
    pub forms: Forms,
}

impl Binding {
    pub(super) const fn new(
        name: &'static str,
        tool: &'static str,
        positional: &'static [Pos],
    ) -> Self {
        Self {
            name,
            tool,
            positional,
            fixed: &[],
            excluded: &[],
            forms: Forms::Both,
        }
    }

    pub(super) const fn fixed(mut self, fixed: &'static [(&'static str, &'static str)]) -> Self {
        self.fixed = fixed;
        self
    }

    pub(super) const fn excluded(mut self, excluded: &'static [&'static str]) -> Self {
        self.excluded = excluded;
        self
    }

    pub(super) const fn with_options_only(mut self) -> Self {
        self.forms = Forms::WithOptions;
        self
    }

    pub(super) const fn without_options(mut self) -> Self {
        self.forms = Forms::WithoutOptions;
        self
    }

    /// Every schema argument the options map may not hold.
    pub(super) fn not_options(&self) -> Vec<&'static str> {
        let mut keys: Vec<&'static str> = self
            .positional
            .iter()
            .flat_map(|pos| pos.keys())
            .chain(self.fixed.iter().map(|(key, _)| *key))
            .chain(self.excluded.iter().copied())
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// The tool's arguments from one call's values (`values`, in positional
    /// order) and options.
    fn args(&self, values: &mut [&mut Dynamic], opts: &Map) -> Fallible<Args> {
        let mut args = tool_options_as(opts, self.tool, &self.not_options(), self.name)?;
        let mut at = 0;
        for pos in self.positional {
            match *pos {
                Pos::Corners => {
                    let mut corners = [0; 6];
                    for (slot, value) in corners.iter_mut().zip(values[at..at + 6].iter()) {
                        *slot = value
                            .as_int()
                            .map_err(|_| runtime("corners are integers"))?;
                    }
                    args.extend(box_args(corners)?);
                    at += 6;
                }
                Pos::Int(key) => {
                    let value = values[at]
                        .as_int()
                        .map_err(|_| runtime(format!("{key} must be an integer")))?;
                    args.insert(key.into(), value.into());
                    at += 1;
                }
                Pos::Number(key) => {
                    let value = &*values[at];
                    if !(value.is_int() || value.is_float()) {
                        return Err(runtime(format!("{key} must be a number")));
                    }
                    args.insert(key.into(), json(value)?);
                    at += 1;
                }
                Pos::Str(key) => {
                    let value = values[at]
                        .clone()
                        .into_string()
                        .map_err(|_| runtime(format!("{key} must be a string")))?;
                    args.insert(key.into(), value.into());
                    at += 1;
                }
                Pos::List(key) | Pos::Any(key) => {
                    args.insert(key.into(), json(&*values[at])?);
                    at += 1;
                }
                Pos::Slots(one, many) => {
                    let value = &*values[at];
                    let key = if value.is_array() {
                        many
                    } else if value.is_int() {
                        one
                    } else {
                        return Err(runtime(format!(
                            "{} {one} must be an index or a list of indices",
                            self.name
                        )));
                    };
                    args.insert(key.into(), json(value)?);
                    at += 1;
                }
            }
        }
        for (key, value) in self.fixed {
            args.insert((*key).into(), Value::String((*value).into()));
        }
        Ok(args)
    }
}

/// Register `binding`'s overloads; `run` takes the tool's arguments and
/// returns the function's result.
pub(super) fn bind(
    engine: &mut Engine,
    binding: Binding,
    run: impl Fn(Args) -> Result<Dynamic, String> + Clone + 'static,
) {
    bind_in_context(engine, binding, move |_, args| run(args).map_err(runtime));
}

/// [`bind`] for a function that needs the call's context, such as the
/// document operations a run records (`add_bone`, `place_joints`).
pub(super) fn bind_in_context(
    engine: &mut Engine,
    binding: Binding,
    run: impl Fn(&NativeCallContext, Args) -> Fallible<Dynamic> + Clone + 'static,
) {
    let types: Vec<TypeId> = binding
        .positional
        .iter()
        .flat_map(|pos| pos.types())
        .collect();
    let forms: &[bool] = match binding.forms {
        Forms::Both => &[false, true],
        Forms::WithOptions => &[true],
        Forms::WithoutOptions => &[false],
    };
    for &with_options in forms {
        let mut types = types.clone();
        if with_options {
            types.push(TypeId::of::<Map>());
        }
        let run = run.clone();
        engine.register_raw_fn(
            binding.name,
            types,
            move |ctx: NativeCallContext, values: &mut [&mut Dynamic]| -> Fallible<Dynamic> {
                let (values, opts) = if with_options {
                    let (last, values) = values.split_last_mut().expect("the options map");
                    (values, last.take().cast::<Map>())
                } else {
                    (&mut values[..], Map::new())
                };
                let args = binding.args(values, &opts)?;
                run(&ctx, args)
            },
        );
    }
}
