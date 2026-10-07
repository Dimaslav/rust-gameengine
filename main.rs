#![allow(dead_code)]
#![allow(unused_imports)]

mod assets;
mod demo;
mod ecs;
mod editor;
mod engine;
mod game;
mod physics;
mod render;
mod scene;
mod ui;

use engine::run;

fn main() {
    run(demo::FortressDemo::new());
}