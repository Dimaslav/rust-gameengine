//! Стабильный ID ассета. Не зависит от пути, не ломается при
//! переименовании файла. Аналог GUID в Unity / FGuid в Unreal.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetId(pub u128);

impl AssetId {
    /// Случайный 128-битный ID. Источник энтропии — время + адрес
    /// стека + PID. Не криптостойкий, но для collision-resistance
    /// 128 бит достаточно.
    pub fn new_random() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let stack_addr = &now as *const _ as u128;
        let pid = std::process::id() as u128;

        // splitmix64-микс.
        let mut z = now ^ (stack_addr << 64) ^ (pid << 32);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z = z ^ (z >> 31);

        let mut w = stack_addr ^ (now << 32) ^ pid;
        w = (w ^ (w >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        w = (w ^ (w >> 27)).wrapping_mul(0x94D049BB133111EB);
        w = w ^ (w >> 31);

        Self((z << 64) | (w & 0xFFFF_FFFF_FFFF_FFFF))
    }

    /// Нулевой ID — используется как "пустая ссылка".
    pub const NIL: AssetId = AssetId(0);

    pub fn is_nil(&self) -> bool {
        self.0 == 0
    }
}

impl Default for AssetId {
    fn default() -> Self {
        Self::NIL
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0.to_be_bytes();
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15],
        )
    }
}