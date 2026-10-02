//! Сериализация сцены в RON и префабы.

pub mod prefab;
pub mod serialize;

pub use prefab::{
    instantiate_prefab, list_prefabs, load_prefab_from_file, prefab_display_name,
    prefab_from_selection, save_prefab_to_file, PrefabFile,
};
pub use serialize::{
    load_scene_from_file, load_scene_from_str, save_scene_to_file, save_scene_to_string,
    SceneFile,
};