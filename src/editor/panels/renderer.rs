//! Renderer panel + play/fly settings.
//!
//! Перенесено из `editor/ui.rs`. Содержит:
//! * `draw(ui, postfx, editor, action)` — бывший `draw_renderer_panel`.
//! * `preset_*` — quality/style presets (Low/Medium/High/Ultra,
//!   RAGE Cinematic / Desert Storm / Night Urban).
//! * `draw_play_settings`, `draw_fly_settings` — секции в конце.

use crate::editor::{EditorAction, EditorState};
use crate::render::{DebugView, PostFx, Tonemapper};

pub fn draw(
    ui: &mut egui::Ui,
    postfx: &mut PostFx,
    editor: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    egui::CollapsingHeader::new("AI / Navmesh")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("🔨 Bake Navmesh").clicked() {
                    *action = Some(EditorAction::BakeNavmesh);
                }
            });
            ui.checkbox(&mut editor.settings.show_navmesh, "Show navmesh");
        });

    ui.separator();
    ui.label(egui::RichText::new("Quality preset").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Low").clicked() { apply_preset(postfx, preset_low()); }
        if ui.button("Medium").clicked() { apply_preset(postfx, preset_medium()); }
        if ui.button("High").clicked() { apply_preset(postfx, preset_high()); }
        if ui.button("Ultra").clicked() { apply_preset(postfx, preset_ultra()); }
        if ui.button("Reset").clicked() { apply_preset(postfx, PostFx::default()); }
    });

    ui.separator();
    ui.label(egui::RichText::new("Style preset").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("🎬 RAGE Cinematic").clicked() { apply_preset(postfx, preset_rage_cinematic()); }
        if ui.button("🌫 Desert Storm").clicked() { apply_preset(postfx, preset_desert_storm()); }
        if ui.button("🌆 Night Urban").clicked() { apply_preset(postfx, preset_night_urban()); }
    });

    ui.separator();
    egui::CollapsingHeader::new("Anti-aliasing")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.taa_strength, 0.0..=1.0).text("TAA strength"));
            ui.add(egui::Slider::new(&mut postfx.taa_sharpening, 0.0..=1.0).text("TAA sharpening"));
            ui.add(egui::Slider::new(&mut postfx.fxaa_strength, 0.0..=1.0).text("FXAA strength"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Post-processing")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.bloom_threshold, 0.1..=5.0).text("Bloom threshold"));
            ui.add(egui::Slider::new(&mut postfx.bloom_strength, 0.0..=3.0).text("Bloom strength"));
            ui.add(egui::Slider::new(&mut postfx.bloom_knee, 0.01..=2.0).text("Bloom knee"));
            ui.add(egui::Slider::new(&mut postfx.bloom_radius, 0.5..=3.0).text("Bloom radius"));
            ui.add(egui::Slider::new(&mut postfx.exposure, 0.1..=3.0).text("Exposure"));
        });

    ui.separator();
    egui::CollapsingHeader::new("🎨 Color grading")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Tonemapper:");
                let tm = &mut postfx.tonemapper;
                if ui.selectable_label(*tm == Tonemapper::AgX, "AgX").clicked() { *tm = Tonemapper::AgX; }
                if ui.selectable_label(*tm == Tonemapper::ACESFilmic, "ACES").clicked() { *tm = Tonemapper::ACESFilmic; }
                if ui.selectable_label(*tm == Tonemapper::Reinhard, "Reinhard").clicked() { *tm = Tonemapper::Reinhard; }
                if ui.selectable_label(*tm == Tonemapper::Uncharted2, "Uncharted2").clicked() { *tm = Tonemapper::Uncharted2; }
                if ui.selectable_label(*tm == Tonemapper::None, "None").clicked() { *tm = Tonemapper::None; }
            });
            ui.separator();
            ui.add(egui::Slider::new(&mut postfx.exposure_bias, 0.25..=4.0).logarithmic(true).text("Exposure bias"));
            ui.add(egui::Slider::new(&mut postfx.color_temperature, -1.0..=1.0).text("Temperature"));
            ui.add(egui::Slider::new(&mut postfx.color_tint, -1.0..=1.0).text("Tint"));
            ui.add(egui::Slider::new(&mut postfx.color_contrast, 0.5..=2.0).text("Contrast"));
            ui.add(egui::Slider::new(&mut postfx.color_saturation, 0.0..=2.0).text("Saturation"));
            ui.separator();
            ui.label(egui::RichText::new("ASC CDL").strong());
            ui.horizontal(|ui| {
                ui.label("Lift:");
                for i in 0..3 {
                    ui.add(
                        egui::DragValue::new(&mut postfx.color_lift[i])
                            .speed(0.005)
                            .range(-0.2..=0.2)
                            .prefix(["R ", "G ", "B "][i]),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Gain:");
                for i in 0..3 {
                    ui.add(
                        egui::DragValue::new(&mut postfx.color_gain[i])
                            .speed(0.005)
                            .range(0.0..=2.0)
                            .prefix(["R ", "G ", "B "][i]),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Gamma:");
                for i in 0..3 {
                    ui.add(
                        egui::DragValue::new(&mut postfx.color_gamma[i])
                            .speed(0.005)
                            .range(0.1..=3.0)
                            .prefix(["R ", "G ", "B "][i]),
                    );
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Reset grading").clicked() {
                    postfx.exposure_bias = 1.0;
                    postfx.color_temperature = 0.0;
                    postfx.color_tint = 0.0;
                    postfx.color_contrast = 1.0;
                    postfx.color_saturation = 1.0;
                    postfx.color_lift = [0.0, 0.0, 0.0];
                    postfx.color_gain = [1.0, 1.0, 1.0];
                    postfx.color_gamma = [1.0, 1.0, 1.0];
                }
                if ui.button("Cinematic").clicked() {
                    postfx.tonemapper = Tonemapper::ACESFilmic;
                    postfx.exposure_bias = 1.0;
                    postfx.color_temperature = 0.15;
                    postfx.color_tint = 0.0;
                    postfx.color_contrast = 1.15;
                    postfx.color_saturation = 1.05;
                    postfx.color_lift = [0.0, -0.005, 0.01];
                    postfx.color_gain = [1.03, 1.0, 0.97];
                    postfx.color_gamma = [1.0, 1.0, 1.02];
                }
                if ui.button("Cold horror").clicked() {
                    postfx.tonemapper = Tonemapper::AgX;
                    postfx.exposure_bias = 0.9;
                    postfx.color_temperature = -0.25;
                    postfx.color_tint = -0.05;
                    postfx.color_contrast = 1.1;
                    postfx.color_saturation = 0.75;
                    postfx.color_lift = [-0.01, -0.005, 0.01];
                    postfx.color_gain = [0.97, 1.0, 1.05];
                    postfx.color_gamma = [1.02, 1.0, 0.98];
                }
                if ui.button("Sunny warm").clicked() {
                    postfx.tonemapper = Tonemapper::Uncharted2;
                    postfx.exposure_bias = 1.1;
                    postfx.color_temperature = 0.25;
                    postfx.color_tint = 0.0;
                    postfx.color_contrast = 1.05;
                    postfx.color_saturation = 1.2;
                    postfx.color_lift = [0.01, 0.005, 0.0];
                    postfx.color_gain = [1.05, 1.0, 0.95];
                    postfx.color_gamma = [1.0, 1.0, 1.0];
                }
            });
        });

    ui.separator();
    egui::CollapsingHeader::new("✨ Lens flare")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.lens_flare_intensity, 0.0..=1.5).text("Intensity"));
            ui.add(egui::Slider::new(&mut postfx.lens_flare_threshold, 0.5..=8.0).logarithmic(true).text("Threshold"));
            let mut ghosts = postfx.lens_flare_ghosts as i32;
            if ui.add(egui::Slider::new(&mut ghosts, 0..=12).text("Ghost count")).changed() {
                postfx.lens_flare_ghosts = ghosts.max(0) as u32;
            }
            ui.add(egui::Slider::new(&mut postfx.lens_flare_streak, 0.0..=0.5).text("Streak length"));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Reset").clicked() {
                    postfx.lens_flare_intensity = 0.0;
                    postfx.lens_flare_threshold = 1.5;
                    postfx.lens_flare_ghosts = 6;
                    postfx.lens_flare_streak = 0.15;
                }
                if ui.button("Cinematic").clicked() {
                    postfx.lens_flare_intensity = 0.4;
                    postfx.lens_flare_threshold = 2.0;
                    postfx.lens_flare_ghosts = 4;
                    postfx.lens_flare_streak = 0.12;
                }
            });
        });

    ui.separator();
    egui::CollapsingHeader::new("🎥 Depth of Field")
        .default_open(false)
        .show(ui, |ui| {
            let mut enabled = postfx.dof_enabled > 0.5;
            if ui.checkbox(&mut enabled, "Enable DOF").changed() {
                postfx.dof_enabled = if enabled { 1.0 } else { 0.0 };
            }
            ui.add_enabled_ui(enabled, |ui| {
                ui.add(egui::Slider::new(&mut postfx.dof_focus_distance, 0.5..=100.0).logarithmic(true).text("Focus distance (m)"));
                ui.add(egui::Slider::new(&mut postfx.dof_focus_range, 0.0..=30.0).text("Focus range (m)"));
                ui.add(egui::Slider::new(&mut postfx.dof_max_blur, 0.0..=32.0).text("Max blur (px)"));
                ui.add(egui::Slider::new(&mut postfx.dof_blur_falloff, 0.5..=20.0).logarithmic(true).text("Blur falloff"));
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Cinematic").clicked() {
                    postfx.dof_enabled = 1.0;
                    postfx.dof_focus_distance = 6.0;
                    postfx.dof_focus_range = 1.5;
                    postfx.dof_max_blur = 12.0;
                    postfx.dof_blur_falloff = 3.0;
                }
                if ui.button("Disable").clicked() {
                    postfx.dof_enabled = 0.0;
                }
            });
        });

    ui.separator();
    egui::CollapsingHeader::new("💨 Motion Blur")
        .default_open(false)
        .show(ui, |ui| {
            let mut enabled = postfx.motion_blur_enabled > 0.5;
            if ui.checkbox(&mut enabled, "Enable motion blur").changed() {
                postfx.motion_blur_enabled = if enabled { 1.0 } else { 0.0 };
            }
            ui.add_enabled_ui(enabled, |ui| {
                ui.add(egui::Slider::new(&mut postfx.motion_blur_intensity, 0.0..=2.0).text("Intensity"));
                ui.add(egui::Slider::new(&mut postfx.motion_blur_max_px, 4.0..=120.0).logarithmic(true).text("Max blur length (px)"));
                let mut samples = postfx.motion_blur_samples as i32;
                if ui.add(egui::Slider::new(&mut samples, 2..=24).text("Samples")).changed() {
                    postfx.motion_blur_samples = samples.max(2) as u32;
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Subtle").clicked() {
                    postfx.motion_blur_enabled = 1.0;
                    postfx.motion_blur_intensity = 0.3;
                    postfx.motion_blur_max_px = 16.0;
                    postfx.motion_blur_samples = 8;
                }
                if ui.button("Cinematic").clicked() {
                    postfx.motion_blur_enabled = 1.0;
                    postfx.motion_blur_intensity = 0.8;
                    postfx.motion_blur_max_px = 48.0;
                    postfx.motion_blur_samples = 14;
                }
                if ui.button("Disable").clicked() {
                    postfx.motion_blur_enabled = 0.0;
                }
            });
        });

    ui.separator();
    egui::CollapsingHeader::new("SSAO")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.ssao_strength, 0.0..=2.0).text("Strength"));
            ui.add(egui::Slider::new(&mut postfx.ssao_radius, 0.05..=2.0).text("Radius"));
        });

    ui.separator();
    egui::CollapsingHeader::new("IBL")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.ibl_strength, 0.0..=3.0).text("IBL strength"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Fog (height)")
        .default_open(false)
        .show(ui, |ui| {
            let mut c = postfx.fog_color;
            if ui.color_edit_button_rgb(&mut c).changed() {
                postfx.fog_color = c;
            }
            ui.add(egui::Slider::new(&mut postfx.fog_density, 0.0..=0.2).text("Density"));
            ui.add(egui::Slider::new(&mut postfx.fog_height_base, -10.0..=20.0).text("Height base"));
            ui.add(egui::Slider::new(&mut postfx.fog_height_falloff, 0.0..=0.5).text("Height falloff"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Volumetric fog")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.volumetric_density, 0.0..=0.02).logarithmic(true).text("Density"));
            ui.add(egui::Slider::new(&mut postfx.volumetric_scattering, 0.0..=1.0).text("Scattering"));
            ui.add(egui::Slider::new(&mut postfx.volumetric_phase_g, 0.0..=0.9).text("Phase g"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Screen effects")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.vignette_strength, 0.0..=1.0).text("Vignette"));
            ui.add(egui::Slider::new(&mut postfx.film_grain, 0.0..=1.0).text("Film grain"));
            ui.add(egui::Slider::new(&mut postfx.chromatic_aberration, 0.0..=1.0).text("Chromatic ab."));
        });

    ui.separator();
    egui::CollapsingHeader::new("Shadows")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.shadow_bias, 0.0..=0.01).text("Depth bias"));
            ui.add(egui::Slider::new(&mut postfx.shadow_normal_bias, 0.0..=10.0).text("Slope bias"));
            ui.add(egui::Slider::new(&mut postfx.shadow_fade_start, 50.0..=250.0).text("Fade start"));
            ui.add(egui::Slider::new(&mut postfx.shadow_fade_end, 60.0..=300.0).text("Fade end"));
        });

    ui.separator();
    egui::CollapsingHeader::new("LOD")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.lod_bias, 0.2..=4.0).logarithmic(true).text("LOD bias"));
            ui.add(egui::Slider::new(&mut postfx.lod_distances[0], 5.0..=200.0).text("Distance LOD0→1"));
            ui.add(egui::Slider::new(&mut postfx.lod_distances[1], 20.0..=500.0).text("Distance LOD1→2"));
            ui.add(egui::Slider::new(&mut postfx.lod_distances[2], 50.0..=1000.0).text("Distance LOD2→3"));

            let d = &mut postfx.lod_distances;
            if d[1] <= d[0] { d[1] = d[0] + 1.0; }
            if d[2] <= d[1] { d[2] = d[1] + 1.0; }
        });

    ui.separator();
    ui.label("Debug view");
    ui.horizontal_wrapped(|ui| {
        debug_button(ui, &mut postfx.debug_view, DebugView::Final, "Final");
        debug_button(ui, &mut postfx.debug_view, DebugView::Ssao, "SSAO");
        debug_button(ui, &mut postfx.debug_view, DebugView::GbufferNormal, "Normal");
        debug_button(ui, &mut postfx.debug_view, DebugView::GbufferDepth, "Depth");
        debug_button(ui, &mut postfx.debug_view, DebugView::HdrPreBloom, "HDR");
        debug_button(ui, &mut postfx.debug_view, DebugView::CsmCascade0, "CSM");
    });

    ui.separator();
    draw_play_settings(ui, editor);
    ui.separator();
    draw_fly_settings(ui, editor);
}

// ============================================================
// Presets
// ============================================================

fn apply_preset(postfx: &mut PostFx, mut preset: PostFx) {
    preset.debug_view = postfx.debug_view;
    *postfx = preset;
}

fn preset_low() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.0;
    p.ssao_strength = 0.0;
    p.ibl_strength = 0.15;
    p.taa_strength = 0.0;
    p.taa_sharpening = 0.0;
    p.fxaa_strength = 1.0;
    p.fog_density = 0.0;
    p.volumetric_density = 0.0;
    p
}

fn preset_medium() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.4;
    p.ssao_strength = 0.5;
    p.ibl_strength = 0.3;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.05;
    p.fxaa_strength = 0.7;
    p.volumetric_density = 0.015;
    p.volumetric_scattering = 0.35;
    p.volumetric_phase_g = 0.5;
    p
}

fn preset_high() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.6;
    p.ssao_strength = 0.8;
    p.ibl_strength = 0.35;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.1;
    p.fxaa_strength = 0.5;
    p.volumetric_density = 0.025;
    p.volumetric_scattering = 0.4;
    p.volumetric_phase_g = 0.6;
    p
}

fn preset_ultra() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.8;
    p.bloom_knee = 0.4;
    p.ssao_strength = 1.0;
    p.ssao_radius = 0.8;
    p.ibl_strength = 0.45;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.15;
    p.fxaa_strength = 0.3;
    p.vignette_strength = 0.15;
    p.volumetric_density = 0.04;
    p.volumetric_scattering = 0.5;
    p.volumetric_phase_g = 0.7;
    p
}

fn preset_rage_cinematic() -> PostFx {
    let mut p = PostFx::default();
    p.tonemapper = Tonemapper::ACESFilmic;
    p.exposure = 0.55;
    p.exposure_bias = 1.05;

    p.color_temperature = 0.30;
    p.color_tint = 0.05;
    p.color_contrast = 1.18;
    p.color_saturation = 0.88;
    p.color_lift = [0.005, 0.0, -0.005];
    p.color_gain = [1.05, 1.0, 0.92];
    p.color_gamma = [0.98, 1.0, 1.02];

    p.bloom_threshold = 1.1;
    p.bloom_strength = 0.55;
    p.bloom_knee = 0.55;
    p.bloom_radius = 1.2;

    p.fog_color = [0.65, 0.48, 0.35];
    p.fog_density = 0.010;
    p.fog_height_base = 0.0;
    p.fog_height_falloff = 0.08;
    p.volumetric_density = 0.018;
    p.volumetric_scattering = 0.45;
    p.volumetric_phase_g = 0.65;

    p.ssao_strength = 0.85;
    p.ssao_radius = 0.7;
    p.ibl_strength = 0.35;

    p.taa_strength = 1.0;
    p.taa_sharpening = 0.08;
    p.fxaa_strength = 0.4;

    p.vignette_strength = 0.22;
    p.film_grain = 0.04;
    p.chromatic_aberration = 0.02;

    p.dof_enabled = 1.0;
    p.dof_focus_distance = 12.0;
    p.dof_focus_range = 4.0;
    p.dof_max_blur = 6.0;
    p.dof_blur_falloff = 5.0;

    p.motion_blur_enabled = 1.0;
    p.motion_blur_intensity = 0.55;
    p.motion_blur_max_px = 32.0;
    p.motion_blur_samples = 12;

    p.lens_flare_intensity = 0.35;
    p.lens_flare_threshold = 2.2;
    p.lens_flare_ghosts = 5;
    p.lens_flare_streak = 0.10;

    p.shadow_bias = 0.0016;
    p.shadow_normal_bias = 3.5;
    p.shadow_fade_start = 140.0;
    p.shadow_fade_end = 200.0;

    p
}

fn preset_desert_storm() -> PostFx {
    let mut p = preset_rage_cinematic();
    p.exposure_bias = 0.9;
    p.color_temperature = 0.20;
    p.color_tint = -0.15;
    p.color_contrast = 1.10;
    p.color_saturation = 0.62;
    p.fog_density = 0.030;
    p.fog_color = [0.72, 0.58, 0.42];
    p.volumetric_density = 0.045;
    p.volumetric_scattering = 0.55;
    p.volumetric_phase_g = 0.75;
    p.vignette_strength = 0.30;
    p.film_grain = 0.06;
    p.lens_flare_intensity = 0.20;
    p.bloom_strength = 0.45;
    p
}

fn preset_night_urban() -> PostFx {
    let mut p = PostFx::default();
    p.tonemapper = Tonemapper::AgX;
    p.exposure = 0.75;
    p.exposure_bias = 1.0;
    p.color_temperature = -0.35;
    p.color_tint = 0.05;
    p.color_contrast = 1.22;
    p.color_saturation = 0.92;
    p.color_lift = [-0.01, 0.0, 0.012];
    p.color_gain = [0.94, 1.0, 1.08];
    p.bloom_threshold = 0.9;
    p.bloom_strength = 0.95;
    p.bloom_knee = 0.4;
    p.bloom_radius = 1.4;
    p.fog_color = [0.18, 0.22, 0.35];
    p.fog_density = 0.015;
    p.volumetric_density = 0.020;
    p.volumetric_scattering = 0.5;
    p.volumetric_phase_g = 0.7;
    p.ssao_strength = 1.1;
    p.ssao_radius = 0.8;
    p.ibl_strength = 0.15;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.12;
    p.fxaa_strength = 0.5;
    p.vignette_strength = 0.28;
    p.film_grain = 0.05;
    p.chromatic_aberration = 0.04;
    p.dof_enabled = 1.0;
    p.dof_focus_distance = 15.0;
    p.dof_focus_range = 5.0;
    p.dof_max_blur = 8.0;
    p.dof_blur_falloff = 6.0;
    p.motion_blur_enabled = 1.0;
    p.motion_blur_intensity = 0.6;
    p.motion_blur_max_px = 36.0;
    p.motion_blur_samples = 14;
    p.lens_flare_intensity = 0.55;
    p.lens_flare_threshold = 1.5;
    p.lens_flare_ghosts = 8;
    p.lens_flare_streak = 0.18;
    p
}

// ============================================================
// Play / Fly settings
// ============================================================

fn draw_play_settings(ui: &mut egui::Ui, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Play")
        .default_open(false)
        .show(ui, |ui| {
            let p = &mut editor.play;

            ui.label(egui::RichText::new("Movement").strong());
            ui.add(egui::Slider::new(&mut p.walk_speed, 0.5..=20.0).text("Walk speed"));
            ui.add(egui::Slider::new(&mut p.run_speed, 1.0..=40.0).text("Run speed"));
            ui.add(egui::Slider::new(&mut p.jump_speed, 1.0..=20.0).text("Jump"));
            ui.add(egui::Slider::new(&mut p.gravity, 1.0..=60.0).text("Gravity"));
            ui.add(egui::Slider::new(&mut p.eye_height, 0.5..=3.0).text("Eye height"));
            ui.add(egui::Slider::new(&mut p.look_sensitivity, 0.0005..=0.01).text("Look sensitivity"));

            ui.separator();
            ui.label(egui::RichText::new("Feel").strong());
            ui.add(egui::Slider::new(&mut p.coyote_time, 0.0..=0.5).text("Coyote time (s)").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut p.jump_buffer_time, 0.0..=0.5).text("Jump buffer (s)").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut p.ground_accel_tau, 0.01..=0.3).text("Ground accel τ (s)").fixed_decimals(3));
            ui.add(egui::Slider::new(&mut p.air_accel_tau, 0.05..=1.0).text("Air accel τ (s)").fixed_decimals(2));

            ui.separator();
            ui.label("Player capsule");
            ui.add(egui::Slider::new(&mut p.player_radius, 0.1..=1.0).text("Radius"));
            ui.add(egui::Slider::new(&mut p.player_height, 0.5..=3.0).text("Height"));

            ui.separator();
            ui.label("Combat");
            ui.add(egui::Slider::new(&mut p.max_health, 10.0..=500.0).text("Max HP"));
            ui.add(egui::Slider::new(&mut p.max_ammo, 0..=500).text("Max ammo"));
            ui.add(egui::Slider::new(&mut p.damage_per_shot, 1.0..=200.0).text("Damage"));
            ui.add(egui::Slider::new(&mut p.gun_range, 5.0..=500.0).text("Range"));

            ui.separator();
            ui.checkbox(&mut p.show_crosshair, "Show crosshair");
            ui.checkbox(&mut p.show_hud, "Show HUD");
        });
}

fn draw_fly_settings(ui: &mut egui::Ui, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Fly (RMB)")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("WASD — move, E/Q — up/down");
            ui.label("Shift — faster, Ctrl — slower");
            ui.add(egui::Slider::new(&mut editor.fly_speed, 1.0..=100.0).text("Fly speed"));
            ui.add(egui::Slider::new(&mut editor.fly_sensitivity, 0.0005..=0.01).text("Fly sensitivity"));
        });
}

fn debug_button(
    ui: &mut egui::Ui,
    current: &mut DebugView,
    target: DebugView,
    label: &str,
) {
    if ui.selectable_label(*current == target, label).clicked() {
        *current = target;
    }
}