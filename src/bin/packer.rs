//! Packer: собирает standalone-дистрибутив игры.
//!
//! Использование:
//!   cargo run --bin packer             # дефолт: rust-engine, release
//!   cargo run --bin packer mygame      # имя проекта mygame
//!   cargo run --bin packer mygame debug

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let game_name = args.get(1).cloned().unwrap_or_else(|| "rust-engine".to_string());
    let profile = args.get(2).cloned().unwrap_or_else(|| "release".to_string());

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dist = root.join("dist").join(&game_name);

    println!("📦 Packer: {} ({})", game_name, profile);
    println!("   Root: {}", root.display());
    println!("   Dist: {}", dist.display());
    println!();

    // 1. Очистить
    if dist.exists() {
        fs::remove_dir_all(&dist).expect("remove old dist");
    }
    fs::create_dir_all(&dist).expect("create dist");

    // 2. Собрать бинарь
    print!("🔨 Building… ");
    let mut cargo_args = vec!["build"];
    if profile == "release" { cargo_args.push("--release"); }
    let status = Command::new("cargo")
        .args(&cargo_args)
        .current_dir(&root)
        .status()
        .expect("cargo build failed");
    if !status.success() {
        eprintln!("❌ cargo build failed");
        std::process::exit(1);
    }
    println!("ok");

    // 3. Скопировать exe
    let exe_name = format!("{}.exe", game_name);
    let exe_src = root.join("target").join(&profile).join(&exe_name);
    if !exe_src.exists() {
        eprintln!("❌ exe не найден: {}", exe_src.display());
        eprintln!("   Подсказка: name в Cargo.toml = \"{}\"?", game_name);
        std::process::exit(1);
    }
    fs::copy(&exe_src, dist.join(&exe_name)).expect("copy exe");
    println!("  ✓ {}", exe_name);

    // 4. assets/
    let assets_src = root.join("assets");
    if assets_src.exists() {
        copy_dir(&assets_src, &dist.join("assets")).expect("copy assets");
        println!("  ✓ assets/");
    } else {
        println!("  ⚠ assets/ отсутствует");
    }

    // 5. конфиги
    for cfg in ["editor.ron", "input.ron", "settings.ron"] {
        let src = root.join(cfg);
        if src.exists() {
            fs::copy(&src, dist.join(cfg)).ok();
            println!("  ✓ {}", cfg);
        }
    }

    // 6. README для игрока
    let readme = format!(
        "{name} — standalone build (v{ver})\n\
         \n\
         Запуск: {exe}\n\
         \n\
         Требования:\n\
           - Windows 10/11 x64\n\
           - GPU с DirectX 12 или Vulkan\n\
           - 4 GB RAM минимум\n\
         \n\
         Управление:\n\
           WASD       — движение\n\
           Мышь       — обзор\n\
           ЛКМ        — стрельба\n\
           ПКМ        — прицел\n\
           Space      — прыжок\n\
           R          — перезарядка\n\
           Esc        — пауза / выход в меню\n\
           F1         — debug overlay\n\
           F9         — toggle editor/play\n\
           F12        — reload shaders (debug only)\n",
        name = game_name,
        ver = env!("CARGO_PKG_VERSION"),
        exe = exe_name,
    );
    fs::write(dist.join("README.txt"), readme).ok();
    println!("  ✓ README.txt");

    let size = dir_size(&dist);
    println!();
    println!("✅ Готово: {}", dist.display());
    println!("   Размер: {:.1} MB", size as f64 / 1_048_576.0);
    println!("   Запуск: {}", dist.join(&exe_name).display());
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    if path.is_file() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    let Ok(entries) = fs::read_dir(path) else { return 0; };
    entries.filter_map(|e| e.ok()).map(|e| dir_size(&e.path())).sum()
}