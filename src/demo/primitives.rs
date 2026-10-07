use glam::{Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::ai::{AiAgent, DebugPath, Enemy, PatrolPath};
use crate::game::audio::{AudioBus, AudioSource};
use crate::game::components::*;
use crate::game::decals::Decal;
use crate::game::lights::PointLight;
use crate::game::timers::Timer;
use crate::physics::{Collider, PhysicsMaterial, RigidBody};

// ============================================================
// СТРУКТУРНЫЕ
// ============================================================

/// Статический ящик/стена. Базовый строительный блок.
pub fn static_box(
    world: &mut World,
    name: impl Into<String>,
    material: &str,
    center: Vec3,
    size: Vec3,
    collider_half: Vec3,
    tiling: f32,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform {
        position: center,
        rotation: Quat::IDENTITY,
        scale: size,
    });
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle(material.to_string()));
    world.insert(e, TextureTiling::new(tiling));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(collider_half));
    world.insert(e, PhysicsMaterial::concrete());
    e
}

/// Стена между двумя точками XZ на высоте `y0` высотой `height`.
///
/// **Исправлено:** rotation теперь имеет значение — коллайдер
/// поворачивается вместе с Transform. До фикса rotation `Wall_W`
/// и `Wall_E` имели перпендикулярный коллайдер.
pub fn wall(
    world: &mut World,
    name: impl Into<String>,
    from: Vec3,
    to: Vec3,
    y0: f32,
    height: f32,
    thickness: f32,
    material: &str,
    tiling: f32,
) -> Entity {
    let mid = (from + to) * 0.5;
    let d = to - from;
    let len = (d.x * d.x + d.z * d.z).sqrt().max(0.01);
    let angle = d.z.atan2(d.x);
    let rot = Quat::from_axis_angle(Vec3::Y, -angle);

    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform {
        position: Vec3::new(mid.x, y0 + height * 0.5, mid.z),
        rotation: rot,
        scale: Vec3::new(len, height, thickness),
    });
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle(material.to_string()));
    world.insert(e, TextureTiling::new(tiling));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(Vec3::new(0.5, 0.5, 0.5)));
    world.insert(e, PhysicsMaterial::concrete());
    e
}

/// Статический объект с кастомным mesh.
pub fn static_mesh(
    world: &mut World,
    name: impl Into<String>,
    mesh: &str,
    material: &str,
    pos: Vec3,
    rot: Quat,
    scale: Vec3,
    collider: Option<Collider>,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform { position: pos, rotation: rot, scale });
    world.insert(e, MeshHandle(mesh.to_string()));
    world.insert(e, MaterialHandle(material.to_string()));
    if let Some(c) = collider {
        world.insert(e, RigidBody::static_body());
        world.insert(e, c);
    }
    e
}

/// Пол-плитка (декоративная, без коллайдера).
///
/// Полезно для визуального разнообразия: разные материалы и tiling
/// на небольших участках пола.
pub fn floor_tile(
    world: &mut World,
    name: impl Into<String>,
    material: &str,
    center: Vec3,
    size: Vec3,
    tiling: f32,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform {
        position: center,
        rotation: Quat::IDENTITY,
        scale: size,
    });
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle(material.to_string()));
    world.insert(e, TextureTiling::new(tiling));
    e
}

// ============================================================
// ОСВЕЩЕНИЕ И ДЕКОР
// ============================================================

/// Факел: warm point light + emissive сфера + looping audio.
pub fn torch(world: &mut World, name: impl Into<String>, pos: Vec3) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(0.15));
    world.insert(e, MeshHandle("sphere".into()));
    world.insert(e, MaterialHandle("emissive_warm".into()));
    world.insert(e, PointLight::new([1.0, 0.55, 0.2], 5.0, 12.0));
    world.insert(e, Spinner::new(Vec3::Y, 0.6));
    world.insert(e, AudioSource::looping("ding")
        .with_bus(AudioBus::Music)
        .with_volume(0.15)
        .with_range(1.0, 6.0));
    e
}

/// Магический кристалл: cold point light + emissive cone + вращение.
pub fn crystal(world: &mut World, name: impl Into<String>, pos: Vec3, color: [f32; 3]) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(0.35));
    world.insert(e, MeshHandle("cone".into()));
    world.insert(e, MaterialHandle("emissive_cold".into()));
    world.insert(e, PointLight::new(color, 4.0, 14.0));
    world.insert(e, Spinner::new(Vec3::new(0.3, 1.0, 0.2), 1.2));
    world.insert(e, Tint([color[0] * 1.5, color[1] * 1.5, color[2] * 1.5, 1.0]));
    e
}

pub fn brazier(world: &mut World, name: impl Into<String>, pos: Vec3) -> Entity {
    let name: String = name.into();

    // Чаша.
    let bowl = world.spawn();
    world.insert(bowl, Name(format!("{}_Bowl", name)));
    world.insert(bowl, Transform::at(pos).with_scale_xyz(0.8, 0.4, 0.8));
    world.insert(bowl, MeshHandle("cylinder".into()));
    world.insert(bowl, MaterialHandle("arena_barrel".into()));
    world.insert(bowl, RigidBody::static_body());
    world.insert(bowl, Collider::aabb(Vec3::splat(0.5)));

    // Огонь.
    let fire = world.spawn();
    world.insert(fire, Name(format!("{}_Fire", name)));
    world.insert(fire, Transform::at(pos + Vec3::Y * 0.6).with_scale(0.3));
    world.insert(fire, MeshHandle("sphere".into()));
    world.insert(fire, MaterialHandle("emissive_warm".into()));
    world.insert(fire, PointLight::new([1.0, 0.5, 0.15], 8.0, 16.0));
    world.insert(fire, Spinner::new(Vec3::Y, 1.0));
    fire
}

// ============================================================
// DECALS
// ============================================================

/// Кровавое пятно.
pub fn blood_decal(world: &mut World, pos: Vec3, size: f32, alpha: f32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name("Decal_Blood".into()));
    world.insert(e, Transform {
        position: pos,
        rotation: Quat::from_axis_angle(Vec3::Y, (pos.x + pos.z) * 0.3),
        scale: Vec3::new(size, 0.01, size),
    });
    world.insert(e, Decal {
        texture: "arena_blood".into(),
        tint: [0.7, 0.05, 0.05, alpha],
    });
    e
}

/// Подпалина от взрыва/огня.
pub fn scorch_decal(world: &mut World, pos: Vec3, size: f32, alpha: f32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name("Decal_Scorch".into()));
    world.insert(e, Transform {
        position: pos,
        rotation: Quat::from_axis_angle(Vec3::Y, (pos.x * 1.7 + pos.z) * 0.4),
        scale: Vec3::new(size, 0.01, size),
    });
    world.insert(e, Decal {
        texture: "arena_blood".into(),
        tint: [0.05, 0.04, 0.03, alpha],
    });
    e
}

/// Руна (синяя/фиолетовая, для подземелья).
pub fn rune_decal(world: &mut World, pos: Vec3, size: f32, color: [f32; 4]) -> Entity {
    let e = world.spawn();
    world.insert(e, Name("Decal_Rune".into()));
    world.insert(e, Transform {
        position: pos,
        rotation: Quat::from_axis_angle(Vec3::Y, (pos.x - pos.z) * 0.5),
        scale: Vec3::new(size, 0.01, size),
    });
    world.insert(e, Decal {
        texture: "arena_blood".into(),
        tint: color,
    });
    e
}

// ============================================================
// ПРОПСЫ
// ============================================================

/// Статический ящик.
pub fn crate_box(world: &mut World, name: impl Into<String>, pos: Vec3, size: f32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(size));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("arena_crate".into()));
    world.insert(e, TextureTiling::new(1.0));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
    world.insert(e, PhysicsMaterial::wood());
    e
}

/// Динамический ящик — падает, можно толкать.
pub fn dynamic_crate(world: &mut World, name: impl Into<String>, pos: Vec3, size: f32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(size));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("arena_barrel".into()));
    world.insert(e, TextureTiling::new(1.0));
    world.insert(e, RigidBody::dynamic(2.0));
    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
    world.insert(e, PhysicsMaterial::wood());
    e
}

/// Бочка (статическая, декоративная).
pub fn barrel(world: &mut World, name: impl Into<String>, pos: Vec3) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale_xyz(0.8, 1.2, 0.8));
    world.insert(e, MeshHandle("cylinder".into()));
    world.insert(e, MaterialHandle("arena_barrel".into()));
    world.insert(e, TextureTiling::new(1.0));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
    world.insert(e, PhysicsMaterial::wood());
    e
}

/// Сундук с золотом. Играет "pickup" при подходе.
pub fn chest(world: &mut World, name: impl Into<String>, pos: Vec3, gold: u32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale_xyz(0.9, 0.5, 0.7));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("arena_crate".into()));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
    world.insert(e, PhysicsMaterial::wood());
    world.insert(e, Spinner::new(Vec3::Y, 0.3));

    // Метка для fortress.rs: Tint-цвет кодирует стоимость.
    world.insert(e, Trigger::repeatable(
        1.5,
        TriggerAction::PlaySound("pickup".into()),
    ));

    // Стоимость через Tint (используется fortress.rs для RPG).
    world.insert(e, Tint([
        1.0,
        (gold as f32 / 2000.0).min(1.0),
        0.0,
        1.0,
    ]));

    e
}

/// Куча щебня (декор).
pub fn rubble(world: &mut World, name: impl Into<String>, pos: Vec3, size: f32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform {
        position: pos,
        rotation: Quat::from_axis_angle(Vec3::Y, pos.x * 0.7),
        scale: Vec3::new(size, size * 0.5, size),
    });
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("rpg_dungeon".into()));
    world.insert(e, TextureTiling::new(0.6));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
    e
}

// ============================================================
// ПИКАПЫ (для кампании)
// ============================================================

/// Ключ. Игрок подходит → звук → fortress.rs ловит и записывает
/// `key_<id>` в state.
///
/// Имя сущности: `key_<id>` — fortress.rs ищет по префиксу.
pub fn key_pickup(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    color: [f32; 3],
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(0.4));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("emissive".into()));
    world.insert(e, Spinner::new(Vec3::new(0.2, 1.0, 0.4), 2.0));
    world.insert(e, Tint([color[0], color[1], color[2], 1.0]));

    world.insert(e, Trigger::new(
        1.5,
        TriggerAction::PlaySound("pickup".into()),
    ));

    // Point-light для заметности.
    world.insert(e, PointLight::new(color, 2.5, 6.0));
    e
}

/// Аптечка. `amount` — сколько HP восстанавливает.
///
/// fortress.rs при срабатывании триггера читает это значение из
/// Tint.r (кодируется) и увеличивает HP игрока.
pub fn health_pickup(world: &mut World, name: impl Into<String>, pos: Vec3, amount: f32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(0.35));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("emissive".into()));
    world.insert(e, Spinner::new(Vec3::Y, 1.5));
    // Кодируем amount в Tint.r.
    world.insert(e, Tint([amount / 100.0, 1.0, 0.3, 1.0]));

    world.insert(e, Trigger::new(
        1.2,
        TriggerAction::PlaySound("pickup".into()),
    ));
    world.insert(e, PointLight::new([0.3, 1.0, 0.4], 2.0, 5.0));
    e
}

/// Патроны. `amount` — сколько патронов.
pub fn ammo_pickup(world: &mut World, name: impl Into<String>, pos: Vec3, amount: u32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale_xyz(0.4, 0.25, 0.25));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("gold".into()));
    world.insert(e, Spinner::new(Vec3::Y, 1.8));
    world.insert(e, Tint([(amount as f32 / 100.0).min(1.0), 0.8, 0.2, 1.0]));

    world.insert(e, Trigger::new(
        1.2,
        TriggerAction::PlaySound("pickup".into()),
    ));
    world.insert(e, PointLight::new([1.0, 0.8, 0.2], 1.8, 4.0));
    e
}

// ============================================================
// КАМПАЙН-ОБЪЕКТЫ
// ============================================================

/// Точка сохранения. При входе — звук + игра сохраняет позицию.
///
/// Имя: `checkpoint_<n>`.
pub fn checkpoint(world: &mut World, name: impl Into<String>, pos: Vec3) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale_xyz(0.6, 0.05, 0.6));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("emissive".into()));
    world.insert(e, Tint([0.3, 0.8, 1.0, 0.7]));
    world.insert(e, Spinner::new(Vec3::Y, 0.4));

    world.insert(e, Trigger::new(
        2.0,
        TriggerAction::PlaySound("ding".into()),
    ));
    world.insert(e, PointLight::new([0.3, 0.8, 1.0], 2.0, 6.0));
    e
}

/// Светящийся маркер цели. Куда идти.
///
/// Имя: `objective_<tag>` (например `objective_key`, `objective_boss`).
/// fortress.rs находит маркер по `Name` и делает видимым/невидимым.
pub fn objective_marker(world: &mut World, name: impl Into<String>, pos: Vec3) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(0.5));
    world.insert(e, MeshHandle("sphere".into()));
    world.insert(e, MaterialHandle("arena_marker".into()));
    world.insert(e, Spinner::new(Vec3::Y, 1.0));
    world.insert(e, PointLight::new([1.0, 0.9, 0.4], 3.5, 10.0));
    e
}

/// Зона с текстом. При входе — fortress.rs показывает диалог.
///
/// Имя: `dialog_<tag>`. fortress.rs маппит tag → текст.
pub fn dialog_zone(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    radius: f32,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos));
    world.insert(e, Trigger::new(
        radius,
        TriggerAction::PlaySound("ding".into()),
    ));
    // Невидимый — без mesh/material.
    e
}

/// Финишная зона. При входе — победа.
///
/// Имя: `exit_zone`.
pub fn exit_zone(world: &mut World, name: impl Into<String>, pos: Vec3, radius: f32) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(1.5));
    world.insert(e, MeshHandle("sphere".into()));
    world.insert(e, MaterialHandle("arena_marker".into()));
    world.insert(e, Spinner::new(Vec3::Y, 2.0));
    world.insert(e, PointLight::new([1.0, 0.5, 0.2], 4.0, 12.0));
    world.insert(e, Trigger::repeatable(
        radius,
        TriggerAction::PlaySound("explosion".into()),
    ));
    e
}

/// Универсальный триггер-маркер для кампайн-событий.
///
/// Имя: `<tag>_<n>`. fortress.rs читает имя и понимает, что делать
/// (например `gate_open_1`, `puzzle_button_2`).
pub fn campaign_trigger(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    radius: f32,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos));
    world.insert(e, Trigger::repeatable(
        radius,
        TriggerAction::PlaySound("ding".into()),
    ));
    e
}

/// Запертые ворота. Открываются fortress.rs по наличию ключа.
///
/// `size` — размеры створки (X, Y, Z) в мировых единицах.
pub fn locked_gate(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    size: Vec3,
    material: &str,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform {
        position: pos,
        rotation: Quat::IDENTITY,
        scale: size,
    });
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle(material.to_string()));
    world.insert(e, TextureTiling::new(1.5));
    world.insert(e, RigidBody::kinematic());
    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
    world.insert(e, Velocity::new(0.0, 0.0, 0.0));
    // Звук открытия (playing=false, активируется fortress-ом).
    world.insert(e, AudioSource::new("ding")
        .with_bus(AudioBus::Music)
        .with_volume(0.5)
        .with_range(2.0, 15.0));
    // Изначально playing=false — не играет.
    if let Some(src) = world.get_mut::<AudioSource>(e) {
        src.playing = false;
    }
    e
}

// ============================================================
// AI
// ============================================================

/// Тип врага. Определяет stats и внешний вид.
pub enum EnemyKind {
    /// Патрульный: средняя скорость, среднее зрение, ближний бой.
    Patrol,
    /// Снайпер: медленный, дальнее зрение, большой урон, дальняя атака.
    Sniper,
    /// Босс: медленный, много HP, ближний бой, большой радиус.
    Boss,
    /// Мини-босс (для ACT 3): быстрее обычного, немного HP босса.
    Elite,
}

impl EnemyKind {
    /// (speed, vision_range, vision_half_angle_rad, hearing,
    ///  attack_range, color, scale, hp, attack_damage, cooldown)
    fn stats(&self) -> (f32, f32, f32, f32, f32, [f32; 3], f32, f32, f32, f32) {
        match self {
            EnemyKind::Patrol => (
                3.2, 16.0, 60f32.to_radians(), 22.0,
                1.8, [1.0, 0.35, 0.35], 0.55,
                60.0, 12.0, 0.9,
            ),
            EnemyKind::Sniper => (
                2.0, 30.0, 40f32.to_radians(), 15.0,
                12.0, [0.9, 0.7, 0.2], 0.6,
                50.0, 25.0, 2.0,
            ),
            EnemyKind::Boss => (
                2.2, 22.0, 70f32.to_radians(), 30.0,
                3.0, [0.65, 0.05, 0.05], 1.6,
                400.0, 30.0, 1.2,
            ),
            EnemyKind::Elite => (
                4.0, 20.0, 65f32.to_radians(), 25.0,
                2.2, [0.85, 0.3, 0.85], 0.8,
                120.0, 18.0, 0.7,
            ),
        }
    }

    fn material(&self) -> &'static str {
        match self {
            EnemyKind::Patrol => "flat_red",
            EnemyKind::Sniper => "gold",
            EnemyKind::Boss => "arena_marker",
            EnemyKind::Elite => "flat_red",
        }
    }

    fn mesh(&self) -> &'static str {
        match self {
            EnemyKind::Boss => "sphere",
            _ => "capsule",
        }
    }
}

/// Создать врага.
///
/// **Коллизия:** враг получает `Collider::capsule(0.5, 1.8)` —
/// игрок упирается в него, не проходит насквозь. `RigidBody` НЕ
/// добавляется — враг двигается AI-системой вручную. В
/// `engine::collision::capsule_hits_impl` коллайдер без RigidBody
/// трактуется как блокирующий (эквивалент Static).
///
/// **Позиция:** `pos` — ноги. `Transform.position` = центр капсулы
/// (pos.y + half_height_world).
pub fn enemy(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    kind: EnemyKind,
    patrol: Option<Vec<Vec3>>,
) -> Entity {
    let (
        speed, vision_range, vision_half, hearing,
        attack_range, _, scale, hp, damage, cooldown,
    ) = kind.stats();

    // Позиция центра капсулы (mesh "capsule" центрирован по origin).
    let local_half_h = 0.9;
    let center = Vec3::new(pos.x, pos.y + local_half_h * scale, pos.z);

    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(center).with_scale(scale));
    world.insert(e, MeshHandle(kind.mesh().into()));
    world.insert(e, MaterialHandle(kind.material().to_string()));
    world.insert(e, Health::new(hp));
    world.insert(e, Enemy);

    // Коллайдер: локальные размеры не масштабируются — коллайдер
    // применяется к world после умножения на Transform.scale
    // (см. `Collider::world_aabb`).
    world.insert(e, Collider::capsule(0.5, 1.8));

    let mut agent = AiAgent::new()
        .with_speed(speed)
        .with_vision(vision_range, vision_half)
        .with_hearing(hearing)
        .with_attack(attack_range, damage, cooldown);

    // Elite поворачивается быстрее.
    if matches!(kind, EnemyKind::Elite) {
        agent.turn_speed = 12.0;
    }

    world.insert(e, agent);

    if let Some(points) = patrol {
        world.insert(e, PatrolPath::new(points));
        world.insert(e, DebugPath);
    }

    // Стационарный снайпер без PatrolPath вообще не двигается —
    // AiSystem не вызывает follow_path без navmesh-пути.
    e
}

// ============================================================
// ПОРТАЛЫ
// ============================================================

/// Портал. Телепортирует + PlaySound + emissive + point light.
pub fn portal(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    destination: Vec3,
    color: [f32; 4],
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale_xyz(2.0, 3.0, 0.5));
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle("glass".into()));
    world.insert(e, Tint(color));
    world.insert(e, Trigger::repeatable(
        1.8,
        TriggerAction::Teleport(destination.to_array()),
    ));
    world.insert(e, Spinner::new(Vec3::Y, 1.5));
    world.insert(e, AudioSource::looping("ding")
        .with_bus(AudioBus::Music)
        .with_volume(0.3)
        .with_range(2.0, 8.0));
    world.insert(e, PointLight::new(
        [color[0] * 3.0, color[1] * 3.0, color[2] * 3.0],
        3.0,
        8.0,
    ));
    e
}

// ============================================================
// ХЕЛПЕРЫ
// ============================================================

/// Пометить entity тегом кампании (записывает в `Name` префикс).
/// Полезно для сущностей, созданных вручную, чтобы fortress.rs
/// мог их найти.
pub fn tag_campaign_entity(world: &mut World, e: Entity, prefix: &str, suffix: &str) {
    let full = format!("{}_{}", prefix, suffix);
    world.insert(e, Name(full));
}

/// Создать invisible timer-spawner: спавнит волну врагов через
/// `delay` секунд. fortress.rs использует `Timer` + `Name("wave_N")`
/// для идентификации.
pub fn wave_timer(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    delay: f32,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos));
    world.insert(e, Timer::new(delay));
    e
}