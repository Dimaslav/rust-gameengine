//! Runtime UI framework.
//!
//! * `UiQuad` — screen-space quad instance.
//! * `font` — процедурный 5×7 пиксельный шрифт.
//! * `UiLayer` — immediate-mode builder (rect, text, button, bar).
//! * `Hud` — высокоуровневые HUD-хелперы (health, ammo, notif).
//!
//! Рендерится отдельным pass'ом поверх 3D, под egui.

pub mod font;
pub mod hud;
pub mod layer;
pub mod quad;

pub use hud::{Hud, Notification};
pub use layer::{UiInput, UiLayer};
pub use quad::{UiGlobal, UiQuad};