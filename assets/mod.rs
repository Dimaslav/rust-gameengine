//! Asset pipeline движка.
//!
//! * `AssetId` — стабильный UUID ассета.
//! * `AssetMeta` — данные `.meta`-файла рядом с ассетом.
//! * `AssetDatabase` — сканирует `assets/`, следит за изменениями.
//! * `HotReload` — polling-watcher, перезагружает текстуры при
//!   изменении файла на диске.

pub mod database;
pub mod hotreload;
pub mod id;
pub mod meta;

pub use database::AssetDatabase;
pub use hotreload::HotReload;
pub use id::AssetId;
pub use meta::{AssetKind, AssetMeta, AssetMetaFile, ImportSettings};