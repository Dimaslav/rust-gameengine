use std::sync::Arc;
use std::time::Instant;
use winit::event::{DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::keyboard::KeyCode;
use winit::window::{CursorGrabMode, Window, WindowBuilder};

use crate::ecs::{Entity, World};
use crate::editor::gizmo::{self, GizmoMode};
use crate::editor::palette::PaletteItem;
use crate::editor::placement;
use crate::editor::ui::{self as editor_ui, Stats, UiAssets, UiState};
use crate::editor::{BoxSelect, Editor, EditorAction};
use crate::game::components::{
    Chase, Health, Interactable, MaterialHandle, MeshHandle, Parent, SkeletonHandle, Spinner,
    Tint, Transform, Trigger, TriggerAction, Velocity,
};
use crate::render::{
    Camera3D, EguiFrameData, GpuLight, GpuPointLight, LineBatch, LineVertex, MeshDraw,
    ParticleInstance, PostFx, Renderer,
};
use glam::Vec3;
use super::audio::AudioSystem;
use super::collision::{self, PlayerCapsule};
use super::input::Input;
use super::particles::{self, Particle, MAX_PARTICLES};
use super::time::Time;

pub trait Game: 'static {
    fn init(&mut self, _world: &mut World, _renderer: &mut Renderer) {}

    fn update(
        &mut self,
        _world: &mut World,
        _input: &Input,
        _renderer: &mut Renderer,
        _dt: f32,
    ) -> bool {
        true
    }

    fn collect_draws(&mut self, _world: &mut World, _renderer: &Renderer) -> Vec<MeshDraw> {
        Vec::new()
    }

    fn collect_lines(
        &mut self,
        _world: &mut World,
        _renderer: &Renderer,
        _selected: &[Entity],
    ) -> Vec<LineVertex> {
        Vec::new()
    }

    fn dir_lights(&self) -> Vec<GpuLight> {
        Vec::new()
    }

    fn point_lights(&self) -> Vec<GpuPointLight> {
        Vec::new()
    }

    fn ambient(&self) -> [f32; 3] {
        [0.18, 0.20, 0.26]
    }

    fn postfx(&self) -> PostFx {
        PostFx::default()
    }

    fn apply_postfx(&mut self, _postfx: PostFx) {}

    fn camera(&self) -> &Camera3D;
    fn camera_mut(&mut self) -> &mut Camera3D;
}

/// Летящая пуля — глобальный runtime-объект (не ECS).
#[derive(Clone, Copy)]
struct Projectile {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    max_age: f32,
    damage: f32,
}

struct App<G: Game> {
    game: G,
    world: World,
    input: Input,
    time: Time,
    window: Arc<Window>,
    renderer: Renderer,
    editor: Editor,
    ui_state: UiState,
    mouse_press_pos: Option<(f32, f32)>,
    viewport_rect: Option<egui::Rect>,

    // === ПКМ: различаем короткий клик (меню) от протяжки (fly) ===
    rmb_press_time: Option<Instant>,
    rmb_press_pos: Option<(f32, f32)>,
    rmb_dragged: bool,

    // === CPU particles ===
    particles: Vec<Particle>,

    // === Летящие пули ===
    projectiles: Vec<Projectile>,

    // === Audio ===
    audio: Option<AudioSystem>,
}

impl<G: Game> App<G> {
    /// FPS-контроллер: движение, стрельба, взаимодействие, триггеры, Chase.
    fn update_player(&mut self, dt: f32) {
        // 1. Mouse look
        {
            let sens = self.editor.state.play.look_sensitivity;
            let (mdx, mdy) = self.input.mouse_motion;
            self.game.camera_mut().fps_look(mdx * sens, mdy * sens);
        }

        // 2. Приседание — читаем Ctrl.
        let crouching = self.input.key_down(KeyCode::ControlLeft)
            || self.input.key_down(KeyCode::ControlRight);
        self.editor.state.play.crouching = crouching;

        // Интерполируем высоту глаз: стоим — eye_height, сидим — crouch_height.
        {
            let play = &mut self.editor.state.play;
            let target = if crouching { play.crouch_height } else { play.eye_height };
            let lerp = (dt * 10.0).clamp(0.0, 1.0);
            play.current_eye_height += (target - play.current_eye_height) * lerp;
        }

        // 3. Параметры (копии)
        let eye_height = self.editor.state.play.current_eye_height;
        let player_radius = self.editor.state.play.player_radius;
        let player_height = if crouching {
            (self.editor.state.play.crouch_height + 0.15)
                .max(2.0 * player_radius + 0.05)
        } else {
            self.editor.state.play.player_height
        };
        let walk_speed = self.editor.state.play.walk_speed;
        let run_speed = self.editor.state.play.run_speed;
        let jump_speed = self.editor.state.play.jump_speed;
        let gravity = self.editor.state.play.gravity;
        let floor_y = self.editor.state.play.floor_y;
        let bob_enabled = self.editor.state.play.bob_enabled;
        let bob_amp = self.editor.state.play.bob_amplitude;
        let gun_range = self.editor.state.play.gun_range;
        let interact_dist = self.editor.state.play.interact_distance;
        let bullet_speed = self.editor.state.play.bullet_speed;
        let crouch_mult = self.editor.state.play.crouch_speed_mult;

        // 4. Движение
        let f = self.game.camera().forward();
        let fwd_xz = Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
        let right_xz = Vec3::new(-fwd_xz.z, 0.0, fwd_xz.x);

        let running = self.input.key_down(KeyCode::ShiftLeft) && !crouching;
        let base_speed = if running { run_speed } else { walk_speed };
        let speed = if crouching { base_speed * crouch_mult } else { base_speed };

        let mut motion = Vec3::ZERO;
        if self.input.key_down(KeyCode::KeyW) { motion += fwd_xz; }
        if self.input.key_down(KeyCode::KeyS) { motion -= fwd_xz; }
        if self.input.key_down(KeyCode::KeyD) { motion += right_xz; }
        if self.input.key_down(KeyCode::KeyA) { motion -= right_xz; }

        let moving = motion.length_squared() > 1e-8;
        if moving {
            motion = motion.normalize() * speed * dt;
        }

        // 5. Прыжок + гравитация
        let mut vvel = self.editor.state.play.vertical_velocity;
        let mut on_ground = self.editor.state.play.on_ground;

        if self.input.key_pressed(KeyCode::Space) && on_ground && !crouching {
            vvel = jump_speed;
            on_ground = false;
        }
        vvel -= gravity * dt;
        let dy = vvel * dt;

        // 6. Коллизии + пол
        let eye_pos = self.game.camera().first_person_pos;
        let feet = eye_pos - Vec3::Y * eye_height;
        let pcap = PlayerCapsule {
            radius: player_radius,
            height: player_height,
        };
        let delta = motion + Vec3::new(0.0, dy, 0.0);
        let (new_feet, landed) = collision::resolve_movement(
            &self.world,
            &self.renderer,
            feet,
            delta,
            &pcap,
            floor_y,
        );

        if landed {
            vvel = 0.0;
            on_ground = true;
        } else if dy < 0.0 && (new_feet.y - feet.y).abs() < 1e-4 {
            vvel = 0.0;
            on_ground = true;
        } else if dy > 0.0 && (new_feet.y - feet.y).abs() < 1e-4 {
            vvel = 0.0;
        }

        // 7. Head bob
        let horizontal_moved =
            ((new_feet.x - feet.x).powi(2) + (new_feet.z - feet.z).powi(2)).sqrt();

        let mut bob_dist = self.editor.state.play.bob_distance;
        let mut bob_cur = self.editor.state.play.bob_current;

        if bob_enabled && on_ground && horizontal_moved > 1e-6 {
            bob_dist += horizontal_moved * if running { 1.4 } else { 1.0 };
        }
        let bob_target = if bob_enabled { bob_dist.sin() * bob_amp } else { 0.0 };
        bob_cur = bob_cur * 0.85 + bob_target * 0.15;
        let bob_offset = bob_cur;

        // 8. Записать позицию и состояние
        let new_eye = new_feet + Vec3::Y * (eye_height + bob_offset);
        self.game.camera_mut().first_person_pos = new_eye;

        {
            let play = &mut self.editor.state.play;
            play.vertical_velocity = vvel;
            play.on_ground = on_ground;
            play.bob_distance = bob_dist;
            play.bob_current = bob_cur;
            play.saved_position = new_eye;
            play.fire_cooldown = (play.fire_cooldown - dt).max(0.0);
            play.interact_cooldown = (play.interact_cooldown - dt).max(0.0);
        }

        // 9. Прицел + стрельба + E
        let origin = self.game.camera().position();
        let dir = self.game.camera().forward();

        let aim_hit = crate::editor::picking::pick_ray(
            &self.world,
            &self.renderer,
            origin,
            dir,
        );

        let new_highlight = aim_hit.as_ref().and_then(|(e, d)| {
            if *d <= interact_dist { Some(*e) } else { None }
        });
        self.editor.state.play.highlight = new_highlight;

        // Стрельба (ЛКМ)
        let lmb = self.input.mouse_down(MouseButton::Left);
        let can_fire = lmb
            && self.editor.state.play.fire_cooldown <= 0.0
            && self.editor.state.play.ammo > 0;

        if can_fire {
            if let Some(audio) = &self.audio {
                audio.play("shot");
            }

            let muzzle = origin + dir * 0.5;
            self.spawn_burst(muzzle, &particles::sparks(dir));

            let damage = self.editor.state.play.damage_per_shot;
            self.projectiles.push(Projectile {
                position: muzzle,
                velocity: dir * bullet_speed.max(1.0),
                age: 0.0,
                max_age: 3.0,
                damage,
            });

            if bullet_speed < 0.5 {
                if let Some((target, d)) = aim_hit {
                    if d <= gun_range {
                        let hit_point = origin + dir * d;
                        self.spawn_burst(hit_point, &particles::sparks(-dir));

                        let mut killed = false;
                        if let Some(h) = self.world.get_mut::<Health>(target) {
                            h.current -= damage;
                            if h.current <= 0.0 {
                                killed = true;
                            }
                        }
                        if killed {
                            let pos = self
                                .world
                                .get::<Transform>(target)
                                .map(|t| t.position)
                                .unwrap_or(Vec3::ZERO);
                            self.spawn_burst(pos, &particles::explosion());
                            if let Some(audio) = &self.audio {
                                audio.play("explosion");
                            }
                            self.world.despawn(target);
                        }
                    }
                }
            }

            let play = &mut self.editor.state.play;
            play.fire_cooldown = play.fire_cooldown_max;
            if play.ammo > 0 {
                play.ammo -= 1;
            }
        }

        // E — взаимодействие
        if self.input.key_pressed(KeyCode::KeyE)
            && self.editor.state.play.interact_cooldown <= 0.0
        {
            if let Some(target) = self.editor.state.play.highlight {
                if let Some(&inter) = self.world.get::<Interactable>(target) {
                    let vfx_pos = self
                        .world
                        .get::<Transform>(target)
                        .map(|t| t.position)
                        .unwrap_or(Vec3::ZERO);

                    match inter {
                        Interactable::Pickup => {
                            self.spawn_burst(vfx_pos, &particles::pickup_glow());
                            if let Some(audio) = &self.audio {
                                audio.play("pickup");
                            }
                            self.world.despawn(target);
                        }
                        Interactable::Paint(c) => {
                            self.world.insert(target, Tint(c));
                        }
                        Interactable::Toggle => {
                            if let Some(sp) = self.world.get_mut::<Spinner>(target) {
                                sp.speed = -sp.speed;
                            }
                        }
                    }
                    self.editor.state.play.interact_cooldown =
                        self.editor.state.play.interact_cooldown_max;
                }
            }
        }

        // 10. Триггеры
        let player_feet_now = self.game.camera().first_person_pos - Vec3::Y * eye_height;

        let triggers: Vec<Entity> = self.world.query::<Trigger>().map(|(e, _)| e).collect();
        for e in triggers {
            let trigger_data = self.world.get::<Trigger>(e).cloned();
            let trigger_pos = crate::game::world_position(&self.world, e);

            let (Some(mut t), Some(pos)) = (trigger_data, trigger_pos) else {
                continue;
            };
            if t.fired && t.once {
                continue;
            }

            let dist = (player_feet_now - pos).length();
            if dist <= t.radius {
                match &t.action {
                    TriggerAction::Teleport(target) => {
                        let new_pos = Vec3::from_array(*target);
                        self.game.camera_mut().first_person_pos = new_pos;
                        self.editor.state.play.vertical_velocity = 0.0;
                    }
                    TriggerAction::Tint(c) => {
                        self.world.insert(e, Tint(*c));
                    }
                    TriggerAction::Despawn => {
                        self.world.despawn(e);
                        continue;
                    }
                }
                t.fired = true;
                if t.once {
                    self.world.insert(e, t);
                }
            }
        }

        // 11. Chase
        let player_pos = self.game.camera().position();
        let chasers: Vec<Entity> = self.world.query::<Chase>().map(|(e, _)| e).collect();
        for e in chasers {
            let c = self.world.get::<Chase>(e).copied();
            if let Some(c) = c {
                if let Some(t) = self.world.get_mut::<Transform>(e) {
                    let to_player = player_pos - t.position;
                    let dist = to_player.length();
                    if dist > c.stop_distance && dist > 1e-4 {
                        let step = (c.speed * dt).min(dist - c.stop_distance);
                        t.position += to_player / dist * step;
                    }
                }
            }
        }
    }

    /// UE5-подобный полёт: RMB + WASD.
    fn update_fly(&mut self, dt: f32) {
        let sens = self.editor.state.fly_sensitivity;
        let (mdx, mdy) = self.input.mouse_motion;
        {
            let cam = self.game.camera_mut();
            cam.fly_look(mdx * sens, mdy * sens);
        }

        let f = self.game.camera().forward();
        let r = self.game.camera().right();

        let mult = if self.input.key_down(KeyCode::ShiftLeft) {
            3.0
        } else if self.input.key_down(KeyCode::ControlLeft) {
            0.3
        } else {
            1.0
        };
        let speed = self.editor.state.fly_speed * mult * dt;

        let mut delta = Vec3::ZERO;
        if self.input.key_down(KeyCode::KeyW) { delta += f; }
        if self.input.key_down(KeyCode::KeyS) { delta -= f; }
        if self.input.key_down(KeyCode::KeyD) { delta += r; }
        if self.input.key_down(KeyCode::KeyA) { delta -= r; }
        if self.input.key_down(KeyCode::KeyE) || self.input.key_down(KeyCode::Space) {
            delta += Vec3::Y;
        }
        if self.input.key_down(KeyCode::KeyQ) {
            delta -= Vec3::Y;
        }

        if delta.length_squared() > 1e-6 {
            let mv = delta.normalize() * speed;
            self.game.camera_mut().fly_move(mv);
        }

        if self.input.scroll_delta.abs() > 0.01 {
            let fs = &mut self.editor.state.fly_speed;
            *fs = (*fs * (1.0 + self.input.scroll_delta * 0.1)).clamp(0.5, 200.0);
        }
    }

    /// Создать burst частиц в позиции.
    fn spawn_burst(&mut self, origin: Vec3, params: &particles::BurstParams) {
        particles::emit_burst(&mut self.particles, origin, params);
    }

    /// Если материал с этим именем используется кем-то ещё — создать
    /// уникальную копию `<name>_uniq_<entity>` и назначить её этому entity.
    /// Возвращает имя итогового материала.
    fn ensure_unique_material_for(&mut self, entity: Entity) -> Option<String> {
        let mh = self.world.get::<MaterialHandle>(entity).cloned()?;
        let current_name = mh.0;

        // Сколько entity используют этот материал?
        let user_count = self
            .world
            .entities()
            .iter()
            .filter(|&&ent| {
                self.world
                    .get::<MaterialHandle>(ent)
                    .map(|m| m.0 == current_name)
                    .unwrap_or(false)
            })
            .count();

        if user_count <= 1 {
            return Some(current_name);
        }

        // Клонируем материал под уникальным именем.
        let unique_name = format!("{}_uniq_{}", current_name, entity);
        if !self.renderer.has_material(&unique_name) {
            let Some(base) = self.renderer.materials.get(&current_name).cloned() else {
                return Some(current_name);
            };
            self.renderer.add_material(&unique_name, base);
        }
        self.world
            .insert(entity, MaterialHandle(unique_name.clone()));
        log::info!(
            "Auto-unique material: entity #{} '{}' → '{}'",
            entity,
            current_name,
            unique_name
        );
        Some(unique_name)
    }

    /// Обновить частицы: возраст, движение, гравитация, удаление мёртвых.
    fn update_particles(&mut self, dt: f32) {
        for p in &mut self.particles {
            p.age += dt;
            p.velocity.y -= p.gravity * dt;
            p.position += p.velocity * dt;
        }
        self.particles.retain(|p| p.age < p.lifetime);

        if self.particles.len() > MAX_PARTICLES {
            let excess = self.particles.len() - MAX_PARTICLES;
            self.particles.drain(0..excess);
        }
    }

    /// Обновить пули: двигать, проверять попадания, удалять мёртвые.
    fn update_projectiles(&mut self, dt: f32) {
        if self.projectiles.is_empty() {
            return;
        }

        let floor_y = self.editor.state.play.floor_y;
        let mut hits: Vec<(Vec3, Option<Entity>, f32)> = Vec::new();
        let mut alive: Vec<Projectile> = Vec::with_capacity(self.projectiles.len());

        for mut p in self.projectiles.drain(..) {
            p.age += dt;
            if p.age >= p.max_age {
                continue;
            }

            let prev = p.position;
            p.position += p.velocity * dt;

            if p.position.y <= floor_y {
                let pt = Vec3::new(p.position.x, floor_y, p.position.z);
                hits.push((pt, None, p.damage));
                continue;
            }

            let seg = p.position - prev;
            let seg_len = seg.length();
            if seg_len < 1e-5 {
                alive.push(p);
                continue;
            }
            let dir = seg / seg_len;

            if let Some((e, t)) =
                crate::editor::picking::pick_ray(&self.world, &self.renderer, prev, dir)
            {
                if t <= seg_len {
                    let hit_point = prev + dir * t;
                    hits.push((hit_point, Some(e), p.damage));
                    continue;
                }
            }

            alive.push(p);
        }

        self.projectiles = alive;

        for (point, target, dmg) in hits {
            self.spawn_burst(point, &particles::sparks(Vec3::Y));

            if let Some(e) = target {
                let mut died = false;
                if let Some(h) = self.world.get_mut::<Health>(e) {
                    h.current -= dmg;
                    if h.current <= 0.0 {
                        died = true;
                    }
                }
                if died {
                    let pos = self
                        .world
                        .get::<Transform>(e)
                        .map(|t| t.position)
                        .unwrap_or(Vec3::ZERO);
                    self.spawn_burst(pos, &particles::explosion());
                    if let Some(audio) = &self.audio {
                        audio.play("explosion");
                    }
                    self.world.despawn(e);
                }
            }
        }
    }

    fn redraw(&mut self, elwt: &EventLoopWindowTarget<()>) {
        self.time.tick();
        self.input.tick_begin_frame();

        let dt = self.time.delta;
        let dt_smooth = self.time.delta_smooth;

        self.world.update_events();

        if self.input.key_pressed(KeyCode::F9) {
            self.editor.state.pending_action = Some(EditorAction::TogglePlay);
        }
        if self.input.key_pressed(KeyCode::Escape) && self.editor.state.play.active {
            self.editor.state.pending_action = Some(EditorAction::TogglePlay);
        }
        if !self.editor.state.play.active
            && !self.editor.state.flying
            && self.input.key_pressed(KeyCode::Escape)
        {
            if self.editor.state.palette.active.is_some() {
                self.editor.state.palette.active = None;
            } else if self.editor.state.context_menu_pos.is_some() {
                self.editor.state.context_menu_pos = None;
            }
        }

        // === RMB + fly (только если пользователь начал движение) ===
        let rmb = self.input.mouse_down(MouseButton::Right);
        let want_fly = rmb && self.rmb_dragged && !self.editor.state.play.active;
        let was_flying = self.editor.state.flying;

        if want_fly != was_flying {
            if want_fly {
                let _ = self.window.set_cursor_grab(CursorGrabMode::Locked);
                self.window.set_cursor_visible(false);
                self.input.on_cursor_enter();
                self.input.mouse_motion = (0.0, 0.0);
                self.input.skip_motion_frames = 4;
            } else {
                let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                self.window.set_cursor_visible(true);
            }
            self.editor.state.flying = want_fly;
            self.input.editor_flying = want_fly;
        }

        // === Play / Fly / Edit ===
        if self.editor.state.play.active {
            self.input.editor_captured = false;
            self.update_player(dt_smooth);
        } else if self.editor.state.flying {
            self.input.editor_captured = true;
            self.update_fly(dt_smooth);
        } else {
            let (mx, my) = self.input.mouse_pos;
            self.input.editor_captured = self.editor.state.gizmo.drag.is_some()
                || !self.in_viewport(mx, my);
        }

        // === Ghost preview активной кисти ===
        if !self.editor.state.play.active && !self.editor.state.flying {
            if self.editor.state.palette.active.is_some() {
                let (mx, my) = self.input.mouse_pos;
                if self.in_viewport(mx, my) {
                    let (origin, dir) = self.game.camera().ray_from_screen(
                        mx,
                        my,
                        self.renderer.size.width as f32,
                        self.renderer.size.height as f32,
                    );
                    let floor_y = self.editor.state.play.floor_y;
                    self.editor.state.palette.preview_pos =
                        placement::ray_ground_plane(origin, dir, floor_y);
                } else {
                    self.editor.state.palette.preview_pos = None;
                }
            } else {
                self.editor.state.palette.preview_pos = None;
            }
        } else {
            self.editor.state.palette.preview_pos = None;
        }

        let continue_running = self
            .game
            .update(&mut self.world, &self.input, &mut self.renderer, dt);
        if !continue_running {
            elwt.exit();
            return;
        }

        // Обновляем частицы и пули (не зависит от игры).
        self.update_particles(dt);
        self.update_projectiles(dt);

        if matches!(
            self.editor.state.pending_action,
            Some(EditorAction::TogglePlay)
        ) {
            self.editor.state.pending_action = None;
            self.handle_editor_action(EditorAction::TogglePlay);
        }

        if !self.editor.state.play.active {
            let ctrl = self.input.key_down(KeyCode::ControlLeft)
                || self.input.key_down(KeyCode::ControlRight);
            if ctrl && self.input.key_pressed(KeyCode::KeyZ) {
                self.editor.state.pending_action = Some(EditorAction::Undo);
            }
            if ctrl && self.input.key_pressed(KeyCode::KeyY) {
                self.editor.state.pending_action = Some(EditorAction::Redo);
            }
            if ctrl && self.input.key_pressed(KeyCode::KeyC) {
                self.editor.state.pending_action = Some(EditorAction::CopyEntity);
            }
            if ctrl && self.input.key_pressed(KeyCode::KeyV) {
                self.editor.state.pending_action = Some(EditorAction::PasteEntity);
            }

            if !self.editor.state.selected.is_empty() {
                if self.input.key_pressed(KeyCode::Digit1) {
                    self.editor.state.gizmo.mode = GizmoMode::Translate;
                }
                if self.input.key_pressed(KeyCode::Digit2) {
                    self.editor.state.gizmo.mode = GizmoMode::Rotate;
                }
                if self.input.key_pressed(KeyCode::Digit3) {
                    self.editor.state.gizmo.mode = GizmoMode::Scale;
                }
                if self.input.key_pressed(KeyCode::KeyF) && !ctrl {
                    self.editor.state.pending_action = Some(EditorAction::FocusSelected);
                }
                if ctrl && self.input.key_pressed(KeyCode::KeyD) {
                    self.editor.state.pending_action = Some(EditorAction::Duplicate);
                }
            }
            if self.input.key_pressed(KeyCode::Delete) {
                self.editor.state.pending_action = Some(EditorAction::DeleteSelected);
            }
        }

        // egui
        let mut postfx = self.game.postfx();

        let mesh_names = self.renderer.mesh_names();
        let material_names = self.renderer.material_names();

        let selected_material: Option<(String, crate::render::Material)> =
            self.editor.state.primary().and_then(|e| {
                let mh = self.world.get::<MaterialHandle>(e)?;
                let mat = self.renderer.materials.get(&mh.0)?.clone();
                Some((mh.0.clone(), mat))
            });

        // Список текстур (sorted by name) + размеры — для Assets-панели.
        let mut texture_list: Vec<(String, u32, u32)> = self
            .renderer
            .textures
            .iter()
            .map(|(n, t)| (n.clone(), t.size.0, t.size.1))
            .collect();
        texture_list.sort_by(|a, b| a.0.cmp(&b.0));

        let assets = UiAssets {
            mesh_names: &mesh_names,
            material_names: &material_names,
            texture_list: &texture_list,
            selected_material,
        };

        let raw_input = self.editor.egui_state.take_egui_input(&self.window);

        let stats = Stats {
            fps: self.time.fps(),
            frame_time_max_ms: self.time.frame_time_max_ms(),
            hitches: self.time.hitches,
            entities: self.world.len(),
            draws: 0,
            instances: 0,
            dir_lights: 0,
            point_lights: 0,
        };

        let egui_ctx = self.editor.egui_ctx.clone();
        let ui_state = &mut self.ui_state;
        let editor_state = &mut self.editor.state;
        let world = &mut self.world;

        let full_output = egui_ctx.run(raw_input, |ctx| {
            let action = editor_ui::draw(
                ctx,
                ui_state,
                editor_state,
                world,
                &mut postfx,
                &stats,
                &assets,
            );
            if let Some(a) = action {
                editor_state.pending_action = Some(a);
            }
        });

        {
            let avail = egui_ctx.available_rect();
            let ppp = egui_ctx.pixels_per_point();
            self.viewport_rect = Some(egui::Rect::from_min_max(
                egui::pos2(avail.min.x * ppp, avail.min.y * ppp),
                egui::pos2(avail.max.x * ppp, avail.max.y * ppp),
            ));
        }

        self.game.apply_postfx(postfx);

        self.editor
            .egui_state
            .handle_platform_output(&self.window, full_output.platform_output);

        let clipped_primitives =
            egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.editor.egui_renderer.update_texture(
                &self.renderer.device,
                &self.renderer.queue,
                *id,
                image_delta,
            );
        }

        if self.editor.state.undo_requested {
            self.editor.state.undo_requested = false;
            self.editor.state.undo.push(&self.world);
        }

        // === Авто-unique материал при правке shared ===
        for (entity, name, mat) in self.editor.state.dirty_materials.drain(..) {
            // Сколько объектов ссылаются на это имя?
            let user_count = self
                .world
                .entities()
                .iter()
                .filter(|&&ent| {
                    self.world
                        .get::<MaterialHandle>(ent)
                        .map(|mh| mh.0 == name)
                        .unwrap_or(false)
                })
                .count();

            let final_name = if user_count > 1 {
                // Материал shared — клонируем под уникальным именем
                // и переключаем на него только этот entity.
                let unique_name = format!("{}_uniq_{}", name, entity);
                if !self.renderer.has_material(&unique_name) {
                    self.renderer.add_material(&unique_name, mat.clone());
                }
                self.world
                    .insert(entity, MaterialHandle(unique_name.clone()));
                log::info!(
                    "Auto-unique on edit: entity #{} '{}' → '{}'",
                    entity,
                    name,
                    unique_name
                );
                unique_name
            } else {
                name
            };

            self.renderer.update_material(&final_name, mat);
        }

        self.editor.state.prune_selection(&self.world);

        if let Some(action) = self.editor.state.pending_action.take() {
            self.handle_editor_action(action);
        }

        let selected = self.editor.state.selected.clone();
        let draws = self.game.collect_draws(&mut self.world, &self.renderer);
        let mut lines = self
            .game
            .collect_lines(&mut self.world, &self.renderer, &selected);

        if !self.editor.state.play.active && !selected.is_empty() {
            let mut batch = LineBatch::new();
            gizmo::draw_gizmo(
                &mut batch,
                &self.world,
                &selected,
                self.game.camera(),
                &self.editor.state.gizmo,
            );
            lines.extend_from_slice(batch.vertices());
        }

        if !self.editor.state.play.active {
            if let Some(pos) = self.editor.state.palette.preview_pos {
                let item = self.editor.state.palette.active;
                let half_h = item.map(|i| i.half_height()).unwrap_or(0.5).max(0.05);
                let c = pos + Vec3::Y * half_h;
                let min = c - Vec3::new(0.5, half_h, 0.5);
                let max = c + Vec3::new(0.5, half_h, 0.5);

                let mut batch = LineBatch::new();
                batch.box_wireframe(min, max, [0.35, 0.9, 0.35, 0.9]);

                let r = 0.4;
                let cc = [0.4, 1.0, 0.4, 0.7];
                batch.line(pos - Vec3::X * r, pos + Vec3::X * r, cc);
                batch.line(pos - Vec3::Z * r, pos + Vec3::Z * r, cc);
                lines.extend_from_slice(batch.vertices());
            }
        }

        let mut particle_instances: Vec<ParticleInstance> = self
            .particles
            .iter()
            .map(|p| ParticleInstance {
                position_size: [p.position.x, p.position.y, p.position.z, p.size()],
                color: p.color(),
            })
            .collect();

        for proj in &self.projectiles {
            particle_instances.push(ParticleInstance {
                position_size: [proj.position.x, proj.position.y, proj.position.z, 0.12],
                color: [3.0, 2.6, 0.6, 1.0],
            });
        }

        let dir_lights = self.game.dir_lights();
        let point_lights = self.game.point_lights();
        let ambient = self.game.ambient();
        let postfx = self.game.postfx();

        let pixels_per_point = full_output.pixels_per_point;
        let egui_data = EguiFrameData {
            renderer: &mut self.editor.egui_renderer,
            clipped_primitives,
            pixels_per_point,
        };

        let res = self.renderer.render(
            self.game.camera(),
            &draws,
            &particle_instances,
            &lines,
            &dir_lights,
            &point_lights,
            ambient,
            postfx,
            Some(egui_data),
        );

        for id in &full_output.textures_delta.free {
            self.editor.egui_renderer.free_texture(id);
        }

        match res {
            Ok(_) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.renderer.resize(self.renderer.size);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
            Err(e) => log::warn!("Surface error: {:?}", e),
        }

        if self.time.frame_count % 30 == 0 {
            let mode_str = if self.editor.state.play.active {
                "PLAY"
            } else if self.editor.state.flying {
                "FLY"
            } else {
                "EDIT"
            };
            self.window.set_title(&format!(
                "Rust Engine 3D [{}] | FPS {:>5.1} | Frame {:.2}/{:.2} ms | Hitches {} | Entities {} | Sel {} | Particles {} | Proj {}",
                mode_str,
                self.time.fps(),
                self.time.frame_time_avg_ms(),
                self.time.frame_time_max_ms(),
                self.time.hitches,
                self.world.len(),
                self.editor.state.selected.len(),
                self.particles.len(),
                self.projectiles.len(),
            ));
        }

        self.input.end_frame();
    }

    fn in_viewport(&self, x: f32, y: f32) -> bool {
        match self.viewport_rect {
            Some(r) => r.contains(egui::pos2(x, y)),
            None => true,
        }
    }

    fn try_pick(&mut self, screen_pos: (f32, f32)) {
        let picked = crate::editor::picking::pick_entity(
            &self.world,
            &self.renderer,
            self.game.camera(),
            screen_pos.0,
            screen_pos.1,
        );
        match picked {
            Some(e) => self.editor.state.select_single(e),
            None => self.editor.state.selected.clear(),
        }
    }

    fn duplicate_selected(&mut self) {
        let originals: Vec<Entity> = self
            .editor
            .state
            .selected
            .iter()
            .copied()
            .filter(|&e| self.world.entities().contains(&e))
            .collect();
        if originals.is_empty() {
            return;
        }

        let mut new_selected = Vec::with_capacity(originals.len());
        for e in &originals {
            let new_e = self.world.spawn();

            if let Some(t) = self.world.get::<Transform>(*e).copied() {
                let mut nt = t;
                nt.position += Vec3::new(1.0, 0.0, 0.0);
                self.world.insert(new_e, nt);
            }
            if let Some(n) = self.world.get::<crate::game::components::Name>(*e).cloned() {
                self.world.insert(
                    new_e,
                    crate::game::components::Name(format!("{}_copy", n.0)),
                );
            }
            if let Some(m) = self.world.get::<MeshHandle>(*e).cloned() {
                self.world.insert(new_e, m);
            }
            if let Some(m) = self.world.get::<MaterialHandle>(*e).cloned() {
                self.world.insert(new_e, m);
            }
            if let Some(s) = self.world.get::<SkeletonHandle>(*e).cloned() {
                self.world.insert(new_e, s);
            }
            if let Some(s) = self.world.get::<Spinner>(*e).copied() {
                self.world.insert(new_e, s);
            }
            if let Some(v) = self.world.get::<Velocity>(*e).copied() {
                self.world.insert(new_e, v);
            }
            if let Some(h) = self.world.get::<Health>(*e).copied() {
                self.world.insert(new_e, h);
            }
            if let Some(c) = self.world.get::<Chase>(*e).copied() {
                self.world.insert(new_e, c);
            }
            if let Some(&i) = self.world.get::<Interactable>(*e) {
                self.world.insert(new_e, i);
            }

            new_selected.push(new_e);
        }

        // Уникализируем материалы всех дубликатов.
        for &ne in &new_selected {
            self.ensure_unique_material_for(ne);
        }

        self.editor.state.selected = new_selected;
        log::info!("Duplicated {} entities", originals.len());
    }

    fn spawn_palette_item(&mut self, item: PaletteItem, hit: Vec3) {
        use crate::game::components::*;

        self.editor.state.undo.push_forced(&self.world);

        let mut pos = hit;
        if item.snaps_to_ground() {
            pos.y += item.half_height();
        }

        let e = self.world.spawn();
        self.world
            .insert(e, Name(format!("{}_{:04}", item.label(), e)));
        self.world
            .insert(e, Transform::at(pos).with_scale(item.default_scale()));
        self.world
            .insert(e, MeshHandle(item.mesh().to_string()));
        self.world
            .insert(e, MaterialHandle(item.material().to_string()));

        match item {
            PaletteItem::Enemy => {
                self.world.insert(e, Health::new(50.0));
                self.world.insert(e, Chase::new(3.0, 1.2));
            }
            PaletteItem::Pickup => {
                self.world.insert(e, Interactable::Pickup);
            }
            PaletteItem::Switch => {
                self.world.insert(e, Interactable::Toggle);
                self.world.insert(e, Spinner::new(Vec3::Y, 1.5));
            }
            PaletteItem::TriggerCube => {
                self.world.insert(
                    e,
                    Trigger::new(2.5, TriggerAction::Teleport([0.0, 2.0, 0.0])),
                );
            }
            _ => {}
        }

        // Уникализируем материал — чтобы не зависеть от других объектов.
        self.ensure_unique_material_for(e);

        self.editor.state.select_single(e);
        log::info!(
            "Placed {} at ({:.2}, {:.2}, {:.2})",
            item.label(),
            pos.x,
            pos.y,
            pos.z
        );
    }

    fn handle_editor_action(&mut self, action: EditorAction) {
        use crate::game::components::{MaterialHandle, MeshHandle, Transform};

        match action {
            EditorAction::TogglePlay => {
                let play = &mut self.editor.state.play;
                play.active = !play.active;
                self.input.play_mode = play.active;

                if play.active {
                    play.vertical_velocity = 0.0;
                    play.on_ground = true;
                    play.bob_distance = 0.0;
                    play.bob_current = 0.0;
                    play.health = play.max_health;
                    play.ammo = play.max_ammo;
                    play.fire_cooldown = 0.0;
                    play.interact_cooldown = 0.0;
                    play.highlight = None;
                    play.crouching = false;
                    play.current_eye_height = play.eye_height;

                    let mut spawn = play.saved_position;
                    if spawn.y < play.floor_y + play.eye_height {
                        spawn.y = play.floor_y + play.eye_height;
                    }
                    self.game.camera_mut().enter_fps(spawn);
                    self.game.camera_mut().yaw = std::f32::consts::FRAC_PI_2;
                    self.game.camera_mut().pitch = 0.0;
                    play.saved_position = spawn;

                    let _ = self.window.set_cursor_grab(CursorGrabMode::Locked);
                    self.window.set_cursor_visible(false);
                    self.input.on_cursor_enter();
                    self.input.mouse_motion = (0.0, 0.0);
                    self.input.skip_motion_frames = 4;

                    self.particles.clear();
                    self.projectiles.clear();

                    log::info!(
                        "Entered play mode at ({:.2}, {:.2}, {:.2})",
                        spawn.x,
                        spawn.y,
                        spawn.z
                    );
                } else {
                    play.saved_position = self.game.camera().first_person_pos;
                    self.game.camera_mut().exit_fps();

                    let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                    self.window.set_cursor_visible(true);

                    self.particles.clear();
                    self.projectiles.clear();

                    log::info!("Exited play mode");
                }
            }
            EditorAction::SpawnPlayerHere => {
                let Some(e) = self.editor.state.primary() else {
                    return;
                };
                let Some(t) = self.world.get::<Transform>(e) else {
                    return;
                };
                let mut pos = t.position;
                pos.y += self.editor.state.play.eye_height;
                self.editor.state.play.saved_position = pos;
                log::info!(
                    "Player spawn set to ({:.2}, {:.2}, {:.2})",
                    pos.x,
                    pos.y,
                    pos.z
                );
            }
            EditorAction::CopyEntity => {
                use crate::scene::serialize::snapshot_entity;
                let snaps: Vec<_> = self
                    .editor
                    .state
                    .selected
                    .iter()
                    .filter_map(|&e| snapshot_entity(&self.world, e))
                    .collect();
                let n = snaps.len();
                self.editor.state.clipboard_entities = snaps;
                log::info!("Copied {} entities to clipboard", n);
            }
            EditorAction::PasteEntity => {
                let snaps = self.editor.state.clipboard_entities.clone();
                if snaps.is_empty() {
                    return;
                }
                self.editor.state.undo.push_forced(&self.world);

                let mut new_selected = Vec::new();
                let offset = Vec3::new(1.0, 0.0, 1.0);
                for mut snap in snaps {
                    if let Some(ref mut t) = snap.transform {
                        t.position[0] += offset.x;
                        t.position[1] += offset.y;
                        t.position[2] += offset.z;
                    }
                    snap.parent = None;
                    if let Some(ref mut n) = snap.name {
                        n.push_str("_paste");
                    }
                    let e = crate::scene::serialize::spawn_snapshot(&mut self.world, snap);
                    new_selected.push(e);
                }
                // Уникализируем материалы вставленных сущностей.
                for &ne in &new_selected {
                    self.ensure_unique_material_for(ne);
                }
                self.editor.state.selected = new_selected;
                log::info!("Pasted entities");
            }
            EditorAction::MakeMaterialUnique => {
                let Some(e) = self.editor.state.primary() else {
                    return;
                };
                let Some(mh) = self.world.get::<MaterialHandle>(e).cloned() else {
                    return;
                };
                let Some(mat) = self.renderer.materials.get(&mh.0).cloned() else {
                    return;
                };

                let new_name = format!("{}_uniq_{}", mh.0, e);
                self.renderer.add_material(&new_name, mat);
                self.world.insert(e, MaterialHandle(new_name.clone()));
                log::info!("Material made unique: {}", new_name);
            }
            EditorAction::Undo => {
                if let Some(new_world) = self.editor.state.undo.undo(&self.world) {
                    self.world = new_world;
                    self.editor.state.prune_selection(&self.world);
                }
            }
            EditorAction::Redo => {
                if let Some(new_world) = self.editor.state.undo.redo(&self.world) {
                    self.world = new_world;
                    self.editor.state.prune_selection(&self.world);
                }
            }
            EditorAction::Save => {
                let path = self.editor.state.save_path.clone();
                let spawn = Some(self.editor.state.play.saved_position);
                if let Err(e) =
                    crate::scene::save_scene_to_file(&self.world, &path, spawn)
                {
                    log::error!("Save failed: {}", e);
                }
            }
            EditorAction::Load => {
                let path = self.editor.state.save_path.clone();
                match crate::scene::load_scene_from_file(&path) {
                    Ok((new_world, spawn)) => {
                        self.world = new_world;
                        self.editor.state.selected.clear();
                        self.editor.state.undo.clear();
                        if let Some(p) = spawn {
                            self.editor.state.play.saved_position = p;
                        }
                    }
                    Err(e) => log::error!("Load failed: {}", e),
                }
            }
            EditorAction::NewScene => {
                self.world = World::new();
                self.editor.state.selected.clear();
                self.editor.state.undo.clear();
                self.editor.state.clipboard_entities.clear();
                self.editor.state.clipboard_transform = None;
                self.particles.clear();
                self.projectiles.clear();
                log::info!("Created new empty scene");
            }
            EditorAction::AddCube | EditorAction::AddSphere => {
                self.editor.state.undo.push_forced(&self.world);

                let (mesh, mat, base_name) = match action {
                    EditorAction::AddCube => ("cube", "flat_blue", "Cube"),
                    EditorAction::AddSphere => ("sphere", "gold", "Sphere"),
                    _ => unreachable!(),
                };

                let cam_target = self.game.camera().target;
                let pos = cam_target + Vec3::new(0.0, 1.0, 0.0);

                let e = self.world.spawn();
                self.world.insert(
                    e,
                    crate::game::components::Name(format!("{}_{:04}", base_name, e)),
                );
                self.world.insert(e, Transform::at(pos));
                self.world.insert(e, MeshHandle(mesh.to_string()));
                self.world.insert(e, MaterialHandle(mat.to_string()));

                // Уникализируем сразу.
                self.ensure_unique_material_for(e);

                self.editor.state.select_single(e);
            }
            EditorAction::DeleteSelected => {
                if self.editor.state.selected.is_empty() {
                    return;
                }
                self.editor.state.undo.push_forced(&self.world);
                let victims: Vec<Entity> = self.editor.state.selected.clone();
                let mut all_victims = victims.clone();
                for &v in &victims {
                    for &c in self.world.entities() {
                        if let Some(Parent(p)) = self.world.get::<Parent>(c).copied() {
                            if p == v && !all_victims.contains(&c) {
                                all_victims.push(c);
                            }
                        }
                    }
                }
                for e in all_victims {
                    self.world.despawn(e);
                }
                self.editor.state.selected.clear();
            }
            EditorAction::Duplicate => {
                self.editor.state.undo.push_forced(&self.world);
                self.duplicate_selected();
            }
            EditorAction::FocusSelected => {
                if self.editor.state.selected.is_empty() {
                    return;
                }
                let Some(center) =
                    gizmo::group_center(&self.world, &self.editor.state.selected)
                else {
                    return;
                };
                let radius =
                    gizmo::group_radius(&self.world, &self.editor.state.selected, &self.renderer);
                self.game.camera_mut().focus_on(center, radius);
            }
            EditorAction::SavePrefab => {
                let name = self.editor.state.prefab_save_name.trim().to_string();
                if name.is_empty() {
                    log::warn!("Prefab name is empty");
                    return;
                }
                if self.editor.state.selected.is_empty() {
                    log::warn!("Nothing selected for prefab");
                    return;
                }

                let prefab = crate::scene::prefab::prefab_from_selection(
                    &self.world,
                    &self.editor.state.selected,
                    Some(name.clone()),
                );
                let n = prefab.entities.len();

                let dir = self.editor.state.prefabs_dir.clone();
                let path = std::path::Path::new(&dir)
                    .join(format!("{}.prefab.ron", name));

                match crate::scene::prefab::save_prefab_to_file(&prefab, &path) {
                    Ok(()) => {
                        log::info!(
                            "Saved prefab '{}' ({} entities) → {}",
                            name,
                            n,
                            path.display()
                        );
                        self.editor.state.prefab_list =
                            crate::scene::prefab::list_prefabs(&dir);
                    }
                    Err(e) => log::error!("Prefab save failed: {}", e),
                }
            }
            EditorAction::RefreshPrefabs => {
                let dir = self.editor.state.prefabs_dir.clone();
                self.editor.state.prefab_list =
                    crate::scene::prefab::list_prefabs(&dir);
                log::info!(
                    "Prefabs refreshed: {} files",
                    self.editor.state.prefab_list.len()
                );
            }
            EditorAction::InstantiatePrefab(idx) => {
                let Some(path) = self
                    .editor
                    .state
                    .prefab_list
                    .get(idx as usize)
                    .cloned()
                else {
                    log::warn!("Prefab index {} out of range", idx);
                    return;
                };

                let prefab = match crate::scene::prefab::load_prefab_from_file(&path) {
                    Ok(p) => p,
                    Err(e) => {
                        log::error!("Failed to load prefab: {}", e);
                        return;
                    }
                };

                self.editor.state.undo.push_forced(&self.world);

                let spawn_pos = self.game.camera().target + Vec3::new(0.0, 1.0, 0.0);
                let new_entities = crate::scene::prefab::instantiate_prefab(
                    &mut self.world,
                    &prefab,
                    spawn_pos,
                );
                let n = new_entities.len();

                // Уникализируем материалы всех сущностей инстанса.
                for &ne in &new_entities {
                    self.ensure_unique_material_for(ne);
                }

                self.editor.state.selected = new_entities;

                log::info!(
                    "Instantiated prefab '{}' ({} entities) at ({:.2}, {:.2}, {:.2})",
                    path.display(),
                    n,
                    spawn_pos.x,
                    spawn_pos.y,
                    spawn_pos.z
                );
            }
            EditorAction::LoadTextures => {
                let files = rfd::FileDialog::new()
                    .add_filter(
                        "Images",
                        &[
                            "png", "jpg", "jpeg", "gif", "webp", "bmp",
                            "tif", "tiff", "tga", "dds", "hdr", "exr",
                            "ico", "pnm", "pbm", "pgm", "ppm", "qoi",
                            "ff", "farbfeld",
                        ],
                    )
                    .add_filter("All files", &["*"])
                    .pick_files();

                let Some(paths) = files else {
                    return;
                };

                let mut loaded = 0usize;
                let mut failed = 0usize;

                for path in paths {
                    let base_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_nanos())
                                .unwrap_or(0);
                            format!("tex_{}", now)
                        });

                    let mut final_name = base_name.clone();
                    let mut counter = 1u32;
                    while self.renderer.textures.contains_key(&final_name) {
                        final_name = format!("{}_{}", base_name, counter);
                        counter += 1;
                    }

                    let path_str = path.to_string_lossy().into_owned();

                    match self.renderer.load_texture(&final_name, &path_str) {
                        Ok(()) => {
                            let (w, h) = self
                                .renderer
                                .texture_size(&final_name)
                                .unwrap_or((0, 0));
                            log::info!(
                                "Loaded texture '{}' ({}×{}) from {}",
                                final_name,
                                w,
                                h,
                                path.display()
                            );
                            loaded += 1;
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to load texture '{}': {}",
                                path.display(),
                                e
                            );
                            failed += 1;
                        }
                    }
                }

                if loaded + failed > 0 {
                    log::info!("Textures: {} loaded, {} failed", loaded, failed);
                }
            }
            EditorAction::RemoveTexture(name) => {
                if self.renderer.remove_texture(&name) {
                    log::info!("Removed texture '{}'", name);
                } else {
                    log::warn!("Texture '{}' not found", name);
                }
            }
            EditorAction::PlacePalette | EditorAction::ClearPalette => {}
        }
    }
}

pub fn run<G: Game>(mut game: G) {
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();

    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Rust Engine 3D")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
            .build(&event_loop)
            .unwrap(),
    );

    let mut renderer = pollster::block_on(Renderer::new(window.clone()));

    let mut world = World::new();
    game.camera_mut()
        .set_viewport(renderer.size.width, renderer.size.height);
    game.init(&mut world, &mut renderer);

    let editor = Editor::new(
        &window,
        &renderer.device,
        renderer.config.format,
        egui::ViewportId::ROOT,
    );

    let mut app = App {
        game,
        world,
        input: Input::new(),
        time: Time::new(),
        window: window.clone(),
        renderer,
        editor,
        ui_state: UiState::new(),
        mouse_press_pos: None,
        viewport_rect: None,
        rmb_press_time: None,
        rmb_press_pos: None,
        rmb_dragged: false,
        particles: Vec::new(),
        projectiles: Vec::new(),
        audio: AudioSystem::new(),
    };

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);

            match event {
                Event::WindowEvent { event, window_id } if window_id == app.window.id() => {
                    let consumed = app.editor.on_window_event(&app.window, &event);

                    let wants_keyboard = app.editor.egui_ctx.wants_keyboard_input();

                    let in_play = app.editor.state.play.active;
                    let (px, py) = app.input.mouse_pos;
                    let in_vp = in_play || app.in_viewport(px, py);

                    match event {
                        WindowEvent::CloseRequested => elwt.exit(),

                        WindowEvent::Resized(size) => {
                            app.renderer.resize(size);
                            app.game
                                .camera_mut()
                                .set_viewport(size.width, size.height);
                        }

                        WindowEvent::KeyboardInput { ref event, .. }
                            if !consumed && !wants_keyboard =>
                        {
                            app.input.on_key(event);

                            if app.editor.state.flying
                                && event.state == ElementState::Pressed
                                && event.physical_key
                                    == winit::keyboard::PhysicalKey::Code(KeyCode::Escape)
                            {
                                app.editor.state.flying = false;
                                app.input.editor_flying = false;
                                let _ = app.window.set_cursor_grab(CursorGrabMode::None);
                                app.window.set_cursor_visible(true);
                            }
                        }

                        WindowEvent::MouseInput { state, button, .. }
                            if !consumed && in_vp && !in_play =>
                        {
                            app.input.on_mouse_button(button, state);

                            if button == MouseButton::Left {
                                match state {
                                    ElementState::Pressed => {
                                        if app.editor.state.palette.active.is_some() {
                                            if let Some(item) = app.editor.state.palette.active {
                                                if let Some(pos) =
                                                    app.editor.state.palette.preview_pos
                                                {
                                                    let ctrl =
                                                        app.input.key_down(KeyCode::ControlLeft)
                                                        || app.input
                                                            .key_down(KeyCode::ControlRight);
                                                    let snap = ctrl
                                                        || app.editor.state.palette.snap_to_grid;
                                                    let step =
                                                        app.editor.state.palette.grid_step;
                                                    let final_pos = if snap {
                                                        placement::snap_to_grid(pos, step)
                                                    } else {
                                                        pos
                                                    };
                                                    app.spawn_palette_item(item, final_pos);
                                                    if !app.editor.state.palette.keep_active {
                                                        app.editor.state.palette.active = None;
                                                    }
                                                }
                                            }
                                            app.mouse_press_pos = None;
                                        } else {
                                            let started_gizmo =
                                                if !app.editor.state.selected.is_empty() {
                                                    let (mx, my) = app.input.mouse_pos;
                                                    let ax = gizmo::pick_axis(
                                                        &app.world,
                                                        &app.editor.state.selected,
                                                        app.game.camera(),
                                                        app.editor.state.gizmo.mode,
                                                        &app.renderer,
                                                        mx,
                                                        my,
                                                    );
                                                    if let Some(axis) = ax {
                                                        let alt = app.input
                                                            .key_down(KeyCode::AltLeft)
                                                            || app.input
                                                                .key_down(KeyCode::AltRight);
                                                        if alt {
                                                            app.editor
                                                                .state
                                                                .undo
                                                                .push_forced(&app.world);
                                                            app.duplicate_selected();
                                                        }

                                                        if let Some(drag) = gizmo::begin_drag(
                                                            &app.world,
                                                            &app.editor.state.selected,
                                                            axis,
                                                            app.editor.state.gizmo.mode,
                                                            app.game.camera(),
                                                            &app.renderer,
                                                            mx,
                                                            my,
                                                        ) {
                                                            app.editor
                                                                .state
                                                                .undo
                                                                .push_forced(&app.world);
                                                            app.editor.state.gizmo.drag =
                                                                Some(drag);
                                                            app.editor.state.gizmo.hovered =
                                                                Some(axis);
                                                            true
                                                        } else {
                                                            false
                                                        }
                                                    } else {
                                                        false
                                                    }
                                                } else {
                                                    false
                                                };

                                            if !started_gizmo {
                                                let (mx, my) = app.input.mouse_pos;
                                                app.editor.state.box_select =
                                                    Some(BoxSelect {
                                                        start: (mx, my),
                                                        current: (mx, my),
                                                    });
                                                app.mouse_press_pos = None;
                                            }
                                        }
                                    }
                                    ElementState::Released => {
                                        app.editor.state.gizmo.drag = None;

                                        if let Some(bs) = app.editor.state.box_select.take() {
                                            let dx = bs.current.0 - bs.start.0;
                                            let dy = bs.current.1 - bs.start.1;

                                            if dx * dx + dy * dy < 9.0 {
                                                let (mx, my) = app.input.mouse_pos;
                                                app.try_pick((mx, my));
                                            } else {
                                                let entities = crate::editor::picking::entities_in_screen_rect(
                                                    &app.world,
                                                    &app.renderer,
                                                    app.game.camera(),
                                                    (bs.start.0, bs.start.1, bs.current.0, bs.current.1),
                                                );

                                                let shift = app.input.key_down(KeyCode::ShiftLeft)
                                                    || app.input.key_down(KeyCode::ShiftRight);

                                                if shift {
                                                    for e in entities {
                                                        if !app.editor.state.selected.contains(&e) {
                                                            app.editor.state.selected.push(e);
                                                        }
                                                    }
                                                } else {
                                                    app.editor.state.selected = entities;
                                                }
                                            }

                                            app.mouse_press_pos = None;
                                        } else if let Some((px, py)) = app.mouse_press_pos.take() {
                                            let (mx, my) = app.input.mouse_pos;
                                            let dx = mx - px;
                                            let dy = my - py;
                                            if dx * dx + dy * dy < 9.0 {
                                                app.try_pick((mx, my));
                                            }
                                        }
                                    }
                                }
                            } else if button == MouseButton::Right {
                                match state {
                                    ElementState::Pressed => {
                                        app.rmb_press_time = Some(Instant::now());
                                        app.rmb_press_pos = Some(app.input.mouse_pos);
                                        app.rmb_dragged = false;
                                    }
                                    ElementState::Released => {
                                        let was_drag = app.rmb_dragged;
                                        if !was_drag {
                                            if let Some(t) = app.rmb_press_time {
                                                if t.elapsed().as_millis() < 300 {
                                                    app.editor.state.context_menu_pos =
                                                        Some(app.input.mouse_pos);
                                                }
                                            }
                                        }
                                        app.rmb_press_time = None;
                                        app.rmb_press_pos = None;
                                        app.rmb_dragged = false;
                                    }
                                }
                            }
                        }

                        WindowEvent::CursorMoved { position, .. } if !consumed => {
                            app.input
                                .on_mouse_move(position.x as f32, position.y as f32);

                            let (mx, my) = app.input.mouse_pos;

                            if let Some(bs) = &mut app.editor.state.box_select {
                                bs.current = (mx, my);
                            }

                            if app.input.mouse_down(MouseButton::Right) {
                                if let Some((px, py)) = app.rmb_press_pos {
                                    if !app.rmb_dragged {
                                        let dx = mx - px;
                                        let dy = my - py;
                                        if dx * dx + dy * dy > 25.0 {
                                            app.rmb_dragged = true;
                                        }
                                    }
                                }
                            }

                            if !in_play
                                && !app.editor.state.flying
                                && !app.editor.state.selected.is_empty()
                                && app.editor.state.box_select.is_none()
                            {
                                if let Some(drag) = app.editor.state.gizmo.drag.clone() {
                                    let snap = app.input.key_down(KeyCode::ControlLeft)
                                        || app.input.key_down(KeyCode::ControlRight)
                                        || app.editor.state.gizmo.snap_enabled;
                                    gizmo::apply_drag_with_mode(
                                        &mut app.world,
                                        &drag,
                                        app.editor.state.gizmo.mode,
                                        snap,
                                        app.game.camera(),
                                        &app.renderer,
                                        mx,
                                        my,
                                    );
                                } else if app.in_viewport(mx, my) {
                                    app.editor.state.gizmo.hovered = gizmo::pick_axis(
                                        &app.world,
                                        &app.editor.state.selected,
                                        app.game.camera(),
                                        app.editor.state.gizmo.mode,
                                        &app.renderer,
                                        mx,
                                        my,
                                    );
                                } else {
                                    app.editor.state.gizmo.hovered = None;
                                }
                            }
                        }

                        WindowEvent::CursorEntered { .. } => {
                            app.input.on_cursor_enter();
                        }
                        WindowEvent::CursorLeft { .. } => {
                            app.input.on_cursor_enter();
                        }

                        WindowEvent::MouseWheel { delta, .. }
                            if !consumed && in_vp && !in_play =>
                        {
                            let d = match delta {
                                MouseScrollDelta::LineDelta(_, y) => y,
                                MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.0,
                            };
                            app.input.on_scroll(d);
                        }

                        WindowEvent::RedrawRequested => {
                            app.redraw(elwt);
                            app.window.request_redraw();
                        }

                        _ => {}
                    }
                }

                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta },
                    ..
                } => {
                    app.input
                        .on_mouse_motion_device(delta.0 as f32, delta.1 as f32);
                }

                Event::AboutToWait => {
                    app.window.request_redraw();
                }

                _ => {}
            }
        })
        .unwrap();
}