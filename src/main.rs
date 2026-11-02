#![allow(dead_code)]
#![allow(unused_imports)]

// Явно разрешённые clippy-category с обоснованием:
// * too_many_arguments — функции построения pipeline'ов, primitive-хелперы
//   (`wall`, `static_mesh`, `collision::resolve_movement_ex`) требуют
//   много параметров по существу (координаты, размеры, материалы, коллизии).
//   Рефакторинг в struct-билдер только ухудшит читаемость.
#![allow(clippy::too_many_arguments)]
// * type_complexity — `[([f32;3], [f32;3], [f32;3], [f32;3]); 6]` для граней
//   куба и кортеж возврата из `load_scene_*` — раскладывать в алиасы
//   без пользы для читаемости.
#![allow(clippy::type_complexity)]
// * mut_from_ref — `ScriptHost::world_mut(&self) -> &mut World` — намеренный
//   unsafe-доступ через `*mut World` (см. комментарии в `scripting/host.rs`).
//   Без этого Rhai не может мутировать мир из скрипта.
#![allow(clippy::mut_from_ref)]
// * wrong_self_convention — `to_light(&self)`, `to_wgpu(&self)`, `to_orbit(&mut self)`
//   называются так по смыслу, менять имена (`as_light`, `into_wgpu`) — churn
//   по всему проекту без пользы.
#![allow(clippy::wrong_self_convention)]
// * upper_case_acronyms — `SMG`, `XYZ`, `ZXY` — это стандартные имена
//   (WeaponKind::SMG, EulerRot::XYZ), они не «acronym в обычном слове».
#![allow(clippy::upper_case_acronyms)]

mod app_state;
mod menu;
mod assets;
// ... (дальше без изменений)
mod demo;
mod ecs;
mod editor;
mod engine;
mod game;
mod physics;
mod render;
mod scene;
mod scripting;
mod ui;

use engine::run;

// ============================================================
// Crash handler
// ============================================================

fn install_crash_handler() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());

        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".into()
        };

        let backtrace = std::backtrace::Backtrace::force_capture();

        let report = format!(
            "=== Rust Engine 3D crash report ===\n\
             Time: {:?}\n\
             Version: {}\n\
             Location: {}\n\
             Message: {}\n\n\
             Backtrace:\n{}\n",
            std::time::SystemTime::now(),
            env!("CARGO_PKG_VERSION"),
            location,
            message,
            backtrace,
        );

        let _ = std::fs::write("crash.log", &report);
        eprintln!("{}", report);

        #[cfg(target_os = "windows")]
        {
            show_crash_dialog(&report);
        }
    }));
}

#[cfg(target_os = "windows")]
fn show_crash_dialog(report: &str) {
    use std::ffi::CString;

    // ИСПРАВЛЕНО: Rust 2024 требует `unsafe extern "system"`.
    unsafe extern "system" {
        fn MessageBoxA(
            hwnd: *mut std::ffi::c_void,
            text: *const u8,
            caption: *const u8,
            utype: u32,
        ) -> i32;
    }

    let text = format!(
        "Движок упал. Отчёт сохранён в crash.log\n\n{}",
        report.lines().take(15).collect::<Vec<_>>().join("\n")
    );

    let Ok(text_cstr) = CString::new(text) else { return };
    let Ok(cap) = CString::new("Rust Engine 3D — Crash") else { return };

    // SAFETY: указатели на null-terminated строки живут весь вызов,
    // окно не передаём (null), флаг MB_ICONERROR = 0x10.
    unsafe {
        MessageBoxA(
            std::ptr::null_mut(),
            text_cstr.as_ptr() as *const u8,
            cap.as_ptr() as *const u8,
            0x10,
        );
    }
}

// ============================================================
// Entry point
// ============================================================

fn main() {
    install_crash_handler();
    run(demo::FortressDemo::new());
}