//! The MCP organic shaping tools in Rhai: `deform`, `loft`, `brush`, and
//! `blend_shapes`.
//!
//! Each call turns its arguments into the MCP tool's arguments and runs
//! `remote::organic_writes` on the object being written, as the MCP handler
//! does on the active object. Options use the MCP names in a trailing map.

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Dynamic, Engine, Map};

use super::bind::{bind, Binding, Pos::*};
use super::shapes::Args;
use super::stone::reply;
use super::Scratch;
use crate::remote::{self, Organic};

/// The organic tools' bindings ([`super::bind`]): `deform` over the whole
/// object, or a box given as corners or in the options map (`min`, `max`).
pub(super) const BINDINGS: &[Binding] = &[
    Binding::new("deform", "deform", &[Str("op")]),
    Binding::new("deform", "deform", &[Corners, Str("op")]),
    Binding::new(
        "loft",
        "loft",
        &[List("points"), Slots("palette", "palettes")],
    ),
    Binding::new("blend_shapes", "blend_shapes", &[List("shapes")]),
    Binding::new("brush", "brush", &[Str("op"), List("points")]),
];

pub(super) fn register(engine: &mut Engine, scratch: &Rc<RefCell<Scratch>>) {
    for binding in BINDINGS {
        let state = scratch.clone();
        let name = binding.name;
        bind(engine, *binding, move |args| {
            let mut state = state.borrow_mut();
            match name {
                "deform" => state.organic(args, Organic::Deform),
                "loft" => state.organic(args, Organic::Loft),
                "blend_shapes" => state.blend(args),
                "brush" => state.organic(args, Organic::Brush),
                other => unreachable!("no runner for `{other}`"),
            }
            .map(Dynamic::from_map)
        });
    }
}

impl Scratch {
    /// Run MCP `blend_shapes` on the object being written.
    fn blend(&mut self, args: Args) -> Result<Map, String> {
        self.keep_going()?;
        let (args, _) = self.localize("voxel.blend_shapes", args)?;
        let (writes, counts) = remote::blend_shape_writes(self.cells(), &args)?;
        let changed = self.apply(&writes)?;
        Ok(reply(counts, changed))
    }

    /// Run an organic tool on the object being written. The reply holds
    /// `changed` and the tool's own counts.
    fn organic(&mut self, args: Args, tool: Organic) -> Result<Map, String> {
        self.keep_going()?;
        let method = match tool {
            Organic::Deform => "voxel.deform",
            Organic::Loft => "voxel.loft",
            Organic::Brush => "voxel.brush",
        };
        let (args, _) = self.localize(method, args)?;
        let (writes, counts) = remote::organic_writes(self.cells(), &args, tool)?;
        let changed = self.apply(&writes)?;
        Ok(reply(counts, changed))
    }
}
