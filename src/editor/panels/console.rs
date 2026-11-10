use crate::editor::EditorState;
use crate::engine::logger::{LogBuffer, LogLevel};

pub fn draw(ui: &mut egui::Ui, _state: &mut EditorState) {
    let logs = LogBuffer::global().snapshot();
    let errors = logs.iter().filter(|e| e.level == LogLevel::Error).count();
    let warnings = logs.iter().filter(|e| e.level == LogLevel::Warn).count();
    let infos = logs.iter().filter(|e| e.level == LogLevel::Info).count();

    ui.horizontal(|ui| {
        ui.heading("Console");
        ui.add_space(10.0);
        
        if ui.button("Clear").clicked() {
            LogBuffer::global().clear();
        }
        // TODO: Collapse и Error Pause
        if ui.button("Collapse").clicked() {}
        if ui.button("Error Pause").clicked() {}
        
        ui.separator();
        ui.label("Editor");
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("{}", infos)).color(egui::Color32::GRAY));
            ui.label("ℹ");
            ui.label(egui::RichText::new(format!("{}", warnings)).color(egui::Color32::YELLOW));
            ui.label("⚠");
            ui.label(egui::RichText::new(format!("{}", errors)).color(egui::Color32::RED));
            ui.label("⛔");
        });
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .stick_to_bottom(true) // Автоскролл к последнему сообщению
        .show(ui, |ui| {
            if logs.is_empty() {
                ui.label(egui::RichText::new("No logs yet...").weak().italics());
                return;
            }

            for entry in &logs {
                let (prefix, color) = match entry.level {
                    LogLevel::Error => ("⛔", egui::Color32::RED),
                    LogLevel::Warn => ("⚠", egui::Color32::YELLOW),
                    LogLevel::Info => ("ℹ", egui::Color32::LIGHT_BLUE),
                    LogLevel::Debug => ("🐛", egui::Color32::GRAY),
                    LogLevel::Trace => ("", egui::Color32::DARK_GRAY),
                };

                let text = format!(
                    "{} [{}] {} at {}:{}",
                    prefix, entry.target, entry.message, entry.file, entry.line
                );
                
                // Используем selectable_label для возможности скопировать текст (в egui 0.37 есть)
                ui.label(egui::RichText::new(text).color(color));
            }
        });
}