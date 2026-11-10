use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Classic,
    Modern,
}

impl Theme {
    pub const ALL: &'static [Theme] =
        &[Theme::Dark, Theme::Light, Theme::Classic, Theme::Modern];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::Classic => "Classic",
            Self::Modern => "Modern",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Dark => "Default dark theme",
            Self::Light => "Light theme (high contrast)",
            Self::Classic => "Classic gray (Windows 9x style)",
            Self::Modern => "VS Code dark+ inspired",
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        let mut visuals = match self {
            Self::Dark => {
                let mut v = egui::Visuals::dark();
                // Фон панелей (как в Unity)
                v.panel_fill = egui::Color32::from_rgb(40, 40, 40);
                v.window_fill = egui::Color32::from_rgb(45, 45, 45);
                v.extreme_bg_color = egui::Color32::from_rgb(28, 28, 28);
                v.faint_bg_color = egui::Color32::from_rgb(50, 50, 50);
                v.selection.bg_fill = egui::Color32::from_rgb(43, 93, 163);
                v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(40, 40, 40);
                v.widgets.inactive.bg_fill = egui::Color32::from_rgb(50, 50, 50);
                v.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 60, 60);
                v.widgets.active.bg_fill = egui::Color32::from_rgb(70, 70, 70);
                v
            }
            Self::Light => {
                let mut v = egui::Visuals::light();
                v.selection.bg_fill = egui::Color32::from_rgb(80, 130, 200);
                v
            }
            Self::Classic => {
                let mut v = egui::Visuals::dark();
                v.panel_fill = egui::Color32::from_rgb(60, 60, 60);
                v.window_fill = egui::Color32::from_rgb(70, 70, 70);
                v.extreme_bg_color = egui::Color32::from_rgb(50, 50, 50);
                v.selection.bg_fill = egui::Color32::from_rgb(200, 160, 60);
                v
            }
            Self::Modern => {
                let mut v = egui::Visuals::dark();
                v.panel_fill = egui::Color32::from_rgb(30, 30, 30);
                v.window_fill = egui::Color32::from_rgb(37, 37, 38);
                v.extreme_bg_color = egui::Color32::from_rgb(20, 20, 20);
                v.selection.bg_fill = egui::Color32::from_rgb(14, 99, 156);
                v.override_text_color = Some(egui::Color32::from_rgb(212, 212, 212));
                v
            }
        };

        visuals.window_corner_radius = egui::CornerRadius::same(0);
        visuals.menu_corner_radius = egui::CornerRadius::same(0);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(2);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(2);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(2);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(2);

        ctx.set_visuals(visuals);
    }
}