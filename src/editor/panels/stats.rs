use crate::editor::panels::Stats;

pub fn draw_section(ui: &mut egui::Ui, stats: &Stats) {
    ui.heading("Stats");
    ui.separator();
    ui.label(format!("FPS: {:.1}", stats.fps));
    ui.label(format!(
        "Frame: {:.2} ms",
        if stats.fps > 0.1 { 1000.0 / stats.fps } else { 0.0 }
    ));
    ui.label(format!("Frame max: {:.2} ms", stats.frame_time_max_ms));
    ui.label(format!("Hitches: {}", stats.hitches));
    ui.separator();
    ui.label(format!("Entities: {}", stats.entities));
    ui.label(format!("Draws: {}", stats.draws));
    ui.label(format!("Instances: {}", stats.instances));
    ui.separator();
    ui.label(format!("Dir lights: {}", stats.dir_lights));
    ui.label(format!("Point lights: {}", stats.point_lights));
    ui.separator();
    ui.label("Selection");
    ui.label(format!("Entities: {}", stats.sel_entities));
    ui.label(format!("Triangles (LOD0): {}", stats.sel_triangles));
    ui.label(format!("Vertices (LOD0): {}", stats.sel_vertices));
    ui.separator();
    ui.label("LOD");
    ui.label(format!(
        "LOD0: {} · LOD1: {} · LOD2: {} · LOD3: {}",
        stats.lod_counts[0], stats.lod_counts[1],
        stats.lod_counts[2], stats.lod_counts[3]
    ));
}