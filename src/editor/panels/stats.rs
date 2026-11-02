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

    // === CPU budget (Этап 0) ===
    ui.separator();
    ui.label(egui::RichText::new("CPU budget").strong());
    let total = stats.total_cpu_ms.max(0.001);
    let rows: [(&str, f32); 7] = [
        ("Update  ", stats.update_ms),
        ("Physics ", stats.physics_ms),
        ("AI      ", stats.ai_ms),
        ("Scripts ", stats.scripts_ms),
        ("Collect ", stats.collect_ms),
        ("Render  ", stats.render_ms),
        ("Egui    ", stats.egui_ms),
    ];
    for (name, ms) in rows {
        let pct = (ms / total * 100.0).clamp(0.0, 999.0);
        let bar_w = 100.0;
        let frac = (ms / total).clamp(0.0, 1.0);
        let color = if pct > 40.0 {
            egui::Color32::from_rgb(230, 100, 100)
        } else if pct > 20.0 {
            egui::Color32::from_rgb(230, 200, 100)
        } else {
            egui::Color32::from_rgb(100, 200, 130)
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(name).monospace().small());
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(bar_w)
                    .fill(color)
                    .text(format!("{:.2} ms", ms)),
            );
            ui.label(
                egui::RichText::new(format!("{:>4.0}%", pct))
                    .monospace()
                    .small()
                    .weak(),
            );
        });
    }
    ui.label(
        egui::RichText::new(format!("Σ CPU ≈ {:.2} ms", stats.total_cpu_ms))
            .monospace()
            .strong(),
    );

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