#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::mut_from_ref)]
#![allow(clippy::wrong_self_convention)]
#![allow(clippy::upper_case_acronyms)]

pub mod app_state;
pub mod assets;
pub mod ecs;
pub mod editor;
pub mod engine;
pub mod game;
pub mod menu;
pub mod physics;
pub mod project;
pub mod render;
pub mod scene;
pub mod scripting;
pub mod ui;

pub use app_state::{AppMode, AppSettings};
pub use ecs::{Entity, World};
pub use engine::{crash, run, Game, Input, WorldLoadRequest};