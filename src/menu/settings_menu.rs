//! Окно Settings: Window + Graphics + Audio + Language.

use super::MenuActions;
use crate::app_state::{
    AppSettings, AudioSettings, GraphicsSettings, QualityPreset, WindowSettings,
    RESOLUTIONS,
};
use crate::render::Tonemapper;
use crate::t;

pub fn draw(ctx: &egui::Context, settings: &mut AppSettings, actions: &mut MenuActions) {
    let screen = ctx.input(|i| i.content_rect());
    let center = screen.center();

    egui::Area::new(egui::Id::unique("settings_menu"))
        .fixed_pos(egui::pos2(center.x - 340.0, center.y - 340.0))
        .show(ctx, |ui| {
            ui.set_width(680.0);
            ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(t!("settings.title"))
                        .size(32.0).strong()
                        .color(egui::Color32::from_rgb(200, 210, 230)),
                );
                ui.add_space(16.0);
            });

            egui::ScrollArea::vertical()
                .max_height(560.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    egui::CollapsingHeader::new(t!("settings.window"))
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            draw_window(ui, &mut settings.window);
                            ui.add_space(4.0);
                        });
                    ui.add_space(6.0);

                    egui::CollapsingHeader::new(t!("settings.graphics"))
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            draw_graphics(ui, &mut settings.graphics);
                            ui.add_space(4.0);
                        });
                    ui.add_space(6.0);

                    egui::CollapsingHeader::new(t!("settings.audio"))
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            draw_audio(ui, &mut settings.audio);
                            ui.add_space(4.0);
                        });
                    ui.add_space(6.0);

                    egui::CollapsingHeader::new(t!("settings.language"))
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.selectable_label(settings.language == "ru", t!("settings.language.ru")).clicked() {
                                    settings.language = "ru".to_string();
                                    crate::game::locale::set_language("ru");
                                }
                                if ui.selectable_label(settings.language == "en", t!("settings.language.en")).clicked() {
                                    settings.language = "en".to_string();
                                    crate::game::locale::set_language("en");
                                }
                            });
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("Changes are applied immediately and saved to settings.ron.")
                                .small().weak().italics());
                        });
                    ui.add_space(4.0);
                });

            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if ui.add_sized([160.0, 38.0], egui::Button::new(t!("settings.back"))).clicked() {
                    actions.back = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_sized([160.0, 38.0], egui::Button::new(t!("settings.apply"))).clicked() {
                        actions.apply_settings = true;
                    }
                });
            });
        });
}

fn draw_window(ui: &mut egui::Ui, s: &mut WindowSettings) {
    ui.checkbox(&mut s.vsync, t!("settings.vsync"))
        .on_hover_text("Sync with monitor refresh rate. Disable for uncapped FPS.");
    ui.checkbox(&mut s.fullscreen, t!("settings.fullscreen"));
    ui.horizontal(|ui| {
        ui.label(format!("{}:", t!("settings.resolution")));
        let current = RESOLUTIONS.get(s.resolution_index).map(|r| r.2).unwrap_or("?");
        egui::ComboBox::from_id_salt("res_combo")
            .selected_text(current)
            .width(220.0)
            .show_ui(ui, |ui| {
                for (i, (_, _, label)) in RESOLUTIONS.iter().enumerate() {
                    ui.selectable_value(&mut s.resolution_index, i, *label);
                }
            });
    });
}

fn draw_graphics(ui: &mut egui::Ui, s: &mut GraphicsSettings) {
    ui.label(egui::RichText::new("Quality preset").strong());
    ui.horizontal_wrapped(|ui| {
        for preset in QualityPreset::all() {
            let selected = s.quality_preset == preset;
            if ui.selectable_label(selected, preset.label())
                .on_hover_text(preset.description()).clicked()
            {
                s.apply_preset(preset);
            }
        }
    });
    ui.label(egui::RichText::new(s.quality_preset.description()).small().weak().italics());

    ui.separator();

    // Sprint 3: SSAA via render_scale
    ui.label(egui::RichText::new("Supersampling (SSAA)").strong());
    ui.horizontal_wrapped(|ui| {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let selected = (s.render_scale - scale).abs() < 0.01;
            let label = format!("{}%", (scale * 100.0) as u32);
            if ui.selectable_label(selected, label)
                .on_hover_text(match scale {
                    1.0 => "Native — без SSAA, максимальный FPS",
                    1.25 => "1.56× пикселей. Умеренный SSAA.",
                    1.5 => "2.25× пикселей. Рекомендуется для Ultra.",
                    2.0 => "4× пикселей. Только для скриншотов.",
                    _ => "",
                })
                .clicked()
            {
                s.apply_render_scale_preset(scale);
            }
        }
    });
    ui.label(egui::RichText::new(format!(
        "Внутреннее разрешение: {}% (рекомендуется для {}: {}%)",
        s.render_scale_percent(), s.quality_preset.label(),
        (s.quality_preset.recommended_render_scale() * 100.0) as u32,
    )).small().weak().italics());

    ui.separator();

    ui.add(egui::Slider::new(&mut s.fov, 60.0..=110.0).text("FOV (vertical)").suffix("°"));

    ui.separator();

    // Sprint 5: Atmosphere
    egui::CollapsingHeader::new("🌅 Atmosphere (procedural sky)")
        .default_open(true)
        .show(ui, |ui| {
            ui.checkbox(&mut s.atmosphere.enabled, "Enable atmosphere")
                .on_hover_text("Procedural sky (Rayleigh + Mie). Disable to use sky.hdr IBL.");

            ui.add_enabled_ui(s.atmosphere.enabled, |ui| {
                ui.add(
                    egui::Slider::new(&mut s.atmosphere.time_of_day, 0.0..=24.0)
                        .text("Time of day").suffix("h").fixed_decimals(1),
                );
                ui.label(
                    egui::RichText::new(match s.atmosphere.time_of_day as u32 {
                        0..=4 => "🌙 Night",
                        5..=7 => "🌅 Sunrise",
                        8..=10 => "☀ Morning",
                        11..=13 => "☀ Noon",
                        14..=16 => "☀ Afternoon",
                        17..=19 => "🌇 Sunset",
                        _ => "🌙 Evening",
                    }).small().weak().italics(),
                );
                ui.add(egui::Slider::new(&mut s.atmosphere.sun_intensity, 1.0..=60.0)
                    .text("Sun intensity").logarithmic(true));
                ui.add(egui::Slider::new(&mut s.atmosphere.turbidity, 1.0..=10.0)
                    .text("Turbidity").fixed_decimals(1));
                ui.add(egui::Slider::new(&mut s.atmosphere.rayleigh_strength, 0.1..=3.0)
                    .text("Rayleigh strength").fixed_decimals(2));
                ui.add(egui::Slider::new(&mut s.atmosphere.mie_strength, 0.0..=5.0)
                    .text("Mie strength").fixed_decimals(2));
                ui.add(egui::Slider::new(&mut s.atmosphere.horizon_haze, 0.0..=1.0)
                    .text("Horizon haze").fixed_decimals(2));
            });
        });

    ui.separator();

    // Exposure & Tone
    egui::CollapsingHeader::new("Exposure & Tone")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut s.postfx.exposure, 0.1..=3.0).text("Exposure").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut s.postfx.exposure_bias, 0.25..=4.0).logarithmic(true).text("Exposure bias").fixed_decimals(2));
            ui.horizontal(|ui| {
                ui.label("Tonemapper:");
                let tm = &mut s.postfx.tonemapper;
                for (id, label) in [
                    (Tonemapper::AgX, "AgX"), (Tonemapper::ACESFilmic, "ACES"),
                    (Tonemapper::Reinhard, "Reinhard"), (Tonemapper::Uncharted2, "Uncharted2"),
                    (Tonemapper::None, "None"),
                ] {
                    if ui.selectable_label(*tm == id, label).clicked() { *tm = id; }
                }
            });
        });

    egui::CollapsingHeader::new("Anti-aliasing").default_open(true).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut s.postfx.taa_strength, 0.0..=1.0).text("TAA strength").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.taa_sharpening, 0.0..=1.0).text("TAA sharpening").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.fxaa_strength, 0.0..=1.0).text("FXAA strength").fixed_decimals(2));
    });

    egui::CollapsingHeader::new("Ambient occlusion (SSAO)").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut s.postfx.ssao_strength, 0.0..=2.0).text("Strength").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.ssao_radius, 0.05..=2.0).text("Radius").fixed_decimals(2));
    });

    egui::CollapsingHeader::new("Bloom").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut s.postfx.bloom_threshold, 0.1..=5.0).text("Threshold").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.bloom_strength, 0.0..=3.0).text("Strength").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.bloom_knee, 0.01..=2.0).text("Knee").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.bloom_radius, 0.5..=3.0).text("Radius").fixed_decimals(2));
    });

    egui::CollapsingHeader::new("Volumetric fog").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut s.postfx.volumetric_density, 0.0..=0.02).logarithmic(true).text("Density").fixed_decimals(4));
        ui.add(egui::Slider::new(&mut s.postfx.volumetric_scattering, 0.0..=1.0).text("Scattering").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.volumetric_phase_g, 0.0..=0.9).text("Phase g").fixed_decimals(2));
    });

    egui::CollapsingHeader::new("Height fog").default_open(false).show(ui, |ui| {
        let mut c = s.postfx.fog_color;
        if ui.color_edit_button_rgb(&mut c).changed() { s.postfx.fog_color = c; }
        ui.add(egui::Slider::new(&mut s.postfx.fog_density, 0.0..=0.2).text("Density").fixed_decimals(4));
        ui.add(egui::Slider::new(&mut s.postfx.fog_height_base, -10.0..=20.0).text("Height base").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.fog_height_falloff, 0.0..=0.5).text("Height falloff").fixed_decimals(3));
    });

    egui::CollapsingHeader::new("Depth of Field").default_open(false).show(ui, |ui| {
        let mut enabled = s.postfx.dof_enabled > 0.5;
        if ui.checkbox(&mut enabled, "Enable").changed() {
            s.postfx.dof_enabled = if enabled { 1.0 } else { 0.0 };
        }
        ui.add_enabled_ui(enabled, |ui| {
            ui.add(egui::Slider::new(&mut s.postfx.dof_focus_distance, 0.5..=100.0).logarithmic(true).text("Focus distance (m)").fixed_decimals(1));
            ui.add(egui::Slider::new(&mut s.postfx.dof_focus_range, 0.0..=30.0).text("Focus range (m)").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut s.postfx.dof_max_blur, 0.0..=32.0).text("Max blur (px)").fixed_decimals(1));
        });
    });

    egui::CollapsingHeader::new("Motion blur").default_open(false).show(ui, |ui| {
        let mut enabled = s.postfx.motion_blur_enabled > 0.5;
        if ui.checkbox(&mut enabled, "Enable").changed() {
            s.postfx.motion_blur_enabled = if enabled { 1.0 } else { 0.0 };
        }
        ui.add_enabled_ui(enabled, |ui| {
            ui.add(egui::Slider::new(&mut s.postfx.motion_blur_intensity, 0.0..=2.0).text("Intensity").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut s.postfx.motion_blur_max_px, 4.0..=120.0).logarithmic(true).text("Max blur length (px)").fixed_decimals(1));
        });
    });

    egui::CollapsingHeader::new("Lens flare").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut s.postfx.lens_flare_intensity, 0.0..=1.5).text("Intensity").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.lens_flare_threshold, 0.5..=8.0).logarithmic(true).text("Threshold").fixed_decimals(2));
    });

    egui::CollapsingHeader::new("Screen effects").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut s.postfx.vignette_strength, 0.0..=1.0).text("Vignette").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.film_grain, 0.0..=1.0).text("Film grain").fixed_decimals(3));
        ui.add(egui::Slider::new(&mut s.postfx.chromatic_aberration, 0.0..=1.0).text("Chromatic aberration").fixed_decimals(3));
    });

    egui::CollapsingHeader::new("Color grading").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut s.postfx.color_temperature, -1.0..=1.0).text("Temperature").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.color_tint, -1.0..=1.0).text("Tint").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.color_contrast, 0.5..=2.0).text("Contrast").fixed_decimals(2));
        ui.add(egui::Slider::new(&mut s.postfx.color_saturation, 0.0..=2.0).text("Saturation").fixed_decimals(2));
    });
}

fn draw_audio(ui: &mut egui::Ui, s: &mut AudioSettings) {
    ui.add(egui::Slider::new(&mut s.master, 0.0..=1.0).text(t!("settings.master")));
    ui.add(egui::Slider::new(&mut s.sfx, 0.0..=1.0).text(t!("settings.sfx")));
    ui.add(egui::Slider::new(&mut s.music, 0.0..=1.0).text(t!("settings.music")));
    ui.add(egui::Slider::new(&mut s.voice, 0.0..=1.0).text(t!("settings.voice")));
    ui.add(egui::Slider::new(&mut s.ui, 0.0..=1.0).text(t!("settings.ui")));
}