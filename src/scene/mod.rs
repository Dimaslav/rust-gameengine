//! Сериализация сцены в RON.

pub mod serialize;

pub use serialize::{
    load_scene_from_file, load_scene_from_str, save_scene_to_file, save_scene_to_string,
    SceneFile,
};