pub mod loader;
pub mod prefab;

pub use loader::{load_scene_from_file, load_scene_from_str, spawn_scene};
pub use prefab::{EntityDef, SceneDef};