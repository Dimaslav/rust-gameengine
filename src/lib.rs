//! Rust Engine 3D — game engine library.
//!
//! This crate is consumed by:
//!   * `engine-editor` binary — full egui editor + demo.
//!   * `rust-engine` binary — legacy alias for engine-editor.
//!   * external game projects via `[dependencies] rust-engine = { path = "..." }`.
//!     See `samples/fallen-citadel/` for a complete example.
//!
//! The engine is a monolith on purpose during Phase 0. Physical crate
//! split (`engine-core`, `engine-render`, `engine-editor`, …) is Phase 2.
//! Until then the entire engine lives in this crate behind `pub mod`.

// Точечные allow обоснованы в комментариях. Глобальные
// `#![allow(dead_code)]` / `#![allow(unused_imports)]` удалены.
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::mut_from_ref)]
#![allow(clippy::wrong_self_convention)]
#![allow(clippy::upper_case_acronyms)]

pub mod app_state;
pub mod assets;
pub mod demo;
pub mod ecs;
pub mod editor;
pub mod engine;
pub mod game;
pub mod menu;
pub mod physics;
pub mod render;
pub mod scene;
pub mod scripting;
pub mod ui;

// ============================================================
// Convenience re-exports for game authors.
// ============================================================

pub use app_state::{AppMode, AppSettings};
pub use ecs::{Entity, World};
pub use engine::{crash, run, Game, Input, WorldLoadRequest};

/// Reference demo project shipped with the engine.
/// Game projects don't need to use it — they implement `Game` themselves.
pub use demo::FortressDemo;