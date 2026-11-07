use rust_engine::engine::crash::install_crash_handler;
use rust_engine::engine::run;

// Подключаем папку demo, которую мы перенесли
mod demo;
use demo::FortressDemo;

fn main() {
    install_crash_handler();
    run(FortressDemo::new());
}