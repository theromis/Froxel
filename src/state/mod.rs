//! Editor session.
//!
//! Geometry, palette, and panel values live here. Brush strokes edit this
//! in-memory volume.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use glam::Vec3;

use crate::rig::{Clip, Skeleton};

pub type Cell = (i32, i32, i32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Brush,
    Line,
    Circle,
    Square,
    Box,
    Face,
    Generator,
    Mutator,
    Marquee,
    BoxSelect,
    FloodSelect,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Brush => "Brush",
            Tool::Line => "Line",
            Tool::Circle => "Circle",
            Tool::Square => "Square",
            Tool::Box => "Box",
            Tool::Face => "Face",
            Tool::Generator => "Generator",
            Tool::Mutator => "Mutator",
            Tool::Marquee => "Marquee",
            Tool::BoxSelect => "Box Select",
            Tool::FloodSelect => "Flood Select",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    Add,
    Remove,
    Paint,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BrushShape {
    Square,
    Circle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LineMode {
    Straight,
    Project,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FaceExtrude {
    Color,
    Geometry,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FaceWith {
    Geometry,
    Palette,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GeneratorKind {
    Ellipsoid,
    RoundedBox,
    Cylinder,
    Cone,
    Pyramid,
    Torus,
    Perlin,
    Tree,
    Helix,
    Conifer,
}

impl GeneratorKind {
    pub fn name(self) -> &'static str {
        match self {
            GeneratorKind::Ellipsoid => "Ellipsoid",
            GeneratorKind::RoundedBox => "Rounded box",
            GeneratorKind::Cylinder => "Cylinder",
            GeneratorKind::Cone => "Cone",
            GeneratorKind::Pyramid => "Pyramid",
            GeneratorKind::Torus => "Torus",
            GeneratorKind::Perlin => "Perlin",
            GeneratorKind::Tree => "Tree",
            GeneratorKind::Helix => "Helix",
            GeneratorKind::Conifer => "Conifer",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Model,
    Object,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Edit,
    Render,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GlassMode {
    Probabilistic,
    Absorption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialType {
    Diffuse,
    Metal,
    Emissive,
    Glass,
}

impl MaterialType {
    pub fn name(self) -> &'static str {
        match self {
            MaterialType::Diffuse => "Diffuse",
            MaterialType::Metal => "Metal",
            MaterialType::Emissive => "Emissive",
            MaterialType::Glass => "Glass",
        }
    }
}

/// How a game shader may move vertices of this palette slot.
/// `None` is still. The amount is unused while the kind is `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Motion {
    #[default]
    None,
    Sway,
    Bob,
}

impl Motion {
    pub fn label(self) -> &'static str {
        match self {
            Motion::None => "None",
            Motion::Sway => "Sway",
            Motion::Bob => "Bob",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Motion::None => "none",
            Motion::Sway => "sway",
            Motion::Bob => "bob",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "none" => Some(Motion::None),
            "sway" => Some(Motion::Sway),
            "bob" => Some(Motion::Bob),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub ty: MaterialType,
    pub roughness: f32,
    pub metallic: f32,
    pub emission: f32,
    pub emission_power: i32,
    pub transparency: f32,
    pub ior: f32,
    pub motion: Motion,
    /// Strength of [`Motion`], from 0 to 4. Ignored when `motion` is `None`.
    pub motion_amount: f32,
}

/// One `set_palette` slot. Omitted motion fields leave the slot as it is.
#[derive(Clone, Copy)]
pub struct PaletteChange {
    pub index: u8,
    pub rgba: [u8; 4],
    pub motion: Option<Motion>,
    pub amount: Option<f32>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            ty: MaterialType::Diffuse,
            roughness: 1.0,
            metallic: 0.0,
            emission: 0.0,
            emission_power: 1,
            transparency: 0.0,
            ior: 1.5,
            motion: Motion::None,
            motion_amount: 1.0,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    pub cells: HashMap<Cell, u8>,
    /// World offset of local cell (0, 0, 0). Copies are placed beside the
    /// source so they do not occupy the same voxels.
    pub translation: Cell,
    /// Minimum corner of the SIZE box, in local cells. `dims` of `(0, 0, 0)`
    /// means the box is just the occupied span.
    pub box_min: Cell,
    pub dims: (i32, i32, i32),
    /// Orientation from the file, kept so a save can write `_r` again.
    /// Identity means the cells already are the model that gets saved.
    pub rot: [[i32; 3]; 3],
    /// World position of the model center. Moves with the object's translation.
    pub pivot: Cell,
    /// File SIZE that `rot` was applied to. `(0, 0, 0)` saves an identity `_r`.
    pub model_dims: (i32, i32, i32),
    /// Any `_hidden` on the node or an ancestor. Hidden objects stay in the
    /// file and the outliner, and are left out of the mesh and the tracer.
    pub hidden: bool,
    /// Leaf transform this object was loaded from. `None` for objects the
    /// editor created. Save uses it to write the original graph back.
    pub file: Option<FileLink>,
    /// Bone this object is rigidly bound to. `None` follows the skeleton
    /// root when the document has bones, and means no skin otherwise.
    pub bone: Option<String>,
    /// Procedural generator. Empty source means this object has no script.
    /// Opening a file does not run it.
    pub script: crate::script::ObjectScript,
}

/// Where a loaded object sits in the `.vox` graph.
#[derive(Clone, PartialEq, Eq)]
pub struct FileLink {
    pub trn: i32,
    pub model: i32,
    pub parent_rot: [[i32; 3]; 3],
    pub parent_t: [i32; 3],
    /// Name stored on the node when the file was opened, after the editor's
    /// fallback. Save writes `_name` only when the object name changes.
    pub name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    None,
    Orbit,
    Pan,
    Paint,
    Select,
    Move,
    Object,
    ObjectMarquee,
}

enum ObjectPress {
    Drag,
    Marquee,
    Idle,
}

#[derive(Clone, Copy)]
pub struct Hit {
    pub cell: Cell,
    pub normal: Cell,
}

pub struct ToolDrag {
    pub rays: Vec<(Vec3, Vec3)>,
    pub face_steps: i32,
    /// Occupied cells inside a marquee, computed on release. `None` when
    /// the gesture is not a marquee.
    pub marquee: Option<Vec<Cell>>,
    pub shift: bool,
    /// Objects whose screen box meets the rubber band. `None` outside an
    /// Object-mode drag. `click` is a press that never grew past 2px.
    pub object_band: Option<ObjectBand>,
}

pub struct ObjectBand {
    pub click: bool,
    pub hits: Vec<usize>,
}

struct Stroke {
    anchor: Hit,
    base: HashMap<Cell, u8>,
    seeds: Vec<Cell>,
    axis: i32,
    sign: i32,
    id: u32,
    key: Option<(Cell, i32, u64)>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum UiAction {
    None,
    Tool(Tool),
    Mode(ToolMode),
    Editor(EditorMode),
    Render(RenderMode),
    NewObject,
    Duplicate,
    DeleteObject,
    /// Show or hide one object.
    ObjectHidden(usize),
    /// Mirror the active object across x, y, or z (0, 1, 2).
    Mirror(usize),
    MirrorBones,
    ClearPose,
    AddClip,
    KeyPose,
    RemoveClip,
    ShowBones,
    Export(ExportFlag),
    Viewport,
    Palette(u8),
    Slider(SliderSpec),
}

/// A checkbox in the Export panel.
#[derive(Clone, Copy, PartialEq)]
pub enum ExportFlag {
    Merge,
    VertexColors,
    Hidden,
    Split,
}

/// A slider `ui_click` can set. `min` and `max` match the widget range.
#[derive(Clone, Copy, PartialEq)]
pub struct SliderSpec {
    pub target: SliderTarget,
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SliderTarget {
    BrushSize,
    FoliageSeed,
    FoliageDensity,
    TorusTube,
    RoundRadius,
    NoiseScale,
    NoiseThreshold,
    NoiseOctaves,
    NoiseSeed,
    PerlinLo,
    PerlinHi,
    TreeDepth,
    TreeWildness,
    TreeTrunk,
    TreeLeaf,
    TreeSeed,
    HelixTurns,
    HelixRadius,
    HelixTube,
    ConiferTiers,
    ConiferRagged,
    ConiferClump,
    ConiferTrunk,
    ConiferCrown,
    ConiferNeedle,
    ConiferSeed,
    MaxSamples,
    Aperture,
    FocusDistance,
    MaterialRoughness,
    MaterialMetallic,
    MaterialTransparency,
    MaterialIor,
    MaterialEmission,
    MaterialPower,
    MotionAmount,
    MaxBounces,
    NeeRis,
    IndirectClamp,
    EmaFloor,
    SkyIntensity,
    HorizonSoftness,
    SunElevation,
    SunAzimuth,
    SunDisc,
    SunIntensity,
}

impl SliderTarget {
    pub(crate) fn id(self) -> &'static str {
        match self {
            SliderTarget::BrushSize => "slider.brush-size",
            SliderTarget::FoliageSeed => "slider.foliage-seed",
            SliderTarget::FoliageDensity => "slider.foliage-density",
            SliderTarget::TorusTube => "slider.torus-tube",
            SliderTarget::RoundRadius => "slider.round-radius",
            SliderTarget::NoiseScale => "slider.noise-scale",
            SliderTarget::NoiseThreshold => "slider.noise-threshold",
            SliderTarget::NoiseOctaves => "slider.noise-octaves",
            SliderTarget::NoiseSeed => "slider.noise-seed",
            SliderTarget::PerlinLo => "slider.perlin-lo",
            SliderTarget::PerlinHi => "slider.perlin-hi",
            SliderTarget::TreeDepth => "slider.tree-depth",
            SliderTarget::TreeWildness => "slider.tree-wildness",
            SliderTarget::TreeTrunk => "slider.tree-trunk",
            SliderTarget::TreeLeaf => "slider.tree-leaf",
            SliderTarget::TreeSeed => "slider.tree-seed",
            SliderTarget::HelixTurns => "slider.helix-turns",
            SliderTarget::HelixRadius => "slider.helix-radius",
            SliderTarget::HelixTube => "slider.helix-tube",
            SliderTarget::ConiferTiers => "slider.conifer-tiers",
            SliderTarget::ConiferRagged => "slider.conifer-ragged",
            SliderTarget::ConiferClump => "slider.conifer-clump",
            SliderTarget::ConiferTrunk => "slider.conifer-trunk",
            SliderTarget::ConiferCrown => "slider.conifer-crown",
            SliderTarget::ConiferNeedle => "slider.conifer-needle",
            SliderTarget::ConiferSeed => "slider.conifer-seed",
            SliderTarget::MaxSamples => "slider.samples",
            SliderTarget::Aperture => "slider.aperture",
            SliderTarget::FocusDistance => "slider.focus",
            SliderTarget::MaterialRoughness => "slider.roughness",
            SliderTarget::MaterialMetallic => "slider.metallic",
            SliderTarget::MaterialTransparency => "slider.transparency",
            SliderTarget::MaterialIor => "slider.ior",
            SliderTarget::MaterialEmission => "slider.emission",
            SliderTarget::MaterialPower => "slider.power",
            SliderTarget::MotionAmount => "slider.motion-amount",
            SliderTarget::MaxBounces => "slider.bounces",
            SliderTarget::NeeRis => "slider.ris",
            SliderTarget::IndirectClamp => "slider.indirect-clamp",
            SliderTarget::EmaFloor => "slider.temporal-smooth",
            SliderTarget::SkyIntensity => "slider.sky-intensity",
            SliderTarget::HorizonSoftness => "slider.horizon",
            SliderTarget::SunElevation => "slider.sun-elevation",
            SliderTarget::SunAzimuth => "slider.sun-azimuth",
            SliderTarget::SunDisc => "slider.sun-disc",
            SliderTarget::SunIntensity => "slider.sun-intensity",
        }
    }

    fn is_material(self) -> bool {
        matches!(
            self,
            SliderTarget::MaterialRoughness
                | SliderTarget::MaterialMetallic
                | SliderTarget::MaterialTransparency
                | SliderTarget::MaterialIor
                | SliderTarget::MaterialEmission
                | SliderTarget::MaterialPower
                | SliderTarget::MotionAmount
        )
    }
}

#[derive(Clone)]
pub struct UiRegion {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub action: UiAction,
}

impl UiRegion {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}
mod dialog;
mod document;
mod drag;
mod files;
mod geom;
mod objects;
mod pick;
mod pointer;
mod selection;
mod vox;

use vox::VoxGraph;

#[derive(Clone, PartialEq)]
pub(super) struct Snapshot {
    objects: Vec<Volume>,
    palette: [[u8; 4]; 256],
    materials: [Material; 256],
    graph: Option<VoxGraph>,
    skeleton: Skeleton,
    pose: crate::rig::Pose,
    clips: Vec<Clip>,
}

#[derive(Clone)]
pub(super) struct VoxelClipboard {
    origin: Cell,
    cells: Vec<(Cell, u8)>,
}

pub(super) struct MoveDrag {
    axis: i32,
    plane: f32,
    anchor: Vec3,
    base_cells: HashMap<Cell, u8>,
    base_picked: Vec<Cell>,
    applied: Cell,
}

#[derive(Clone)]
pub(super) struct ObjectDrag {
    index: usize,
    locked: Option<i32>,
    plane_axis: i32,
    plane: f32,
    anchor: Vec3,
    normal: Vec3,
    base: Cell,
    bases: Vec<(usize, Cell, Cell)>,
    changed: bool,
}

pub struct Session {
    pub tool: Tool,
    pub tool_mode: ToolMode,
    pub brush_size: i32,
    pub brush_shape: BrushShape,
    pub brush_3d: bool,
    pub line_mode: LineMode,
    pub circle_fill: bool,
    pub circle_even: bool,
    pub square_fill: bool,
    pub square_even: bool,
    pub box_fill: bool,
    pub face_extrude: FaceExtrude,
    pub face_with: FaceWith,
    pub face_eight: bool,
    pub generator: GeneratorKind,
    pub gen_axis: Axis,
    pub apex_at_max: bool,
    pub torus_tube: f32,
    /// Edge bevel of the Rounded box generator, in voxels.
    pub round_radius: f32,
    pub tree_depth: i32,
    pub tree_wildness: f32,
    pub tree_trunk: f32,
    pub tree_leaf: i32,
    pub tree_seed: i32,
    pub conifer_tiers: i32,
    pub conifer_ragged: f32,
    pub conifer_clump: f32,
    pub conifer_trunk: f32,
    pub conifer_crown: f32,
    pub conifer_needle: i32,
    pub conifer_seed: i32,
    pub helix_turns: f32,
    pub helix_radius: f32,
    pub helix_tube: f32,
    pub helix_cw: bool,
    pub noise_scale: f32,
    pub noise_threshold: f32,
    pub noise_octaves: i32,
    pub noise_seed: i32,
    pub perlin_lo: i32,
    pub perlin_hi: i32,
    pub foliage_seed: i32,
    pub foliage_density: f32,

    pub palette: [[u8; 4]; 256],
    pub materials: [Material; 256],
    pub active_slot: u8,
    /// The next press on a voxel copies its palette index into `active_slot`
    /// and does not edit. The phone arms this from a long-press on the color
    /// swatch. A desktop Alt-click samples without arming it.
    pub sample_armed: bool,

    pub objects: Vec<Volume>,
    /// Primary object. Tools and the gizmo use this one.
    pub selected: usize,
    /// Object-mode selection, including `selected`. A rubber band fills it.
    pub object_selection: Vec<usize>,
    pub next_object: u32,
    pub path: Option<PathBuf>,
    /// Scene graph from the last opened `.vox`. Save writes it back, including
    /// groups, layers, hidden flags, and animation frames past the first.
    graph: Option<VoxGraph>,
    pub skeleton: Skeleton,
    /// Local rotations and scales on top of the bind pose. Empty leaves the
    /// bind pose. Undo restores this preview; it does not write `Bone.rotation`.
    pub pose: crate::rig::Pose,
    /// Named clips. Saved in the `MVAN` chunk and exported as glTF animations.
    pub clips: Vec<Clip>,
    /// Bone highlighted in the viewport. Not part of undo.
    pub selected_bone: Option<usize>,
    /// Parallel rays for a still. The window leaves this false.
    pub orthographic: bool,
    /// Draw joints on a still PNG and on Save render. The viewport draws
    /// them whenever a skeleton exists.
    pub show_bones: bool,
    /// Clip highlighted in the Clips panel. Not part of undo.
    pub selected_clip: Option<usize>,
    /// Name typed for the next clip.
    pub clip_draft: String,
    /// Time in seconds that Key pose writes.
    pub clip_time: f32,
    /// Mirror copies the bound bone subtree too.
    pub mirror_bones: bool,
    /// Options for File > Export Mesh.
    pub export: crate::export::MeshExport,

    pub editor_mode: EditorMode,
    pub render_mode: RenderMode,
    pub stochastic_gi: bool,
    pub glass_mode: GlassMode,
    pub max_samples: i32,
    pub aces: bool,
    pub denoise: bool,
    /// Viewport-only sway and bob. Off leaves the mesh still. Not saved.
    pub motion_preview: bool,
    /// Last script error for the panel. Not part of undo.
    pub script_error: String,
    /// `(object index, time)` when an auto-regenerate should run.
    pub script_due: Option<(usize, f64)>,
    /// The script running on a worker thread, collected by `poll_script`.
    pub script_job: Option<crate::script::ScriptJob>,
    pub aperture: f32,
    pub focus_distance: f32,
    pub max_bounces: i32,
    pub power_weighted_nee: bool,
    pub nee_ris: i32,
    pub indirect_clamp: f32,
    pub ema_floor: f32,
    pub atmospheric_sky: bool,
    pub sky_intensity: f32,
    pub horizon_softness: f32,
    pub sun_elevation: f32,
    pub sun_azimuth: f32,
    pub sun_disc: f32,
    pub sun_intensity: f32,
    pub sun_color: [f32; 3],
    /// Latest CPU frame. The GPU path leaves this empty and saves from the texture.
    pub(crate) render_preview: Option<crate::trace::Image>,
    pub(crate) render_key: u64,
    /// Set while the viewport is showing the compute shader, so Save render
    /// reads that texture instead of the CPU frame.
    pub(crate) trace_on_gpu: bool,
    pub(crate) render_save_path: Option<PathBuf>,
    /// Sample count of the frame when the pending save last looked at it.
    pub(crate) render_save_seen: Option<u32>,

    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,

    pub mesh_epoch: u64,
    pub status: String,
    pub hover: Option<Hit>,
    /// Occupied cells of the active object chosen by Marquee, Box Select,
    /// or Flood Select. Rotate, flip, and delete act on this set when it
    /// is not empty.
    pub picked: HashSet<Cell>,
    pub selection_epoch: u64,
    select_anchor: Option<Cell>,
    clipboard: Option<VoxelClipboard>,
    move_drag: Option<MoveDrag>,
    object_drag: Option<ObjectDrag>,
    /// Axis under the cursor in Object mode. The viewport sets this before
    /// the pointer handler, so a press on an arrow locks that axis.
    pub hover_gizmo: Option<i32>,
    stroke: Option<Stroke>,
    stroke_serial: u32,
    pub(crate) grab: Grab,
    /// Slots highlighted in the palette. The active slot is always one of them.
    pub palette_slots: HashSet<u8>,
    color_clipboard: Option<[u8; 4]>,
    palette_press: Option<(i32, i32)>,
    palette_current: Option<(i32, i32)>,
    palette_shift: bool,
    pub palette_anchor: u8,
    pending: Option<Snapshot>,
    /// Document as last opened, saved, or created. Compared only when the
    /// window asks to close, so edits are not hashed every frame.
    clean: Option<Box<Snapshot>>,
    /// A panel drag in progress: edits gather into one undo step until it ends.
    gesture: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Controls drawn last frame, in paint order. A click hits the last match.
    pub ui_regions: Vec<UiRegion>,
    recent: Vec<PathBuf>,
}

pub(super) const IDLE: &str = "Right-drag orbits, scroll zooms, left-drag edits with the active tool. Render mode path-traces the scene.";

pub use geom::palette_slot_at;
#[allow(unused_imports)] // keeps `crate::state::raycast` after the move into geom
pub use geom::raycast;
pub(crate) use geom::{occupied_bounds, resolved_box};
pub(crate) use vox::rotation_codec_errors;

#[cfg(test)]
#[path = "../../tests/state.rs"]
mod tests;

/// Where a phone or tablet keeps the user's files: the app's own Documents
/// folder. The desktop picks paths in native dialogs instead.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub fn documents_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join("Documents");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
