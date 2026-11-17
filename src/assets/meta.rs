use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    #[serde(default = "default_meta_format")]
    pub format: u32,
    pub id: String,
    pub asset_type: String,
    pub source_hash: String,
    #[serde(default)]
    pub import_settings: ron::Value,
    #[serde(default)]
    pub dependencies: Vec<AssetDependency>,
}

fn default_meta_format() -> u32 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetDependency {
    pub id: String,
    pub kind: String,
}

pub fn meta_path_for(asset: &Path) -> PathBuf {
    let mut p = asset.as_os_str().to_owned();
    p.push(".meta");
    PathBuf::from(p)
}

fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

fn generate_id(seed_path: &Path) -> String {
    let rs = RandomState::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let mut h1 = rs.build_hasher();
    now.hash(&mut h1);
    seed_path.hash(&mut h1);
    std::process::id().hash(&mut h1);

    let mut h2 = rs.build_hasher();
    h1.finish().hash(&mut h2);
    now.hash(&mut h2);

    let mut a = h1.finish().to_le_bytes();
    let b = h2.finish().to_le_bytes();
    a[6] = (a[6] & 0x0f) | 0x40;
    a[8] = (a[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7],
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    )
}

pub fn ensure_meta(asset: &Path, asset_type: &str) -> Result<AssetMeta> {
    let mp = meta_path_for(asset);
    if mp.exists() {
        let text = std::fs::read_to_string(&mp)
            .with_context(|| format!("read {:?}", mp))?;
        return ron::from_str(&text)
            .with_context(|| format!("parse {:?}", mp));
    }

    #[cfg(any(feature = "editor", feature = "hot-reload"))]
    {
        let bytes = std::fs::read(asset)
            .with_context(|| format!("read {:?}", asset))?;
        let meta = AssetMeta {
            format: default_meta_format(),
            id: generate_id(asset),
            asset_type: asset_type.to_string(),
            source_hash: fnv1a_hex(&bytes),
            import_settings: ron::Value::Unit,
            dependencies: vec![],
        };
        let s = ron::ser::to_string_pretty(
            &meta,
            ron::ser::PrettyConfig::default(),
        )?;
        std::fs::write(&mp, s).with_context(|| format!("write {:?}", mp))?;
        log::warn!("generated {:?} — commit it to VCS", mp);
        Ok(meta)
    }
    #[cfg(not(any(feature = "editor", feature = "hot-reload")))]
    {
        bail!(
            "missing .meta for asset {:?}. \
             Runtime builds are read-only. Generate .meta via \
             `engine-cli assets init` (in a dev environment) and commit it.",
            asset
        );
    }
}

pub fn generate_missing_metas(assets_root: &Path) -> Result<usize> {
    let mut count = 0usize;
    walk(assets_root, &mut count)?;
    Ok(count)
}

fn walk(dir: &Path, count: &mut usize) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("read_dir {:?}", dir))?
    {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk(&path, count)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("meta") {
            continue;
        }
        // Пропускаем сам `.meta`-уже-рядом.
        if meta_path_for(&path).exists() {
            continue;
        }
        let ty = infer_asset_type(&path);
        ensure_meta(&path, &ty)?;
        *count += 1;
    }
    Ok(())
}

pub fn infer_asset_type(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "ron" | "json" => "Ron",
        "png" | "jpg" | "jpeg" | "ktx2" | "dds" | "hdr" | "exr" => "Texture",
        "gltf" | "glb" | "fbx" => "Model",
        "ogg" | "wav" | "flac" | "mp3" => "Audio",
        "wgsl" => "Shader",
        _ => "Unknown",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_id_survives_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("potion.ron");
        std::fs::write(&a, "(id:\"health_potion\")").unwrap();

        #[cfg(any(feature = "editor", feature = "hot-reload"))]
        {
            let m1 = ensure_meta(&a, "Ron").unwrap();
            let meta_a = meta_path_for(&a);
            let b = tmp.path().join("renamed.ron");
            let meta_b = meta_path_for(&b);
            std::fs::rename(&a, &b).unwrap();
            std::fs::rename(&meta_a, &meta_b).unwrap();
            let m2 = ensure_meta(&b, "Ron").unwrap();
            assert_eq!(m1.id, m2.id, "id must be stable across rename");
        }
    }

    #[test]
    fn fnv_is_deterministic() {
        assert_eq!(fnv1a_hex(b"hello"), fnv1a_hex(b"hello"));
        assert_ne!(fnv1a_hex(b"hello"), fnv1a_hex(b"world"));
    }
}