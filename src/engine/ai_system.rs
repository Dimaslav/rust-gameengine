//! AI system: FSM, восприятие (зрение + слух), движение по navmesh,
//! agent-agent collision.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::ai::{
    AiAgent, AiState, AiTarget, NoiseEvent, PatrolPath,
};
use crate::game::components::{Health, Transform};
use crate::physics::navmesh::{segment_clear, Navmesh};

pub struct AiTuning {
    pub repath_interval: f32,
    pub waypoint_tolerance: f32,
    /// ИЗМЕНЕНО (agent-agent collision): радиус «личного пространства»
    /// агента. Если два агента ближе 2*agent_radius — расталкиваются.
    pub agent_radius: f32,
    /// Сила расталкивания (множитель к перекрытию за кадр).
    pub push_strength: f32,
    /// Максимум итераций расталкивания за кадр.
    pub push_iterations: u32,
}

impl Default for AiTuning {
    fn default() -> Self {
        Self {
            repath_interval: 0.4,
            waypoint_tolerance: 0.35,
            agent_radius: 0.5,
            push_strength: 0.5,
            push_iterations: 2,
        }
    }
}

pub struct AiSystem {
    pub tuning: AiTuning,
}

impl Default for AiSystem {
    fn default() -> Self { Self { tuning: AiTuning::default() } }
}

impl AiSystem {
    pub fn new() -> Self { Self::default() }

    pub fn update(&mut self, world: &mut World, navmesh: Option<&Navmesh>, dt: f32) {
        let target = self.find_target(world);
        let noises: Vec<NoiseEvent> = world.read_events_current::<NoiseEvent>().copied().collect();

        let agents: Vec<Entity> = world.query::<AiAgent>().map(|(e, _)| e).collect();

        for e in agents {
            self.update_agent(world, e, target, &noises, navmesh, dt);
        }

        // ИЗМЕНЕНО (agent-agent collision): расталкивание после всех FSM.
        self.resolve_agent_collisions(world, dt);
    }

    fn find_target(&self, world: &World) -> Option<(Entity, Vec3)> {
        for (e, _) in world.query::<AiTarget>() {
            let Some(t) = world.get::<Transform>(e) else { continue };
            if let Some(h) = world.get::<Health>(e) {
                if h.current <= 0.0 { continue; }
            }
            return Some((e, t.position));
        }
        None
    }

    fn update_agent(
        &mut self,
        world: &mut World,
        e: Entity,
        target: Option<(Entity, Vec3)>,
        noises: &[NoiseEvent],
        navmesh: Option<&Navmesh>,
        dt: f32,
    ) {
        let Some(mut agent) = world.get::<AiAgent>(e).cloned() else { return };
        let Some(agent_pos) = world.get::<Transform>(e).map(|t| t.position) else { return };

        let dead = world.get::<Health>(e).map(|h| h.current <= 0.0).unwrap_or(false);
        if dead {
            if agent.state != AiState::Dead {
                agent.state = AiState::Dead;
                agent.path.clear();
                world.insert(e, agent);
            }
            return;
        }

        agent.attack_timer = (agent.attack_timer - dt).max(0.0);
        agent.repath_timer = (agent.repath_timer - dt).max(0.0);
        agent.state_timer += dt;

        // --- Perception: зрение ---
        let sees = target.map_or(false, |(_, tp)| {
            self.can_see(world, agent_pos, agent.yaw, tp, &agent)
        });
        if sees {
            agent.time_since_seen = 0.0;
            agent.last_seen_pos = target.map(|(_, p)| p);
        } else {
            agent.time_since_seen += dt;
        }

        // --- ИЗМЕНЕНО: Perception: слух ---
        // Находим самый громкий шум в радиусе слышимости.
        let mut heard: Option<Vec3> = None;
        let mut heard_dist = f32::INFINITY;
        for n in noises {
            let d = (n.position - agent_pos).length();
            if d > agent.hearing_range { continue; }
            if d > n.radius { continue; }
            if d < heard_dist {
                heard_dist = d;
                heard = Some(n.position);
            }
        }

        // --- FSM ---
        let old_state = agent.state;
        match agent.state {
            AiState::Idle => {
                if sees {
                    agent.state = AiState::Chase;
                } else if let Some(p) = heard {
                    agent.last_seen_pos = Some(p);
                    agent.state = AiState::Investigate;
                } else if world.has::<PatrolPath>(e) {
                    agent.state = AiState::Patrol;
                }
            }
            AiState::Patrol => {
                if sees {
                    agent.state = AiState::Chase;
                } else if let Some(p) = heard {
                    agent.last_seen_pos = Some(p);
                    agent.state = AiState::Investigate;
                }
            }
            AiState::Investigate => {
                if sees {
                    agent.state = AiState::Chase;
                } else if let Some(p) = heard {
                    // Обновляем точку интереса.
                    agent.last_seen_pos = Some(p);
                    agent.state_timer = 0.0;
                } else if let Some(lp) = agent.last_seen_pos {
                    // Дошли до точки?
                    let d = (lp - agent_pos).length();
                    if d < self.tuning.waypoint_tolerance * 2.0 {
                        agent.last_seen_pos = None;
                        agent.state = if world.has::<PatrolPath>(e) { AiState::Patrol } else { AiState::Idle };
                    }
                }
                // Timeout — забываем.
                if agent.state_timer > 8.0 {
                    agent.last_seen_pos = None;
                    agent.state = if world.has::<PatrolPath>(e) { AiState::Patrol } else { AiState::Idle };
                }
            }
            AiState::Chase => {
                let distance = target.map(|(_, tp)| (tp - agent_pos).length());
                if let Some(d) = distance {
                    if d <= agent.attack_range {
                        agent.state = AiState::Attack;
                    }
                }
                if !sees && agent.time_since_seen > agent.lose_target_time {
                    // Переходим в Investigate к последней известной позиции.
                    if let Some(lp) = agent.last_seen_pos {
                        agent.state = AiState::Investigate;
                        agent.state_timer = 0.0;
                        agent.path.clear();
                        let _ = lp;
                    } else {
                        agent.state = AiState::Idle;
                        agent.path.clear();
                    }
                }
            }
            AiState::Attack => {
                let distance = target.map(|(_, tp)| (tp - agent_pos).length());
                if let Some(d) = distance {
                    if d > agent.attack_range * 1.2 {
                        agent.state = AiState::Chase;
                    }
                }
                if !sees && agent.time_since_seen > agent.lose_target_time {
                    if let Some(_) = agent.last_seen_pos {
                        agent.state = AiState::Investigate;
                        agent.state_timer = 0.0;
                    } else {
                        agent.state = AiState::Idle;
                    }
                }
            }
            AiState::Dead => {}
        }
        if agent.state != old_state {
            agent.state_timer = 0.0;
            agent.path.clear();
            agent.path_index = 0;
        }

        // --- Действия ---
        match agent.state {
            AiState::Idle => {}
            AiState::Patrol => self.do_patrol(world, e, &mut agent, navmesh, dt),
            AiState::Investigate => {
                if let Some(p) = agent.last_seen_pos {
                    self.do_chase(world, e, &mut agent, p, navmesh, dt);
                }
            }
            AiState::Chase => {
                if let Some((_, tp)) = target {
                    self.do_chase(world, e, &mut agent, tp, navmesh, dt);
                }
            }
            AiState::Attack => {
                if let Some((te, tp)) = target {
                    self.face_towards(&mut agent, tp - agent_pos, dt);
                    if agent.attack_timer <= 0.0 {
                        agent.attack_timer = agent.attack_cooldown;
                        if let Some(h) = world.get_mut::<Health>(te) {
                            h.current -= agent.damage;
                        }
                    }
                }
            }
            AiState::Dead => {}
        }

        world.insert(e, agent);
    }

    fn can_see(
        &self,
        world: &World,
        agent_pos: Vec3,
        agent_yaw: f32,
        target_pos: Vec3,
        agent: &AiAgent,
    ) -> bool {
        let to = target_pos - agent_pos;
        let dist = to.length();
        if dist > agent.vision_range { return false; }
        if dist < 1e-4 { return true; }

        let to_dir = to / dist;
        let fwd = yaw_to_forward(agent_yaw);
        if fwd.dot(to_dir) < agent.vision_angle_cos { return false; }

        let eye = agent_pos + Vec3::Y * 1.0;
        let target_eye = target_pos + Vec3::Y * 1.0;
        segment_clear(world, eye, target_eye)
    }

    fn do_patrol(
        &mut self, world: &mut World, e: Entity, agent: &mut AiAgent,
        navmesh: Option<&Navmesh>, dt: f32,
    ) {
        let Some(mut patrol) = world.get::<PatrolPath>(e).cloned() else { return };
        if patrol.waiting {
            patrol.tick_wait(dt);
            world.insert(e, patrol);
            return;
        }
        let Some(target) = patrol.current_target() else { return };
        let Some(agent_pos) = world.get::<Transform>(e).map(|t| t.position) else { return };

        if (target - agent_pos).length() < self.tuning.waypoint_tolerance {
            patrol.start_wait();
            world.insert(e, patrol);
            return;
        }
        if let Some(nm) = navmesh {
            if agent.repath_timer <= 0.0 || agent.path.is_empty() {
                if let Some(path) = nm.find_path(agent_pos, target) {
                    agent.path = nm.smooth_path(path, |a, b| segment_clear(world, a, b));
                    agent.path_index = 0;
                }
                agent.repath_timer = self.tuning.repath_interval;
            }
        }
        self.follow_path(world, e, agent, dt);
        world.insert(e, patrol);
    }

    fn do_chase(
        &mut self, world: &mut World, e: Entity, agent: &mut AiAgent,
        target_pos: Vec3, navmesh: Option<&Navmesh>, dt: f32,
    ) {
        let Some(agent_pos) = world.get::<Transform>(e).map(|t| t.position) else { return };
        if let Some(nm) = navmesh {
            if agent.repath_timer <= 0.0 || agent.path.is_empty() {
                if let Some(path) = nm.find_path(agent_pos, target_pos) {
                    agent.path = nm.smooth_path(path, |a, b| segment_clear(world, a, b));
                    agent.path_index = 0;
                }
                agent.repath_timer = self.tuning.repath_interval;
            }
        } else {
            agent.path = vec![target_pos];
            agent.path_index = 0;
        }
        self.follow_path(world, e, agent, dt);
    }

    fn follow_path(&mut self, world: &mut World, e: Entity, agent: &mut AiAgent, dt: f32) {
        if agent.path.is_empty() || agent.path_index >= agent.path.len() { return; }
        let Some(transform) = world.get::<Transform>(e) else { return };
        let pos = transform.position;
        let target = agent.path[agent.path_index];
        let to = target - pos;
        let dist_xz = (to.x * to.x + to.z * to.z).sqrt();
        if dist_xz < self.tuning.waypoint_tolerance {
            agent.path_index += 1;
            if agent.path_index >= agent.path.len() {
                agent.path.clear();
                agent.path_index = 0;
            }
            return;
        }
        let dir = if dist_xz > 1e-4 { Vec3::new(to.x / dist_xz, 0.0, to.z / dist_xz) } else { Vec3::ZERO };
        let step = (agent.speed * dt).min(dist_xz);
        let new_pos = pos + dir * step;
        self.face_towards(agent, dir, dt);
        let new_rotation = yaw_to_quat(agent.yaw);
        if let Some(t) = world.get_mut::<Transform>(e) {
            t.position = new_pos;
            t.rotation = new_rotation;
        }
    }

    fn face_towards(&self, agent: &mut AiAgent, dir: Vec3, dt: f32) {
        let want_yaw = dir.x.atan2(-dir.z);
        let mut diff = want_yaw - agent.yaw;
        while diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
        while diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
        let step = agent.turn_speed * dt;
        if diff.abs() <= step { agent.yaw = want_yaw; }
        else { agent.yaw += diff.signum() * step; }
    }

    /// ИЗМЕНЕНО (agent-agent collision): расталкивание агентов.
    ///
    /// Простой O(n²) в пределах ограниченного радиуса. Для каждой
    /// пары агентов, чьи XZ-проекции ближе 2*R, сдвигаем обоих
    /// в стороны на половину перекрытия.
    ///
    /// `dt` не используется явно — расталкивание per-frame, но
    /// ограничено `push_strength`, чтобы не «выстреливать» тела.
    /// Несколько итераций сглаживают ситуацию при 3+ агентах.
    fn resolve_agent_collisions(&mut self, world: &mut World, _dt: f32) {
        let agents: Vec<Entity> = world.query::<AiAgent>().map(|(e, _)| e).collect();
        if agents.len() < 2 { return; }

        let r = self.tuning.agent_radius;
        let r2 = (r * 2.0) * (r * 2.0);

        for _ in 0..self.tuning.push_iterations {
            // Собираем позиции в локальный массив — избегаем
            // borrow checker'а.
            let mut positions: Vec<(Entity, Vec3)> = Vec::with_capacity(agents.len());
            for &e in &agents {
                if let Some(t) = world.get::<Transform>(e) {
                    positions.push((e, t.position));
                }
            }
            let n = positions.len();
            let mut shifts: Vec<Vec3> = vec![Vec3::ZERO; n];

            for i in 0..n {
                for j in (i + 1)..n {
                    let pi = positions[i].1;
                    let pj = positions[j].1;
                    let dx = pj.x - pi.x;
                    let dz = pj.z - pi.z;
                    let d2 = dx * dx + dz * dz;
                    if d2 >= r2 || d2 < 1e-8 { continue; }
                    let d = d2.sqrt();
                    let overlap = (r * 2.0) - d;
                    if overlap <= 0.0 { continue; }
                    let inv_d = 1.0 / d;
                    let nx = dx * inv_d;
                    let nz = dz * inv_d;
                    let half = overlap * 0.5 * self.tuning.push_strength;
                    shifts[i] += Vec3::new(-nx * half, 0.0, -nz * half);
                    shifts[j] += Vec3::new( nx * half, 0.0,  nz * half);
                }
            }

            for i in 0..n {
                if shifts[i].length_squared() < 1e-8 { continue; }
                let (e, _) = positions[i];
                if let Some(t) = world.get_mut::<Transform>(e) {
                    t.position += shifts[i];
                }
            }
        }
    }
}

fn yaw_to_forward(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

fn yaw_to_quat(yaw: f32) -> glam::Quat {
    glam::Quat::from_rotation_y(yaw)
}