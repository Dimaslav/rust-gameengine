use crate::ecs::{Entity, World};
use crate::game::ai::AiTarget;
use crate::game::components::{Health, Transform, Velocity};
use crate::render::Camera3D;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeleeAttackKind { Light, Heavy }

impl Default for MeleeAttackKind {
    fn default() -> Self { Self::Light }
}

#[derive(Debug, Clone, Copy)]
pub struct MeleeWeapon {
    pub damage: f32,
    pub range: f32,
    pub arc_degrees: f32,
    pub cooldown: f32,
    pub combo_window: f32,
    pub knockback: f32,
}

impl MeleeWeapon {
    pub fn sword() -> Self {
        Self {
            damage: 35.0, range: 2.5, arc_degrees: 90.0,
            cooldown: 0.55, combo_window: 0.6, knockback: 4.0,
        }
    }
    pub fn fists() -> Self {
        Self {
            damage: 10.0, range: 1.6, arc_degrees: 60.0,
            cooldown: 0.35, combo_window: 0.4, knockback: 1.5,
        }
    }
    pub fn heavy_cooldown(&self) -> f32 { self.cooldown * 1.6 }
}

impl Default for MeleeWeapon {
    fn default() -> Self { Self::sword() }
}

#[derive(Debug, Clone, Copy)]
pub struct MeleeState {
    pub combo_index: u8,
    pub combo_timer: f32,
    pub attack_timer: f32,
    pub pending_hit: bool,
    pub pending_timer: f32,
    pub pending_kind: MeleeAttackKind,
}

impl Default for MeleeState {
    fn default() -> Self {
        Self {
            combo_index: 0,
            combo_timer: 0.0,
            attack_timer: 0.0,
            pending_hit: false,
            pending_timer: 0.0,
            pending_kind: MeleeAttackKind::Light,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PendingHit {
    pub combo_index: u8,
    pub kind: MeleeAttackKind,
}

#[derive(Debug, Clone, Copy)]
pub struct MeleeHitResult {
    pub target: Entity,
    pub damage: f32,
    pub is_crit: bool,
}

/// Начать атаку. `None` = на кулдауне, удар не начат.
pub fn start_attack(
    state: &mut MeleeState,
    weapon: &MeleeWeapon,
    kind: MeleeAttackKind,
) -> Option<u8> {
    if state.attack_timer > 0.0 {
        return None;
    }
    let combo_idx = match kind {
        MeleeAttackKind::Heavy => 0,
        MeleeAttackKind::Light => {
            if state.combo_timer > 0.0 {
                (state.combo_index + 1) % 3
            } else { 0 }
        }
    };
    state.combo_index = combo_idx;
    state.combo_timer = weapon.combo_window;
    state.attack_timer = match kind {
        MeleeAttackKind::Heavy => weapon.heavy_cooldown(),
        MeleeAttackKind::Light => weapon.cooldown,
    };
    state.pending_hit = true;
    state.pending_timer = 0.15;
    state.pending_kind = kind;
    Some(combo_idx)
}

/// Тикать таймеры каждый кадр. Возвращает `Some` при fallback-срабатывании.
pub fn tick_timers(state: &mut MeleeState, dt: f32) -> Option<PendingHit> {
    state.combo_timer = (state.combo_timer - dt).max(0.0);
    state.attack_timer = (state.attack_timer - dt).max(0.0);

    if state.pending_hit {
        state.pending_timer -= dt;
        if state.pending_timer <= 0.0 {
            state.pending_hit = false;
            return Some(PendingHit {
                combo_index: state.combo_index,
                kind: state.pending_kind,
            });
        }
    }
    None
}

/// Вызывается из `on_animation_event("melee_hit")`.
/// Отменяет fallback и возвращает pending-атаку, если она есть.
pub fn consume_animation_hit(state: &mut MeleeState) -> Option<PendingHit> {
    if !state.pending_hit { return None; }
    state.pending_hit = false;
    Some(PendingHit {
        combo_index: state.combo_index,
        kind: state.pending_kind,
    })
}

pub fn perform_hit(
    world: &mut World,
    camera: &Camera3D,
    weapon: &MeleeWeapon,
    attack: PendingHit,
) -> Vec<MeleeHitResult> {
    let origin = camera.first_person_pos;
    let forward = camera.forward().normalize_or_zero();
    if forward.length_squared() < 1e-6 { return Vec::new(); }
    let damage_mult = match (attack.kind, attack.combo_index) {
        (MeleeAttackKind::Heavy, _) => 1.35,
        (MeleeAttackKind::Light, 0) => 1.0,
        (MeleeAttackKind::Light, 1) => 1.15,
        (MeleeAttackKind::Light, _) => 1.30,
    };
    let damage = weapon.damage * damage_mult;
    let knockback = weapon.knockback
        * if matches!(attack.kind, MeleeAttackKind::Heavy) { 2.0 } else { 1.0 };
    let is_crit = matches!(attack.kind, MeleeAttackKind::Heavy) || attack.combo_index == 2;

    let cos_half_arc = (weapon.arc_degrees * 0.5).to_radians().cos();

    let candidates: Vec<Entity> = world
        .query::<Health>()
        .filter(|(e, h)| {
            h.current > 0.0
                && world.get::<Transform>(*e).is_some()
                && !world.has::<AiTarget>(*e)
        })
        .map(|(e, _)| e)
        .collect();

    let mut results = Vec::new();
    for target in candidates {
        let Some(tpos) = world.get::<Transform>(target).map(|t| t.position) else {
            continue;
        };
        let to_target = tpos - origin;
        let dist = to_target.length();
        if dist > weapon.range || dist < 1e-4 { continue; }
        let dir = to_target / dist;
        if dir.dot(forward) < cos_half_arc { continue; }

        if let Some(h) = world.get_mut::<Health>(target) {
            h.current = (h.current - damage).max(0.0);
            results.push(MeleeHitResult { target, damage, is_crit });
        }
        if let Some(v) = world.get_mut::<Velocity>(target) {
            v.value += dir * knockback;
        }
    }
    results
}