//! Демо-сцены движка. Каждая — полноценная игра-пример.
//!
//! `FortressDemo` — максимально проработанная сцена "The Fallen Citadel",
//! использующая все возможности движка: физика, AI, освещение,
//! триггеры, лифты, двери, decals, аудио, UI, партиклы.

pub mod fortress;
pub mod primitives;

pub use fortress::FortressDemo;