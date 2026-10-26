use std::collections::{HashMap, HashSet};

use crate::ecs::{Entity, World};
use crate::editor::{EditorAction, EditorState};
use crate::game::components::{
    AnimationPlayer, Chase, Elevator, Health, Interactable, MaterialHandle, MeshHandle, Name,
    Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint, Transform, Trigger,
    Velocity, Visible,
};
use crate::game::lights::{DirectionalLight, PointLight};
use crate::physics::PhysicsMaterial;

pub struct HierarchySnapshot {
    children_of: HashMap<Entity, Vec<Entity>>,
    roots: Vec<Entity>,
    visible: HashSet<Entity>,
    names: HashMap<Entity, String>,
    filter_active: bool,
}

#[derive(Clone, Copy)]
pub struct HierarchyRow {
    pub entity: Entity,
    pub depth: usize,
}

impl HierarchySnapshot {
    pub fn build(world: &World, filter: &str) -> Self {
        let all: Vec<Entity> = world.entities().to_vec();
        let all_set: HashSet<Entity> = all.iter().copied().collect();

        let filter_active = !filter.trim().is_empty();
        let filter_lower = filter.to_lowercase();

        let mut names: HashMap<Entity, String> = HashMap::with_capacity(all.len());
        let mut visible: HashSet<Entity> = HashSet::with_capacity(all.len());

        for &e in &all {
            let name = world
                .get::<Name>(e)
                .map(|n| n.0.clone())
                .unwrap_or_else(|| {
                    let mesh = world
                        .get::<MeshHandle>(e)
                        .map(|m| m.0.clone())
                        .unwrap_or_else(|| "?".to_string());
                    format!("#{} {}", e, mesh)
                });

            if !filter_active || name.to_lowercase().contains(&filter_lower) {
                visible.insert(e);
            }
            names.insert(e, name);
        }

        let mut children_of: HashMap<Entity, Vec<Entity>> = HashMap::new();
        let mut has_parent: HashSet<Entity> = HashSet::new();

        for &e in &all {
            if let Some(Parent(p)) = world.get::<Parent>(e).copied() {
                if all_set.contains(&p) && p != e {
                    children_of.entry(p).or_default().push(e);
                    has_parent.insert(e);
                }
            }
        }

        let mut roots: Vec<Entity> = all
            .iter()
            .copied()
            .filter(|e| !has_parent.contains(e))
            .collect();
        roots.sort();

        for kids in children_of.values_mut() {
            kids.sort_by(|a, b| {
                let na = names.get(a).cloned().unwrap_or_default();
                let nb = names.get(b).cloned().unwrap_or_default();
                na.cmp(&nb)
            });
        }

        if filter_active {
            let snapshot: Vec<Entity> = visible.iter().copied().collect();
            for mut e in snapshot {
                for _ in 0..64 {
                    match world.get::<Parent>(e).copied() {
                        Some(Parent(p)) if all_set.contains(&p) => {
                            visible.insert(p);
                            e = p;
                        }
                        _ => break,
                    }
                }
            }
        }

        Self {
            children_of,
            roots,
            visible,
            names,
            filter_active,
        }
    }

    fn children_of(&self, e: Entity) -> &[Entity] {
        self.children_of
            .get(&e)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn is_visible(&self, e: Entity) -> bool {
        !self.filter_active || self.visible.contains(&e)
    }

    fn visible_count(&self) -> usize {
        self.flatten(|_| true).len()
    }

    fn flatten(&self, is_expanded: impl Fn(Entity) -> bool) -> Vec<HierarchyRow> {
        let mut out = Vec::new();
        for &root in &self.roots {
            if !self.is_visible(root) {
                continue;
            }
            self.flatten_rec(root, 0, &is_expanded, &mut out);
        }
        out
    }

    fn flatten_rec(
        &self,
        e: Entity,
        depth: usize,
        is_expanded: &impl Fn(Entity) -> bool,
        out: &mut Vec<HierarchyRow>,
    ) {
        out.push(HierarchyRow { entity: e, depth });
        if !is_expanded(e) {
            return;
        }
        for &c in self.children_of(e) {
            if self.is_visible(c) {
                self.flatten_rec(c, depth + 1, is_expanded, out);
            }
        }
    }

    fn flat_order(&self) -> Vec<Entity> {
        self.flatten(|_| true)
            .into_iter()
            .map(|r| r.entity)
            .collect()
    }
}

// ============================================================
// Section (публичная точка входа для left_panel)
// ============================================================

pub fn draw_section(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    world: &mut World,
    action: &mut Option<EditorAction>,
) {
    ui.heading("Hierarchy");
    ui.separator();

    ui.horizontal(|ui| {
        if ui.button("+ Cube").clicked() {
            *action = Some(EditorAction::AddCube);
        }
        if ui.button("+ Sphere").clicked() {
            *action = Some(EditorAction::AddSphere);
        }
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !editor.selected.is_empty(),
                egui::Button::new("Duplicate (Ctrl+D)"),
            )
            .clicked()
        {
            *action = Some(EditorAction::Duplicate);
        }
        if ui
            .add_enabled(!editor.selected.is_empty(), egui::Button::new("Delete"))
            .clicked()
        {
            *action = Some(EditorAction::DeleteSelected);
        }
    });

    ui.horizontal(|ui| {
        if ui.small_button("⤢ Expand").clicked() {
            for &e in world.entities() {
                editor.hierarchy_expanded.insert(e);
            }
        }
        if ui.small_button("⤡ Collapse").clicked() {
            editor.hierarchy_expanded.clear();
        }
        if !editor.selected.is_empty() {
            if ui.small_button("→ Selected").clicked() {
                let selected: Vec<Entity> = editor.selected.clone();
                for e in selected {
                    let mut cur = e;
                    for _ in 0..64 {
                        editor.hierarchy_expanded.insert(cur);
                        match world.get::<Parent>(cur).copied() {
                            Some(Parent(p)) => cur = p,
                            None => break,
                        }
                    }
                }
            }
        }
    });

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("🔍");
        ui.add(
            egui::TextEdit::singleline(&mut editor.search_filter)
                .desired_width(120.0)
                .hint_text("name…"),
        );
        if !editor.search_filter.is_empty() && ui.small_button("✖").clicked() {
            editor.search_filter.clear();
        }
    });

    let snapshot = HierarchySnapshot::build(world, &editor.search_filter);
    let total = snapshot.visible_count();
    let sel_count = editor.selected.len();
    if sel_count > 1 {
        ui.label(format!("{} shown · {} selected", total, sel_count));
    } else {
        ui.label(format!("{} shown", total));
    }

    let anchor = editor.primary();

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(340.0)
        .show(ui, |ui| {
            let rows = snapshot.flatten(|e| editor.hierarchy_expanded.contains(&e));
            for row in rows {
                draw_row(
                    ui,
                    editor,
                    world,
                    row.entity,
                    &snapshot,
                    anchor,
                    action,
                    row.depth,
                );
            }
        });
}

// ============================================================
// Row
// ============================================================

fn draw_row(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    world: &mut World,
    e: Entity,
    snapshot: &HierarchySnapshot,
    anchor: Option<Entity>,
    action: &mut Option<EditorAction>,
    depth: usize,
) {
    let is_selected = editor.is_selected(e);
    let is_locked = editor.is_locked(world, e);
    let has_children = !snapshot.children_of(e).is_empty();
    let is_expanded = editor.hierarchy_expanded.contains(&e);

    ui.horizontal(|ui| {
        if depth > 0 {
            ui.add_space(depth as f32 * 14.0);
        }

        if has_children {
            let label = if is_expanded { "▼" } else { "▶" };
            if ui
                .add(egui::Button::new(label).frame(false).small())
                .clicked()
            {
                if is_expanded {
                    editor.hierarchy_expanded.remove(&e);
                } else {
                    editor.hierarchy_expanded.insert(e);
                }
            }
        } else {
            ui.add_space(16.0);
        }

        let solo_active = editor.solo == Some(e);
        let solo_icon = if solo_active { "◉" } else { "○" };
        let solo_resp = ui
            .add(egui::Button::new(solo_icon).frame(false).small())
            .on_hover_text("Solo");
        if solo_resp.clicked() {
            editor.solo = if solo_active { None } else { Some(e) };
        }

        let lock_icon = if is_locked { "🔒" } else { "🔓" };
        let lock_resp = ui
            .add(egui::Button::new(lock_icon).frame(false).small())
            .on_hover_text("Lock");
        if lock_resp.clicked() {
            if is_locked {
                editor.locked.remove(&e);
            } else {
                editor.locked.insert(e);
            }
        }

        let visible = world.get::<Visible>(e).map(|v| v.0).unwrap_or(true);
        let eye_label = if visible { "👁" } else { "✖" };
        let eye_resp = ui.add(egui::Button::new(eye_label).frame(false).small());
        if eye_resp.clicked() {
            editor.undo_requested = true;
            if visible {
                world.insert(e, Visible(false));
            } else {
                world.remove::<Visible>(e);
            }
        }

        if editor.renaming == Some(e) {
            draw_rename(ui, editor, world, e);
            return;
        }

        let label = snapshot
            .names
            .get(&e)
            .cloned()
            .unwrap_or_else(|| entity_display_name(world, e));
        let badges = component_badges(world, e);

        let text_color = if is_locked {
            ui.visuals().weak_text_color()
        } else {
            ui.visuals().text_color()
        };

        let drag_id = egui::Id::new(("hier_drag", e));
        let drag_inner = ui.dnd_drag_source(drag_id, e, |ui| {
            let resp = ui.add(
                egui::Label::new(egui::RichText::new(&label).color(text_color))
                    .sense(egui::Sense::click_and_drag()),
            );
            if !badges.is_empty() {
                ui.add_space(2.0);
                ui.label(egui::RichText::new(&badges).small().weak().monospace());
            }
            resp
        });
        let drag_resp = drag_inner.response;

        if let Some(payload) = drag_resp.dnd_release_payload::<Entity>() {
            let dragged = *payload;
            if dragged != e && !would_create_cycle(world, dragged, e) {
                editor.undo_requested = true;
                world.insert(dragged, Parent(e));
                editor.hierarchy_expanded.insert(e);
            }
        }

        let clicked = drag_resp.clicked();
        let double_clicked = drag_resp.double_clicked();

        if is_selected {
            let rect = drag_resp.rect.expand2(egui::vec2(2.0, 1.0));
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(2),
                ui.visuals().selection.bg_fill.linear_multiply(0.5),
            );
        }

        if clicked {
            if is_locked {
                editor.hud_hint("Entity is locked");
            } else {
                let modifiers = ui.input(|i| i.modifiers);
                if modifiers.ctrl || modifiers.command {
                    editor.toggle_select(e);
                } else if modifiers.shift {
                    let flat = snapshot.flat_order();
                    editor.select_range(&flat, anchor.unwrap_or(e), e);
                } else {
                    editor.select_single(e);
                }
            }
        }

        if double_clicked && !is_locked {
            editor.select_single(e);
            editor.renaming = Some(e);
            editor.rename_buffer = snapshot
                .names
                .get(&e)
                .cloned()
                .unwrap_or_else(|| entity_display_name(world, e));
        }

        drag_resp.context_menu(|ui| {
            context_menu(ui, editor, world, e, action);
        });
    });
}

fn draw_rename(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    world: &mut World,
    e: Entity,
) {
    let response = ui.add(
        egui::TextEdit::singleline(&mut editor.rename_buffer).desired_width(160.0),
    );
    response.request_focus();
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if enter || response.lost_focus() {
        let new_name = editor.rename_buffer.trim().to_string();
        if !new_name.is_empty() {
            editor.undo_requested = true;
            world.insert(e, Name(new_name));
        }
        editor.renaming = None;
        editor.rename_buffer.clear();
    } else if esc {
        editor.renaming = None;
        editor.rename_buffer.clear();
    }
}

fn context_menu(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    world: &mut World,
    e: Entity,
    action: &mut Option<EditorAction>,
) {
    if ui.button("Rename").clicked() {
        editor.renaming = Some(e);
        editor.rename_buffer = world
            .get::<Name>(e)
            .map(|n| n.0.clone())
            .unwrap_or_else(|| entity_display_name(world, e));
        ui.close();
    }
    if ui.button("Focus (F)").clicked() {
        editor.select_single(e);
        *action = Some(EditorAction::FocusSelected);
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        editor.select_single(e);
        *action = Some(EditorAction::Duplicate);
        ui.close();
    }
    if ui.button("Delete").clicked() {
        editor.select_single(e);
        *action = Some(EditorAction::DeleteSelected);
        ui.close();
    }
    ui.separator();
    if ui.button("Lock / Unlock").clicked() {
        if editor.locked.contains(&e) {
            editor.locked.remove(&e);
        } else {
            editor.locked.insert(e);
        }
        ui.close();
    }
    if editor.solo == Some(e) {
        if ui.button("Unsolo").clicked() {
            editor.solo = None;
            ui.close();
        }
    } else if ui.button("Solo").clicked() {
        editor.solo = Some(e);
        ui.close();
    }
    ui.separator();
    if ui.button("Export FBX…").clicked() {
        editor.select_single(e);
        *action = Some(EditorAction::ExportFbxSelected);
        ui.close();
    }
    ui.separator();
    if world.has::<Parent>(e) && ui.button("Clear Parent").clicked() {
        editor.undo_requested = true;
        world.remove::<Parent>(e);
        ui.close();
    }
}

// ============================================================
// Helpers
// ============================================================

fn component_badges(world: &World, e: Entity) -> String {
    let mut s = String::with_capacity(24);
    if world.has::<Transform>(e) { s.push_str("T "); }
    if world.has::<MeshHandle>(e) { s.push_str("M "); }
    if world.has::<MaterialHandle>(e) { s.push_str("Mat "); }
    if world.has::<Parent>(e) { s.push_str("P "); }
    if world.has::<SkeletonHandle>(e) { s.push_str("Sk "); }
    if world.has::<AnimationPlayer>(e) { s.push_str("A "); }
    if world.has::<Spinner>(e) { s.push_str("Sp "); }
    if world.has::<Velocity>(e) { s.push_str("V "); }
    if world.has::<Tint>(e) { s.push_str("Ti "); }
    if world.has::<Visible>(e) { s.push_str("Vi "); }
    if world.has::<TextureTiling>(e) { s.push_str("Tt "); }
    if world.has::<Elevator>(e) { s.push_str("El "); }
    if world.has::<SlidingDoor>(e) { s.push_str("SD "); }
    if world.has::<Health>(e) { s.push_str("H "); }
    if world.has::<Chase>(e) { s.push_str("Ch "); }
    if world.has::<Interactable>(e) { s.push_str("In "); }
    if world.has::<Trigger>(e) { s.push_str("Tr "); }
    if world.has::<crate::physics::RigidBody>(e) { s.push_str("Ph "); }
    if world.has::<crate::physics::Collider>(e) { s.push_str("C "); }
    if world.has::<PhysicsMaterial>(e) { s.push_str("PM "); }
    if world.has::<DirectionalLight>(e) { s.push_str("☀ "); }
    if world.has::<PointLight>(e) { s.push_str("💡 "); }
    if world.has::<crate::game::decals::Decal>(e) { s.push_str("🎨 "); }
    if world.has::<crate::game::audio::AudioSource>(e) { s.push_str("🔊 "); }
    if world.has::<crate::game::ai::AiAgent>(e) { s.push_str("🤖 "); }
    if world.has::<crate::game::ai::Enemy>(e) { s.push_str("👹 "); }
    if world.has::<crate::game::ai::AiTarget>(e) { s.push_str("🎯 "); }
    s.trim_end().to_string()
}

fn entity_display_name(world: &World, e: Entity) -> String {
    if let Some(n) = world.get::<Name>(e) {
        return n.0.clone();
    }
    let mesh = world
        .get::<MeshHandle>(e)
        .map(|m| m.0.as_str())
        .unwrap_or("?");
    format!("#{} {}", e, mesh)
}

fn would_create_cycle(world: &World, child: Entity, new_parent: Entity) -> bool {
    let mut cur = new_parent;
    for _ in 0..64 {
        if cur == child {
            return true;
        }
        match world.get::<Parent>(cur) {
            Some(&Parent(p)) => cur = p,
            None => return false,
        }
    }
    true
}