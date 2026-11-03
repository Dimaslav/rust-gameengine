//! Legacy entrypoint.
//!
//! Полноценный редактор — `engine-editor` (`src/bin/engine-editor.rs`).
//! Этот бинарь оставлен для скриптов/CI, которые запускают `rust-engine`
//! по старому имени. Вся реализация — через `rust_engine::...`, чтобы
//! не компилировать 30+ модулей дважды (lib + bin).

use rust_engine::demo::FortressDemo;
use rust_engine::engine::crash::install_crash_handler;
use rust_engine::engine::run;

fn main() {
    install_crash_handler();
    run(FortressDemo::new());
}