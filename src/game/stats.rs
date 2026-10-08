//! RPG-статы игрока: атрибуты, опыт, уровни.
//!
//! `PlayerStats` — синглтон-структура, живёт в `FortressDemo`. Не ECS
//! компонент, потому что игрок один и статы нужны в HUD/формулах.

use serde::{Deserialize, Serialize};

pub const STAT_NAMES: [&str; 5] = [
    "Strength", "Dexterity", "Intelligence", "Vitality", "Luck",
];

pub const STAT_DESCRIPTIONS: [&str; 5] = [
    "+5% melee damage",
    "+5% ranged damage",
    "+5% magic damage",
    "+10 max HP",
    "+1% crit, +2% crit dmg",
];

// ============================================================
// Attributes
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Attributes {
    pub strength: u32,
    pub dexterity: u32,
    pub intelligence: u32,
    pub vitality: u32,
    pub luck: u32,
}

impl Default for Attributes {
    fn default() -> Self {
        Self {
            strength: 5,
            dexterity: 5,
            intelligence: 5,
            vitality: 5,
            luck: 5,
        }
    }
}

impl Attributes {
    pub fn melee_damage_mult(&self) -> f32 {
        1.0 + self.strength as f32 * 0.05
    }
    pub fn ranged_damage_mult(&self) -> f32 {
        1.0 + self.dexterity as f32 * 0.05
    }
    pub fn magic_damage_mult(&self) -> f32 {
        1.0 + self.intelligence as f32 * 0.05
    }
    pub fn max_hp_bonus(&self) -> f32 {
        self.vitality as f32 * 10.0
    }
    pub fn crit_chance(&self) -> f32 {
        (0.05 + self.luck as f32 * 0.01).min(0.5)
    }
    pub fn crit_mult(&self) -> f32 {
        1.5 + self.luck as f32 * 0.02
    }

    /// Доступ к стате по индексу (для UI-кнопок).
    pub fn stat_mut(&mut self, idx: usize) -> Option<&mut u32> {
        match idx {
            0 => Some(&mut self.strength),
            1 => Some(&mut self.dexterity),
            2 => Some(&mut self.intelligence),
            3 => Some(&mut self.vitality),
            4 => Some(&mut self.luck),
            _ => None,
        }
    }

    pub fn get(&self, idx: usize) -> u32 {
        match idx {
            0 => self.strength,
            1 => self.dexterity,
            2 => self.intelligence,
            3 => self.vitality,
            4 => self.luck,
            _ => 0,
        }
    }
}

// ============================================================
// Experience
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Experience {
    pub level: u32,
    pub xp: u32,
    pub xp_to_next: u32,
    pub total_xp: u32,
}

impl Default for Experience {
    fn default() -> Self {
        Self {
            level: 1,
            xp: 0,
            xp_to_next: 100,
            total_xp: 0,
        }
    }
}

impl Experience {
    /// Добавить XP. Возвращает количество поднятых уровней.
    pub fn add_xp(&mut self, amount: u32) -> u32 {
        self.xp = self.xp.saturating_add(amount);
        self.total_xp = self.total_xp.saturating_add(amount);
        let mut levels_gained = 0;
        while self.xp >= self.xp_to_next {
            self.xp -= self.xp_to_next;
            self.level += 1;
            levels_gained += 1;
            // Кривая: следующий уровень требует на 25% больше + 50.
            self.xp_to_next = (self.xp_to_next as f32 * 1.25) as u32 + 50;
        }
        levels_gained
    }

    pub fn progress(&self) -> f32 {
        if self.xp_to_next == 0 { 0.0 }
        else { self.xp as f32 / self.xp_to_next as f32 }
    }
}

// ============================================================
// PlayerStats
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStats {
    pub attributes: Attributes,
    pub experience: Experience,
    pub unspent_points: u32,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            attributes: Attributes::default(),
            experience: Experience::default(),
            unspent_points: 0,
        }
    }
}

impl PlayerStats {
    /// Получить опыт. Возвращает количество поднятых уровней.
    /// На каждый уровень даём 2 очка характеристик.
    pub fn gain_xp(&mut self, amount: u32) -> u32 {
        let levels = self.experience.add_xp(amount);
        if levels > 0 {
            self.unspent_points += levels * 2;
        }
        levels
    }

    /// Вложить 1 очко в стат с индексом 0..4. `true`, если успешно.
    pub fn spend_point(&mut self, idx: usize) -> bool {
        if self.unspent_points == 0 { return false; }
        if let Some(s) = self.attributes.stat_mut(idx) {
            *s = s.saturating_add(1);
            self.unspent_points -= 1;
            true
        } else {
            false
        }
    }

    pub fn max_hp(&self, base: f32) -> f32 {
        base + self.attributes.max_hp_bonus()
    }
}