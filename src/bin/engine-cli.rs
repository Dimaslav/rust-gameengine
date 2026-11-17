//! engine-cli — управление проектами на Rust Engine 3D.
//!
//! Команды:
//!   engine-cli new <name>              создать проект
//!   engine-cli list                    список проектов
//!   engine-cli assets init [<path>]    сгенерировать отсутствующие .meta

use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = match args.next() {
        Some(c) => c,
        None => {
            print_help();
            return Ok(());
        }
    };

    match cmd.as_str() {
        "new" => {
            let name = args.next().ok_or_else(|| anyhow::anyhow!("usage: engine-cli new <name>"))?;
            cmd_new(&name)
        }
        "list" => cmd_list(),
        "assets" => {
            let sub = args.next().unwrap_or_default();
            if sub != "init" {
                anyhow::bail!("usage: engine-cli assets init [<project-path>]");
            }
            let path = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
            cmd_assets_init(&path)
        }
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => {
            anyhow::bail!("unknown command `{other}`. Try `engine-cli --help`.")
        }
    }
}

fn print_help() {
    eprintln!(
        "engine-cli — Rust Engine 3D\n\
         \n\
         USAGE:\n\
         \x20 engine-cli new <name>              Create project under projects/<name>/\n\
         \x20 engine-cli list                    List projects under projects/\n\
         \x20 engine-cli assets init [<path>]    Generate missing .meta files\n"
    );
}

// ---------------------------------------------------------------------------
// new
// ---------------------------------------------------------------------------

fn cmd_new(name: &str) -> anyhow::Result<()> {
    let dest = PathBuf::from("projects").join(name);
    if dest.exists() {
        anyhow::bail!("project `{name}` already exists at {}", dest.display());
    }
    let tpl = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/blank_project");
    if !tpl.is_dir() {
        anyhow::bail!(
            "template not found at {}. Expected `templates/blank_project/` in engine repo.",
            tpl.display()
        );
    }
    copy_dir_recursive(&tpl, &dest)?;
    substitute_placeholders(&dest, name)?;

    // Сгенерировать .meta для ассетов шаблона.
    let assets = dest.join("assets");
    if assets.is_dir() {
        let _ = rust_engine::assets::meta::generate_missing_metas(&assets)?;
    }

    println!("created project at {}", dest.display());
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn substitute_placeholders(root: &Path, project_name: &str) -> anyhow::Result<()> {
    // Файлы-шаблоны: заменить __PROJECT_NAME__.
    // Файлы, где не нужно (assets/*.ron) — пропускаем по расширению.
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "rs" | "toml" | "ron" | "md" | "json") {
            continue;
        }
        // .ron из assets/ не трогаем — там нет __PROJECT_NAME__.
        if p.components().any(|c| c.as_os_str() == "assets") {
            continue;
        }
        let text = std::fs::read_to_string(p)?;
        if text.contains("__PROJECT_NAME__") {
            std::fs::write(p, text.replace("__PROJECT_NAME__", project_name))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn cmd_list() -> anyhow::Result<()> {
    let projects = PathBuf::from("projects");
    if !projects.is_dir() {
        println!("(no projects/ directory)");
        return Ok(());
    }
    let mut found = false;
    for entry in std::fs::read_dir(&projects)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        // проект = папка с Cargo.toml
        if entry.path().join("Cargo.toml").is_file() {
            println!("{name}");
            found = true;
        }
    }
    if !found {
        println!("(no projects)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// assets init
// ---------------------------------------------------------------------------

fn cmd_assets_init(project: &Path) -> anyhow::Result<()> {
    let root = project.join("assets");
    if !root.is_dir() {
        anyhow::bail!("{}: no `assets/` directory", project.display());
    }
    let n = rust_engine::assets::meta::generate_missing_metas(&root)?;
    println!("generated {n} .meta file(s) under {}", root.display());
    Ok(())
}