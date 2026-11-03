//! Глобальное состояние приложения: режим (меню / игра), настройки.
//! Настройки сериализуются в `settings.ron` рядом с exe.
//!
//! ## История формата
//!
//! * **v1** — исходный формат. Не содержал `postfx`.
//! * **v2** — добавлен `GraphicsSettings::postfx`. Дефолты `High`/`Ultra`
//!   были слишком яркими (`ibl_strength` 0.35/0.45,
//!   `volumetric_density` 0.025/0.040) — сцена пересвечена.
//! * **v3** — `High`/`Ultra` перекалиброваны под
//!   `FortressDemo::default_postfx()`.
//! * **v4** — добавлены `AntiAliasingMode`, `ShadowQuality`,
//!   `TextureQuality`, `FpsLimit`, `UpscaleMode`, `ControlsSettings`,
//!   `AccessibilitySettings`, `HudSettings`. Все новые поля — с
//!   `#[serde(default)]`, старые файлы парсятся без миграции.
//!
//! ## Бэкап битого файла
//!
//! `load_or_default` при ошибке парсинга переименовывает файл в
//! `settings.ron.bak`, а не молча теряет настройки.

use serde::{Deserialize, Serialize};

use crate::render::PostFx;

// ============================================================
// AppMode
// ============================================================

#[derive(Debug, Clone, Default)]
pub enum AppMode {
    MainMenu,
    #[default]
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

fn current_format() -> u32 { 4 }

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

fn default_language() -> String { "ru".to_string() }

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
                log::error!(
                    "Failed to parse settings '{}': {}. Using defaults.",
                    path, e
                );
                let backup = format!("{}.bak", path);
                match std::fs::rename(path, &backup) {
                    Ok(()) => log::warn!(
                        "Corrupt settings backed up to {}", backup
                    ),
                    Err(io_err) => log::warn!(
                        "Could not back up corrupt settings to {}: {}",
                        backup, io_err
                    ),
                }
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
        if self.format < 1 {
            self.format = 1;
        }
        if self.format < 2 {
            self.graphics.postfx = self.graphics.quality_preset.to_postfx();
            self.format = 2;
        }
        if self.format < 3 {
            // v2 имел слишком яркие `High`/`Ultra` дефолты.
            self.graphics.postfx = self.graphics.quality_preset.to_postfx();
            self.format = 3;
        }
        if self.format < 4 {
            // v4: добавлены ControlsSettings, AccessibilitySettings,
            // HudSettings, AntiAliasingMode, ShadowQuality,
            // TextureQuality, FpsLimit, UpscaleMode. serde(default)
            // заполняет их дефолтами. Здесь только синхронизируем
            // `aa_mode` с фактическим `postfx` (у старых файлов
            // `aa_mode` будет `Custom` из дефолта).
            self.graphics.anti_aliasing_mode =
                AntiAliasingMode::detect_from(&self.graphics.postfx);
            self.format = 4;
        }
        self.validate();
    }

    /// Санити-чек: клампим числовые поля в разумные диапазоны.
    /// Вызывается после миграции и на каждом `save` (через UI).
    pub fn validate(&mut self) {
        let g = &mut self.graphics;
        g.fov = g.fov.clamp(30.0, 130.0);
        g.render_scale = g.render_scale.clamp(0.5, 2.0);

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

/// Высокоуровневый селектор anti-aliasing. Работает как «shortcut»:
/// при выборе режима через `apply_to` перезаписывает
/// `postfx.taa_strength` и `postfx.fxaa_strength`. Если пользователь
/// потом крутит слайдеры TAA/FXAA напрямую — `detect_from` вернёт
/// `Custom`, и UI покажет «Custom».
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

    /// Применить режим к `postfx`. `Custom` — no-op.
    pub fn apply_to(self, postfx: &mut PostFx) {
        match self {
            Self::Off => {
                postfx.taa_strength = 0.0;
                postfx.fxaa_strength = 0.0;
            }
            Self::Fxaa => {
                postfx.taa_strength = 0.0;
                postfx.fxaa_strength = 1.0;
            }
            Self::Taa => {
                postfx.taa_strength = 1.0;
                postfx.fxaa_strength = 0.0;
            }
            Self::TaaFxaa => {
                postfx.taa_strength = 1.0;
                postfx.fxaa_strength = 0.5;
            }
            Self::Custom => {}
        }
    }

    /// Определить режим по текущим `postfx.taa_strength` / `fxaa_strength`.
    /// Возвращает `Custom`, если значения не совпадают ни с одним
    /// пресетом.
    pub fn detect_from(postfx: &PostFx) -> Self {
        let taa = postfx.taa_strength;
        let fxaa = postfx.fxaa_strength;
        let taa_off = taa < 0.01;
        let fxaa_off = fxaa < 0.01;
        match (taa_off, fxaa_off) {
            (true, true) => Self::Off,
            (true, false) if (fxaa - 1.0).abs() < 0.05 => Self::Fxaa,
            (false, true) if (taa - 1.0).abs() < 0.05 => Self::Taa,
            (false, false)
                if (taa - 1.0).abs() < 0.05 && (fxaa - 0.5).abs() < 0.1 =>
            {
                Self::TaaFxaa
            }
            _ => Self::Custom,
        }
    }
}

// ============================================================
// Graphics — shadows
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShadowQuality {
    Off,
    Low,
    Medium,
    #[default]
    High,
    Ultra,
}

impl ShadowQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }

    pub fn all() -> [Self; 5] {
        [Self::Off, Self::Low, Self::Medium, Self::High, Self::Ultra]
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "No shadows. Flat lighting.",
            Self::Low => "Short fade, high bias. Fast.",
            Self::Medium => "Balanced quality/perf.",
            Self::High => "Default. Long fade, low bias.",
            Self::Ultra => "Longest fade, minimal bias. Max quality.",
        }
    }

    /// Применить к `postfx.shadow_*` полям. CSM-разрешение и число
    /// каскадов — константы в рендерере (`CASCADE_SIZE`,
    /// `CASCADE_COUNT`); их изменение потребует пересоздания CSM-текстур
    /// (см. TODO в `renderer/mod.rs`). Пока настраиваются только bias
    /// и fade.
    pub fn apply_to(self, postfx: &mut PostFx) {
        match self {
            Self::Off => {
                // Fade_start=0 + fade_end=0.01 → shadow ≈ 1.0 (нет тени)
                // уже на первом метре от камеры.
                postfx.shadow_fade_start = 0.0;
                postfx.shadow_fade_end = 0.01;
            }
            Self::Low => {
                postfx.shadow_bias = 0.0030;
                postfx.shadow_normal_bias = 5.0;
                postfx.shadow_fade_start = 60.0;
                postfx.shadow_fade_end = 80.0;
            }
            Self::Medium => {
                postfx.shadow_bias = 0.0020;
                postfx.shadow_normal_bias = 4.0;
                postfx.shadow_fade_start = 100.0;
                postfx.shadow_fade_end = 140.0;
            }
            Self::High => {
                postfx.shadow_bias = 0.0015;
                postfx.shadow_normal_bias = 3.0;
                postfx.shadow_fade_start = 150.0;
                postfx.shadow_fade_end = 200.0;
            }
            Self::Ultra => {
                postfx.shadow_bias = 0.0010;
                postfx.shadow_normal_bias = 2.0;
                postfx.shadow_fade_start = 200.0;
                postfx.shadow_fade_end = 300.0;
            }
        }
    }
}

// ============================================================
// Graphics — textures
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextureQuality {
    Low,
    Medium,
    #[default]
    High,
    Ultra,
}

impl TextureQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Low, Self::Medium, Self::High, Self::Ultra]
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Low => "Half-resolution textures. Max FPS.",
            Self::Medium => "75% resolution textures.",
            Self::High => "Full resolution (default).",
            Self::Ultra => "Full resolution + 16× anisotropic.",
        }
    }
}

// ============================================================
// Graphics — FPS limit
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FpsLimit {
    #[default]
    Unlimited,
    Fps30,
    Fps60,
    Fps90,
    Fps120,
    Fps144,
    Fps240,
}

impl FpsLimit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unlimited => "Unlimited",
            Self::Fps30 => "30",
            Self::Fps60 => "60",
            Self::Fps90 => "90",
            Self::Fps120 => "120",
            Self::Fps144 => "144",
            Self::Fps240 => "240",
        }
    }

    pub fn all() -> [Self; 7] {
        [
            Self::Unlimited,
            Self::Fps30,
            Self::Fps60,
            Self::Fps90,
            Self::Fps120,
            Self::Fps144,
            Self::Fps240,
        ]
    }

    /// `None` — unlimited. `Some(hz)` — target FPS.
    pub fn target_hz(self) -> Option<f32> {
        match self {
            Self::Unlimited => None,
            Self::Fps30 => Some(30.0),
            Self::Fps60 => Some(60.0),
            Self::Fps90 => Some(90.0),
            Self::Fps120 => Some(120.0),
            Self::Fps144 => Some(144.0),
            Self::Fps240 => Some(240.0),
        }
    }
}

// ============================================================
// Graphics — upscaling
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UpscaleMode {
    #[default]
    Native,
    // FSR и DLSS — заложены на будущее. Пока только Native.
}

impl UpscaleMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "Native",
        }
    }

    pub fn all() -> [Self; 1] {
        [Self::Native]
    }
}

// ============================================================
// Graphics — QualityPreset
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum QualityPreset {
    Low,
    Medium,
    #[default]
    High,
    Ultra,
}

impl QualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Low, Self::Medium, Self::High, Self::Ultra]
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Low => "No AA, no SSAO, no bloom. Max FPS.",
            Self::Medium => "TAA, soft SSAO, light bloom.",
            Self::High => "Full SSAO, bloom, TAA + sharpen.",
            Self::Ultra => "High + volumetric, DoF, motion blur, lens flare.",
        }
    }

    /// Готовый `PostFx` для этого пресета.
    /// `High` совпадает с `FortressDemo::default_postfx()`.
    pub fn to_postfx(self) -> PostFx {
        let mut p = PostFx::default();
        match self {
            Self::Low => {
                p.bloom_strength = 0.0;
                p.ssao_strength = 0.0;
                p.ibl_strength = 0.15;
                p.taa_strength = 0.0;
                p.taa_sharpening = 0.0;
                p.fxaa_strength = 1.0;
                p.fog_density = 0.0;
                p.volumetric_density = 0.0;
                p.dof_enabled = 0.0;
                p.motion_blur_enabled = 0.0;
                p.lens_flare_intensity = 0.0;
                p.vignette_strength = 0.0;
                p.film_grain = 0.0;
                p.chromatic_aberration = 0.0;
            }
            Self::Medium => {
                p.bloom_threshold = 1.0;
                p.bloom_strength = 0.40;
                p.bloom_knee = 0.5;
                p.bloom_radius = 1.0;
                p.exposure = 0.6;
                p.ssao_strength = 0.50;
                p.ssao_radius = 0.6;
                p.ibl_strength = 0.20;
                p.taa_strength = 1.0;
                p.taa_sharpening = 0.05;
                p.fxaa_strength = 0.7;
                p.fog_color = [0.35, 0.40, 0.55];
                p.fog_density = 0.008;
                p.volumetric_density = 0.006;
                p.volumetric_scattering = 0.35;
                p.volumetric_phase_g = 0.5;
                p.dof_enabled = 0.0;
                p.motion_blur_enabled = 0.0;
                p.lens_flare_intensity = 0.0;
                p.vignette_strength = 0.05;
                p.film_grain = 0.0;
                p.chromatic_aberration = 0.0;
            }
            Self::High => {
                p.bloom_threshold = 0.9;
                p.bloom_strength = 0.55;
                p.bloom_knee = 0.5;
                p.bloom_radius = 1.1;
                p.exposure = 0.6;
                p.ssao_strength = 0.9;
                p.ssao_radius = 0.65;
                p.ibl_strength = 0.20;
                p.taa_strength = 1.0;
                p.taa_sharpening = 0.10;
                p.fxaa_strength = 1.0;
                p.fog_color = [0.35, 0.40, 0.55];
                p.fog_density = 0.008;
                p.fog_height_base = 0.0;
                p.fog_height_falloff = 0.06;
                p.volumetric_density = 0.006;
                p.volumetric_scattering = 0.45;
                p.volumetric_phase_g = 0.6;
                p.vignette_strength = 0.15;
                p.film_grain = 0.03;
                p.chromatic_aberration = 0.0;
                p.dof_enabled = 0.0;
                p.motion_blur_enabled = 0.0;
                p.lens_flare_intensity = 0.0;
            }
            Self::Ultra => {
                p.bloom_threshold = 0.9;
                p.bloom_strength = 0.65;
                p.bloom_knee = 0.4;
                p.bloom_radius = 1.2;
                p.exposure = 0.6;
                p.ssao_strength = 1.0;
                p.ssao_radius = 0.8;
                p.ibl_strength = 0.25;
                p.taa_strength = 1.0;
                p.taa_sharpening = 0.15;
                p.fxaa_strength = 0.5;
                p.fog_color = [0.35, 0.40, 0.55];
                p.fog_density = 0.010;
                p.fog_height_base = 0.0;
                p.fog_height_falloff = 0.06;
                p.volumetric_density = 0.010;
                p.volumetric_scattering = 0.50;
                p.volumetric_phase_g = 0.7;
                p.dof_enabled = 1.0;
                p.dof_focus_distance = 12.0;
                p.dof_focus_range = 4.0;
                p.dof_max_blur = 6.0;
                p.dof_blur_falloff = 5.0;
                p.motion_blur_enabled = 1.0;
                p.motion_blur_intensity = 0.5;
                p.motion_blur_max_px = 32.0;
                p.motion_blur_samples = 12;
                p.lens_flare_intensity = 0.35;
                p.lens_flare_threshold = 2.2;
                p.lens_flare_ghosts = 5;
                p.lens_flare_streak = 0.10;
                p.vignette_strength = 0.22;
                p.film_grain = 0.04;
                p.chromatic_aberration = 0.02;
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

    /// Масштаб внутреннего разрешения (0.5 — half-res). Не применяется
    /// в рендерере пока — см. TODO в `Renderer::resize`.
    #[serde(default = "default_one_f32")]
    pub render_scale: f32,

    #[serde(default = "default_fov")]
    pub fov: f32,

    #[serde(default = "default_true")]
    pub show_fps: bool,

    /// Полный набор пост-эффектов.
    #[serde(default)]
    pub postfx: PostFx,

    /// Высокоуровневый AA-селектор. Синхронизируется с
    /// `postfx.taa_strength` / `postfx.fxaa_strength` через
    /// `detect_from`. При `apply_to` перезаписывает их.
    #[serde(default)]
    pub anti_aliasing_mode: AntiAliasingMode,

    /// Настройка теней. Маппится на `postfx.shadow_*`.
    #[serde(default)]
    pub shadow_quality: ShadowQuality,

    /// Качество текстур. Пока только сохраняется (применение — в
    /// `Texture::from_*` при загрузке; TODO).
    #[serde(default)]
    pub texture_quality: TextureQuality,

    /// Ограничение FPS. Применяется в `App::redraw`.
    #[serde(default)]
    pub fps_limit: FpsLimit,

    /// Режим апскейлинга. Пока только `Native`.
    #[serde(default)]
    pub upscale_mode: UpscaleMode,
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
        }
    }
}

impl GraphicsSettings {
    /// Применить пресет: перезаписать `postfx` значениями пресета,
    /// сохранив `debug_view`. `anti_aliasing_mode` синхронизируется
    /// с новым `postfx`. `shadow_quality` / `texture_quality` /
    /// `fps_limit` — независимые настройки, не меняются.
    pub fn apply_preset(&mut self, preset: QualityPreset) {
        let debug_view = self.postfx.debug_view;
        self.postfx = preset.to_postfx();
        self.postfx.debug_view = debug_view;
        self.quality_preset = preset;
        self.anti_aliasing_mode = AntiAliasingMode::detect_from(&self.postfx);
    }

    /// Применить `anti_aliasing_mode` к `postfx`.
    pub fn apply_aa_mode(&mut self) {
        self.anti_aliasing_mode.apply_to(&mut self.postfx);
    }

    /// Применить `shadow_quality` к `postfx`.
    pub fn apply_shadow_quality(&mut self) {
        self.shadow_quality.apply_to(&mut self.postfx);
    }

    /// Собрать итоговый `PostFx` — `postfx` + все настройки, которые
    /// маппятся на его поля. Вызывается из
    /// `App::apply_settings_to_game`.
    pub fn resolved_postfx(&self) -> PostFx {
        let mut p = self.postfx.clone();

        // Accessibility: motion blur force-off.
        // (передаём через AppSettings::accessibility, но если хочется —
        // здесь можно встроить, если GraphicsSettings получит
        // ссылку. Пока — App::apply_settings_to_game сам отключает.)

        // AA mode и shadow quality уже применены к p при изменении
        // через UI. Если что-то разошлось — detect/apply в UI.

        p
    }
}

// ============================================================
// Controls
// ============================================================

fn default_sensitivity() -> f32 { 1.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlsSettings {
    /// Множитель чувствительности мыши. 1.0 — дефолт (как
    /// `PlayState::look_sensitivity` в `app.rs`).
    #[serde(default = "default_sensitivity")]
    pub mouse_sensitivity: f32,

    #[serde(default)]
    pub invert_y: bool,

    /// `true` — Sprint на toggle (нажал/отпустил). `false` — hold
    /// (удерживать Shift).
    #[serde(default)]
    pub sprint_toggle: bool,

    /// `true` — Crouch на toggle. `false` — hold (удерживать Ctrl).
    #[serde(default)]
    pub crouch_toggle: bool,

    /// Зарезервировано: raw mouse input (без OS-акселерации).
    /// Требует `winit::window::CursorGrabMode::Locked` + device events.
    #[serde(default)]
    pub raw_input: bool,
}

impl Default for ControlsSettings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            invert_y: false,
            sprint_toggle: false,
            crouch_toggle: false,
            raw_input: false,
        }
    }
}

// ============================================================
// Accessibility
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubtitleSize {
    Small,
    #[default]
    Medium,
    Large,
    Huge,
}

impl SubtitleSize {
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::Huge => "Huge",
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Small, Self::Medium, Self::Large, Self::Huge]
    }

    /// Множитель к дефолтному размеру шрифта UI.
    pub fn multiplier(self) -> f32 {
        match self {
            Self::Small => 0.85,
            Self::Medium => 1.0,
            Self::Large => 1.25,
            Self::Huge => 1.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorblindMode {
    #[default]
    Off,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

impl ColorblindMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Protanopia => "Protanopia (red-blind)",
            Self::Deuteranopia => "Deuteranopia (green-blind)",
            Self::Tritanopia => "Tritanopia (blue-blind)",
        }
    }

    pub fn all() -> [Self; 4] {
        [
            Self::Off,
            Self::Protanopia,
            Self::Deuteranopia,
            Self::Tritanopia,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilitySettings {
    /// Отключить тряску камеры (screen shake от выстрелов/ударов).
    #[serde(default)]
    pub disable_screen_shake: bool,

    /// Отключить motion blur независимо от `postfx.motion_blur_enabled`.
    #[serde(default)]
    pub disable_motion_blur: bool,

    /// Отключить все быстрые анимации UI (fades, slides).
    #[serde(default)]
    pub reduced_motion: bool,

    /// Размер субтитров.
    #[serde(default)]
    pub subtitle_size: SubtitleSize,

    /// Режим цветокоррекции для дальтоников.
    #[serde(default)]
    pub colorblind_mode: ColorblindMode,

    /// Высокий контраст (для HUD).
    #[serde(default)]
    pub high_contrast: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            disable_screen_shake: false,
            disable_motion_blur: false,
            reduced_motion: false,
            subtitle_size: SubtitleSize::Medium,
            colorblind_mode: ColorblindMode::Off,
            high_contrast: false,
        }
    }
}

// ============================================================
// HUD
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HudSettings {
    #[serde(default = "default_true")]
    pub show_crosshair: bool,
    #[serde(default = "default_true")]
    pub show_health: bool,
    #[serde(default = "default_true")]
    pub show_ammo: bool,
    #[serde(default = "default_true")]
    pub show_objectives: bool,
    #[serde(default = "default_true")]
    pub show_hints: bool,
    /// Всплывающие цифры урона.
    #[serde(default)]
    pub show_damage_numbers: bool,
    /// Мини-карта (зарезервировано — нет реализации).
    #[serde(default)]
    pub show_minimap: bool,
}

impl Default for HudSettings {
    fn default() -> Self {
        Self {
            show_crosshair: true,
            show_health: true,
            show_ammo: true,
            show_objectives: true,
            show_hints: true,
            show_damage_numbers: false,
            show_minimap: false,
        }
    }
}

// ============================================================
// Audio
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub master: f32,
    pub sfx: f32,
    pub music: f32,
    pub voice: f32,
    pub ui: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { master: 1.0, sfx: 1.0, music: 0.7, voice: 1.0, ui: 0.9 }
    }
}