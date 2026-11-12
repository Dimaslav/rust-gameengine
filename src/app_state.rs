use serde::{Deserialize, Serialize};

use crate::render::PostFx;
use crate::render::renderer::atmosphere::AtmosphereSettings;

// ============================================================
// AppMode
// ============================================================

#[derive(Debug, Clone, Default)]
pub enum AppMode {
    #[default]
    MainMenu,
    InGame,
    Settings { return_to: Box<AppMode> },
}

impl AppMode {
    pub fn is_menu(&self) -> bool {
        matches!(self, AppMode::MainMenu | AppMode::Settings { .. })
    }
}

// ============================================================
// AppSettings
// ============================================================

fn current_format() -> u32 { 5 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "current_format")]
    pub format: u32,
    #[serde(default)] pub window: WindowSettings,
    #[serde(default)] pub graphics: GraphicsSettings,
    #[serde(default)] pub audio: AudioSettings,
    #[serde(default)] pub controls: ControlsSettings,
    #[serde(default)] pub accessibility: AccessibilitySettings,
    #[serde(default)] pub hud: HudSettings,
    #[serde(default = "default_language")]
    pub language: String,
}

/// FIX #12: было `"ru"`. HUD/диалоги в демо на русском, но
/// `FONT_GLYPHS` содержит только ASCII 32-127 → кириллица
/// рендерится пустотой. Переключили дефолт на английский +
/// перевод строк в `fortress.rs`. Русский остаётся доступен
/// через Settings → Language → Русский (но HUD всё равно
/// потребует расширения атласа шрифта).
fn default_language() -> String { "en".to_string() }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            format: current_format(),
            window: WindowSettings::default(),
            graphics: GraphicsSettings::default(),
            audio: AudioSettings::default(),
            controls: ControlsSettings::default(),
            accessibility: AccessibilitySettings::default(),
            hud: HudSettings::default(),
            language: default_language(),
        }
    }
}

impl AppSettings {
    pub fn load_or_default(path: &str) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match ron::from_str::<Self>(&text) {
            Ok(mut settings) => {
                settings.migrate();
                settings
            }
            Err(e) => {
                log::error!("Failed to parse settings '{}': {}. Using defaults.", path, e);
                let backup = format!("{}.bak", path);
                let _ = std::fs::rename(path, &backup);
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    fn migrate(&mut self) {
        if self.format < 1 { self.format = 1; }
        if self.format < 2 {
            self.graphics.postfx = self.graphics.quality_preset.to_postfx();
            self.format = 2;
        }
        if self.format < 3 {
            self.graphics.postfx = self.graphics.quality_preset.to_postfx();
            self.format = 3;
        }
        if self.format < 4 {
            self.graphics.anti_aliasing_mode =
                AntiAliasingMode::detect_from(&self.graphics.postfx);
            self.format = 4;
        }
        if self.format < 5 {
            self.format = 5;
        }
        self.validate();
    }

    pub fn validate(&mut self) {
        let g = &mut self.graphics;
        g.fov = g.fov.clamp(30.0, 130.0);
        // FIX: было 1.0..2.0. Теперь можно опустить до 0.5 для повышения FPS (SSAA вниз).
        g.render_scale = g.render_scale.clamp(0.5, 2.0);
        g.atmosphere.validate();

        let c = &mut self.controls;
        c.mouse_sensitivity = c.mouse_sensitivity.clamp(0.1, 10.0);

        let a = &mut self.audio;
        a.master = a.master.clamp(0.0, 1.0);
        a.sfx = a.sfx.clamp(0.0, 1.0);
        a.music = a.music.clamp(0.0, 1.0);
        a.voice = a.voice.clamp(0.0, 1.0);
        a.ui = a.ui.clamp(0.0, 1.0);
    }
}

// ============================================================
// Window
// ============================================================

pub const RESOLUTIONS: &[(u32, u32, &str)] = &[
    (1280, 720, "1280 × 720"),
    (1366, 768, "1366 × 768"),
    (1600, 900, "1600 × 900"),
    (1920, 1080, "1920 × 1080"),
    (2560, 1440, "2560 × 1440"),
    (3840, 2160, "3840 × 2160"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSettings {
    pub vsync: bool,
    pub fullscreen: bool,
    pub resolution_index: usize,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { vsync: true, fullscreen: false, resolution_index: 3 }
    }
}

// ============================================================
// Graphics — anti-aliasing
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AntiAliasingMode {
    Off,
    Fxaa,
    Taa,
    TaaFxaa,
    #[default]
    Custom,
}

impl AntiAliasingMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Fxaa => "FXAA",
            Self::Taa => "TAA",
            Self::TaaFxaa => "TAA + FXAA",
            Self::Custom => "Custom",
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Off, Self::Fxaa, Self::Taa, Self::TaaFxaa]
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "No anti-aliasing. Max FPS, sharp edges.",
            Self::Fxaa => "Fast post-process AA. Slight blur.",
            Self::Taa => "Temporal AA. Best for static scenes.",
            Self::TaaFxaa => "TAA + light FXAA cleanup. Recommended.",
            Self::Custom => "Manual TAA/FXAA strengths.",
        }
    }

    pub fn apply_to(self, postfx: &mut PostFx) {
        match self {
            Self::Off => { postfx.taa_strength = 0.0; postfx.fxaa_strength = 0.0; }
            Self::Fxaa => { postfx.taa_strength = 0.0; postfx.fxaa_strength = 1.0; }
            Self::Taa => { postfx.taa_strength = 1.0; postfx.fxaa_strength = 0.0; }
            Self::TaaFxaa => { postfx.taa_strength = 1.0; postfx.fxaa_strength = 0.5; }
            Self::Custom => {}
        }
    }

    pub fn detect_from(postfx: &PostFx) -> Self {
        let taa = postfx.taa_strength;
        let fxaa = postfx.fxaa_strength;
        let taa_off = taa < 0.01;
        let fxaa_off = fxaa < 0.01;
        match (taa_off, fxaa_off) {
            (true, true) => Self::Off,
            (true, false) if (fxaa - 1.0).abs() < 0.05 => Self::Fxaa,
            (false, true) if (taa - 1.0).abs() < 0.05 => Self::Taa,
            (false, false) if (taa - 1.0).abs() < 0.05 && (fxaa - 0.5).abs() < 0.1 => Self::TaaFxaa,
            _ => Self::Custom,
        }
    }
}

// ============================================================
// Graphics — shadows / textures / fps / upscale
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShadowQuality {
    Off, Low, Medium, #[default] High, Ultra,
}

impl ShadowQuality {
    pub fn label(self) -> &'static str {
        match self { Self::Off => "Off", Self::Low => "Low", Self::Medium => "Medium", Self::High => "High", Self::Ultra => "Ultra" }
    }
    pub fn all() -> [Self; 5] { [Self::Off, Self::Low, Self::Medium, Self::High, Self::Ultra] }
    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "No shadows. Flat lighting.",
            Self::Low => "Short fade, high bias. Fast.",
            Self::Medium => "Balanced quality/perf.",
            Self::High => "Default. Long fade, low bias.",
            Self::Ultra => "Longest fade, minimal bias. Max quality.",
        }
    }
    pub fn apply_to(self, postfx: &mut PostFx) {
        match self {
            Self::Off => { postfx.shadow_fade_start = 0.0; postfx.shadow_fade_end = 0.01; }
            Self::Low => { postfx.shadow_bias = 0.0030; postfx.shadow_normal_bias = 5.0; postfx.shadow_fade_start = 60.0; postfx.shadow_fade_end = 80.0; }
            Self::Medium => { postfx.shadow_bias = 0.0020; postfx.shadow_normal_bias = 4.0; postfx.shadow_fade_start = 100.0; postfx.shadow_fade_end = 140.0; }
            Self::High => { postfx.shadow_bias = 0.0015; postfx.shadow_normal_bias = 3.0; postfx.shadow_fade_start = 150.0; postfx.shadow_fade_end = 200.0; }
            Self::Ultra => { postfx.shadow_bias = 0.0010; postfx.shadow_normal_bias = 2.0; postfx.shadow_fade_start = 200.0; postfx.shadow_fade_end = 300.0; }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextureQuality { Low, Medium, #[default] High, Ultra }

impl TextureQuality {
    pub fn label(self) -> &'static str {
        match self { Self::Low => "Low", Self::Medium => "Medium", Self::High => "High", Self::Ultra => "Ultra" }
    }
    pub fn all() -> [Self; 4] { [Self::Low, Self::Medium, Self::High, Self::Ultra] }
    pub fn description(self) -> &'static str {
        match self {
            Self::Low => "Half-resolution textures. Max FPS.",
            Self::Medium => "75% resolution textures.",
            Self::High => "Full resolution (default).",
            Self::Ultra => "Full resolution + 16× anisotropic.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FpsLimit {
    #[default]
    Unlimited, Fps30, Fps60, Fps90, Fps120, Fps144, Fps240,
}

impl FpsLimit {
    pub fn label(self) -> &'static str {
        match self { Self::Unlimited => "Unlimited", Self::Fps30 => "30", Self::Fps60 => "60", Self::Fps90 => "90", Self::Fps120 => "120", Self::Fps144 => "144", Self::Fps240 => "240" }
    }
    pub fn all() -> [Self; 7] { [Self::Unlimited, Self::Fps30, Self::Fps60, Self::Fps90, Self::Fps120, Self::Fps144, Self::Fps240] }
    pub fn target_hz(self) -> Option<f32> {
        match self {
            Self::Unlimited => None, Self::Fps30 => Some(30.0), Self::Fps60 => Some(60.0),
            Self::Fps90 => Some(90.0), Self::Fps120 => Some(120.0), Self::Fps144 => Some(144.0),
            Self::Fps240 => Some(240.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UpscaleMode { #[default] Native }

impl UpscaleMode {
    pub fn label(self) -> &'static str { match self { Self::Native => "Native" } }
    pub fn all() -> [Self; 1] { [Self::Native] }
}

// ============================================================
// QualityPreset
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum QualityPreset { Low, Medium, #[default] High, Ultra }

impl QualityPreset {
    pub fn label(self) -> &'static str {
        match self { Self::Low => "Low", Self::Medium => "Medium", Self::High => "High", Self::Ultra => "Ultra" }
    }
    pub fn all() -> [Self; 4] { [Self::Low, Self::Medium, Self::High, Self::Ultra] }
    pub fn description(self) -> &'static str {
        match self {
            Self::Low => "No AA, no SSAO, no bloom. Max FPS.",
            Self::Medium => "TAA, soft SSAO, light bloom.",
            Self::High => "Full SSAO, bloom, TAA + sharpen, contact shadows.",
            Self::Ultra => "High + volumetric, DoF, motion blur, lens flare. SSAA 1.5× recommended.",
        }
    }
    pub fn recommended_render_scale(self) -> f32 {
        match self { Self::Low | Self::Medium | Self::High => 1.0, Self::Ultra => 1.5 }
    }

    pub fn to_postfx(self) -> PostFx {
        let mut p = PostFx::default();
        match self {
            Self::Low => {
                p.bloom_strength = 0.0; p.ssao_strength = 0.0; p.ibl_strength = 0.15;
                p.taa_strength = 0.0; p.taa_sharpening = 0.0;
                p.fxaa_strength = 0.0;
                p.fog_density = 0.0; p.volumetric_density = 0.0; p.dof_enabled = 0.0;
                p.motion_blur_enabled = 0.0; p.lens_flare_intensity = 0.0;
                p.contact_shadow_strength = 0.0; p.vignette_strength = 0.0;
                p.film_grain = 0.0; p.chromatic_aberration = 0.0;
            }
            Self::Medium => {
                p.bloom_threshold = 1.0; p.bloom_strength = 0.40; p.bloom_knee = 0.5; p.bloom_radius = 1.0;
                p.exposure = 0.6; p.ssao_strength = 0.50; p.ssao_radius = 0.6; p.ibl_strength = 0.20;
                p.taa_strength = 1.0; p.taa_sharpening = 0.05;
                p.fxaa_strength = 0.0;
                p.fog_color = [0.35, 0.40, 0.55]; p.fog_density = 0.0008;
                p.volumetric_density = 0.0003; p.volumetric_scattering = 0.35; p.volumetric_phase_g = 0.5;
                p.dof_enabled = 0.0; p.motion_blur_enabled = 0.0; p.lens_flare_intensity = 0.0;
                p.contact_shadow_strength = 0.5; p.vignette_strength = 0.05;
                p.film_grain = 0.0; p.chromatic_aberration = 0.0;
            }
            Self::High => {
                p.bloom_threshold = 0.9; p.bloom_strength = 0.55; p.bloom_knee = 0.5; p.bloom_radius = 1.1;
                p.exposure = 0.6; p.ssao_strength = 0.9; p.ssao_radius = 0.65; p.ibl_strength = 0.20;
                p.taa_strength = 1.0; p.taa_sharpening = 0.10;
                p.fxaa_strength = 0.5;
                p.fog_color = [0.35, 0.40, 0.55]; p.fog_density = 0.0008; p.fog_height_base = 0.0; p.fog_height_falloff = 0.06;
                p.volumetric_density = 0.0006; p.volumetric_scattering = 0.45; p.volumetric_phase_g = 0.6;
                p.vignette_strength = 0.15; p.film_grain = 0.03; p.chromatic_aberration = 0.0;
                p.dof_enabled = 0.0; p.motion_blur_enabled = 0.0; p.lens_flare_intensity = 0.0;
                p.contact_shadow_strength = 0.7; p.contact_shadow_length = 0.5; p.contact_shadow_thickness = 0.05;
            }
            Self::Ultra => {
                p.bloom_threshold = 0.9; p.bloom_strength = 0.65; p.bloom_knee = 0.4; p.bloom_radius = 1.2;
                p.exposure = 0.6; p.ssao_strength = 1.0; p.ssao_radius = 0.8; p.ibl_strength = 0.25;
                p.taa_strength = 1.0; p.taa_sharpening = 0.15; p.fxaa_strength = 0.5;
                p.fog_color = [0.35, 0.40, 0.55]; p.fog_density = 0.001; p.fog_height_base = 0.0; p.fog_height_falloff = 0.06;
                p.volumetric_density = 0.0008; p.volumetric_scattering = 0.50; p.volumetric_phase_g = 0.7;
                p.dof_enabled = 1.0; p.dof_focus_distance = 12.0; p.dof_focus_range = 4.0;
                p.dof_max_blur = 6.0; p.dof_blur_falloff = 5.0;
                p.motion_blur_enabled = 1.0; p.motion_blur_intensity = 0.5;
                p.motion_blur_max_px = 32.0; p.motion_blur_samples = 12;
                p.lens_flare_intensity = 0.35; p.lens_flare_threshold = 2.2;
                p.lens_flare_ghosts = 5; p.lens_flare_streak = 0.10;
                p.vignette_strength = 0.22; p.film_grain = 0.04; p.chromatic_aberration = 0.02;
                p.contact_shadow_strength = 0.8; p.contact_shadow_length = 0.7; p.contact_shadow_thickness = 0.05;
            }
        }
        p
    }
}

// ============================================================
// GraphicsSettings
// ============================================================

fn default_one_f32() -> f32 { 1.0 }
fn default_fov() -> f32 { 75.0 }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsSettings {
    pub quality_preset: QualityPreset,

    #[serde(default = "default_one_f32")]
    pub render_scale: f32,

    #[serde(default = "default_fov")]
    pub fov: f32,

    #[serde(default = "default_true")]
    pub show_fps: bool,

    #[serde(default)]
    pub postfx: PostFx,

    #[serde(default)]
    pub anti_aliasing_mode: AntiAliasingMode,

    #[serde(default)]
    pub shadow_quality: ShadowQuality,

    #[serde(default)]
    pub texture_quality: TextureQuality,

    #[serde(default)]
    pub fps_limit: FpsLimit,

    #[serde(default)]
    pub upscale_mode: UpscaleMode,

    #[serde(default)]
    pub atmosphere: AtmosphereSettings,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            quality_preset: QualityPreset::High,
            render_scale: 1.0,
            fov: 75.0,
            show_fps: true,
            postfx: QualityPreset::High.to_postfx(),
            anti_aliasing_mode: AntiAliasingMode::TaaFxaa,
            shadow_quality: ShadowQuality::High,
            texture_quality: TextureQuality::High,
            fps_limit: FpsLimit::Unlimited,
            upscale_mode: UpscaleMode::Native,
            atmosphere: AtmosphereSettings::default(),
        }
    }
}

impl GraphicsSettings {
    pub fn apply_preset(&mut self, preset: QualityPreset) {
        let debug_view = self.postfx.debug_view;
        self.postfx = preset.to_postfx();
        self.postfx.debug_view = debug_view;
        self.quality_preset = preset;
        self.anti_aliasing_mode = AntiAliasingMode::detect_from(&self.postfx);
    }
    pub fn apply_aa_mode(&mut self) { self.anti_aliasing_mode.apply_to(&mut self.postfx); }
    pub fn apply_shadow_quality(&mut self) { self.shadow_quality.apply_to(&mut self.postfx); }
    pub fn resolved_postfx(&self) -> PostFx { self.postfx.clone() }

    pub fn render_scale_percent(&self) -> u32 {
        (self.render_scale.clamp(0.5, 2.0) * 100.0).round() as u32
    }
    pub fn apply_render_scale_preset(&mut self, scale: f32) {
        self.render_scale = scale.clamp(0.5, 2.0);
    }
}

// ============================================================
// Controls / Accessibility / HUD / Audio
// ============================================================

fn default_sensitivity() -> f32 { 1.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlsSettings {
    #[serde(default = "default_sensitivity")] pub mouse_sensitivity: f32,
    #[serde(default)] pub invert_y: bool,
    #[serde(default)] pub sprint_toggle: bool,
    #[serde(default)] pub crouch_toggle: bool,
    #[serde(default)] pub raw_input: bool,
}

impl Default for ControlsSettings {
    fn default() -> Self {
        Self { mouse_sensitivity: 1.0, invert_y: false, sprint_toggle: false, crouch_toggle: false, raw_input: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubtitleSize {
    Small, #[default] Medium, Large, Huge,
}

impl SubtitleSize {
    pub fn label(self) -> &'static str {
        match self { Self::Small => "Small", Self::Medium => "Medium", Self::Large => "Large", Self::Huge => "Huge" }
    }
    pub fn all() -> [Self; 4] { [Self::Small, Self::Medium, Self::Large, Self::Huge] }
    pub fn multiplier(self) -> f32 {
        match self { Self::Small => 0.85, Self::Medium => 1.0, Self::Large => 1.25, Self::Huge => 1.5 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorblindMode {
    #[default] Off, Protanopia, Deuteranopia, Tritanopia,
}

impl ColorblindMode {
    pub fn label(self) -> &'static str {
        match self { Self::Off => "Off", Self::Protanopia => "Protanopia (red-blind)", Self::Deuteranopia => "Deuteranopia (green-blind)", Self::Tritanopia => "Tritanopia (blue-blind)" }
    }
    pub fn all() -> [Self; 4] { [Self::Off, Self::Protanopia, Self::Deuteranopia, Self::Tritanopia] }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilitySettings {
    #[serde(default)] pub disable_screen_shake: bool,
    #[serde(default)] pub disable_motion_blur: bool,
    #[serde(default)] pub reduced_motion: bool,
    #[serde(default)] pub subtitle_size: SubtitleSize,
    #[serde(default)] pub colorblind_mode: ColorblindMode,
    #[serde(default)] pub high_contrast: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            disable_screen_shake: false, disable_motion_blur: false, reduced_motion: false,
            subtitle_size: SubtitleSize::Medium, colorblind_mode: ColorblindMode::Off, high_contrast: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HudSettings {
    #[serde(default = "default_true")] pub show_crosshair: bool,
    #[serde(default = "default_true")] pub show_health: bool,
    #[serde(default = "default_true")] pub show_ammo: bool,
    #[serde(default = "default_true")] pub show_objectives: bool,
    #[serde(default = "default_true")] pub show_hints: bool,
    #[serde(default)] pub show_damage_numbers: bool,
    #[serde(default)] pub show_minimap: bool,
}

impl Default for HudSettings {
    fn default() -> Self {
        Self {
            show_crosshair: true, show_health: true, show_ammo: true, show_objectives: true,
            show_hints: true, show_damage_numbers: false, show_minimap: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub master: f32, pub sfx: f32, pub music: f32, pub voice: f32, pub ui: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { master: 1.0, sfx: 1.0, music: 0.7, voice: 1.0, ui: 0.9 }
    }
}