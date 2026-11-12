use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use crate::game::dialogs::DialogGraph;

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
        }
    }
}

/// Сканирует `assets/dialogs/*.ron` (все языковые варианты — `.en.ron` тоже).
pub fn scan_dialog_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("assets/dialogs") {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("ron") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

// ============================================================
// Публичный вход
// ============================================================

pub fn draw_window(ctx: &egui::Context, state: &mut DialogEditorState) {
    if !state.open {
        return;
    }
    let mut open = state.open;
    egui::Window::new("💬 Dialog Editor")
        .open(&mut open)
        .default_size([960.0, 720.0])
        .resizable(true)
        .show(ctx, |ui| {
            draw_header(ui, state);
            ui.separator();
            draw_graph(ui, state);
        });
    state.open = open;
}

// ============================================================
// Header
// ============================================================

fn draw_header(ui: &mut egui::Ui, state: &mut DialogEditorState) {
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
            .width(260.0)
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

        if ui
            .button("⟳ Refresh list")
            .on_hover_text("Rescan assets/dialogs/")
            .clicked()
        {
            state.available = scan_dialog_files();
        }

        if state.current.is_some() && ui.button("✖ Unload").clicked() {
            state.current = None;
            state.selected = None;
            state.node_positions.clear();
        }

        ui.separator();
        if let Some((_, g)) = &state.current {
            ui.label(
                egui::RichText::new(format!("{} nodes", g.nodes.len()))
                    .small()
                    .weak(),
            );
        }
    });

    if let Some(err) = &state.error {
        ui.colored_label(egui::Color32::from_rgb(230, 120, 120), format!("⚠ {}", err));
    }
}

fn file_name_str(p: &PathBuf) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string()
}

// ============================================================
// Load + layout
// ============================================================

fn load_dialog(state: &mut DialogEditorState, path: &PathBuf) {
    match DialogGraph::load_from_file(path) {
        Ok(g) => {
            state.node_positions = layered_layout(&g);
            state.current = Some((path.clone(), g));
            state.error = None;
            state.selected = None;
            state.pan = egui::Vec2::ZERO;
            state.zoom = 1.0;
            log::info!("Dialog Editor: loaded '{}'", path.display());
        }
        Err(e) => {
            log::warn!("Dialog Editor: load failed {}: {:#}", path.display(), e);
            state.error = Some(format!("{}", e));
            state.current = None;
            state.node_positions.clear();
        }
    }
}

/// BFS-layered layout:
///   * node.depth  = длина кратчайшего пути от start (X-координата);
///   * node.order  = порядок обнаружения в BFS на этом depth (Y-координата).
///
/// Достаточно для читаемого графа без внешних библиотек. Фаза 4 заменит
/// на ручные позиции из `elder.layout.ron` (после drag-n-drop).
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

        out.insert(
            node_id.clone(),
            (d as f32 * 320.0, *ord as f32 * 140.0),
        );
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

// ============================================================
// Graph drawing
// ============================================================

const NODE_W: f32 = 240.0;
const NODE_H: f32 = 60.0;

fn draw_graph(ui: &mut egui::Ui, state: &mut DialogEditorState) {
    let Some((_, graph)) = state.current.as_ref() else {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Выберите диалог в ComboBox выше")
                    .italics()
                    .weak(),
            );
            ui.label(
                egui::RichText::new(
                    "Файлы читаются из assets/dialogs/*.ron",
                )
                .small()
                .weak(),
            );
        });
        return;
    };

    let available = ui.available_size();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
    let rect = response.rect;

    // Фон + сетка.
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(18, 20, 24));

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

    // === Pass 1: connections (под нодами) ===
    for (node_id, node) in &graph.nodes {
        let Some(&(sx, sy)) = state.node_positions.get(node_id) else {
            continue;
        };
        let from = egui::pos2(sx, sy) * state.zoom + offset;
        let start = egui::pos2(
            from.x + NODE_W * state.zoom,
            from.y + NODE_H * state.zoom * 0.5,
        );

        for opt in &node.options {
            let Some(next) = &opt.next else { continue };
            let Some(&(tx, ty)) = state.node_positions.get(next) else {
                continue;
            };
            let to = egui::pos2(tx, ty) * state.zoom + offset;
            let end = egui::pos2(to.x, to.y + NODE_H * state.zoom * 0.5);
            draw_bezier(
                &painter,
                start,
                end,
                egui::Color32::from_rgb(110, 130, 175),
            );
        }
    }

    // === Pass 2: nodes (поверх связей) ===
    for (node_id, node) in &graph.nodes {
        let Some(&(nx, ny)) = state.node_positions.get(node_id) else {
            continue;
        };
        let pos = egui::pos2(nx, ny) * state.zoom + offset;
        let size = egui::vec2(NODE_W * state.zoom, NODE_H * state.zoom);
        let node_rect = egui::Rect::from_min_size(pos, size);

        let is_start = node_id == &graph.start;
        let is_selected = state.selected.as_deref() == Some(node_id.as_str());

        let fill = if is_start {
            egui::Color32::from_rgb(55, 100, 60)
        } else if is_selected {
            egui::Color32::from_rgb(60, 70, 95)
        } else {
            egui::Color32::from_rgb(48, 52, 65)
        };

        let stroke_color = if is_start {
            egui::Color32::from_rgb(120, 210, 130)
        } else if is_selected {
            egui::Color32::from_rgb(230, 200, 100)
        } else {
            egui::Color32::from_rgb(130, 150, 185)
        };

        painter.rect_filled(node_rect, 5.0, fill);
        painter.rect_stroke(
            node_rect,
            5.0,
            egui::Stroke::new(1.5, stroke_color),
            egui::StrokeKind::Inside,
        );

        // ID (сверху).
        painter.text(
            pos + egui::vec2(10.0, 7.0) * state.zoom,
            egui::Align2::LEFT_TOP,
            node_id,
            egui::FontId::proportional(13.0 * state.zoom),
            egui::Color32::WHITE,
        );

        // Preview текста (снизу, обрезан).
        let preview: String = node.text.chars().take(34).collect();
        painter.text(
            pos + egui::vec2(10.0, 30.0) * state.zoom,
            egui::Align2::LEFT_TOP,
            preview,
            egui::FontId::proportional(11.0 * state.zoom),
            egui::Color32::from_rgb(200, 205, 215),
        );

        // Маркер start-ноды.
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

    let space_held = ui.input(|i| i.key_down(egui::Key::Space));
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
            // Зум «вокруг курсора»: сохраняем точку под мышью.
            if let Some(mouse) = ui.input(|i| i.pointer.hover_pos()) {
                let rel = mouse - rect.min - state.pan;
                state.pan = state.pan - rel * (new / old - 1.0);
            }
            state.zoom = new;
        }
    }

    // === Status bar ===
    let info = format!(
        "Nodes: {}  ·  Zoom: {:.0}%  ·  Pan: ({:.0}, {:.0})",
        graph.nodes.len(),
        state.zoom * 100.0,
        state.pan.x,
        state.pan.y,
    );
    painter.text(
        rect.right_bottom() + egui::vec2(-12.0, -10.0),
        egui::Align2::RIGHT_BOTTOM,
        info,
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(140, 150, 170),
    );

    // Хинт по управлению.
    painter.text(
        rect.left_bottom() + egui::vec2(12.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        "MMB / Space+LMB — pan  ·  Scroll — zoom",
        egui::FontId::proportional(10.0),
        egui::Color32::from_rgb(110, 120, 140),
    );
}

// ============================================================
// Bezier
// ============================================================

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

fn cubic_bezier(
    p0: egui::Pos2,
    p1: egui::Pos2,
    p2: egui::Pos2,
    p3: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    let mt = 1.0 - t;
    let x = mt * mt * mt * p0.x
        + 3.0 * mt * mt * t * p1.x
        + 3.0 * mt * t * t * p2.x
        + t * t * t * p3.x;
    let y = mt * mt * mt * p0.y
        + 3.0 * mt * mt * t * p1.y
        + 3.0 * mt * t * t * p2.y
        + t * t * t * p3.y;
    egui::pos2(x, y)
}