//! The object and bone tree, their inspectors, the pose, and clips.
//!
//! As in Godot, the tree only selects; the selected item's fields sit in an
//! inspector grid above it, so editing never shifts the rows.

use egui::{RichText, Ui};

use super::*;

/// Width of the property-name column in every inspector grid.
const LABEL_WIDTH: f32 = 60.0;
/// Width of one x, y, or z field, so the columns line up across rows.
const AXIS_WIDTH: f32 = 54.0;
/// Width of a single field that spans the three axis columns.
const WIDE_WIDTH: f32 = AXIS_WIDTH * 3.0 + 8.0;
/// Height of every inspector row. Taller than any field's natural height, so
/// labels, fields, buttons, and combo boxes share one center line.
const ROW_HEIGHT: f32 = 22.0;
fn inspector(ui: &mut Ui, id: &str, add_rows: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size = egui::vec2(AXIS_WIDTH, ROW_HEIGHT);
        egui::Grid::new(id)
            .num_columns(2)
            .min_col_width(LABEL_WIDTH)
            .min_row_height(ROW_HEIGHT)
            .spacing([8.0, 4.0])
            .show(ui, add_rows);
    });
}

fn prop_label(ui: &mut Ui, text: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(LABEL_WIDTH, ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| ui.label(RichText::new(text).small().color(DIM)),
    );
}

fn axis_row(ui: &mut Ui, add_fields: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        add_fields(ui);
    });
}

/// `interact_size.x` is the drag value's minimum width inside an inspector.
fn axis_field(ui: &mut Ui, drag: egui::DragValue<'_>) -> egui::Response {
    ui.add(drag)
}

fn axis_button(ui: &mut Ui, session: &mut Session, id: &str, label: &str, action: UiAction) {
    let resp = ui.add(egui::Button::new(label).min_size(egui::vec2(AXIS_WIDTH, ROW_HEIGHT)));
    run_action(session, id, label, resp, action);
}

#[derive(Clone, Copy, PartialEq)]
enum TreeTab {
    Objects,
    Bones,
}

#[derive(Clone, Copy)]
struct TreeState {
    tab: TreeTab,
    open: bool,
}

impl Default for TreeState {
    fn default() -> Self {
        Self {
            tab: TreeTab::Objects,
            open: true,
        }
    }
}

/// The scene tree docked under the inspector: objects or bones, one at a
/// time. Collapsed, it keeps only its tab strip. The gap above it is the
/// resize handle.
pub(super) fn tree_panel(ui: &mut Ui, session: &mut Session) {
    let id = egui::Id::unique("tree-state");
    let mut state: TreeState = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let panel = if state.open {
        egui::Panel::bottom("tree-open")
            .resizable(true)
            .default_size(300.0)
            .size_range(120.0..=900.0)
    } else {
        egui::Panel::bottom("tree-closed").resizable(false)
    };
    let frame = egui::Frame::NONE.inner_margin(egui::Margin {
        top: DOCK_GAP,
        ..egui::Margin::ZERO
    });
    let tab = state.tab;
    let open = state.open;
    let mut toggle = false;
    panel
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let selected = match tab {
                TreeTab::Objects => 0,
                TreeTab::Bones => 1,
            };
            let trailing = |ui: &mut Ui| {
                let (icon, hint) = if open {
                    ("⏷", "Hide the tree")
                } else {
                    ("⏶", "Show the tree")
                };
                toggle = ui.small_button(icon).on_hover_text(hint).clicked();
            };
            let picked = dock(
                ui,
                &["Objects", "Bones"],
                selected,
                open,
                trailing,
                |ui| match tab {
                    TreeTab::Objects => object_list(ui, session),
                    TreeTab::Bones => bone_list(ui, session),
                },
            );
            match picked {
                Some(0) => state.tab = TreeTab::Objects,
                Some(_) => state.tab = TreeTab::Bones,
                None => {}
            }
            if picked.is_some() && !open {
                state.open = true;
            }
        });
    if toggle {
        state.open = !state.open;
    }
    ui.data_mut(|data| data.insert_temp(id, state));
}

fn object_list(ui: &mut Ui, session: &mut Session) {
    ui.horizontal(|ui| {
        action_button(ui, session, "object.new", "New", UiAction::NewObject);
        action_button(
            ui,
            session,
            "object.duplicate",
            "Duplicate",
            UiAction::Duplicate,
        );
        ui.add_enabled_ui(session.objects.len() > 1, |ui| {
            action_button(
                ui,
                session,
                "object.delete",
                "Delete",
                UiAction::DeleteObject,
            );
        });
    });
    let mut select = None;
    let mut frame = false;
    egui::ScrollArea::vertical()
        .id_salt("object-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for i in 0..session.objects.len() {
                let chosen = session.object_selection.contains(&i);
                let name = session.objects[i].name.clone();
                let label = if i == session.selected {
                    RichText::new(name).strong()
                } else {
                    RichText::new(name)
                };
                let resp = ui
                    .horizontal(|ui| {
                        let hidden = session.objects[i].hidden;
                        action_checkbox(
                            ui,
                            session,
                            &format!("object.visible.{i}"),
                            "",
                            !hidden,
                            UiAction::ObjectHidden(i),
                        );
                        ui.selectable_label(chosen, label)
                    })
                    .inner;
                if resp.clicked() {
                    select = Some(i);
                }
                if resp.double_clicked() {
                    select = Some(i);
                    frame = true;
                }
                resp.context_menu(|ui| {
                    if ui.button("Make active").clicked() {
                        select = Some(i);
                        ui.close();
                    }
                });
            }
        });
    if let Some(i) = select {
        session.select_object(i);
    }
    if frame {
        session.frame_camera();
    }
}

pub(super) fn object(ui: &mut Ui, session: &mut Session) {
    section(ui, "Object");
    let mut rename = None;
    let at = session.active().translation;
    let mut position = [at.0, at.1, at.2];
    let mut moved = false;
    inspector(ui, "object-inspector", |ui| {
        prop_label(ui, "Name");
        let mut text = session.active().name.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut text).desired_width(WIDE_WIDTH))
            .changed()
        {
            rename = Some(text);
        }
        ui.end_row();

        prop_label(ui, "Size");
        axis_row(ui, |ui| {
            if let Some((x, y, z)) = session.size_xyz() {
                for (axis, value) in [(0usize, x), (1, y), (2, z)] {
                    let mut next = value;
                    axis_field(
                        ui,
                        egui::DragValue::new(&mut next).range(1..=256).speed(0.2),
                    );
                    if next != value {
                        session.resize_active(axis, next);
                    }
                }
            } else {
                ui.monospace("—");
            }
        });
        ui.end_row();

        prop_label(ui, "Position");
        axis_row(ui, |ui| {
            for value in &mut position {
                let response = axis_field(
                    ui,
                    egui::DragValue::new(value).range(-1024..=1023).speed(0.2),
                );
                if response.drag_started() {
                    session.begin_gesture();
                }
                moved |= response.changed();
            }
        });
        ui.end_row();

        prop_label(ui, "Mirror");
        axis_row(ui, |ui| {
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                axis_button(
                    ui,
                    session,
                    &format!("object.mirror.{}", name.to_ascii_lowercase()),
                    name,
                    UiAction::Mirror(axis),
                );
            }
        });
        ui.end_row();

        ui.label("");
        let bones = session.mirror_bones;
        action_checkbox(
            ui,
            session,
            "object.mirror-bones",
            "Mirror bones too",
            bones,
            UiAction::MirrorBones,
        );
        ui.end_row();

        prop_label(ui, "Bone");
        bind_object(ui, session);
        ui.end_row();
    });
    if let Some(name) = rename {
        session.rename_selected(name);
    }
    if moved {
        let index = session.selected;
        if let Err(err) = session.set_object(index, None, Some(position), None) {
            session.status = err;
        }
    }
}

/// Bones in tree order with their depth, so children sit under parents.
fn bone_tree(skeleton: &crate::rig::Skeleton) -> Vec<(usize, usize)> {
    fn visit(
        skeleton: &crate::rig::Skeleton,
        parent: Option<&str>,
        depth: usize,
        out: &mut Vec<(usize, usize)>,
    ) {
        for (index, bone) in skeleton.bones.iter().enumerate() {
            if bone.parent.as_deref() == parent && !out.iter().any(|(seen, _)| *seen == index) {
                out.push((index, depth));
                visit(skeleton, Some(&bone.name), depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(skeleton, None, 0, &mut out);
    // A bone whose parent is missing still gets a row.
    for index in 0..skeleton.bones.len() {
        if !out.iter().any(|(seen, _)| *seen == index) {
            out.push((index, 0));
        }
    }
    out
}

fn bone_list(ui: &mut Ui, session: &mut Session) {
    if ui.button("Add bone").clicked() {
        let name = session.fresh_bone_name();
        let parent = session
            .selected_bone
            .and_then(|index| session.skeleton.bones.get(index))
            .map(|bone| bone.name.clone());
        let pivot = session.active_bone_pivot();
        if let Err(err) = session.add_bone(&name, parent.as_deref(), pivot, None) {
            session.status = err;
        }
    }
    let mut pick = None;
    egui::ScrollArea::vertical()
        .id_salt("bone-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, depth) in bone_tree(&session.skeleton) {
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let name = &session.skeleton.bones[index].name;
                    if ui
                        .selectable_label(session.selected_bone == Some(index), name)
                        .clicked()
                    {
                        pick = Some(index);
                    }
                });
            }
        });
    if let Some(index) = pick {
        session.selected_bone = Some(index);
    }
}

/// Inspector of the bone picked in the tree.
pub(super) fn bone(ui: &mut Ui, session: &mut Session) {
    if session.skeleton.bones.is_empty() {
        session.selected_bone = None;
        return;
    }
    section(ui, "Bone");
    match session
        .selected_bone
        .filter(|index| *index < session.skeleton.bones.len())
    {
        Some(index) => bone_fields(ui, session, index),
        None => {
            session.selected_bone = None;
            ui.label(
                RichText::new("Pick a bone in the Bones tree.")
                    .small()
                    .color(DIM),
            );
        }
    }
}

fn bone_fields(ui: &mut Ui, session: &mut Session, index: usize) {
    let bone_name = session.skeleton.bones[index].name.clone();
    let parent = session.skeleton.bones[index].parent.clone();
    let pivot = session.skeleton.bones[index].pivot;
    let rotation = session.skeleton.bones[index].rotation;
    let names: Vec<String> = session
        .skeleton
        .bones
        .iter()
        .map(|bone| bone.name.clone())
        .collect();
    let mut rename = None;
    let mut next_parent = parent.clone();
    let mut parent_changed = false;
    let mut turned = None;
    inspector(ui, "bone-inspector", |ui| {
        prop_label(ui, "Name");
        let mut text = bone_name.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut text).desired_width(WIDE_WIDTH))
            .changed()
            && text != bone_name
        {
            rename = Some(text);
        }
        ui.end_row();

        prop_label(ui, "Parent");
        egui::ComboBox::from_id_salt("bone-parent")
            .width(WIDE_WIDTH)
            .selected_text(parent.as_deref().unwrap_or("Root"))
            .show_ui(ui, |ui| {
                if ui.selectable_label(parent.is_none(), "Root").clicked() {
                    next_parent = None;
                    parent_changed = true;
                }
                for name in &names {
                    if !session.skeleton.accepts_parent(&bone_name, name) {
                        continue;
                    }
                    if ui
                        .selectable_label(parent.as_deref() == Some(name.as_str()), name)
                        .clicked()
                    {
                        next_parent = Some(name.clone());
                        parent_changed = true;
                    }
                }
            });
        ui.end_row();

        prop_label(ui, "Pivot");
        axis_row(ui, |ui| {
            for (axis, start) in pivot.into_iter().enumerate() {
                let mut value = start;
                let response = axis_field(ui, egui::DragValue::new(&mut value).speed(0.05));
                if response.changed() {
                    session.preview_bone_pivot(index, axis, value);
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    session.finish_bone_edit();
                }
            }
        });
        ui.end_row();

        prop_label(ui, "Rotation");
        let (next, started) = angle_fields(ui, rotation);
        if started {
            session.begin_gesture();
        }
        turned = next;
        ui.end_row();
    });
    if let Some(text) = rename {
        if let Err(err) = session.set_bone(&bone_name, None, None, None, Some(text.trim())) {
            session.status = err;
        }
        return;
    }
    if parent_changed && next_parent != parent {
        if let Err(err) = session.set_bone(&bone_name, Some(next_parent), None, None, None) {
            session.status = err;
        }
    }
    if let Some(rotation) = turned {
        if let Err(err) = session.set_bone(&bone_name, None, None, Some(rotation), None) {
            session.status = err;
        }
    }
    if ui.button("Delete bone").clicked() {
        if let Err(err) = session.remove_bone(&bone_name) {
            session.status = err;
        }
    }
}

pub(super) fn bind_object(ui: &mut Ui, session: &mut Session) {
    let names: Vec<String> = session
        .skeleton
        .bones
        .iter()
        .map(|bone| bone.name.clone())
        .collect();
    let current = session
        .objects
        .get(session.selected)
        .and_then(|object| object.bone.clone());
    let mut next = current.clone();
    let mut changed = false;
    egui::ComboBox::from_id_salt("object-bone")
        .width(WIDE_WIDTH)
        .selected_text(current.as_deref().unwrap_or("None"))
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "None").clicked() {
                next = None;
                changed = true;
            }
            for name in &names {
                if ui
                    .selectable_label(current.as_deref() == Some(name.as_str()), name)
                    .clicked()
                {
                    next = Some(name.clone());
                    changed = true;
                }
            }
        });
    if changed && next != current {
        let index = session.selected;
        if let Err(err) = session.bind_object(index, next.as_deref()) {
            session.status = err;
        }
    }
}

/// Three angle fields, in degrees, for a quaternion `[x, y, z, w]`. Returns
/// the new rotation when a field changed, and whether a drag began.
fn angle_fields(ui: &mut Ui, rotation: [f32; 4]) -> (Option<[f32; 4]>, bool) {
    let (x, y, z) = glam::Quat::from_array(rotation)
        .normalize()
        .to_euler(glam::EulerRot::XYZ);
    // `+ 0.0` turns -0 into 0, so a still bone does not read "-0°".
    let mut angles = [x, y, z].map(|angle| angle.to_degrees() + 0.0);
    let mut changed = false;
    let mut started = false;
    axis_row(ui, |ui| {
        for angle in angles.iter_mut() {
            let response = axis_field(
                ui,
                egui::DragValue::new(angle)
                    .range(-180.0..=180.0)
                    .speed(0.5)
                    .max_decimals(1)
                    .suffix("°"),
            );
            started |= response.drag_started();
            changed |= response.changed();
        }
    });
    let next = changed.then(|| {
        let [x, y, z] = angles.map(f32::to_radians);
        glam::Quat::from_euler(glam::EulerRot::XYZ, x, y, z)
            .normalize()
            .to_array()
    });
    (next, started)
}

/// Preview pose of the selected bone: a turn and a scale on top of the bind
/// pose, the same as `set_pose`.
pub(super) fn pose(ui: &mut Ui, session: &mut Session) {
    section(ui, "Pose");
    let selected = session
        .selected_bone
        .filter(|index| *index < session.skeleton.bones.len());
    if let Some(index) = selected {
        let name = session.skeleton.bones[index].name.clone();
        let extra = session
            .pose
            .get(&name)
            .copied()
            .unwrap_or(crate::rig::BonePose::IDENTITY);
        let mut turned = None;
        let mut started = false;
        let mut scale = extra.scale;
        let mut scaled = false;
        inspector(ui, "pose-inspector", |ui| {
            prop_label(ui, "Turn");
            let (next, began) = angle_fields(ui, extra.rotation);
            turned = next;
            started |= began;
            ui.end_row();

            prop_label(ui, "Scale");
            axis_row(ui, |ui| {
                for value in scale.iter_mut() {
                    let response = axis_field(
                        ui,
                        egui::DragValue::new(value)
                            .range(0.0..=16.0)
                            .speed(0.01)
                            .max_decimals(2),
                    );
                    started |= response.drag_started();
                    scaled |= response.changed();
                }
            });
            ui.end_row();
        });
        if started {
            session.begin_gesture();
        }
        if turned.is_some() || scaled {
            let next = crate::rig::BonePose {
                rotation: turned.unwrap_or(extra.rotation),
                scale,
            };
            if let Err(err) = session.set_bone_pose(index, next) {
                session.status = err;
            }
        }
        if ui.button("Reset bone").clicked() {
            if let Err(err) = session.set_bone_pose(index, crate::rig::BonePose::IDENTITY) {
                session.status = err;
            }
        }
    } else {
        ui.label(
            RichText::new("Select a bone to pose it.")
                .small()
                .color(DIM),
        );
    }
    ui.add_enabled_ui(!session.pose.is_empty(), |ui| {
        action_button(ui, session, "pose.clear", "Clear pose", UiAction::ClearPose);
    });
}

/// Animation clips: add, delete, and key the current pose at a time.
pub(super) fn clips(ui: &mut Ui, session: &mut Session) {
    section(ui, "Clips");
    let mut pick = None;
    for (index, clip) in session.clips.iter().enumerate() {
        let keys: usize = clip.tracks.iter().map(|track| track.keys.len()).sum();
        let bones = clip.tracks.len();
        let label = format!(
            "{}  ·  {bones} {}, {keys} {}",
            clip.name,
            if bones == 1 { "bone" } else { "bones" },
            if keys == 1 { "key" } else { "keys" },
        );
        if ui
            .selectable_label(session.selected_clip == Some(index), label)
            .clicked()
        {
            pick = Some(index);
        }
    }
    if let Some(index) = pick {
        session.selected_clip = Some(index);
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut session.clip_draft)
                .desired_width(120.0)
                .hint_text("New clip"),
        );
        action_button(ui, session, "clip.add", "Add", UiAction::AddClip);
    });
    let Some(index) = session
        .selected_clip
        .filter(|index| *index < session.clips.len())
    else {
        session.selected_clip = None;
        return;
    };
    for track in &session.clips[index].tracks {
        let times: Vec<String> = track
            .keys
            .iter()
            .map(|key| format!("{:.2}", key.time))
            .collect();
        ui.label(
            RichText::new(format!("{}: {}", track.bone, times.join(" ")))
                .small()
                .monospace()
                .color(DIM),
        );
    }
    ui.horizontal(|ui| {
        ui.label("Time");
        ui.add(
            egui::DragValue::new(&mut session.clip_time)
                .range(0.0..=600.0)
                .speed(0.05)
                .max_decimals(2)
                .suffix(" s"),
        );
        action_button(ui, session, "clip.key", "Key pose", UiAction::KeyPose);
        action_button(ui, session, "clip.delete", "Delete", UiAction::RemoveClip);
    });
}
