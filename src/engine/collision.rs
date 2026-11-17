use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::{CharacterConfig, CharacterController};
use crate::physics::Collider;
use crate::render::terrain::Heightmap;

use super::collision_cache::ColliderCache;

const SUPPORT_TOLERANCE: f32 = 0.15;
const SUBSTEP_SIZE: f32 = 0.15;

#[derive(Clone, Copy)]
pub struct PlayerCapsule {
    pub radius: f32,
    pub height: f32,
}

impl Default for PlayerCapsule {
    fn default() -> Self {
        Self { radius: 0.35, height: 1.8 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SupportInfo {
    pub entity: Entity,
    pub normal: Vec3,
    pub top_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct MovementResult {
    pub new_feet: Vec3,
    pub landed: bool,
    pub support: Option<SupportInfo>,
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let t = (p - a).dot(ab) / ab.length_squared().max(1e-12);
    a + ab * t.clamp(0.0, 1.0)
}

fn surface_normal_at_entry(e: &super::collision_cache::ColliderEntry, probe: Vec3) -> Vec3 {
    // C1.2: для mesh-коллайдера сначала пробуем найти нормаль через
    // raycast вниз. Если попали — возвращаем нормаль треугольника.
    if let Some(mesh) = &e.mesh
        && let Some(n) = mesh_surface_normal(mesh, e, probe)
    {
        return n;
    }

    // Fallback: primitive.
    let inv_rot = e.world_rot.inverse();
    match e.collider {
        Some(Collider::Sphere { radius }) => {
            let r = radius * e.world_scale.max_element();
            let to = probe - e.world_pos;
            let dist = to.length();
            if dist < 1e-4 || r < 1e-6 { Vec3::Y }
            else { (to / dist).normalize_or(Vec3::Y) }
        }
        Some(Collider::Aabb { half_extents }) => {
            let h = half_extents * e.world_scale;
            let d = inv_rot * (probe - e.world_pos);
            let dx = (h.x - d.x.abs()).max(0.0);
            let dy = (h.y - d.y.abs()).max(0.0);
            let dz = (h.z - d.z.abs()).max(0.0);
            let local_normal = if dy <= dx && dy <= dz {
                Vec3::new(0.0, if d.y >= 0.0 { 1.0 } else { -1.0 }, 0.0)
            } else if dx <= dz {
                Vec3::new(d.x.signum(), 0.0, 0.0)
            } else {
                Vec3::new(0.0, 0.0, d.z.signum())
            };
            (e.world_rot * local_normal).normalize_or(Vec3::Y)
        }
        Some(Collider::Capsule { radius, height }) => {
            let r = radius * e.world_scale.max_element();
            let hy = height * e.world_scale.y * 0.5;
            let up = e.world_rot * Vec3::new(0.0, hy, 0.0);
            let a = e.world_pos - up;
            let b = e.world_pos + up;
            let closest = closest_on_segment(probe, a, b);
            let to = probe - closest;
            let dist = to.length();
            if dist < 1e-4 || r < 1e-6 { Vec3::Y }
            else { (to / dist).normalize_or(Vec3::Y) }
        }
        None => Vec3::Y,
    }
}

fn mesh_surface_normal(
    mesh: &crate::physics::MeshCollider,
    e: &super::collision_cache::ColliderEntry,
    probe: Vec3,
) -> Option<Vec3> {
    let scale_inv = Vec3::new(
        if e.world_scale.x.abs() > 1e-6 { 1.0 / e.world_scale.x } else { 1.0 },
        if e.world_scale.y.abs() > 1e-6 { 1.0 / e.world_scale.y } else { 1.0 },
        if e.world_scale.z.abs() > 1e-6 { 1.0 / e.world_scale.z } else { 1.0 },
    );
    let inv_rot = e.world_rot.inverse();
    let origin_world = probe + Vec3::Y * 0.3;
    let local_o = inv_rot * ((origin_world - e.world_pos) * scale_inv);
    let local_d_unnorm = inv_rot * (Vec3::NEG_Y * scale_inv);
    let local_d_len = local_d_unnorm.length();
    if local_d_len < 1e-12 { return None; }
    let local_d = local_d_unnorm / local_d_len;
    let triangles = mesh.triangles();
    if triangles.is_empty() { return None; }
    let max_t_local = 0.6 * local_d_len;
    let (tri_idx, _t_local) = mesh.bvh().raycast(local_o, local_d, max_t_local, &triangles)?;
    let tri = triangles.get(tri_idx as usize)?;
    let e1 = tri[1] - tri[0];
    let e2 = tri[2] - tri[0];
    let n_local = e1.cross(e2).normalize_or(Vec3::Y);
    let n_scaled = Vec3::new(
        n_local.x / e.world_scale.x.max(1e-6),
        n_local.y / e.world_scale.y.max(1e-6),
        n_local.z / e.world_scale.z.max(1e-6),
    );
    Some((e.world_rot * n_scaled).normalize_or(Vec3::Y))
}

pub fn find_support(
    cache: &ColliderCache,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<SupportInfo> {
    let r = cap.radius;
    let mut best: Option<SupportInfo> = None;

    for e in cache.entries() {
        let amin = e.aabb_min;
        let amax = e.aabb_max;
        let top_y = amax.y;
        let dy = feet_pos.y - top_y;
        if !(-0.02..=SUPPORT_TOLERANCE).contains(&dy) { continue; }
        if feet_pos.x + r < amin.x || feet_pos.x - r > amax.x { continue; }
        if feet_pos.z + r < amin.z || feet_pos.z - r > amax.z { continue; }

        let normal = surface_normal_at_entry(e, feet_pos);

        if best.as_ref().is_none_or(|b| top_y > b.top_y) {
            best = Some(SupportInfo { entity: e.entity, normal, top_y });
        }
    }
    best
}

pub fn find_support_entity(
    cache: &ColliderCache,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<Entity> {
    find_support(cache, feet_pos, cap).map(|s| s.entity)
}

pub fn resolve_movement_ex(
    _world: &World,
    cache: &ColliderCache,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
    step_down_max: f32,
    terrain: Option<&Heightmap>,
) -> MovementResult {
    let mut pos = start_feet;
    let mut on_ground = false;

    let can_step_up = true;
    let step_heights = [0.15_f32, 0.30, 0.45];

    let r = cap.radius;
    let h = cap.height.max(2.0 * r + 1e-3);

    if delta.x.abs() > 1e-6 {
        let steps = (delta.x.abs() / SUBSTEP_SIZE).ceil().max(1.0) as i32;
        let step = delta.x / steps as f32;
        for _ in 0..steps {
            let try_pos = pos + Vec3::new(step, 0.0, 0.0);
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, false) {
                pos = try_pos;
            } else if can_step_up {
                let mut stepped = false;
                for sh in step_heights {
                    let lifted = try_pos + Vec3::new(0.0, sh, 0.0);
                    let a2 = lifted + Vec3::Y * r;
                    let b2 = lifted + Vec3::Y * (h - r);
                    if !cache.capsule_hits(a2, b2, r, false) {
                        pos = lifted;
                        stepped = true;
                        break;
                    }
                }
                if !stepped { break; }
            } else {
                break;
            }
        }
    }

    if delta.z.abs() > 1e-6 {
        let steps = (delta.z.abs() / SUBSTEP_SIZE).ceil().max(1.0) as i32;
        let step = delta.z / steps as f32;
        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, 0.0, step);
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, false) {
                pos = try_pos;
            } else if can_step_up {
                let mut stepped = false;
                for sh in step_heights {
                    let lifted = try_pos + Vec3::new(0.0, sh, 0.0);
                    let a2 = lifted + Vec3::Y * r;
                    let b2 = lifted + Vec3::Y * (h - r);
                    if !cache.capsule_hits(a2, b2, r, false) {
                        pos = lifted;
                        stepped = true;
                        break;
                    }
                }
                if !stepped { break; }
            } else {
                break;
            }
        }
    }

    if delta.y.abs() > 1e-6 {
        let max_step = 0.1_f32;
        let steps = (delta.y.abs() / max_step).ceil().max(1.0) as i32;
        let step = delta.y / steps as f32;

        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, step, 0.0);
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, true) {
                pos = try_pos;
            } else {
                if step < 0.0 { on_ground = true; }
                break;
            }
        }
    }

    if !on_ground && delta.y <= 0.0 && step_down_max > 1e-4 {
        let probe_step = 0.05_f32;
        let n = (step_down_max / probe_step).ceil() as i32;
        let mut probe = pos;
        for _ in 0..n {
            let try_pos = probe + Vec3::new(0.0, -probe_step, 0.0);
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, true) {
                probe = try_pos;
            } else {
                pos = probe;
                on_ground = true;
                break;
            }
        }
    }

    let local_floor = match terrain {
        Some(t) if t.contains(pos.x, pos.z) => t.sample(pos.x, pos.z).max(floor_y),
        _ => floor_y,
    };
    if pos.y <= local_floor {
        pos.y = local_floor;
        on_ground = true;
    }

    let support = find_support(cache, pos, cap);
    if support.is_some() && delta.y <= 0.0 {
        on_ground = true;
    }

    MovementResult { new_feet: pos, landed: on_ground, support }
}

pub fn resolve_movement(
    world: &World,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
) -> (Vec3, bool, Option<Entity>) {
    let mut cache = ColliderCache::new();
    cache.build(world);
    let r = resolve_movement_ex(world, &cache, start_feet, delta, cap, floor_y, 0.0, None);
    (r.new_feet, r.landed, r.support.map(|s| s.entity))
}

pub struct CharacterInput {
    pub target_horizontal_velocity: Vec3,
    pub jump_pressed: bool,
    pub crouching: bool,
    pub dt: f32,
}

pub struct CharacterMoveResult {
    pub new_feet: Vec3,
    pub new_eye: Vec3,
    pub on_ground: bool,
    pub hit_ceiling: bool,
    pub pushed_bodies: u32,
}

pub fn move_character(
    world: &mut World,
    cache: &ColliderCache,
    controller: &mut CharacterController,
    config: &CharacterConfig,
    current_feet: Vec3,
    input: &CharacterInput,
    terrain: Option<&crate::render::terrain::Heightmap>,
) -> CharacterMoveResult {
    // === 0. validate config (idempotent, дёшево) ===
    let mut config = *config;
    config.validate();

    let dt = input.dt.clamp(1e-4, 0.1);

    // === 1. Coyote / jump buffer ===
    if controller.was_on_ground {
        controller.coyote_timer = config.coyote_time;
    } else {
        controller.coyote_timer = (controller.coyote_timer - dt).max(0.0);
    }
    if input.jump_pressed {
        controller.jump_buffer_timer = config.jump_buffer_time;
    } else {
        controller.jump_buffer_timer = (controller.jump_buffer_timer - dt).max(0.0);
    }

    // === 2. Crouch: плавная интерполяция eye_height ===
    controller.crouching = input.crouching;
    let target_eye_h = if controller.crouching {
        config.crouch_height
    } else {
        config.eye_height
    };
    let lerp = (dt * 10.0).clamp(0.0, 1.0);
    controller.current_eye_height +=
        (target_eye_h - controller.current_eye_height) * lerp;

    // === 3. Сглаживание horizontal velocity ===
    let target_h = Vec3::new(
        input.target_horizontal_velocity.x,
        0.0,
        input.target_horizontal_velocity.z,
    );
    let tau = if controller.on_ground {
        config.ground_accel_tau
    } else {
        config.air_accel_tau
    };
    let alpha = if tau > 1e-4 {
        1.0 - (-dt / tau).exp()
    } else {
        1.0
    };
    controller.horizontal_velocity += (target_h - controller.horizontal_velocity) * alpha;

    // === 4. Jump ===
    let can_jump = controller.jump_buffer_timer > 0.0
        && controller.coyote_timer > 0.0
        && !controller.crouching;
    let mut did_jump = false;
    if can_jump {
        controller.vertical_velocity = config.jump_speed;
        controller.coyote_timer = 0.0;
        controller.jump_buffer_timer = 0.0;
        controller.on_ground = false;
        did_jump = true;
    }

    // === 5. Gravity ===
    controller.vertical_velocity -= config.gravity * dt;

    // === 6. Slope slide ===
    if controller.on_ground
        && controller.ground_normal.y < config.slope_walk_limit_cos
    {
        let n = controller.ground_normal;
        let g = Vec3::new(0.0, -config.gravity, 0.0);
        let tangent = g - n * g.dot(n);
        let steepness = ((config.slope_walk_limit_cos - n.y)
            / config.slope_walk_limit_cos.max(1e-4))
            .clamp(0.0, 1.0);
        let slide = tangent.normalize_or_zero()
            * config.slope_slide_speed
            * steepness;
        controller.horizontal_velocity += slide * dt;
    }

    // === 7. Total delta ===
    let horizontal = controller.horizontal_velocity;
    let dy = controller.vertical_velocity * dt;
    let delta = horizontal * dt + Vec3::new(0.0, dy, 0.0);

    // === 8. Resolve movement ===
    let pcap = PlayerCapsule {
        radius: config.radius,
        height: config.height,
    };
    let result = resolve_movement_ex(
        &*world,
        cache,
        current_feet,
        delta,
        &pcap,
        config.floor_y,
        config.step_down_max,
        terrain,
    );

    let mut new_feet = result.new_feet;
    let mut on_ground = result.landed;

    // === 9. Snap to ground ===
    if config.snap_to_ground
        && !on_ground
        && controller.vertical_velocity <= 0.0
        && (new_feet.y - current_feet.y).abs() < config.step_down_max
    {
        let probe_delta = Vec3::new(0.0, -config.step_down_max, 0.0);
        let probe = resolve_movement_ex(
            &*world,
            cache,
            new_feet,
            probe_delta,
            &pcap,
            config.floor_y,
            config.step_down_max,
            terrain,
        );
        if probe.landed
            && (new_feet.y - probe.new_feet.y).abs() < config.step_down_max
        {
            new_feet = probe.new_feet;
            on_ground = true;
        }
    }

    // === 10. Ground normal ===
    let ground_normal = if on_ground {
        result.support.map(|s| s.normal).unwrap_or(Vec3::Y)
    } else {
        Vec3::Y
    };

    // === 11. Update runtime ===
    controller.was_on_ground = controller.on_ground;
    controller.on_ground = on_ground;
    controller.ground_normal = ground_normal;

    // === 12. Vertical velocity resolution ===
    let hit_ceiling = dy > 0.0 && (new_feet.y - current_feet.y).abs() < 1e-4;
    if did_jump {
        controller.on_ground = false;
    } else if on_ground && controller.vertical_velocity <= 0.0 {
        controller.vertical_velocity = 0.0;
    } else if hit_ceiling {
        controller.vertical_velocity = 0.0;
    }

    // === 13. Head-bob ===
    if config.bob_enabled && on_ground {
        let moved = ((new_feet.x - current_feet.x).powi(2)
            + (new_feet.z - current_feet.z).powi(2))
        .sqrt();
        controller.bob_distance += moved;
        let bob_target = controller.bob_distance.sin() * config.bob_amplitude;
        controller.bob_current = controller.bob_current * 0.85 + bob_target * 0.15;
    } else {
        controller.bob_current *= 0.85;
    }

    // === 14. Push dynamic bodies ===
    let pushed = push_dynamic_bodies(
        world,
        config.radius,
        config.height,
        new_feet,
        controller.horizontal_velocity,
        config.push_force,
        dt,
    );

    // === 15. Eye position ===
    let new_eye = new_feet
        + Vec3::Y * (controller.current_eye_height + controller.bob_current);

    CharacterMoveResult {
        new_feet,
        new_eye,
        on_ground,
        hit_ceiling,
        pushed_bodies: pushed,
    }
}

fn push_dynamic_bodies(
    world: &mut World,
    radius: f32,
    height: f32,
    feet: Vec3,
    velocity: Vec3,
    force: f32,
    dt: f32,
) -> u32 {
    if force <= 0.0 {
        return 0;
    }
    let player_horizontal = Vec3::new(velocity.x, 0.0, velocity.z);
    let speed = player_horizontal.length();
    if speed < 0.1 {
        return 0;
    }

    let player_center = feet + Vec3::Y * (height * 0.5);
    let query_r = radius + 1.0;

    let candidates: Vec<Entity> = world
        .query::<crate::physics::RigidBody>()
        .filter(|(_, rb)| rb.body_type == crate::physics::BodyType::Dynamic)
        .map(|(e, _)| e)
        .collect();

    let mut count = 0u32;
    for e in candidates {
        let Some(t) = world.get::<crate::game::components::Transform>(e) else {
            continue;
        };
        let to_player = t.position - player_center;
        let to_player_h = Vec3::new(to_player.x, 0.0, to_player.z);
        let dist = to_player_h.length();
        if dist > query_r || dist < 1e-4 {
            continue;
        }
        let dir = to_player_h / dist;
        if dir.dot(player_horizontal) < 0.1 {
            continue;
        }
        if let Some(rb) = world.get_mut::<crate::physics::RigidBody>(e) {
            let mass = rb.mass.max(0.1);
            let impulse = dir * force * mass * speed * dt;
            rb.velocity += impulse / mass;
            rb.wake();
            count += 1;
        }
    }
    count
}

pub fn character_config_from_play(
    play: &crate::editor::play::PlayState,
) -> CharacterConfig {
    CharacterConfig {
        radius: play.player_radius,
        height: play.player_height,
        eye_height: play.eye_height,
        crouch_height: play.crouch_height,
        walk_speed: play.walk_speed,
        run_speed: play.run_speed,
        crouch_speed_mult: play.crouch_speed_mult,
        jump_speed: play.jump_speed,
        gravity: play.gravity,
        slope_walk_limit_cos: play.slope_walk_limit_cos,
        slope_slide_speed: play.slope_slide_speed,
        step_up_max: 0.3,
        step_down_max: play.step_down_max,
        snap_to_ground: true,
        push_force: play.push_strength * 5.0,
        ground_accel_tau: play.ground_accel_tau,
        air_accel_tau: play.air_accel_tau,
        coyote_time: play.coyote_time,
        jump_buffer_time: play.jump_buffer_time,
        bob_enabled: play.bob_enabled,
        bob_amplitude: play.bob_amplitude,
        floor_y: play.floor_y,
    }
}