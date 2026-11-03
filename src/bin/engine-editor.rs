//! `engine-editor` — full engine editor with egui panels.
//!
//! This binary runs the reference demo (Fallen Citadel) with the editor
//! enabled, exactly like the legacy `rust-engine` binary. In Phase 2 this
//! will accept `--project <path>` to open an arbitrary project instead
//! of the bundled demo.

use rust_engine::demo::FortressDemo;
use rust_engine::engine::crash::install_crash_handler;
use rust_engine::engine::run;

fn main() {
    install_crash_handler();
    run(FortressDemo::new());
}