//! Visual dialog editor (node graph) — Фаза 5: Undo/Redo + UX.
//!
//! **Что умеет:**
//!   * Загрузка/сохранение `.ron` (граф) и `.layout.ron` (позиции).
//!   * Интерактивное редактирование: спикер, текст, опции, эффекты.
//!   * Добавление/дублирование/удаление нод.
//!   * Drag нод, pan (MMB/Space+LMB), zoom (scroll).
//!   * Undo / Redo (Ctrl+Z / Ctrl+Y, до 50 шагов).
//!   * Авто-fit при загрузке (для больших графов).
//!
//! **Горячие клавиши:**
//!   * Ctrl+S — Save.
//!   * Ctrl+Z — Undo.
//!   * Ctrl+Y / Ctrl+Shift+Z — Redo.
//!   * Ctrl+D — Duplicate selected node.
//!   * Delete — Delete selected node.
//!   * F — Fit to view.
//!   * Esc — Deselect.
//!
//! **Коалесцирование Undo:**
//! Текстовые поля (`speaker` / `text` / опции / эффекты) не пушат
//! undo на каждый символ — только при потере фокуса полем. Discrete
//! операции (add/delete/duplicate node, delete option/effect) пушат
//! undo сразу.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::game::dialogs::{
    DialogEffect, DialogGraph, DialogNode, DialogOption,
};

const MAX_UNDO: usize = 50;

// ============================================================
// State
// ============================================================

pub struct DialogEditorState {
    pub open: bool,
    pub available: Vec<PathBuf>,
    pub current: Option<(PathBuf, DialogGraph)>,
    pub error: Option<String>,
    pub node_positions: HashMap<String, (f32, f32)>,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub selected: Option<String>,
    pub dragging: Option<String>,
    pub dirty: bool,
    pub add_node_at: Option<(f32, f32)>,
    pub status: Option<(String, f32)>,

    // === Undo / Redo ===
    pub undo_stack: VecDeque<Snapshot>,
    pub redo_stack: Vec<Snapshot>,
    /// Позиция ноды до drag — чтобы запушить undo один раз на drag_stop.
    drag_start_pos: Option<(String, (f32, f32))>,
}

#[derive(Clone)]
pub struct Snapshot {
    pub graph: DialogGraph,
    pub positions: HashMap<String, (f32, f32)>,
    pub selected: Option<String>,
}

impl Default for DialogEditorState {
    fn default() -> Self {
        Self {
            open: false,
            available: scan_dialog_files(),
            current: None,
            error: None,
            node_positions: HashMap::new(),
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            selected: None,
            dragging: None,
            dirty: false,
            add_node_at: None,
            status: None,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            drag_start_pos: None,
        }
    }
}

impl DialogEditorState {
    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), 3.0));
    }

    /// Зафиксировать текущее состояние в undo-стеке. Очищает redo.
    /// Вызывать **перед** мутацией (add/delete/edit).
    fn push_undo(&mut self) {
        let Some((_, graph)) = &self.current else { return };
        if self.undo_stack.len() >= MAX_UNDO {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(Snapshot {
            graph: graph.clone(),
            positions: self.node_positions.clone(),
            selected: self.selected.clone(),
        });
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        let Some(snap) = self.undo_stack.pop_back() else { return };
        let Some((_, graph)) = &self.current else { return };
        // Текущее — в redo.
        self.redo_stack.push(Snapshot {
            graph: graph.clone(),
            positions: self.node_positions.clone(),
            selected: self.selected.clone(),
        });
        // Восстановить.
        if let Some((_, g)) = &mut self.current {
            *g = snap.graph;
        }
        self.node_positions = snap.positions;
        self.selected = snap.selected;
        self.dirty = true;
        self.set_status("Undo");
    }

    fn redo(&mut self) {
        let Some(snap) = self.redo_stack.pop() else { return };
        let Some((_, graph)) = &self.current else { return };
        if self.undo_stack.len() >= MAX_UNDO {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(Snapshot {
            graph: graph.clone(),
            positions: self.node_positions.clone(),
            selected: self.selected.clone(),
        });
        if let Some((_, g)) = &mut self.current {
            *g = snap.graph;
        }
        self.node_positions = snap.positions;
        self.selected = snap.selected;
        self.dirty = true;
        self.set_status("Redo");
    }
}

pub fn scan_dialog_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("assets/dialogs") {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("ron")
                && !p.to_string_lossy().ends_with(".layout.ron")
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

// ============================================================
// Layout persistence
// ============================================================

#[derive(Serialize, Deserialize, Default)]
struct DialogLayout {
    positions: HashMap<String, (f32, f32)>,
}

fn layout_path_for(graph_path: &PathBuf) -> PathBuf {
    let stem = graph_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dialog");
    let parent = graph_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    parent.join(format!("{}.layout.ron", stem))
}

// ============================================================
// Окно
// ============================================================

pub fn draw_window(ctx: &egui::Context, state: &mut DialogEditorState) {
    if !state.open {
        return;
    }
    let mut open = state.open;

    let title = if state.dirty {
        "💬 Dialog Editor *"
    } else {
        "💬 Dialog Editor"
    };

    egui::Window::new(title)
        .open(&mut open)
        .default_size([1280.0, 800.0])
        .resizable(true)
        .show(ctx, |ui| {
            handle_hotkeys(ui, state);
            draw_toolbar(ui, state);
            ui.separator();

            if state.current.is_some() {
                egui::containers::panel::Panel::right("dialog_props")
                    .resizable(true)
                    .default_size(340.0)
                    .show(ui, |ui| {
                        draw_properties(ui, state);
                    });
            }

            draw_graph(ui, state);

            if let Some((msg, ttl)) = &mut state.status {
                *ttl -= ui.input(|i| i.stable_dt);
                if *ttl > 0.0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(140, 200, 140),
                        format!("✓ {}", msg),
                    );
                } else {
                    state.status = None;
                }
            }
        });

    state.open = open;
}

// ============================================================
// Hotkeys
// ============================================================

fn handle_hotkeys(ui: &egui::Ui, state: &mut DialogEditorState) {
    let (ctrl_s, ctrl_z, ctrl_y, ctrl_shift_z, ctrl_d, del, fit, esc) = ui.input(|i| {
        let cmd = i.modifiers.command;
        let shift = i.modifiers.shift;
        (
            i.key_pressed(egui::Key::S) && cmd && !shift,
            i.key_pressed(egui::Key::Z) && cmd && !shift,
            i.key_pressed(egui::Key::Y) && cmd && !shift,
            i.key_pressed(egui::Key::Z) && cmd && shift,
            i.key_pressed(egui::Key::D) && cmd,
            i.key_pressed(egui::Key::Delete),
            i.key_pressed(egui::Key::F) && i.modifiers.is_none(),
            i.key_pressed(egui::Key::Escape),
        )
    });

    if ctrl_s {
        save_current(state);
    }
    if ctrl_z {
        state.undo();
    }
    if ctrl_y || ctrl_shift_z {
        state.redo();
    }
    if ctrl_d {
        if let Some(id) = state.selected.clone() {
            duplicate_node(state, &id);
        }
    }
    if del {
        if let Some(id) = state.selected.clone() {
            delete_node(state, &id);
        }
    }
    if fit && state.current.is_some() {
        fit_to_view(state);
    }
    if esc {
        state.selected = None;
    }
}

// ============================================================
// Toolbar
// ============================================================

fn draw_toolbar(ui: &mut egui::Ui, state: &mut DialogEditorState) {
    ui.horizontal(|ui| {
        ui.label("Dialog:");

        let current_name = state
            .current
            .as_ref()
            .map(|(p, _)| file_name_str(p))
            .unwrap_or_else(|| "(none)".to_string());

        let mut to_load: Option<PathBuf> = None;
        egui::ComboBox::from_id_salt("dialog_selector")
            .selected_text(&current_name)
            .width(220.0)
            .show_ui(ui, |ui| {
                let files = state.available.clone();
                for path in files {
                    let name = file_name_str(&path);
                    let is_sel = current_name == name;
                    if ui.selectable_label(is_sel, &name).clicked() {
                        to_load = Some(path);
                    }
                }
            });

        if let Some(path) = to_load {
            load_dialog(state, &path);
        }

        if ui.button("⟳").on_hover_text("Rescan assets/dialogs/").clicked() {
            state.available = scan_dialog_files();
        }

        ui.separator();

        if state.current.is_some() {
            let can_undo = !state.undo_stack.is_empty();
            let can_redo = !state.redo_stack.is_empty();

            if ui
                .add_enabled(can_undo, egui::Button::new("↶ Undo").small())
                .on_hover_text("Ctrl+Z")
                .clicked()
            {
                state.undo();
            }
            if ui
                .add_enabled(can_redo, egui::Button::new("↷ Redo").small())
                .on_hover_text("Ctrl+Y")
                .clicked()
            {
                state.redo();
            }

            ui.separator();

            let save_enabled = state.dirty;
            if ui
                .add_enabled(save_enabled, egui::Button::new("💾 Save").small())
                .on_hover_text("Ctrl+S")
                .clicked()
            {
                save_current(state);
            }

            if ui
                .button("🎯 Fit")
                .on_hover_text("Fit all nodes to view (F)")
                .clicked()
            {
                fit_to_view(state);
            }

            if ui
                .button("➕ Add")
                .on_hover_text("Add a new empty node")
                .clicked()
            {
                add_new_node(state);
            }

            if ui
                .button("✖ Unload")
                .on_hover_text("Close this dialog")
                .clicked()
            {
                if state.dirty {
                    state.set_status("Unsaved changes discarded");
                }
                state.current = None;
                state.selected = None;
                state.dragging = None;
                state.node_positions.clear();
                state.dirty = false;
                state.undo_stack.clear();
                state.redo_stack.clear();
            }

            ui.separator();
            if let Some((_, g)) = &state.current {
                ui.label(
                    egui::RichText::new(format!(
                        "{} nodes · undo {} · redo {}",
                        g.nodes.len(),
                        state.undo_stack.len(),
                        state.redo_stack.len()
                    ))
                    .small()
                    .weak(),
                );
            }
        }

        if let Some(err) = &state.error {
            ui.separator();
            ui.colored_label(
                egui::Color32::from_rgb(230, 120, 120),
                format!("⚠ {}", err),
            );
        }
    });
}

fn file_name_str(p: &PathBuf) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string()
}

// ============================================================
// Load / Save
// ============================================================

fn load_dialog(state: &mut DialogEditorState, path: &PathBuf) {
    match DialogGraph::load_from_file(path) {
        Ok(g) => {
            let layout_path = layout_path_for(path);
            let positions = std::fs::read_to_string(&layout_path)
                .ok()
                .and_then(|t| ron::from_str::<DialogLayout>(&t).ok())
                .map(|l| l.positions)
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| layered_layout(&g));

            let node_count = g.nodes.len();
            state.node_positions = positions;
            state.current = Some((path.clone(), g));
            state.error = None;
            state.selected = None;
            state.dragging = None;
            state.pan = egui::Vec2::ZERO;
            state.zoom = 1.0;
            state.dirty = false;
            state.undo_stack.clear();
            state.redo_stack.clear();

            // Авто-fit для больших графов.
            if node_count >= 6 {
                fit_to_view(state);
            }

            log::info!("Dialog Editor: loaded '{}' ({} nodes)", path.display(), node_count);
        }
        Err(e) => {
            log::warn!("Dialog Editor: load failed {}: {:#}", path.display(), e);
            state.error = Some(format!("{}", e));
            state.current = None;
            state.node_positions.clear();
        }
    }
}

fn save_current(state: &mut DialogEditorState) {
    let Some((path, graph)) = &state.current else { return };
    let path_clone = path.clone();
    let node_count = graph.nodes.len();

    if let Err(e) = graph.save_to_file(path) {
        state.error = Some(format!("save graph: {}", e));
        return;
    }

    let layout = DialogLayout {
        positions: state.node_positions.clone(),
    };
    let layout_path = layout_path_for(&path_clone);
    let pretty = ron::ser::PrettyConfig::default();
    match ron::ser::to_string_pretty(&layout, pretty) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&layout_path, text) {
                state.error = Some(format!("save layout: {}", e));
                return;
            }
        }
        Err(e) => {
            state.error = Some(format!("serialize layout: {}", e));
            return;
        }
    }

    state.dirty = false;
    state.error = None;
    state.set_status(format!("Saved: {}", file_name_str(&path_clone)));
    log::info!(
        "Dialog Editor: saved '{}' ({} nodes)",
        path_clone.display(),
        node_count
    );
}

// ============================================================
// Layout / Add / Delete / Fit
// ============================================================

fn layered_layout(graph: &DialogGraph) -> HashMap<String, (f32, f32)> {
    let mut out: HashMap<String, (f32, f32)> = HashMap::new();
    if !graph.nodes.contains_key(&graph.start) {
        return out;
    }
    let mut depth: HashMap<String, u32> = HashMap::new();
    let mut order_at_depth: HashMap<u32, u32> = HashMap::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    depth.insert(graph.start.clone(), 0);
    queue.push_back(graph.start.clone());
    while let Some(node_id) = queue.pop_front() {
        let d = *depth.get(&node_id).unwrap_or(&0);
        let ord = order_at_depth.entry(d).or_insert(0);
        out.insert(node_id.clone(), (d as f32 * 320.0, *ord as f32 * 140.0));
        *ord += 1;
        if let Some(node) = graph.nodes.get(&node_id) {
            for opt in &node.options {
                if let Some(next) = &opt.next
                    && graph.nodes.contains_key(next)
                    && !depth.contains_key(next)
                {
                    depth.insert(next.clone(), d + 1);
                    queue.push_back(next.clone());
                }
            }
        }
    }
    out
}

fn fit_to_view(state: &mut DialogEditorState) {
    let Some((_, graph)) = &state.current else { return };
    if graph.nodes.is_empty() {
        return;
    }
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (id, _) in &graph.nodes {
        let Some(&(x, y)) = state.node_positions.get(id) else { continue };
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + NODE_W);
        max_y = max_y.max(y + NODE_H);
    }
    if !min_x.is_finite() {
        return;
    }
    let margin = 60.0;
    state.zoom = 1.0;
    state.pan = egui::vec2(-min_x + margin, -min_y + margin);
}

fn add_new_node(state: &mut DialogEditorState) {
    state.push_undo();

    let Some((_, graph)) = &mut state.current else { return };

    let mut n = 1;
    let id = loop {
        let candidate = format!("node_{}", n);
        if !graph.nodes.contains_key(&candidate) {
            break candidate;
        }
        n += 1;
    };

    let pos = state.add_node_at.take().unwrap_or_else(|| {
        let cx = (-state.pan.x) / state.zoom + 400.0 / state.zoom;
        let cy = (-state.pan.y) / state.zoom + 200.0 / state.zoom;
        (cx, cy)
    });

    graph.nodes.insert(
        id.clone(),
        DialogNode {
            speaker: graph.speaker_name.clone(),
            text: String::from("(new line)"),
            options: vec![DialogOption {
                text: String::from("Continue"),
                next: None,
                condition: None,
                effect: Vec::new(),
            }],
            on_enter: Vec::new(),
        },
    );
    state.node_positions.insert(id.clone(), pos);
    state.selected = Some(id);
    state.dirty = true;
    state.set_status("Node added");
}

fn delete_node(state: &mut DialogEditorState, id: &str) {
    let Some((_, graph)) = &mut state.current else { return };
    if !graph.nodes.contains_key(id) {
        return;
    }
    if id == graph.start {
        state.error = Some("Cannot delete the start node".into());
        return;
    }

    state.push_undo();
    let Some((_, graph)) = &mut state.current else { return };

    graph.nodes.remove(id);
    state.node_positions.remove(id);

    for (_, node) in graph.nodes.iter_mut() {
        for opt in &mut node.options {
            if opt.next.as_deref() == Some(id) {
                opt.next = None;
            }
        }
    }

    if state.selected.as_deref() == Some(id) {
        state.selected = None;
    }
    state.dirty = true;
    state.set_status(format!("Deleted node '{}'", id));
}

fn duplicate_node(state: &mut DialogEditorState, id: &str) {
    let Some((_, graph_src)) = &state.current else { return };
    let Some(node) = graph_src.nodes.get(id).cloned() else { return };

    state.push_undo();
    let Some((_, graph)) = &mut state.current else { return };

    let mut n = 1;
    let new_id = loop {
        let candidate = if n == 1 {
            format!("{}_copy", id)
        } else {
            format!("{}_copy{}", id, n)
        };
        if !graph.nodes.contains_key(&candidate) {
            break candidate;
        }
        n += 1;
    };

    graph.nodes.insert(new_id.clone(), node);
    let old_pos = state.node_positions.get(id).copied().unwrap_or((0.0, 0.0));
    state.node_positions.insert(new_id.clone(), (old_pos.0 + 60.0, old_pos.1 + 60.0));
    state.selected = Some(new_id);
    state.dirty = true;
    state.set_status("Node duplicated");
}

// ============================================================
// Properties Panel
// ============================================================

fn draw_properties(ui: &mut egui::Ui, state: &mut DialogEditorState) {
    ui.heading("Properties");
    ui.separator();

    let Some((_, graph)) = &state.current else { return };
    let Some(sel_id) = state.selected.clone() else {
        ui.label(egui::RichText::new("Выберите ноду на графе").weak().italics());
        return;
    };
    if !graph.nodes.contains_key(&sel_id) {
        ui.label("Node not found");
        return;
    }

    ui.label(egui::RichText::new("Node ID").strong());
    ui.code(&sel_id);
    ui.add_space(6.0);

    let mut push_undo_speaker = false;
    let mut push_undo_text = false;
    let (mut new_speaker, mut new_text) = {
        let Some((_, graph)) = &state.current else { return };
        let Some(node) = graph.nodes.get(&sel_id) else { return };
        (node.speaker.clone(), node.text.clone())
    };

    ui.label(egui::RichText::new("Speaker").strong());
    let resp_speaker = ui.text_edit_singleline(&mut new_speaker);
    if resp_speaker.gained_focus() {
    }
    if resp_speaker.lost_focus() {
        push_undo_speaker = true;
    }
    ui.add_space(6.0);

    ui.label(egui::RichText::new("Text").strong());
    let resp_text = ui.add(egui::TextEdit::multiline(&mut new_text).desired_rows(3));
    if resp_text.lost_focus() {
        push_undo_text = true;
    }
    ui.add_space(8.0);

    let speaker_changed = {
        let Some((_, graph)) = &state.current else { return };
        graph
            .nodes
            .get(&sel_id)
            .map(|n| n.speaker != new_speaker)
            .unwrap_or(false)
    };
    let text_changed = {
        let Some((_, graph)) = &state.current else { return };
        graph
            .nodes
            .get(&sel_id)
            .map(|n| n.text != new_text)
            .unwrap_or(false)
    };

    if (push_undo_speaker && speaker_changed) || (push_undo_text && text_changed) {
        state.push_undo();
    }
    if speaker_changed {
        if let Some((_, graph)) = &mut state.current {
            if let Some(n) = graph.nodes.get_mut(&sel_id) {
                n.speaker = new_speaker;
            }
        }
        state.dirty = true;
    }
    if text_changed {
        if let Some((_, graph)) = &mut state.current {
            if let Some(n) = graph.nodes.get_mut(&sel_id) {
                n.text = new_text;
            }
        }
        state.dirty = true;
    }

    // === Options ===
    let mut push_undo_options = false;
    let mut add_option = false;
    let mut delete_option: Option<usize> = None;

    {
        let Some((_, graph)) = &state.current else { return };
        let Some(node) = graph.nodes.get(&sel_id) else { return };
        let node_ids: Vec<String> = graph.nodes.keys().cloned().collect();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Options ({})", node.options.len())).strong());
            if ui.small_button("➕ Add").clicked() {
                add_option = true;
            }
        });

        // Изменения опций будем складывать в pending_changes, чтобы не
        // держать mutable borrow графа + state одновременно.
        let mut pending_opt_changes: Vec<(usize, Option<String>, Option<String>, Option<String>)> =
            Vec::new();
        // tuple: (opt_idx, new_text, new_next, new_condition)

        for (i, opt) in node.options.iter().enumerate() {
            let mut opt_text = opt.text.clone();
            let mut opt_next = opt.next.clone();
            let mut opt_cond = opt.condition.clone();

            let header = format!("{}. {}", i + 1, truncate(&opt.text, 28));
            egui::CollapsingHeader::new(header)
                .id_salt(format!("opt_{}_{}", sel_id, i))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Text").small().weak());
                    let r = ui.text_edit_multiline(&mut opt_text);
                    if r.lost_focus() {
                        // Мутация вне этого блока — через pending_opt_changes.
                    }

                    ui.label(egui::RichText::new("Next").small().weak());
                    let next_label = opt_next.as_deref().unwrap_or("(end)");
                    egui::ComboBox::from_id_salt(format!("next_{}_{}", sel_id, i))
                        .selected_text(next_label)
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(opt_next.is_none(), "(end)").clicked() {
                                opt_next = None;
                            }
                            for nid in &node_ids {
                                if nid == &sel_id {
                                    // Не даём линковать на самого себя.
                                    continue;
                                }
                                let is_sel = opt_next.as_deref() == Some(nid.as_str());
                                if ui.selectable_label(is_sel, nid).clicked() {
                                    opt_next = Some(nid.clone());
                                }
                            }
                        });

                    ui.label(egui::RichText::new("Condition").small().weak());
                    let mut cond_str = opt_cond.clone().unwrap_or_default();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut cond_str)
                                .hint_text("quest_active:hunt_wolves / gold >= 100"),
                        )
                        .lost_focus()
                    {
                        let trimmed = cond_str.trim().to_string();
                        opt_cond = if trimmed.is_empty() {
                            None
                        } else {
                            Some(cond_str)
                        };
                    }

                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Effects").small().weak());
                    ui.add_space(4.0);
                    if ui.small_button("🗑 Delete option").clicked() {
                        delete_option = Some(i);
                    }
                });

            if opt.text != opt_text || opt.next != opt_next || opt.condition != opt_cond {
                pending_opt_changes.push((i, Some(opt_text), opt_next, opt_cond));
            }
        }

        // Применить pending changes к графу.
        if !pending_opt_changes.is_empty() {
            push_undo_options = true;
            let Some((_, graph)) = &mut state.current else { return };
            if let Some(node) = graph.nodes.get_mut(&sel_id) {
                for (i, text, next, cond) in pending_opt_changes {
                    if let Some(opt) = node.options.get_mut(i) {
                        if let Some(t) = text {
                            opt.text = t;
                        }
                        opt.next = next;
                        opt.condition = cond;
                    }
                }
            }
            state.dirty = true;
        }
    }

    // Effects — вне основного цикла, потому что они тоже мутируют node.
    draw_effects_editor(ui, state, &sel_id, &mut push_undo_options);

    if push_undo_options {
        state.push_undo();
    }

    if add_option {
        state.push_undo();
        let Some((_, graph)) = &mut state.current else { return };
        if let Some(node) = graph.nodes.get_mut(&sel_id) {
            node.options.push(DialogOption {
                text: String::from("..."),
                next: None,
                condition: None,
                effect: Vec::new(),
            });
        }
        state.dirty = true;
        state.set_status("Option added");
    }

    if let Some(i) = delete_option {
        state.push_undo();
        let Some((_, graph)) = &mut state.current else { return };
        if let Some(node) = graph.nodes.get_mut(&sel_id) {
            if i < node.options.len() {
                node.options.remove(i);
            }
        }
        state.dirty = true;
        state.set_status("Option deleted");
    }
}

fn draw_effects_editor(
    ui: &mut egui::Ui,
    state: &mut DialogEditorState,
    sel_id: &str,
    dirty_flag: &mut bool,
) {
    let mut add_effect: Option<DialogEffect> = None;
    let mut remove_effect_at: Option<(usize, usize)> = None; // (opt_i, eff_i)
    let mut pending_changes: Vec<(usize, usize, DialogEffect)> = Vec::new();

    {
        let Some((_, graph)) = &state.current else { return };
        let Some(node) = graph.nodes.get(sel_id) else { return };

        for (oi, opt) in node.options.iter().enumerate() {
            if opt.effect.is_empty() {
                continue;
            }
            ui.push_id(format!("eff_{}_{}", sel_id, oi), |ui| {
                ui.label(
                    egui::RichText::new(format!("Opt {} effects:", oi + 1))
                        .small()
                        .weak(),
                );
                for (ei, effect) in opt.effect.iter().enumerate() {
                    let mut eff = effect.clone();
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                            remove_effect_at = Some((oi, ei));
                        }
                        match &mut eff {
                            DialogEffect::StartQuest(id) => {
                                ui.label("StartQuest:");
                                if ui.add(egui::TextEdit::singleline(id).desired_width(140.0)).lost_focus() {
                                    changed = true;
                                }
                            }
                            DialogEffect::CompleteQuest(id) => {
                                ui.label("CompleteQuest:");
                                if ui.add(egui::TextEdit::singleline(id).desired_width(140.0)).lost_focus() {
                                    changed = true;
                                }
                            }
                            DialogEffect::GiveItem { id, count } => {
                                ui.label("GiveItem:");
                                if ui.add(egui::TextEdit::singleline(id).desired_width(120.0)).lost_focus() {
                                    changed = true;
                                }
                                ui.label("×");
                                if ui.add(egui::DragValue::new(count).speed(1.0).range(1..=9999)).changed() {
                                    changed = true;
                                }
                            }
                            DialogEffect::GiveGold(amount) => {
                                ui.label("GiveGold:");
                                if ui.add(egui::DragValue::new(amount).speed(1.0)).changed() {
                                    changed = true;
                                }
                            }
                            DialogEffect::GiveXp(amount) => {
                                ui.label("GiveXp:");
                                if ui.add(egui::DragValue::new(amount).speed(1.0)).changed() {
                                    changed = true;
                                }
                            }
                            DialogEffect::OpenVendor(id) => {
                                ui.label("OpenVendor:");
                                if ui.add(egui::TextEdit::singleline(id).desired_width(140.0)).lost_focus() {
                                    changed = true;
                                }
                            }
                            DialogEffect::OpenCrafting(id) => {
                                ui.label("OpenCrafting:");
                                if ui.add(egui::TextEdit::singleline(id).desired_width(140.0)).lost_focus() {
                                    changed = true;
                                }
                            }
                        }
                    });
                    if changed {
                        pending_changes.push((oi, ei, eff));
                    }
                }
                ui.menu_button("➕ Add effect to this option", |ui| {
                    if ui.button("StartQuest").clicked() {
                        add_effect = Some(DialogEffect::StartQuest(String::new()));
                        ui.close();
                    }
                    if ui.button("CompleteQuest").clicked() {
                        add_effect = Some(DialogEffect::CompleteQuest(String::new()));
                        ui.close();
                    }
                    if ui.button("GiveItem").clicked() {
                        add_effect = Some(DialogEffect::GiveItem {
                            id: String::new(),
                            count: 1,
                        });
                        ui.close();
                    }
                    if ui.button("GiveGold").clicked() {
                        add_effect = Some(DialogEffect::GiveGold(100));
                        ui.close();
                    }
                    if ui.button("GiveXp").clicked() {
                        add_effect = Some(DialogEffect::GiveXp(50));
                        ui.close();
                    }
                    if ui.button("OpenVendor").clicked() {
                        add_effect = Some(DialogEffect::OpenVendor(String::new()));
                        ui.close();
                    }
                    if ui.button("OpenCrafting").clicked() {
                        add_effect = Some(DialogEffect::OpenCrafting(String::new()));
                        ui.close();
                    }
                });
            });
        }
    }

    // Применить.
    if !pending_changes.is_empty() || add_effect.is_some() || remove_effect_at.is_some() {
        state.push_undo();
        *dirty_flag = true;
        let Some((_, graph)) = &mut state.current else { return };
        let Some(node) = graph.nodes.get_mut(sel_id) else { return };

        for (oi, ei, eff) in pending_changes {
            if let Some(opt) = node.options.get_mut(oi)
                && let Some(e) = opt.effect.get_mut(ei)
            {
                *e = eff;
            }
        }
        if let Some((oi, ei)) = remove_effect_at
            && let Some(opt) = node.options.get_mut(oi)
                && ei < opt.effect.len()
            {
                opt.effect.remove(ei);
            }
        if let Some(eff) = add_effect {
            // Добавляем к последней опции (наиболее типичный сценарий).
            // Пользователь может переназначить опцию через select.
            if let Some(opt) = node.options.first_mut() {
                opt.effect.push(eff);
            }
        }
        state.dirty = true;
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// ============================================================
// Graph
// ============================================================

const NODE_W: f32 = 240.0;
const NODE_H: f32 = 60.0;

fn draw_graph(ui: &mut egui::Ui, state: &mut DialogEditorState) {
    // Проверяем наличие графа без borrow.
    if state.current.is_none() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Выберите диалог в ComboBox выше").italics().weak());
            ui.label(egui::RichText::new("Файлы читаются из assets/dialogs/*.ron").small().weak());
        });
        return;
    }

    let available = ui.available_size();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
    let rect = response.rect;

    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(18, 20, 24));

    // Сетка
    let grid_step = 40.0 * state.zoom;
    if grid_step > 8.0 {
        let offset = rect.min.to_vec2() + state.pan;
        let mut x = (offset.x % grid_step) + rect.min.x;
        while x < rect.max.x {
            painter.line_segment(
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(0.5, egui::Color32::from_rgb(28, 30, 36)),
            );
            x += grid_step;
        }
        let mut y = (offset.y % grid_step) + rect.min.y;
        while y < rect.max.y {
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                egui::Stroke::new(0.5, egui::Color32::from_rgb(28, 30, 36)),
            );
            y += grid_step;
        }
    }

    let offset = rect.min.to_vec2() + state.pan;
    let space_held = ui.input(|i| i.key_down(egui::Key::Space));

    // Hit-test + click
    let mut hovered_node: Option<String> = None;
    let mut clicked_node: Option<String> = None;
    let mut right_clicked_node: Option<String> = None;

    {
        let Some((_, graph)) = &state.current else { return };
        for (node_id, _) in &graph.nodes {
            let Some(&(nx, ny)) = state.node_positions.get(node_id) else { continue };
            let pos = egui::pos2(nx, ny) * state.zoom + offset;
            let size = egui::vec2(NODE_W * state.zoom, NODE_H * state.zoom);
            let node_rect = egui::Rect::from_min_size(pos, size);
            if ui.rect_contains_pointer(node_rect) {
                hovered_node = Some(node_id.clone());
                if response.clicked() {
                    clicked_node = Some(node_id.clone());
                }
                if response.secondary_clicked() {
                    right_clicked_node = Some(node_id.clone());
                }
            }
        }
    }

    if let Some(id) = clicked_node {
        state.selected = Some(id);
    } else if response.clicked() {
        state.selected = None;
    }

    // Drag
    if response.dragged_by(egui::PointerButton::Primary)
        && !space_held
        && state.dragging.is_none()
    {
        if let Some(node) = &hovered_node {
            state.dragging = Some(node.clone());
            state.selected = Some(node.clone());
            state.drag_start_pos = state
                .node_positions
                .get(node)
                .copied()
                .map(|p| (node.clone(), p));
        }
    }

    if response.drag_stopped() {
        if let Some(node_id) = state.dragging.take() {
            if let Some((snap_id, start_pos)) = state.drag_start_pos.take() {
                if snap_id == node_id {
                    let cur_pos = state.node_positions.get(&node_id).copied();
                    let moved = cur_pos.is_some_and(|c| {
                        (c.0 - start_pos.0).abs() > 0.5 || (c.1 - start_pos.1).abs() > 0.5
                    });
                    if moved {
                        state.node_positions.insert(node_id.clone(), start_pos);
                        state.push_undo();
                        if let Some(cur) = cur_pos {
                            state.node_positions.insert(node_id, cur);
                        }
                        state.dirty = true;
                    }
                }
            }
        }
    }

    if let Some(node_id) = &state.dragging {
        if let Some(pos) = state.node_positions.get_mut(node_id) {
            let delta = response.drag_delta() / state.zoom;
            pos.0 += delta.x;
            pos.1 += delta.y;
        }
    }

    if let Some(id) = right_clicked_node {
        state.selected = Some(id.clone());
        response.context_menu(|ui| {
            if ui.button("Duplicate (Ctrl+D)").clicked() {
                duplicate_node(state, &id);
                ui.close();
            }
            if ui.button("Delete (Del)").clicked() {
                delete_node(state, &id);
                ui.close();
            }
        });
    } else if response.secondary_clicked() {
        if let Some(mouse) = ui.input(|i| i.pointer.hover_pos()) {
            let wx = (mouse.x - rect.min.x - state.pan.x) / state.zoom;
            let wy = (mouse.y - rect.min.y - state.pan.y) / state.zoom;
            state.add_node_at = Some((wx, wy));
        }
        response.context_menu(|ui| {
            if ui.button("➕ Add node here").clicked() {
                add_new_node(state);
                ui.close();
            }
        });
    }

    // Draw connections
    {
        let Some((_, graph)) = &state.current else { return };
        for (node_id, node) in &graph.nodes {
            let Some(&(sx, sy)) = state.node_positions.get(node_id) else { continue };
            let from = egui::pos2(sx, sy) * state.zoom + offset;
            let start = egui::pos2(from.x + NODE_W * state.zoom, from.y + NODE_H * state.zoom * 0.5);
            for opt in &node.options {
                let Some(next) = &opt.next else { continue };
                let Some(&(tx, ty)) = state.node_positions.get(next) else { continue };
                let to = egui::pos2(tx, ty) * state.zoom + offset;
                let end = egui::pos2(to.x, to.y + NODE_H * state.zoom * 0.5);
                draw_bezier(&painter, start, end, egui::Color32::from_rgb(110, 130, 175));
            }
        }
    }

    // Draw nodes
    {
        let Some((_, graph)) = &state.current else { return };
        for (node_id, node) in &graph.nodes {
            let Some(&(nx, ny)) = state.node_positions.get(node_id) else { continue };
            let pos = egui::pos2(nx, ny) * state.zoom + offset;
            let size = egui::vec2(NODE_W * state.zoom, NODE_H * state.zoom);
            let node_rect = egui::Rect::from_min_size(pos, size);

            let is_start = node_id == &graph.start;
            let is_sel = state.selected.as_deref() == Some(node_id.as_str());
            let is_hov = hovered_node.as_deref() == Some(node_id.as_str());

            let fill = if is_start {
                egui::Color32::from_rgb(55, 100, 60)
            } else if is_sel {
                egui::Color32::from_rgb(60, 70, 95)
            } else if is_hov {
                egui::Color32::from_rgb(55, 60, 75)
            } else {
                egui::Color32::from_rgb(48, 52, 65)
            };
            let stroke = if is_start {
                egui::Color32::from_rgb(120, 210, 130)
            } else if is_sel {
                egui::Color32::from_rgb(230, 200, 100)
            } else if is_hov {
                egui::Color32::from_rgb(180, 200, 240)
            } else {
                egui::Color32::from_rgb(130, 150, 185)
            };

            painter.rect_filled(node_rect, 5.0, fill);
            painter.rect_stroke(
                node_rect,
                5.0,
                egui::Stroke::new(1.5, stroke),
                egui::StrokeKind::Inside,
            );
            painter.text(
                pos + egui::vec2(10.0, 7.0) * state.zoom,
                egui::Align2::LEFT_TOP,
                node_id,
                egui::FontId::proportional(13.0 * state.zoom),
                egui::Color32::WHITE,
            );
            let preview: String = node.text.chars().take(34).collect();
            painter.text(
                pos + egui::vec2(10.0, 30.0) * state.zoom,
                egui::Align2::LEFT_TOP,
                preview,
                egui::FontId::proportional(11.0 * state.zoom),
                egui::Color32::from_rgb(200, 205, 215),
            );
            if is_start {
                painter.text(
                    node_rect.right_top() + egui::vec2(-10.0, 7.0) * state.zoom,
                    egui::Align2::RIGHT_TOP,
                    "START",
                    egui::FontId::proportional(9.0 * state.zoom),
                    egui::Color32::from_rgb(180, 240, 190),
                );
            }
        }
    }

    // Pan / zoom
    let pan_drag = response.dragged_by(egui::PointerButton::Middle)
        || (response.dragged_by(egui::PointerButton::Primary) && space_held);
    if pan_drag {
        state.pan += response.drag_delta();
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.01 {
            let old = state.zoom;
            let new = (state.zoom * (1.0 + scroll * 0.0015)).clamp(0.3, 3.0);
            if let Some(mouse) = ui.input(|i| i.pointer.hover_pos()) {
                let rel = mouse - rect.min - state.pan;
                state.pan = state.pan - rel * (new / old - 1.0);
            }
            state.zoom = new;
        }
    }

    // Status
    let (nodes, dirty) = state
        .current
        .as_ref()
        .map(|(_, g)| (g.nodes.len(), state.dirty))
        .unwrap_or((0, false));
    let info = format!(
        "Nodes: {}  ·  Zoom: {:.0}%  ·  undo {} · redo {}  ·  {}",
        nodes,
        state.zoom * 100.0,
        state.undo_stack.len(),
        state.redo_stack.len(),
        if dirty { "UNSAVED" } else { "saved" },
    );
    painter.text(
        rect.right_bottom() + egui::vec2(-12.0, -10.0),
        egui::Align2::RIGHT_BOTTOM,
        info,
        egui::FontId::proportional(11.0),
        if dirty {
            egui::Color32::from_rgb(240, 200, 120)
        } else {
            egui::Color32::from_rgb(140, 150, 170)
        },
    );
    painter.text(
        rect.left_bottom() + egui::vec2(12.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        "ЛКМ — select/drag · MMB/Space+LMB — pan · Scroll — zoom · Ctrl+Z — undo · Ctrl+D — dup · Del — delete",
        egui::FontId::proportional(10.0),
        egui::Color32::from_rgb(110, 120, 140),
    );
}

fn draw_bezier(painter: &egui::Painter, from: egui::Pos2, to: egui::Pos2, color: egui::Color32) {
    let dx = (to.x - from.x).abs() * 0.5;
    let ctrl1 = egui::pos2(from.x + dx, from.y);
    let ctrl2 = egui::pos2(to.x - dx, to.y);
    let mut points = Vec::with_capacity(21);
    for i in 0..=20 {
        let t = i as f32 / 20.0;
        points.push(cubic_bezier(from, ctrl1, ctrl2, to, t));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
}

fn cubic_bezier(p0: egui::Pos2, p1: egui::Pos2, p2: egui::Pos2, p3: egui::Pos2, t: f32) -> egui::Pos2 {
    let mt = 1.0 - t;
    let x = mt * mt * mt * p0.x + 3.0 * mt * mt * t * p1.x + 3.0 * mt * t * t * p2.x + t * t * t * p3.x;
    let y = mt * mt * mt * p0.y + 3.0 * mt * mt * t * p1.y + 3.0 * mt * t * t * p2.y + t * t * t * p3.y;
    egui::pos2(x, y)
}