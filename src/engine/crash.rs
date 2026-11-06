//! Cross-platform crash handler.
//!
//! Installs a panic hook that:
//!   1. Formats a crash report (time, version, location, backtrace).
//!   2. Writes it to `crash.log` in the current working directory.
//!   3. On Windows — shows a MessageBox so the user knows what happened.
//!
//! Call `install_crash_handler()` once, at process start, before
//! `run(...)`. Every entrypoint (`engine-editor`, `rust-engine`, sample
//! projects) should call it.

/// Install the panic hook. Idempotent-ish: calling twice replaces the hook
/// with an identical one, which is fine.
pub fn install_crash_handler() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
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
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // FIX (P1.4): MessageBoxA (ANSI) с UTF-8 кириллицей даёт `??????`.
    // MessageBoxW + wide-строки — корректно.
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            utype: u32,
        ) -> i32;
    }

    let text = format!(
        "Движок упал. Отчёт сохранён в crash.log\n\n{}",
        report.lines().take(15).collect::<Vec<_>>().join("\n")
    );

    let text_w: Vec<u16> = OsStr::new(&text).encode_wide().chain(Some(0)).collect();
    let cap_w: Vec<u16> = OsStr::new("Rust Engine 3D — Crash")
        .encode_wide()
        .chain(Some(0))
        .collect();

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text_w.as_ptr(),
            cap_w.as_ptr(),
            0x10, // MB_ICONERROR
        );
    }
}