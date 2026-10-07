use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::KeyCode;
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::app_state::{AppMode, AppSettings, RESOLUTIONS};
use crate::ecs::{Entity, World};
use crate::editor::gizmo::{self, GizmoMode};
use crate::editor::picking::PickableSet;
use crate::editor::palette::PaletteItem;
use crate::editor::placement;
use crate::editor::terrain_tool::TerrainBrush;
use crate::editor::ui::{self as editor_ui, AudioSnapshot, Stats, UiAssets, UiState};
use crate::editor::{BoxSelect, Editor, EditorAction};
use crate::game::ai::{NoiseEvent, NoiseKind};
use crate::game::audio::{AudioBus, AudioSource};
use crate::game::components::{
    Chase, Elevator, ElevatorState, Health, Interactable, MaterialHandle, MeshHandle, Parent,
    SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint, Transform, Trigger, TriggerAction,
    Velocity, Visible,
};
use crate::game::decals::Decal;
use crate::game::lights::{DirectionalLight, PointLight};
use crate::menu as game_menu;
use crate::physics::navmesh::{BakeOpts, Navmesh};
use crate::physics::{BodyType, PhysicsWorld};
use crate::render::decal::{DecalDraw, DecalInstance};
use crate::render::terrain::{update_terrain_mesh_region, update_terrain_texture_region};
use crate::render::{
    camera::CameraMode, Camera3D, EguiFrameData, GpuLight, GpuPointLight, LineBatch, LineVertex,
    MeshDraw, ParticleInstance, PostFx, Renderer,
};
use glam::Vec3;

use super::ai_system::AiSystem;
use super::audio::AudioSystem;
use super::collision::{self, PlayerCapsule};
use super::input::Input;
use super::particles::{self, Particle, MAX_PARTICLES};
use super::time::Time;

pub trait Game: 'static {
    fn init(&mut self, _world: &mut World, _renderer: &mut Renderer) {}

    fn init_with_assets(
        &mut self,
        world: &mut World,
        renderer: &mut Renderer,
        _assets: &crate::assets::AssetDatabase,
    ) {
        self.init(world, renderer);
    }

    fn configure_input(&mut self, _map: &mut crate::engine::InputMap) {}

    fn wants_navmesh_bake(&mut self) -> bool { false }

    fn update(&mut self, _world: &mut World, _input: &Input, _renderer: &mut Renderer, _dt: f32) -> bool { true }
    fn collect_draws(&mut self, _world: &mut World, _renderer: &Renderer) -> Vec<MeshDraw> { Vec::new() }
    fn collect_lines(&mut self, _world: &mut World, _renderer: &Renderer, _selected: &[Entity]) -> Vec<LineVertex> { Vec::new() }
    fn dir_lights(&self, world: &World) -> Vec<GpuLight> { let _ = world; Vec::new() }
    fn point_lights(&self, world: &World) -> Vec<GpuPointLight> { let _ = world; Vec::new() }
    fn ambient(&self) -> [f32; 3] { [0.18, 0.20, 0.26] }
    fn postfx(&self) -> PostFx { PostFx::default() }
    fn apply_postfx(&mut self, _postfx: PostFx) {}
    fn camera(&self) -> &Camera3D;
    fn camera_mut(&mut self) -> &mut Camera3D;
    fn lod_stats(&self) -> [usize; 4] { [0; 4] }
    fn rpg_hud(&self) -> Vec<(String, String)> { Vec::new() }
    fn on_kill(&mut self, _world: &mut World, _target: Entity) {}

    fn collect_ui(
        &mut self,
        _world: &mut World,
        _renderer: &Renderer,
        _ui: &mut crate::ui::UiLayer,
    ) {}
    fn on_play_enter(&mut self, _world: &World) -> Option<Box<dyn Any>> { None }
    fn on_play_exit(&mut self, _state: Box<dyn Any>) {}
    fn save_game_state(&self) -> Option<String> { None }
    fn load_game_state(&mut self, _ron: &str) {}
    fn take_hit_stop(&mut self) -> Option<(f32, f32)> { None }
    fn on_pause_changed(&mut self, _paused: bool) {}
    fn load_world_request(&mut self) -> Option<WorldLoadRequest> {
        None
    }

    /// Игра запрашивает возврат в главное меню (например, игрок нажал
    /// «Quit to Main Menu» в паузе). App прочитает это и переключит mode.
    fn take_quit_to_menu(&mut self) -> bool { false }
}

pub struct WorldLoadRequest {
    pub scene_ron: String,
    pub game_state_ron: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ShotFired {
    pub origin: Vec3,
    pub direction: Vec3,
}

#[derive(Clone, Copy)]
struct Projectile { position: Vec3, velocity: Vec3, age: f32, max_age: f32, damage: f32 }

struct PlaySnapshot {
    scene_ron: String,
    saved_position: Vec3,
    selection: Vec<Entity>,
    camera_mode: CameraMode,
    camera_pos: Vec3,
    camera_yaw: f32,
    camera_pitch: f32,
    camera_target: Vec3,
    camera_distance: f32,
    game_state: Option<Box<dyn Any>>,
}

pub struct App<G: Game> {
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
    rmb_press_time: Option<Instant>,
    rmb_press_pos: Option<(f32, f32)>,
    rmb_dragged: bool,
    particles: Vec<Particle>,
    projectiles: Vec<Projectile>,
    audio: Option<AudioSystem>,
    physics: PhysicsWorld,
    play_snapshot: Option<PlaySnapshot>,
    elevator_prev_state: HashMap<Entity, ElevatorState>,
    was_paused: bool,

    pickable: Option<PickableSet>,

    navmesh: Option<Navmesh>,
    ai_system: AiSystem,

    footstep_timer: f32,
    pub asset_db: crate::assets::AssetDatabase,
    hot_reload: crate::assets::HotReload,

    // === App-level state ===
    pub mode: AppMode,
    pub settings: AppSettings,
}

impl<G: Game> Drop for App<G> {
    fn drop(&mut self) {
        if let Err(e) = self.settings.save("settings.ron") {
            log::warn!("Failed to save settings.ron: {}", e);
        }

        let mut s = self.editor.state.collect_settings(
            self.ui_state.show_renderer_panel,
            self.ui_state.show_stats_panel,
            self.ui_state.show_hierarchy_panel,
            self.ui_state.show_inspector_panel,
            self.ui_state.show_audio_panel,
            self.ui_state.left_panel_width,
            self.ui_state.right_panel_width,
        );
        s.postfx = self.game.postfx();

        if let Some(audio) = self.audio.as_ref() {
            s.audio_bus_volumes.clear();
            for (bus, vol) in audio.all_bus_volumes() {
                s.audio_bus_volumes.insert(bus.name().to_string(), vol);
            }
        }

        if let Err(e) = s.save("editor.ron") {
            log::warn!("Failed to save editor settings: {}", e);
        } else {
            log::info!("Editor settings saved to editor.ron");
        }

        if let Err(e) = self.input.map.save("input.ron") {
            log::warn!("Failed to save input.ron: {}", e);
        } else {
            log::info!("InputMap saved to input.ron");
        }
    }
}

impl<G: Game> App<G> {
    fn new(elwt: &ActiveEventLoop, mut game: G) -> Self {
        let window = Arc::new(
            elwt.create_window(
                Window::default_attributes()
                    .with_title("Rust Engine 3D")
                    .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
            ).expect("create window failed"),
        );
        let mut renderer = pollster::block_on(Renderer::new(window.clone()));
        let mut world = World::new();
        game.camera_mut().set_viewport(renderer.size.width, renderer.size.height);

        let asset_db = crate::assets::AssetDatabase::open("assets")
            .expect("failed to open assets directory");

        game.init_with_assets(&mut world, &mut renderer, &asset_db);

        let input = {
            let mut input = Input::new();
            game.configure_input(&mut input.map);
            let restored = crate::engine::InputMap::load_or_default("input.ron", input.map.clone());
            input.map = restored;
            input
        };

        let editor = Editor::new(&window, &renderer.device, renderer.config.format, egui::ViewportId::ROOT);
        let app_settings = AppSettings::load_or_default("settings.ron");
        let app_mode = AppMode::default();
        let mut app = Self {
            game, world, input, time: Time::new(),
            window: window.clone(), renderer, editor,
            ui_state: UiState::new(),
            mouse_press_pos: None, viewport_rect: None,
            rmb_press_time: None, rmb_press_pos: None, rmb_dragged: false,
            particles: Vec::new(), projectiles: Vec::new(),
            audio: AudioSystem::new(),
            physics: PhysicsWorld::default(),
            play_snapshot: None,
            elevator_prev_state: HashMap::new(),
            was_paused: false,
            pickable: None,
            navmesh: None,
            ai_system: AiSystem::new(),
            footstep_timer: 0.0,
            asset_db,
            hot_reload: crate::assets::HotReload::new(),
            mode: app_mode,
            settings: app_settings,
        };
        let initial_postfx = app.editor.state.settings.postfx;
        app.game.apply_postfx(initial_postfx);

        if let Some(audio) = app.audio.as_mut() {
            let saved = app.editor.state.settings.audio_bus_volumes.clone();
            for (name, vol) in &saved {
                for b in AudioBus::ALL {
                    if b.name() == name {
                        audio.set_bus_volume(b, *vol);
                    }
                }
            }
            if !saved.is_empty() {
                log::info!("audio: applied {} bus volume(s) from editor.ron", saved.len());
            }
        }

        // Применить начальные настройки к окну и аудио
        app.apply_settings_to_window(window.clone());
        app.apply_settings_to_audio();

        app
    }

    fn apply_settings_to_window(&self, window: Arc<Window>) {
        let s = &self.settings.window;
        if let Some((w, h, _)) = RESOLUTIONS.get(s.resolution_index) {
            let _ = window.request_inner_size(winit::dpi::LogicalSize::new(*w, *h));
        }
        window.set_fullscreen(if s.fullscreen {
            Some(winit::window::Fullscreen::Borderless(None))
        } else {
            None
        });
    }

    fn apply_settings_to_audio(&mut self) {
        if let Some(audio) = self.audio.as_mut() {
            let a = &self.settings.audio;
            audio.set_bus_volume(AudioBus::Master, a.master);
            audio.set_bus_volume(AudioBus::Sfx, a.sfx);
            audio.set_bus_volume(AudioBus::Music, a.music);
            audio.set_bus_volume(AudioBus::Voice, a.voice);
            audio.set_bus_volume(AudioBus::Ui, a.ui);
        }
    }

    fn ensure_pickable<'a>(
        world: &World,
        renderer: &Renderer,
        slot: &'a mut Option<PickableSet>,
    ) -> &'a PickableSet {
        if slot.is_none() {
            *slot = Some(PickableSet::build(world, renderer));
        }
        slot.as_ref().unwrap()
    }

    fn apply_camera_preset(&mut self, preset: u8) {
        let cam = self.game.camera_mut();
        let t = cam.target;
        let r = cam.distance;
        match preset {
            0 => { cam.yaw = -std::f32::consts::FRAC_PI_2; cam.pitch = 0.0; }
            1 => { cam.yaw = std::f32::consts::FRAC_PI_2; cam.pitch = 0.0; }
            2 => { cam.yaw = std::f32::consts::PI; cam.pitch = 0.0; }
            3 => { cam.yaw = 0.0; cam.pitch = 0.0; }
            4 => { cam.pitch = -std::f32::consts::FRAC_PI_2 + 0.001; }
            5 => { cam.pitch = std::f32::consts::FRAC_PI_2 - 0.001; }
            6 => { cam.yaw = -std::f32::consts::FRAC_PI_4; cam.pitch = std::f32::consts::FRAC_PI_6; }
            _ => {}
        }
        cam.target = t;
        cam.distance = r;
        log::info!("Camera preset {} applied", preset);
    }

    fn capture_play_snapshot(&mut self) -> PlaySnapshot {
        let scene_ron = crate::scene::save_scene_with_assets_to_string(
            &self.world, &self.renderer, None,
        ).unwrap_or_else(|e| {
            log::error!("Failed to snapshot scene for Play: {}", e);
            String::new()
        });
        let cam = self.game.camera();
        let camera_mode = cam.mode;
        let camera_pos = cam.first_person_pos;
        let camera_yaw = cam.yaw;
        let camera_pitch = cam.pitch;
        let camera_target = cam.target;
        let camera_distance = cam.distance;
        let game_state = self.game.on_play_enter(&self.world);
        PlaySnapshot {
            scene_ron,
            saved_position: self.editor.state.play.saved_position,
            selection: self.editor.state.selected.clone(),
            camera_mode, camera_pos, camera_yaw, camera_pitch, camera_target, camera_distance,
            game_state,
        }
    }

    fn restore_play_snapshot(&mut self, snap: PlaySnapshot) {
        if snap.scene_ron.is_empty() {
            log::error!("Play-in-Editor: empty snapshot, skipping restore");
            return;
        }
        match crate::scene::load_scene_with_assets_from_str_full_with_ids(
            &snap.scene_ron, &mut self.renderer,
        ) {
            Ok((mut new_world, _spawn, _embedded_gs, id_map)) => {
                new_world.sync_next_id();
                self.world = new_world;
                self.editor.state.selected = snap.selection.iter()
                    .filter_map(|old| id_map.get(old).copied()).collect();
                log::info!(
                    "Play-in-Editor: world restored ({} entities, {} selected remapped)",
                    self.world.len(), self.editor.state.selected.len()
                );
                self.navmesh = None;
            }
            Err(e) => log::error!("Failed to restore world after Play: {}", e),
        }
        let cam = self.game.camera_mut();
        cam.mode = snap.camera_mode;
        cam.first_person_pos = snap.camera_pos;
        cam.yaw = snap.camera_yaw;
        cam.pitch = snap.camera_pitch;
        cam.target = snap.camera_target;
        cam.distance = snap.camera_distance;
        self.editor.state.play.saved_position = snap.saved_position;
        if let Some(state) = snap.game_state {
            self.game.on_play_exit(state);
        }
    }

    fn update_elevator_ding(&mut self) {
        let states: Vec<(Entity, ElevatorState)> = self.world.query::<Elevator>()
            .map(|(e, el)| (e, el.state)).collect();
        for (e, state) in states {
            let prev = self.elevator_prev_state.get(&e).copied();
            if matches!((prev, state), (Some(ElevatorState::Moving), ElevatorState::DoorsOpening)) {
                if let Some(audio) = &mut self.audio { audio.play("ding"); }
            }
            self.elevator_prev_state.insert(e, state);
        }
        let live: std::collections::HashSet<Entity> = self.world.query::<Elevator>().map(|(e, _)| e).collect();
        self.elevator_prev_state.retain(|k, _| live.contains(k));
    }

    fn bake_navmesh(&mut self) {
        let opts = BakeOpts::default();
        let nm = Navmesh::bake(&self.world, &opts);
        log::info!(
            "Navmesh baked: {} walkable cells, grid {}×{}, cell_size {:.2}",
            nm.walkable_count(),
            nm.grid_size().0,
            nm.grid_size().1,
            nm.cell_size(),
        );
        self.navmesh = Some(nm);
    }

    fn flush_terrain_dirty(&mut self) {
        let Some(region) = self.editor.state.terrain.dirty_region.take() else { return };
        let brush = self.editor.state.terrain.brush;
        let tex_size = self.editor.state.terrain.texture_size;
        let Some(hm) = self.renderer.terrain.as_ref() else { return };

        let (x0, z0, x1, z1) = region.to_mesh_vertex_rect(hm.resolution);
        if let Some(mesh) = self.renderer.meshes.get_mut("terrain") {
            update_terrain_mesh_region(
                &self.renderer.queue, mesh, hm, x0, z0, x1, z1,
            );
        }
        if matches!(brush, TerrainBrush::Paint(_)) {
            if let Some(tex) = self.renderer.textures.get("terrain_tex") {
                update_terrain_texture_region(
                    &self.renderer.queue, &tex.texture, hm,
                    tex_size, region,
                );
            }
        }
        self.pickable = None;
    }

    fn update_terrain_cursor(&mut self) {
        if self.editor.state.play.active || self.editor.state.flying { return; }
        if !self.editor.state.terrain.active || self.editor.state.terrain.dragging { return; }
        let (mx, my) = self.input.mouse_pos;
        if !self.in_viewport(mx, my) {
            self.editor.state.terrain.cursor_world = None;
            return;
        }
        let (o, d) = self.game.camera().ray_from_screen(
            mx, my,
            self.renderer.size.width as f32,
            self.renderer.size.height as f32,
        );
        if let Some(hm) = self.renderer.terrain.as_ref() {
            let _ = self.editor.state.terrain.update_cursor(hm, o, d);
        }
    }

    fn update_player(&mut self, dt: f32) {
        {
            let sens = self.editor.state.play.look_sensitivity;
            let (mdx, mdy) = self.input.mouse_motion;
            self.game.camera_mut().fps_look(mdx * sens, mdy * sens);
        }

        let crouching = self.input.key_down(KeyCode::ControlLeft)
            || self.input.key_down(KeyCode::ControlRight);
        self.editor.state.play.crouching = crouching;
        {
            let play = &mut self.editor.state.play;
            let target = if crouching { play.crouch_height } else { play.eye_height };
            let lerp = (dt * 10.0).clamp(0.0, 1.0);
            play.current_eye_height += (target - play.current_eye_height) * lerp;
        }

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
        let crouch_mult = self.editor.state.play.crouch_speed_mult;
        let _push_strength = self.editor.state.play.push_strength;
        let step_down_max = self.editor.state.play.step_down_max;
        let slope_walk_limit_cos = self.editor.state.play.slope_walk_limit_cos;
        let slope_slide_speed = self.editor.state.play.slope_slide_speed;
        let ground_accel_tau = self.editor.state.play.ground_accel_tau;
        let air_accel_tau = self.editor.state.play.air_accel_tau;

        let cam_f = self.game.camera().forward();
        let fwd_xz = Vec3::new(cam_f.x, 0.0, cam_f.z).normalize_or_zero();
        let right_xz = Vec3::new(-fwd_xz.z, 0.0, fwd_xz.x);

        let running = self.input.key_down(KeyCode::ShiftLeft) && !crouching;
        let base_speed = if running { run_speed } else { walk_speed };
        let target_speed = if crouching { base_speed * crouch_mult } else { base_speed };

        let mut input_dir = Vec3::ZERO;
        if self.input.key_down(KeyCode::KeyW) { input_dir += fwd_xz; }
        if self.input.key_down(KeyCode::KeyS) { input_dir -= fwd_xz; }
        if self.input.key_down(KeyCode::KeyD) { input_dir += right_xz; }
        if self.input.key_down(KeyCode::KeyA) { input_dir -= right_xz; }
        if input_dir.length_squared() > 1e-8 { input_dir = input_dir.normalize(); }

        let target_velocity = input_dir * target_speed;

        let was_on_ground = self.editor.state.play.on_ground;

        {
            let play = &mut self.editor.state.play;
            play.coyote_timer = if was_on_ground {
                play.coyote_time
            } else {
                (play.coyote_timer - dt).max(0.0)
            };
        }

        let jump_pressed = self.input.key_pressed(KeyCode::Space);
        {
            let play = &mut self.editor.state.play;
            if jump_pressed {
                play.jump_buffer_timer = play.jump_buffer_time;
            } else {
                play.jump_buffer_timer = (play.jump_buffer_timer - dt).max(0.0);
            }
        }

        {
            let play = &mut self.editor.state.play;
            let tau = if was_on_ground { ground_accel_tau } else { air_accel_tau };
            let alpha = if tau > 1e-4 { 1.0 - (-dt / tau).exp() } else { 1.0 };
            play.horizontal_velocity +=
                (target_velocity - play.horizontal_velocity) * alpha;
        }

        let mut vvel = self.editor.state.play.vertical_velocity;
        let mut on_ground = self.editor.state.play.on_ground;
        let mut did_jump = false;

        let can_jump = {
            let play = &self.editor.state.play;
            play.jump_buffer_timer > 0.0
                && play.coyote_timer > 0.0
                && !crouching
        };
        if can_jump {
            vvel = jump_speed;
            on_ground = false;
            did_jump = true;
            let play = &mut self.editor.state.play;
            play.jump_buffer_timer = 0.0;
            play.coyote_timer = 0.0;
        }

        vvel -= gravity * dt;
        let dy = vvel * dt;

        let eye_pos = self.game.camera().first_person_pos;
        let feet = eye_pos - Vec3::Y * eye_height;
        let pcap = PlayerCapsule { radius: player_radius, height: player_height };

        let support_delta_y = {
            let mut dy_extra = 0.0_f32;
            if let Some(support) = collision::find_support_entity(&self.world, feet, &pcap) {
                if let Some(rb) = self.world.get::<crate::physics::RigidBody>(support) {
                    if rb.body_type == BodyType::Kinematic {
                        dy_extra = rb.velocity.y * dt;
                    }
                }
            }
            dy_extra
        };

        let horizontal = self.editor.state.play.horizontal_velocity;

        let delta = horizontal * dt + Vec3::new(0.0, dy + support_delta_y, 0.0);

        let terrain_ref = self.renderer.terrain.as_ref();
        let result = collision::resolve_movement_ex(
            &self.world, feet, delta, &pcap, floor_y, step_down_max, terrain_ref,
        );

        let new_feet = result.new_feet;
        let landed = result.landed;

        let mut slide_velocity = Vec3::ZERO;
        if let Some(support) = result.support {
            if support.normal.y < slope_walk_limit_cos {
                let n = support.normal;
                let g = Vec3::new(0.0, -gravity, 0.0);
                let tangent = g - n * g.dot(n);
                let steepness = ((slope_walk_limit_cos - n.y)
                    / slope_walk_limit_cos.max(1e-4))
                    .clamp(0.0, 1.0);
                slide_velocity = tangent.normalize_or_zero()
                    * slope_slide_speed
                    * steepness;
            }
        }

        if slide_velocity.length_squared() > 1e-8 {
            let play = &mut self.editor.state.play;
            play.horizontal_velocity += slide_velocity * dt;
        }

        if did_jump {
            on_ground = false;
        } else if landed && dy <= 0.0 {
            vvel = 0.0;
            on_ground = true;
        } else if dy < 0.0 && (new_feet.y - feet.y).abs() < 1e-4 {
            vvel = 0.0;
            on_ground = true;
        } else if dy > 0.0 && (new_feet.y - feet.y).abs() < 1e-4 {
            vvel = 0.0;
        } else {
            on_ground = false;
        }

        let horizontal_moved =
            ((new_feet.x - feet.x).powi(2) + (new_feet.z - feet.z).powi(2)).sqrt();
        let mut bob_dist = self.editor.state.play.bob_distance;
        let mut bob_cur = self.editor.state.play.bob_current;
        if bob_enabled && on_ground && horizontal_moved > 1e-6 {
            bob_dist += horizontal_moved * if running { 1.4 } else { 1.0 };
        }
        let bob_target = if bob_enabled { bob_dist.sin() * bob_amp } else { 0.0 };
        bob_cur = bob_cur * 0.85 + bob_target * 0.15;
        let new_eye = new_feet + Vec3::Y * (eye_height + bob_cur);
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
            play.ground_normal = result.support.map(|s| s.normal).unwrap_or(Vec3::Y);
        }

        if on_ground && horizontal_moved > 0.001 {
            self.footstep_timer -= dt;
            if self.footstep_timer <= 0.0 {
                self.footstep_timer = if running { 0.3 } else { 0.5 };
                let pos = self.game.camera().position();
                self.world.send(NoiseEvent {
                    position: pos,
                    radius: NoiseKind::Footstep.default_radius(),
                    kind: NoiseKind::Footstep,
                });
            }
        } else {
            self.footstep_timer = 0.0;
        }

        let origin = self.game.camera().position();
        let dir = self.game.camera().forward();

        let aim_hit = {
            let cache = Self::ensure_pickable(&self.world, &self.renderer, &mut self.pickable);
            crate::editor::picking::pick_ray_cached(&self.renderer, cache, origin, dir)
        };

        let interact_dist = self.editor.state.play.interact_distance;
        self.editor.state.play.highlight = aim_hit.as_ref().and_then(|(e, d)| {
            if *d <= interact_dist { Some(*e) } else { None }
        });

        let player_feet_now = self.game.camera().first_person_pos - Vec3::Y * eye_height;
        let triggers: Vec<Entity> = self.world.query::<Trigger>().map(|(e, _)| e).collect();
        for e in triggers {
            let trigger_data = self.world.get::<Trigger>(e).cloned();
            let trigger_pos = crate::game::world_position(&self.world, e);
            let (Some(mut t), Some(pos)) = (trigger_data, trigger_pos) else { continue; };
            if t.fired && t.once { continue; }
            let dist = (player_feet_now - pos).length();
            if dist <= t.radius {
                match &t.action {
                    TriggerAction::Teleport(target) => {
                        let new_feet = Vec3::from_array(*target);
                        let eye_h = self.editor.state.play.current_eye_height;
                        self.game.camera_mut().first_person_pos = new_feet + Vec3::Y * eye_h;
                        self.editor.state.play.vertical_velocity = 0.0;
                        self.editor.state.play.on_ground = true;
                    }
                    TriggerAction::Tint(c) => { self.world.insert(e, Tint(*c)); }
                    TriggerAction::Despawn => { self.world.despawn(e); continue; }
                    TriggerAction::CallElevator { elevator, floor_idx } => {
                        if let Some(el) = self.world.get_mut::<Elevator>(*elevator) {
                            el.call(*floor_idx as usize);
                        }
                    }
                    TriggerAction::PlaySound(name) => {
                        if let Some(audio) = &mut self.audio {
                            let n: &'static str = match name.as_str() {
                                "shot" => "shot", "explosion" => "explosion",
                                "pickup" => "pickup", "ding" => "ding", _ => "pickup",
                            };
                            audio.play(n);
                        }
                    }
                }
                t.fired = true;
                if t.once { self.world.insert(e, t); }
            }
        }

        let player_pos = self.game.camera().position();
        let chasers: Vec<Entity> = self.world.query::<Chase>().map(|(e, _)| e).collect();
        for e in chasers {
            if self.world.has::<crate::game::ai::AiAgent>(e) { continue; }
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

    fn update_fly(&mut self, dt: f32) {
        let sens = self.editor.state.fly_sensitivity;
        let (mdx, mdy) = self.input.mouse_motion;
        self.game.camera_mut().fly_look(mdx * sens, mdy * sens);
        let f = self.game.camera().forward();
        let r = self.game.camera().right();
        let mult = if self.input.key_down(KeyCode::ShiftLeft) { 3.0 }
            else if self.input.key_down(KeyCode::ControlLeft) { 0.3 } else { 1.0 };
        let speed = self.editor.state.fly_speed * mult * dt;
        let mut delta = Vec3::ZERO;
        if self.input.key_down(KeyCode::KeyW) { delta += f; }
        if self.input.key_down(KeyCode::KeyS) { delta -= f; }
        if self.input.key_down(KeyCode::KeyD) { delta += r; }
        if self.input.key_down(KeyCode::KeyA) { delta -= r; }
        if self.input.key_down(KeyCode::KeyE) || self.input.key_down(KeyCode::Space) { delta += Vec3::Y; }
        if self.input.key_down(KeyCode::KeyQ) { delta -= Vec3::Y; }
        if delta.length_squared() > 1e-6 {
            self.game.camera_mut().fly_move(delta.normalize() * speed);
        }
        if self.input.scroll_delta.abs() > 0.01 {
            let fs = &mut self.editor.state.fly_speed;
            *fs = (*fs * (1.0 + self.input.scroll_delta * 0.1)).clamp(0.5, 200.0);
        }
    }

    fn handle_shot_fired(&mut self, shot: ShotFired) {
        let dir = shot.direction.normalize_or_zero();
        if dir.length_squared() < 1e-8 { return; }
        let origin = shot.origin;

        if let Some(audio) = &mut self.audio {
            audio.play("shot");
        }

        self.world.send(NoiseEvent {
            position: origin,
            radius: NoiseKind::Gunshot.default_radius(),
            kind: NoiseKind::Gunshot,
        });

        let muzzle = origin + dir * 0.5;
        self.spawn_burst(muzzle, &particles::sparks(dir));

        let gun_range = self.editor.state.play.gun_range;
        let damage = self.editor.state.play.damage_per_shot;

        let hit = {
            let cache = Self::ensure_pickable(&self.world, &self.renderer, &mut self.pickable);
            crate::editor::picking::pick_ray_cached(&self.renderer, cache, origin, dir)
        };

        if let Some((target, dist)) = hit {
            if dist <= gun_range {
                let hit_point = origin + dir * dist;
                self.spawn_burst(hit_point, &particles::sparks(-dir));

                let mut killed = false;
                if let Some(h) = self.world.get_mut::<Health>(target) {
                    h.current -= damage;
                    if h.current <= 0.0 { killed = true; }
                }
                if killed {
                    let pos = self.world.get::<Transform>(target)
                        .map(|t| t.position)
                        .unwrap_or(Vec3::ZERO);
                    self.spawn_burst(pos, &particles::explosion());
                    if let Some(audio) = &mut self.audio {
                        audio.play("explosion");
                    }
                    self.game.on_kill(&mut self.world, target);
                    self.world.despawn(target);
                }
            }
        }
    }

    fn spawn_burst(&mut self, origin: Vec3, params: &particles::BurstParams) {
        particles::emit_burst(&mut self.particles, origin, params);
    }

    fn load_textures_common(&mut self, linear: bool) {
        let files = rfd::FileDialog::new()
            .add_filter("Images", &[
                "png", "jpg", "jpeg", "gif", "webp", "bmp",
                "tif", "tiff", "tga", "dds", "hdr", "exr",
                "ico", "pnm", "pbm", "pgm", "ppm", "qoi",
                "ff", "farbfeld",
            ])
            .add_filter("All files", &["*"])
            .pick_files();
        let Some(paths) = files else { return; };

        let mut loaded = 0usize;
        let mut failed = 0usize;

        for path in paths {
            let base_name = path.file_stem().and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos()).unwrap_or(0);
                    format!("tex_{}", now)
                });

            let mut final_name = base_name.clone();
            let mut counter = 1u32;
            while self.renderer.textures.contains_key(&final_name) {
                final_name = format!("{}_{}", base_name, counter);
                counter += 1;
            }

            let path_str = path.to_string_lossy().into_owned();

            let result = if linear {
                self.renderer.load_texture_linear(&final_name, &path_str)
            } else {
                self.renderer.load_texture(&final_name, &path_str)
            };

            match result {
                Ok(()) => {
                    log::info!("Loaded texture '{}' from {}", final_name, path.display());
                    loaded += 1;
                }
                Err(e) => {
                    log::error!("Failed to load texture '{}': {}", path.display(), e);
                    failed += 1;
                }
            }
        }

        if loaded + failed > 0 {
            log::info!("Textures: {} loaded ({}), {} failed",
                loaded, if linear { "linear" } else { "sRGB" }, failed);
        }
    }

    fn ensure_unique_material_for(&mut self, entity: Entity) -> Option<String> {
        let mh = self.world.get::<MaterialHandle>(entity).cloned()?;
        let current_name = mh.0;
        let user_count = self.world.entities().iter().filter(|&&ent| {
            self.world.get::<MaterialHandle>(ent).map(|m| m.0 == current_name).unwrap_or(false)
        }).count();
        if user_count <= 1 { return Some(current_name); }
        let unique_name = format!("{}_uniq_{}", current_name, entity);
        if !self.renderer.has_material(&unique_name) {
            let Some(base) = self.renderer.materials.get(&current_name).cloned() else { return Some(current_name); };
            self.renderer.add_material(&unique_name, base);
        }
        self.world.insert(entity, MaterialHandle(unique_name.clone()));
        log::info!("Auto-unique material: entity #{} '{}' → '{}'", entity, current_name, unique_name);
        Some(unique_name)
    }

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

    fn update_projectiles(&mut self, dt: f32) {
        if self.projectiles.is_empty() { return; }
        let floor_y = self.editor.state.play.floor_y;
        let mut hits: Vec<(Vec3, Option<Entity>, f32, Vec3)> = Vec::new();

        let projectiles = std::mem::take(&mut self.projectiles);
        let mut alive: Vec<Projectile> = Vec::with_capacity(projectiles.len());

        {
            let cache = Self::ensure_pickable(&self.world, &self.renderer, &mut self.pickable);

            for mut p in projectiles {
                p.age += dt;
                if p.age >= p.max_age { continue; }
                let prev = p.position;
                p.position += p.velocity * dt;
                let vel_dir = p.velocity.normalize_or_zero();
                if p.position.y <= floor_y {
                    hits.push((Vec3::new(p.position.x, floor_y, p.position.z), None, p.damage, vel_dir));
                    continue;
                }
                let seg = p.position - prev;
                let seg_len = seg.length();
                if seg_len < 1e-5 { alive.push(p); continue; }
                let dir = seg / seg_len;

                if let Some((e, t)) =
                    crate::editor::picking::pick_ray_cached(&self.renderer, cache, prev, dir)
                {
                    if t <= seg_len {
                        hits.push((prev + dir * t, Some(e), p.damage, vel_dir));
                        continue;
                    }
                }
                alive.push(p);
            }
        }

        self.projectiles = alive;

        for (point, target, dmg, vel_dir) in hits {
            self.spawn_burst(point, &particles::sparks(Vec3::Y));
            if let Some(e) = target {
                if let Some(rb) = self.world.get_mut::<crate::physics::RigidBody>(e) {
                    if rb.body_type == BodyType::Dynamic { rb.apply_impulse(vel_dir * dmg * 0.05); }
                }
                let mut died = false;
                if let Some(h) = self.world.get_mut::<Health>(e) {
                    h.current -= dmg;
                    if h.current <= 0.0 { died = true; }
                }
                if died {
                    let pos = self.world.get::<Transform>(e).map(|t| t.position).unwrap_or(Vec3::ZERO);
                    self.spawn_burst(pos, &particles::explosion());
                    if let Some(audio) = &mut self.audio { audio.play("explosion"); }
                    self.game.on_kill(&mut self.world, e);
                    self.world.despawn(e);
                }
            }
        }
    }

    fn process_state_input(&mut self, elwt: &ActiveEventLoop) {
        self.input.map.clear_contexts();
        if self.editor.state.play.active {
            if self.editor.state.play.paused {
                self.input.map.push_context("pause_menu");
            } else {
                self.input.map.push_context("gameplay");
            }
        } else {
            self.input.map.push_context("editor");
            if self.editor.state.flying {
                self.input.map.push_context("fly");
            }
        }

        let egui_wants_keyboard = self.editor.egui_ctx.wants_keyboard_input();
        let esc_pressed = self.input.key_pressed(KeyCode::Escape);
        let esc_free = esc_pressed && !egui_wants_keyboard && !self.ui_state.command_palette_open;

        if esc_free {
            if self.editor.state.play.active {
                if self.editor.state.play.paused {
                    self.editor.state.play.paused = false;
                    self.time.paused = false;
                    log::info!("Resumed play");
                    self.editor.state.play.coyote_timer = self.editor.state.play.coyote_time;
                    self.editor.state.play.jump_buffer_timer = 0.0;
                    let _ = self.window.set_cursor_grab(CursorGrabMode::Locked);
                    self.window.set_cursor_visible(false);
                    self.input.on_cursor_enter();
                    self.input.mouse_motion = (0.0, 0.0);
                    self.input.skip_motion_frames = 4;
                } else {
                    self.editor.state.play.paused = true;
                    self.time.paused = true;
                    log::info!("Paused play");
                    let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                    self.window.set_cursor_visible(true);
                }
            } else if !self.editor.state.flying {
                let mut consumed = false;
                if self.editor.state.palette.active.is_some() {
                    self.editor.state.palette.active = None;
                    consumed = true;
                } else if self.editor.state.terrain.active {
                    self.editor.state.terrain.active = false;
                    consumed = true;
                } else if self.editor.state.context_menu_pos.is_some() {
                    self.editor.state.context_menu_pos = None;
                    consumed = true;
                }
                if !consumed { elwt.exit(); }
            }
        }

        if self.input.key_pressed(KeyCode::F9) {
            self.editor.state.pending_action = Some(EditorAction::TogglePlay);
        }
    }

    fn redraw(&mut self, elwt: &ActiveEventLoop) {
        self.time.tick();
        self.input.tick_begin_frame();
        self.game.camera_mut().update_shake(self.time.delta);

        // === TERRAIN: flush dirty region (один раз за кадр) ===
        self.flush_terrain_dirty();

        let reloaded = self.hot_reload.tick(&mut self.asset_db, &mut self.renderer);
        if !reloaded.is_empty() {
            self.pickable = None;
        }
        self.pickable = None;

        let dt = self.time.delta;

        self.world.update_events();

        let in_menu = self.mode.is_menu();

        if !in_menu {
            self.process_state_input(elwt);
        }

        let paused = self.editor.state.play.active && self.editor.state.play.paused;

        if paused != self.was_paused {
            self.was_paused = paused;
            self.game.on_pause_changed(paused);
        }

        let rmb = self.input.mouse_down(MouseButton::Right);
        let want_fly = rmb && self.rmb_dragged && !self.editor.state.play.active && !in_menu;
        let was_flying = self.editor.state.flying;
        if want_fly != was_flying {
            if want_fly {
                let cur_pos = self.game.camera().position();
                self.game.camera_mut().enter_fly(cur_pos);
                let _ = self.window.set_cursor_grab(CursorGrabMode::Locked);
                self.window.set_cursor_visible(false);
                self.input.on_cursor_enter();
                self.input.mouse_motion = (0.0, 0.0);
                self.input.skip_motion_frames = 4;
            } else {
                self.game.camera_mut().exit_fly();
                let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                self.window.set_cursor_visible(true);
            }
            self.editor.state.flying = want_fly;
            self.input.editor_flying = want_fly;
        }

        if !paused && !in_menu {
            let continue_running = self.game.update(&mut self.world, &self.input, &mut self.renderer, dt);
            if !continue_running { elwt.exit(); return; }
            if let Some((duration, scale)) = self.game.take_hit_stop() {
                self.time.add_hit_stop(duration, scale);
            }

            let shots: Vec<ShotFired> = self.world
                .read_events_current::<ShotFired>()
                .copied()
                .collect();
            for shot in shots {
                self.handle_shot_fired(shot);
            }

            if let Some(req) = self.game.load_world_request() {
                match crate::scene::load_scene_with_assets_from_str_full(
                    &req.scene_ron,
                    &mut self.renderer,
                ) {
                    Ok((mut new_world, _spawn, _embedded_gs)) => {
                        new_world.sync_next_id();
                        self.world = new_world;
                        self.navmesh = None;
                        self.pickable = None;
                        self.editor.state.selected.clear();
                        self.editor.state.undo.clear();
                        log::info!(
                            "World replaced from load_world_request: {} entities",
                            self.world.len()
                        );
                        if let Some(gs) = req.game_state_ron {
                            self.game.load_game_state(&gs);
                        }
                    }
                    Err(e) => log::error!("Failed to load world: {}", e),
                }
            }

            if self.game.wants_navmesh_bake() {
                self.bake_navmesh();
            }

            if let Some(audio) = self.audio.as_mut() {
                let listener_pos = self.game.camera().position();
                audio.update(&mut self.world, listener_pos, dt);
            }

            self.update_elevator_ding();
            self.physics.step(&mut self.world, self.renderer.terrain.as_ref(), dt);

            self.ai_system.update(&mut self.world, self.navmesh.as_ref(), dt);

            if self.editor.state.play.active {
                self.input.editor_captured = false;
                self.update_player(dt);
            } else if self.editor.state.flying {
                self.input.editor_captured = true;
                self.update_fly(dt);
            } else {
                let (mx, my) = self.input.mouse_pos;
                self.input.editor_captured = self.editor.state.gizmo.drag.is_some() || !self.in_viewport(mx, my);
            }

            self.update_particles(dt);
            self.update_projectiles(dt);
        } else if !in_menu {
            self.input.editor_captured = false;
            self.editor.state.flying = false;
            self.input.editor_flying = false;
            if let Some(audio) = self.audio.as_mut() {
                let listener_pos = self.game.camera().position();
                audio.update(&mut self.world, listener_pos, 0.0);
            }
        }

        // === TERRAIN: обновить cursor_world для UI ===
        if !in_menu {
            self.update_terrain_cursor();
        }

        if !in_menu && !self.editor.state.play.active && !self.editor.state.flying {
            if self.editor.state.palette.active.is_some() {
                let (mx, my) = self.input.mouse_pos;
                if self.in_viewport(mx, my) {
                    let (origin, dir) = self.game.camera().ray_from_screen(
                        mx, my, self.renderer.size.width as f32, self.renderer.size.height as f32,
                    );
                    let floor_y = self.editor.state.play.floor_y;
                    self.editor.state.palette.preview_pos = placement::ray_ground_plane(origin, dir, floor_y);
                } else { self.editor.state.palette.preview_pos = None; }
            } else { self.editor.state.palette.preview_pos = None; }
        } else { self.editor.state.palette.preview_pos = None; }

        if matches!(self.editor.state.pending_action, Some(EditorAction::TogglePlay)) {
            self.editor.state.pending_action = None;
            self.handle_editor_action(EditorAction::TogglePlay);
        }

        if !in_menu && !self.editor.state.play.active {
            let ctrl = self.input.key_down(KeyCode::ControlLeft) || self.input.key_down(KeyCode::ControlRight);
            let alt = self.input.key_down(KeyCode::AltLeft) || self.input.key_down(KeyCode::AltRight);
            let shift = self.input.key_down(KeyCode::ShiftLeft) || self.input.key_down(KeyCode::ShiftRight);

            if ctrl && !shift && !alt && self.input.key_pressed(KeyCode::KeyP) {
                self.ui_state.command_palette_open = true;
                self.ui_state.command_palette_query.clear();
                self.ui_state.command_palette_selected = 0;
            }
            if self.ui_state.command_palette_open && self.input.key_pressed(KeyCode::Escape) {
                self.ui_state.command_palette_open = false;
                self.ui_state.command_palette_query.clear();
            }
            if ctrl && !shift && !alt {
                for (i, key) in [
                    KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3,
                    KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6,
                    KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
                ].iter().enumerate() {
                    if self.input.key_pressed(*key) {
                        self.editor.state.pending_action = Some(EditorAction::SaveCameraBookmark(i));
                    }
                }
            }
            if alt && !ctrl && !shift {
                for (i, key) in [
                    KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3,
                    KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6,
                    KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
                ].iter().enumerate() {
                    if self.input.key_pressed(*key) {
                        self.editor.state.pending_action = Some(EditorAction::GotoCameraBookmark(i));
                    }
                }
            }
            if ctrl && self.input.key_pressed(KeyCode::KeyZ) { self.editor.state.pending_action = Some(EditorAction::Undo); }
            if ctrl && self.input.key_pressed(KeyCode::KeyY) { self.editor.state.pending_action = Some(EditorAction::Redo); }
            if ctrl && self.input.key_pressed(KeyCode::KeyC) { self.editor.state.pending_action = Some(EditorAction::CopyEntity); }
            if ctrl && self.input.key_pressed(KeyCode::KeyV) { self.editor.state.pending_action = Some(EditorAction::PasteEntity); }
            if ctrl && !shift && self.input.key_pressed(KeyCode::KeyA) { self.editor.state.pending_action = Some(EditorAction::SelectAll); }
            if ctrl && self.input.key_pressed(KeyCode::KeyI) { self.editor.state.pending_action = Some(EditorAction::InvertSelection); }
            if !self.editor.state.selected.is_empty() {
                if self.input.key_pressed(KeyCode::Digit1) && !ctrl && !alt { self.editor.state.gizmo.mode = GizmoMode::Translate; }
                if self.input.key_pressed(KeyCode::Digit2) && !ctrl && !alt { self.editor.state.gizmo.mode = GizmoMode::Rotate; }
                if self.input.key_pressed(KeyCode::Digit3) && !ctrl && !alt { self.editor.state.gizmo.mode = GizmoMode::Scale; }
                if self.input.key_pressed(KeyCode::KeyF) && !ctrl { self.editor.state.pending_action = Some(EditorAction::FocusSelected); }
                if ctrl && self.input.key_pressed(KeyCode::KeyD) { self.editor.state.pending_action = Some(EditorAction::Duplicate); }
            }
            if self.input.key_pressed(KeyCode::Delete) {
                self.editor.state.pending_action = Some(EditorAction::DeleteSelected);
            }
        }

        let mut postfx = self.game.postfx();
        let mesh_names = self.renderer.mesh_names();
        let material_names = self.renderer.material_names();
        let selected_material: Option<(String, crate::render::Material)> =
            self.editor.state.primary().and_then(|e| {
                let mh = self.world.get::<MaterialHandle>(e)?;
                let mat = self.renderer.materials.get(&mh.0)?.clone();
                Some((mh.0.clone(), mat))
            });

        let mut texture_list: Vec<(String, u32, u32, bool)> = self.renderer.textures.iter()
            .map(|(n, t)| (n.clone(), t.size.0, t.size.1, t.is_srgb)).collect();
        texture_list.sort_by(|a, b| a.0.cmp(&b.0));

        let assets = UiAssets {
            mesh_names: &mesh_names, material_names: &material_names,
            texture_list: &texture_list, selected_material,
        };

        let selected_source_entity = self.editor.state.selected.iter().copied()
            .find(|&e| self.world.has::<AudioSource>(e));

        let audio_snapshot = if let Some(audio) = self.audio.as_ref() {
            AudioSnapshot {
                bus_volumes: audio.all_bus_volumes(),
                sound_names: audio.sound_names().into_iter().map(String::from).collect(),
                active_count: audio.active_count(),
                available: true,
                selected_source_entity,
            }
        } else {
            AudioSnapshot {
                available: false,
                selected_source_entity,
                ..AudioSnapshot::default()
            }
        };

        let pre_ui_undo_snapshot: Option<String> = if self.editor.state.undo.can_push_now() {
            crate::scene::save_scene_with_assets_to_string(&self.world, &self.renderer, None).ok()
        } else { None };

        let raw_input = self.editor.egui_state.take_egui_input(&*self.window);
        let lod_counts = self.game.lod_stats();

        let mut sel_triangles = 0usize;
        let mut sel_vertices = 0usize;
        for &e in &self.editor.state.selected {
            if let Some(mh) = self.world.get::<MeshHandle>(e) {
                if let Some(mesh) = self.renderer.meshes.get(&mh.0) {
                    sel_triangles += mesh.cpu_indices.len() / 3;
                    sel_vertices += mesh.cpu_vertices.len();
                }
            }
        }

        let extra_lines = if self.editor.state.play.active { self.game.rpg_hud() } else { Vec::new() };
        let dir_lights_pre = self.game.dir_lights(&self.world);
        let point_lights_pre = self.game.point_lights(&self.world);

        let stats = Stats {
            fps: self.time.fps(),
            frame_time_max_ms: self.time.frame_time_max_ms(),
            hitches: self.time.hitches,
            entities: self.world.len(),
            draws: self.renderer.last_draw_count,
            instances: self.renderer.last_instance_count,
            dir_lights: dir_lights_pre.len(),
            point_lights: point_lights_pre.len(),
            lod_counts, lod_triangles: [0; 4],
            sel_entities: self.editor.state.selected.len(),
            sel_triangles, sel_vertices, extra_lines,
        };

        let egui_ctx = self.editor.egui_ctx.clone();
        let ui_state = &mut self.ui_state;
        let editor_state = &mut self.editor.state;
        let world = &mut self.world;
        let asset_db = &mut self.asset_db;

        // Меню рендерится ПОВЕРХ редактора, если мы в главном меню.
        let has_save = std::path::Path::new("saves/slot_0.ron").exists();
        let in_menu_for_ui = self.mode.is_menu();

        let mode_ref = &mut self.mode;
        let settings_ref = &mut self.settings;

        let mut menu_actions = game_menu::MenuActions::default();

        let full_output = egui_ctx.run(raw_input, |ctx| {
            if in_menu_for_ui {
                menu_actions = game_menu::draw(
                    ctx, mode_ref, settings_ref, has_save,
                );
            } else {
                let action = editor_ui::draw(
                    ctx, ui_state, editor_state, world,
                    &mut postfx, &stats, &assets, &audio_snapshot,
                    asset_db,
                );
                if let Some(a) = action { editor_state.pending_action = Some(a); }
            }
        });

        // Применить действия меню.
        if in_menu_for_ui {
            if menu_actions.new_game {
                self.mode = AppMode::InGame;
                log::info!("Starting new game");
            }
            if menu_actions.continue_game {
                self.editor.state.pending_action = Some(EditorAction::Load);
                self.mode = AppMode::InGame;
                log::info!("Continuing game");
            }
            if menu_actions.open_settings {
                self.mode = AppMode::Settings { return_to: Box::new(AppMode::MainMenu) };
            }
            if menu_actions.back {
                if let AppMode::Settings { return_to } = self.mode.clone() {
                    self.mode = *return_to;
                }
            }
            if menu_actions.apply_settings {
                if let Err(e) = self.settings.save("settings.ron") {
                    log::warn!("Failed to save settings.ron: {}", e);
                } else {
                    log::info!("Settings saved to settings.ron");
                }
                self.apply_settings_to_window(self.window.clone());
                self.apply_settings_to_audio();
            }
            if menu_actions.quit {
                elwt.exit();
                return;
            }
        }

        // === Проверить запрос на выход в главное меню от игры ===
        if self.game.take_quit_to_menu() {
            self.mode = AppMode::MainMenu;
            log::info!("Returning to main menu");
        }

        {
            let avail = egui_ctx.available_rect();
            let ppp = egui_ctx.pixels_per_point();
            self.viewport_rect = Some(egui::Rect::from_min_max(
                egui::pos2(avail.min.x * ppp, avail.min.y * ppp),
                egui::pos2(avail.max.x * ppp, avail.max.y * ppp),
            ));
        }

        self.game.apply_postfx(postfx);
        self.editor.egui_state.handle_platform_output(&*self.window, full_output.platform_output);

        let clipped_primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, image_delta) in &full_output.textures_delta.set {
            self.editor.egui_renderer.update_texture(&self.renderer.device, &self.renderer.queue, *id, image_delta);
        }

        if self.editor.state.undo_requested {
            self.editor.state.undo_requested = false;
            let gs = self.game.save_game_state();
            match pre_ui_undo_snapshot {
                Some(snap) => { self.editor.state.undo.push_snapshot(snap, gs); }
                None => {
                    if self.editor.state.undo.can_push_now() {
                        self.editor.state.undo.push(&self.world, &self.renderer, gs);
                    }
                }
            }
        }

        for (entity, name, mat) in self.editor.state.dirty_materials.drain(..) {
            let user_count = self.world.entities().iter().filter(|&&ent| {
                self.world.get::<MaterialHandle>(ent).map(|mh| mh.0 == name).unwrap_or(false)
            }).count();
            let final_name = if user_count > 1 {
                let unique_name = format!("{}_uniq_{}", name, entity);
                if !self.renderer.has_material(&unique_name) {
                    self.renderer.add_material(&unique_name, mat.clone());
                }
                self.world.insert(entity, MaterialHandle(unique_name.clone()));
                unique_name
            } else { name };
            self.renderer.update_material(&final_name, mat);
        }

        self.editor.state.prune_selection(&self.world);
        if let Some(action) = self.editor.state.pending_action.take() {
            self.handle_editor_action(action);
        }

        let selected = self.editor.state.selected.clone();
        let draws = self.game.collect_draws(&mut self.world, &self.renderer);

        let mut lines = self.game.collect_lines(&mut self.world, &self.renderer, &selected);

        if self.editor.state.settings.show_navmesh {
            if let Some(nm) = &self.navmesh {
                let mut batch = LineBatch::new();
                for (a, b) in nm.walkable_edges() {
                    batch.line(a, b, [0.25, 1.0, 0.4, 0.75]);
                }
                lines.extend_from_slice(batch.vertices());
            }
        }

        if !in_menu && !self.editor.state.play.active && !selected.is_empty() {
            let mut batch = LineBatch::new();
            gizmo::draw_gizmo(&mut batch, &self.world, &selected, self.game.camera(), &self.editor.state.gizmo);
            lines.extend_from_slice(batch.vertices());
        }

        if !in_menu && !self.editor.state.play.active {
            if self.editor.state.terrain.active {
                if let Some(p) = self.editor.state.terrain.cursor_world {
                    let r = self.editor.state.terrain.radius;
                    let mut batch = LineBatch::new();
                    batch.sphere_wireframe(p, r, [1.0, 0.7, 0.2, 0.85], 32);
                    batch.line(p, p + Vec3::Y * 5.0, [1.0, 0.7, 0.2, 0.9]);
                    lines.extend_from_slice(batch.vertices());
                }
            }

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

        let mut particle_instances: Vec<ParticleInstance> = self.particles.iter()
            .map(|p| ParticleInstance {
                position_size: [p.position.x, p.position.y, p.position.z, p.size()],
                color: p.color(),
            }).collect();
        for proj in &self.projectiles {
            particle_instances.push(ParticleInstance {
                position_size: [proj.position.x, proj.position.y, proj.position.z, 0.12],
                color: [3.0, 2.6, 0.6, 1.0],
            });
        }

        let mut decal_draws: Vec<DecalDraw> = Vec::new();
        for &e in self.world.entities() {
            let Some(dec) = self.world.get::<Decal>(e) else { continue; };
            self.renderer.ensure_decal_bg(&dec.texture);
            let model = crate::game::world_matrix(&self.world, e);
            let inst = DecalInstance::new(model, dec.tint);
            if let Some(group) = decal_draws.iter_mut().find(|g| g.texture == dec.texture) {
                group.instances.push(inst);
            } else {
                decal_draws.push(DecalDraw { texture: dec.texture.clone(), instances: vec![inst] });
            }
        }

        let dir_lights = self.game.dir_lights(&self.world);
        let point_lights = self.game.point_lights(&self.world);
        let ambient = self.game.ambient();
        let postfx = self.game.postfx();

        let mouse_clicked = self.input.mouse_pressed(winit::event::MouseButton::Left);
        let mouse_down = self.input.mouse_down(winit::event::MouseButton::Left);
        let mut ui_layer = crate::ui::UiLayer::new(
            self.renderer.size.width as f32,
            self.renderer.size.height as f32,
            crate::ui::UiInput {
                mouse_pos: self.input.mouse_pos,
                mouse_clicked,
                mouse_down,
            },
        );
        if !in_menu {
            let game = &mut self.game;
            let renderer = &self.renderer;
            let world = &mut self.world;
            game.collect_ui(world, renderer, &mut ui_layer);
        }
        let ui_quads = ui_layer.into_quads();

        let pixels_per_point = full_output.pixels_per_point;
        let egui_data = EguiFrameData {
            renderer: &mut self.editor.egui_renderer,
            clipped_primitives,
            pixels_per_point,
        };

        let res = self.renderer.render(
            self.game.camera(),
            &draws,
            &decal_draws,
            &particle_instances,
            &lines,
            &dir_lights,
            &point_lights,
            ambient,
            postfx,
            self.time.elapsed,
            &ui_quads,
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
            let mode_str = if in_menu {
                "MENU"
            } else if self.editor.state.play.active {
                if self.editor.state.play.paused { "PAUSED" } else { "PLAY" }
            }
            else if self.editor.state.flying { "FLY" }
            else if self.editor.state.terrain.active { "TERRAIN" }
            else { "EDIT" };
            self.window.set_title(&format!(
                "Rust Engine 3D [{}] | FPS {:>5.1} | Entities {} | Sel {} | Particles {}",
                mode_str, self.time.fps(), self.world.len(),
                self.editor.state.selected.len(), self.particles.len(),
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
            &self.world, &self.renderer, self.game.camera(), screen_pos.0, screen_pos.1,
        );
        match picked {
            Some(e) => self.editor.state.select_single(e),
            None => self.editor.state.selected.clear(),
        }
    }

    fn duplicate_selected(&mut self) {
        use crate::game::components::{
            AnimationPlayer, Chase, Health, Interactable, MaterialHandle, MeshHandle, Name,
            Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint, Transform,
            Trigger, TriggerAction, Velocity, Visible,
        };
        use crate::game::lights::{DirectionalLight, PointLight};
        use crate::physics::{Collider, PhysicsMaterial, RigidBody};

        let originals: Vec<Entity> = self.editor.state.selected.iter().copied()
            .filter(|&e| self.world.entities().contains(&e)).collect();
        if originals.is_empty() { return; }

        let originals_set: std::collections::HashSet<Entity> =
            originals.iter().copied().collect();

        let mut old_to_new: HashMap<Entity, Entity> = HashMap::new();
        for &e in &originals {
            let new_e = self.world.spawn();
            old_to_new.insert(e, new_e);
        }

        for &e in &originals {
            let new_e = old_to_new[&e];

            if let Some(t) = self.world.get::<Transform>(e).copied() {
                let mut nt = t;
                let parent_in_selection = self.world
                    .get::<Parent>(e)
                    .map(|p| originals_set.contains(&p.0))
                    .unwrap_or(false);
                if !parent_in_selection {
                    nt.position += Vec3::new(1.0, 0.0, 0.0);
                }
                self.world.insert(new_e, nt);
            }
            if let Some(n) = self.world.get::<Name>(e).cloned() {
                self.world.insert(new_e, Name(format!("{}_copy", n.0)));
            }
            if let Some(m) = self.world.get::<MeshHandle>(e).cloned() { self.world.insert(new_e, m); }
            if let Some(m) = self.world.get::<MaterialHandle>(e).cloned() { self.world.insert(new_e, m); }
            if let Some(s) = self.world.get::<SkeletonHandle>(e).cloned() { self.world.insert(new_e, s); }
            if let Some(a) = self.world.get::<AnimationPlayer>(e).cloned() { self.world.insert(new_e, a); }
            if let Some(s) = self.world.get::<Spinner>(e).copied() { self.world.insert(new_e, s); }
            if let Some(v) = self.world.get::<Velocity>(e).copied() { self.world.insert(new_e, v); }
            if let Some(h) = self.world.get::<Health>(e).copied() { self.world.insert(new_e, h); }
            if let Some(c) = self.world.get::<Chase>(e).copied() { self.world.insert(new_e, c); }
            if let Some(&i) = self.world.get::<Interactable>(e) { self.world.insert(new_e, i); }
            if let Some(t) = self.world.get::<TextureTiling>(e).copied() { self.world.insert(new_e, t); }
            if let Some(t) = self.world.get::<Tint>(e).copied() { self.world.insert(new_e, t); }
            if let Some(v) = self.world.get::<Visible>(e).copied() { self.world.insert(new_e, v); }
            if let Some(el) = self.world.get::<Elevator>(e).cloned() { self.world.insert(new_e, el); }
            if let Some(sd) = self.world.get::<SlidingDoor>(e).copied() { self.world.insert(new_e, sd); }
            if let Some(rb) = self.world.get::<RigidBody>(e).copied() { self.world.insert(new_e, rb); }
            if let Some(col) = self.world.get::<Collider>(e).copied() { self.world.insert(new_e, col); }
            if let Some(mat) = self.world.get::<PhysicsMaterial>(e).copied() { self.world.insert(new_e, mat); }
            if let Some(dec) = self.world.get::<Decal>(e).cloned() { self.world.insert(new_e, dec); }
            if let Some(src) = self.world.get::<AudioSource>(e).cloned() {
                let mut s = src;
                s.playing = true;
                self.world.insert(new_e, s);
            }

            if let Some(t) = self.world.get::<crate::game::timers::Timer>(e).cloned() {
                self.world.insert(new_e, t);
            }
            if let Some(agent) = self.world.get::<crate::game::ai::AiAgent>(e).cloned() {
                let mut a = agent;
                a.path.clear();
                a.path_index = 0;
                a.repath_timer = 0.0;
                a.attack_timer = 0.0;
                a.last_seen_pos = None;
                a.time_since_seen = 999.0;
                a.state = crate::game::ai::AiState::Idle;
                a.state_timer = 0.0;
                self.world.insert(new_e, a);
            }
            if let Some(patrol) = self.world.get::<crate::game::ai::PatrolPath>(e).cloned() {
                self.world.insert(new_e, patrol);
            }
            if self.world.has::<crate::game::ai::AiTarget>(e) {
                self.world.insert(new_e, crate::game::ai::AiTarget);
            }
            if self.world.has::<crate::game::ai::Enemy>(e) {
                self.world.insert(new_e, crate::game::ai::Enemy);
            }
            if self.world.has::<crate::game::ai::DebugPath>(e) {
                self.world.insert(new_e, crate::game::ai::DebugPath);
            }

            if let Some(mut t) = self.world.get::<Trigger>(e).cloned() {
                if let TriggerAction::CallElevator { elevator, floor_idx } = t.action {
                    let new_el = old_to_new.get(&elevator).copied().unwrap_or(elevator);
                    t.action = TriggerAction::CallElevator { elevator: new_el, floor_idx };
                }
                self.world.insert(new_e, t);
            }
            if let Some(l) = self.world.get::<DirectionalLight>(e).copied() { self.world.insert(new_e, l); }
            if let Some(l) = self.world.get::<PointLight>(e).copied() { self.world.insert(new_e, l); }

            if let Some(&Parent(p)) = self.world.get::<Parent>(e) {
                let new_parent = old_to_new.get(&p).copied().unwrap_or(p);
                self.world.insert(new_e, Parent(new_parent));
            }
        }

        let new_selected: Vec<Entity> = originals.iter().map(|e| old_to_new[e]).collect();
        for &ne in &new_selected { self.ensure_unique_material_for(ne); }

        self.editor.state.selected = new_selected;
        log::info!("Duplicated {} entities", originals.len());
    }

    fn spawn_palette_item(&mut self, item: PaletteItem, hit: Vec3) {
        use crate::game::components::*;
        use crate::game::lights::{DirectionalLight, PointLight};

        let gs = self.game.save_game_state();
        self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);

        let mut pos = hit;
        if item.snaps_to_ground() { pos.y += item.half_height(); }
        else if item.is_light() || item.is_decal() { pos.y = hit.y + item.half_height(); }

        let e = self.world.spawn();
        self.world.insert(e, Name(format!("{}_{:04}", item.label(), e)));
        self.world.insert(e, Transform::at(pos));

        if item.is_light() {
            match item {
                PaletteItem::Sun => { self.world.insert(e, DirectionalLight::sun()); }
                PaletteItem::PointLight => { self.world.insert(e, PointLight::default()); }
                _ => unreachable!(),
            }
            self.editor.state.select_single(e);
            return;
        }

        if item.is_decal() {
            self.world.insert(e, Decal::default());
            if let Some(t) = self.world.get_mut::<Transform>(e) { t.scale = Vec3::splat(1.0); }
            self.editor.state.select_single(e);
            return;
        }

        self.world.insert(e, Transform::at(pos).with_scale(item.default_scale()));
        self.world.insert(e, MeshHandle(item.mesh().to_string()));
        self.world.insert(e, MaterialHandle(item.material().to_string()));
        self.world.insert(e, TextureTiling::new(item.default_tiling_size()));

        match item {
            PaletteItem::Enemy => {
                self.world.insert(e, Health::new(50.0));
                self.world.insert(e, crate::game::ai::Enemy);
                let agent = crate::game::ai::AiAgent::new()
                    .with_speed(3.5)
                    .with_vision(15.0, 60_f32.to_radians())
                    .with_hearing(20.0)
                    .with_attack(1.5, 8.0, 0.9);
                self.world.insert(e, agent);
            }
            PaletteItem::Pickup => { self.world.insert(e, Interactable::Pickup); }
            PaletteItem::Switch => {
                self.world.insert(e, Interactable::Toggle);
                self.world.insert(e, Spinner::new(Vec3::Y, 1.5));
            }
            PaletteItem::TriggerCube => {
                self.world.insert(e, Trigger::new(2.5, TriggerAction::Teleport([0.0, 2.0, 0.0])));
            }
            _ => {}
        }

        self.ensure_unique_material_for(e);
        self.editor.state.select_single(e);
    }

    fn handle_editor_action(&mut self, action: EditorAction) {
        use crate::game::components::{MaterialHandle, MeshHandle, Transform};

        match action {
            EditorAction::TogglePlay => {
                let was_active = self.editor.state.play.active;
                if !was_active {
                    self.play_snapshot = Some(self.capture_play_snapshot());
                    let play = &mut self.editor.state.play;
                    play.active = true;
                    play.paused = false;
                    self.time.paused = false;
                    self.input.play_mode = true;
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
                    play.horizontal_velocity = Vec3::ZERO;
                    play.coyote_timer = play.coyote_time;
                    play.jump_buffer_timer = 0.0;
                    play.ground_normal = Vec3::Y;

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
                    self.footstep_timer = 0.0;
                } else {
                    self.game.camera_mut().exit_fps();
                    let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                    self.window.set_cursor_visible(true);
                    if let Some(snap) = self.play_snapshot.take() {
                        self.restore_play_snapshot(snap);
                    }
                    let play = &mut self.editor.state.play;
                    play.active = false;
                    play.paused = false;
                    self.time.paused = false;
                    self.input.play_mode = false;
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
                    play.horizontal_velocity = Vec3::ZERO;
                    play.coyote_timer = 0.0;
                    play.jump_buffer_timer = 0.0;
                    self.particles.clear();
                    self.projectiles.clear();
                    self.footstep_timer = 0.0;
                }
            }
            EditorAction::SpawnPlayerHere => {
                let Some(e) = self.editor.state.primary() else { return; };
                let Some(t) = self.world.get::<Transform>(e) else { return; };
                let mut pos = t.position;
                pos.y += self.editor.state.play.eye_height;
                self.editor.state.play.saved_position = pos;
            }
            EditorAction::CopyEntity => {
                use crate::scene::serialize::snapshot_entity;
                let snaps: Vec<_> = self.editor.state.selected.iter()
                    .filter_map(|&e| snapshot_entity(&self.world, e)).collect();
                self.editor.state.clipboard_entities = snaps;
            }
            EditorAction::PasteEntity => {
                use std::collections::HashMap;
                let snaps = self.editor.state.clipboard_entities.clone();
                if snaps.is_empty() { return; }
                let gs = self.game.save_game_state();
                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                let offset = Vec3::new(1.0, 0.0, 1.0);
                let mut old_to_new: HashMap<u32, Entity> = HashMap::new();
                let mut new_selected: Vec<Entity> = Vec::with_capacity(snaps.len());

                let snap_ids: std::collections::HashSet<u32> = snaps.iter()
                    .enumerate()
                    .map(|(idx, s)| s.entity_id.unwrap_or(idx as u32))
                    .collect();

                for (idx, mut snap) in snaps.iter().cloned().enumerate() {
                    let old_id = snap.entity_id.unwrap_or(idx as u32);
                    let parent_in_buffer = snap.parent
                        .map(|p| snap_ids.contains(&p))
                        .unwrap_or(false);

                    if !parent_in_buffer {
                        let mut parent_world = glam::Mat4::IDENTITY;
                        if let Some(old_parent) = snap.parent {
                            let pe = old_parent as Entity;
                            if self.world.entities().contains(&pe) {
                                parent_world = crate::game::world_matrix(&self.world, pe);
                            }
                        }

                        if let Some(ref mut t) = snap.transform {
                            let local = glam::Mat4::from_scale_rotation_translation(
                                Vec3::from_array(t.scale),
                                glam::Quat::from_array(t.rotation),
                                Vec3::from_array(t.position),
                            );
                            let world = parent_world * local;
                            let (s, r, p) = world.to_scale_rotation_translation();
                            t.position = [p.x + offset.x, p.y + offset.y, p.z + offset.z];
                            t.rotation = r.to_array();
                            t.scale = s.to_array();
                        }

                        snap.parent = None;
                    }

                    snap.entity_id = None;
                    if let Some(ref mut n) = snap.name { n.push_str("_paste"); }
                    let e = crate::scene::serialize::spawn_snapshot(&mut self.world, snap);
                    old_to_new.insert(old_id, e);
                    new_selected.push(e);
                }
                for (idx, &new_e) in new_selected.iter().enumerate() {
                    let old_parent = snaps[idx].parent;
                    if let Some(op) = old_parent {
                        if let Some(&new_parent) = old_to_new.get(&op) {
                            self.world.insert(new_e, Parent(new_parent));
                        }
                    }
                }
                for &ne in &new_selected { self.ensure_unique_material_for(ne); }
                self.editor.state.selected = new_selected;
            }
            EditorAction::MakeMaterialUnique => {
                let Some(e) = self.editor.state.primary() else { return; };
                let Some(mh) = self.world.get::<MaterialHandle>(e).cloned() else { return; };
                let Some(mat) = self.renderer.materials.get(&mh.0).cloned() else { return; };
                let new_name = format!("{}_uniq_{}", mh.0, e);
                self.renderer.add_material(&new_name, mat);
                self.world.insert(e, MaterialHandle(new_name.clone()));
            }
            EditorAction::Undo => {
                let gs = self.game.save_game_state();
                if let Some((new_world, game_state_ron)) =
                    self.editor.state.undo.undo(&self.world, &mut self.renderer, gs)
                {
                    self.world = new_world;
                    self.editor.state.prune_selection(&self.world);
                    if let Some(ron) = game_state_ron {
                        self.game.load_game_state(&ron);
                    }
                    self.navmesh = None;
                }
            }
            EditorAction::Redo => {
                let gs = self.game.save_game_state();
                if let Some((new_world, game_state_ron)) =
                    self.editor.state.undo.redo(&self.world, &mut self.renderer, gs)
                {
                    self.world = new_world;
                    self.editor.state.prune_selection(&self.world);
                    if let Some(ron) = game_state_ron {
                        self.game.load_game_state(&ron);
                    }
                    self.navmesh = None;
                }
            }
            EditorAction::Save => {
                let path = self.editor.state.save_path.clone();
                let spawn = Some(self.editor.state.play.saved_position);
                let game_state = self.game.save_game_state();
                if let Err(e) = crate::scene::save_scene_with_game_state_to_file(
                    &self.world, &self.renderer, &path, spawn, game_state,
                ) {
                    log::error!("Save failed: {}", e);
                }
            }
            EditorAction::Load => {
                let path = self.editor.state.save_path.clone();
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => { log::error!("Load failed: {}", e); return; }
                };
                match crate::scene::load_scene_with_assets_from_str_full(&text, &mut self.renderer) {
                    Ok((mut new_world, spawn, game_state_ron)) => {
                        new_world.sync_next_id();
                        self.world = new_world;
                        self.editor.state.selected.clear();
                        self.editor.state.undo.clear();
                        self.navmesh = None;
                        if let Some(p) = spawn { self.editor.state.play.saved_position = p; }
                        if let Some(ron) = game_state_ron {
                            self.game.load_game_state(&ron);
                        }
                        self.editor.state.settings.push_recent_scene(&path);
                    }
                    Err(e) => log::error!("Load failed: {}", e),
                }
            }
            EditorAction::LoadPath(path) => {
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => { log::error!("Load failed: {}", e); return; }
                };
                match crate::scene::load_scene_with_assets_from_str_full(&text, &mut self.renderer) {
                    Ok((mut new_world, spawn, game_state_ron)) => {
                        new_world.sync_next_id();
                        self.world = new_world;
                        self.editor.state.selected.clear();
                        self.editor.state.undo.clear();
                        self.navmesh = None;
                        if let Some(p) = spawn { self.editor.state.play.saved_position = p; }
                        if let Some(ron) = game_state_ron {
                            self.game.load_game_state(&ron);
                        }
                        self.editor.state.save_path = path.clone();
                        self.editor.state.settings.push_recent_scene(&path);
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
                self.navmesh = None;
            }
            EditorAction::AddCube | EditorAction::AddSphere => {
                let gs = self.game.save_game_state();
                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                let (mesh, mat, base_name) = match action {
                    EditorAction::AddCube => ("cube", "flat_blue", "Cube"),
                    EditorAction::AddSphere => ("sphere", "gold", "Sphere"),
                    _ => unreachable!(),
                };
                let cam_target = self.game.camera().target;
                let pos = cam_target + Vec3::new(0.0, 1.0, 0.0);
                let e = self.world.spawn();
                self.world.insert(e, crate::game::components::Name(format!("{}_{:04}", base_name, e)));
                self.world.insert(e, Transform::at(pos));
                self.world.insert(e, MeshHandle(mesh.to_string()));
                self.world.insert(e, MaterialHandle(mat.to_string()));
                self.world.insert(e, TextureTiling::default());
                self.ensure_unique_material_for(e);
                self.editor.state.select_single(e);
            }
            EditorAction::DeleteSelected => {
                use std::collections::HashSet;
                if self.editor.state.selected.is_empty() { return; }
                let gs = self.game.save_game_state();
                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                let mut to_delete: HashSet<Entity> = self.editor.state.selected.iter().copied().collect();
                let mut stack: Vec<Entity> = self.editor.state.selected.clone();
                while let Some(parent) = stack.pop() {
                    for &c in self.world.entities() {
                        if let Some(Parent(p)) = self.world.get::<Parent>(c).copied() {
                            if p == parent && to_delete.insert(c) { stack.push(c); }
                        }
                    }
                }
                let victims: Vec<Entity> = to_delete.into_iter().collect();
                for e in victims { self.world.despawn(e); }
                self.editor.state.selected.clear();
            }
            EditorAction::Duplicate => {
                let gs = self.game.save_game_state();
                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                self.duplicate_selected();
            }
            EditorAction::FocusSelected => {
                if self.editor.state.selected.is_empty() { return; }
                let Some(center) = gizmo::group_center(&self.world, &self.editor.state.selected) else { return; };
                let radius = gizmo::group_radius(&self.world, &self.editor.state.selected, &self.renderer);
                self.game.camera_mut().focus_on(center, radius);
            }
            EditorAction::SavePrefab => {
                let name = self.editor.state.prefab_save_name.trim().to_string();
                if name.is_empty() || self.editor.state.selected.is_empty() { return; }
                let prefab = crate::scene::prefab::prefab_from_selection(
                    &self.world, &self.editor.state.selected, Some(name.clone()));
                let dir = self.editor.state.prefabs_dir.clone();
                let path = std::path::Path::new(&dir).join(format!("{}.prefab.ron", name));
                match crate::scene::prefab::save_prefab_to_file(&prefab, &path) {
                    Ok(()) => {
                        self.editor.state.prefab_list = crate::scene::prefab::list_prefabs(&dir);
                    }
                    Err(e) => log::error!("Prefab save failed: {}", e),
                }
            }
            EditorAction::RefreshPrefabs => {
                let dir = self.editor.state.prefabs_dir.clone();
                self.editor.state.prefab_list = crate::scene::prefab::list_prefabs(&dir);
            }
            EditorAction::InstantiatePrefab(idx) => {
                let Some(path) = self.editor.state.prefab_list.get(idx as usize).cloned() else { return; };
                let prefab = match crate::scene::prefab::load_prefab_from_file(&path) {
                    Ok(p) => p,
                    Err(e) => { log::error!("Failed to load prefab: {}", e); return; }
                };
                let gs = self.game.save_game_state();
                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                let spawn_pos = self.game.camera().target + Vec3::new(0.0, 1.0, 0.0);
                let new_entities = crate::scene::prefab::instantiate_prefab(&mut self.world, &prefab, spawn_pos);
                for &ne in &new_entities { self.ensure_unique_material_for(ne); }
                self.editor.state.selected = new_entities;
            }
            EditorAction::LoadTextures => self.load_textures_common(false),
            EditorAction::LoadTexturesLinear => self.load_textures_common(true),
            EditorAction::RemoveTexture(name) => {
                if self.renderer.remove_texture(&name) {
                    log::info!("Removed texture '{}'", name);
                }
            }
            EditorAction::ExportFbxAll => {
                let default_name = self.editor.state.fbx_export_path.clone();
                let path = rfd::FileDialog::new()
                    .set_file_name(&default_name)
                    .add_filter("FBX", &["fbx"])
                    .save_file();
                let Some(path) = path else { return; };
                if let Some(p) = path.to_str() { self.editor.state.fbx_export_path = p.to_string(); }
                let opts = crate::scene::fbx_export::FbxExportOptions { selected: None };
                match crate::scene::fbx_export::export_fbx(&self.world, &self.renderer, &path, &opts) {
                    Ok(stats) => log::info!(
                        "FBX exported: {} entities, {} verts, {} tris → {}",
                        stats.entities, stats.total_vertices, stats.total_triangles, path.display()
                    ),
                    Err(e) => log::error!("FBX export failed: {:#}", e),
                }
            }
            EditorAction::ExportFbxSelected => {
                if self.editor.state.selected.is_empty() { return; }
                let default_name = self.editor.state.fbx_export_path.clone();
                let path = rfd::FileDialog::new()
                    .set_file_name(&default_name)
                    .add_filter("FBX", &["fbx"])
                    .save_file();
                let Some(path) = path else { return; };
                if let Some(p) = path.to_str() { self.editor.state.fbx_export_path = p.to_string(); }
                let opts = crate::scene::fbx_export::FbxExportOptions {
                    selected: Some(self.editor.state.selected.clone()),
                };
                match crate::scene::fbx_export::export_fbx(&self.world, &self.renderer, &path, &opts) {
                    Ok(stats) => log::info!(
                        "FBX exported: {} entities, {} verts, {} tris → {}",
                        stats.entities, stats.total_vertices, stats.total_triangles, path.display()
                    ),
                    Err(e) => log::error!("FBX export failed: {:#}", e),
                }
            }
            EditorAction::ImportFbx => {
                let path = rfd::FileDialog::new().add_filter("FBX", &["fbx"]).pick_file();
                let Some(path) = path else { return; };
                let opts = crate::scene::fbx_import::FbxImportOptions { scale: 1.0, prefix: String::new() };
                match crate::scene::fbx_import::import_fbx(&mut self.world, &mut self.renderer, &path, &opts) {
                    Ok(stats) => {
                        log::info!("FBX imported: {} verts, {} tris", stats.total_vertices, stats.total_triangles);
                        self.editor.state.selected.clear();
                        self.navmesh = None;
                    }
                    Err(e) => log::error!("FBX import failed: {:#}", e),
                }
            }
            EditorAction::SaveCameraBookmark(slot) => {
                use crate::editor::camera_bookmarks::CameraBookmark;
                let cam = self.game.camera();
                let bm = CameraBookmark {
                    target: cam.target.to_array(), distance: cam.distance,
                    yaw: cam.yaw, pitch: cam.pitch,
                };
                self.editor.state.settings.camera_bookmarks.save(slot, bm);
            }
            EditorAction::GotoCameraBookmark(slot) => {
                if let Some(bm) = self.editor.state.settings.camera_bookmarks.get(slot) {
                    let cam = self.game.camera_mut();
                    cam.target = Vec3::from_array(bm.target);
                    cam.distance = bm.distance;
                    cam.yaw = bm.yaw;
                    cam.pitch = bm.pitch;
                }
            }
            EditorAction::CameraPreset(preset) => { self.apply_camera_preset(preset); }
            EditorAction::DeselectAll => { self.editor.state.selected.clear(); }
            EditorAction::SelectAll => { self.editor.state.selected = self.world.entities().to_vec(); }
            EditorAction::InvertSelection => {
                let current: std::collections::HashSet<Entity> = self.editor.state.selected.iter().copied().collect();
                let inverted: Vec<Entity> = self.world.entities().iter().copied()
                    .filter(|e| !current.contains(e)).collect();
                self.editor.state.selected = inverted;
            }
            EditorAction::CleanupEmptyEntities => {
                let gs = self.game.save_game_state();
                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                let victims: Vec<Entity> = self.world.entities().iter().copied()
                    .filter(|&e| {
                        !self.world.has::<Transform>(e)
                            && !self.world.has::<MeshHandle>(e)
                            && !self.world.has::<crate::physics::RigidBody>(e)
                    }).collect();
                for e in victims { self.world.despawn(e); }
            }
            EditorAction::PlacePalette | EditorAction::ClearPalette => {}

            EditorAction::SetBusVolume(bus, vol) => {
                if let Some(audio) = self.audio.as_mut() {
                    audio.set_bus_volume(bus, vol);
                }
            }
            EditorAction::LoadSound => {
                let path = rfd::FileDialog::new()
                    .add_filter("Audio", &["wav", "ogg", "flac", "mp3"])
                    .pick_file();
                let Some(path) = path else { return; };
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "sound".to_string());
                if let Some(audio) = self.audio.as_mut() {
                    match audio.load_sound_from_file(&name, &path) {
                        Ok(()) => log::info!("Sound '{}' loaded", name),
                        Err(e) => log::error!("Failed to load sound: {:#}", e),
                    }
                }
            }
            EditorAction::PreviewSound(name) => {
                if let Some(audio) = self.audio.as_mut() {
                    audio.play(&name);
                }
            }

            EditorAction::BakeNavmesh => {
                self.bake_navmesh();
            }

            // === TERRAIN ===
            EditorAction::TerrainRegenerate => {
                if let Some(hm) = self.renderer.terrain.as_mut() {
                    hm.auto_bake_splat();
                    let tex_data = crate::render::terrain::generate_terrain_texture(hm, 2048);
                    let _ = self.renderer.load_texture_rgba("terrain_tex", &tex_data, 2048, 2048);
                    if let Some(mat) = self.renderer.materials.get("terrain_mat").cloned() {
                        self.renderer.update_material("terrain_mat", mat);
                    }
                    log::info!("Terrain: splat re-baked");
                }
            }
            EditorAction::TerrainSaveFile => {
                let path = rfd::FileDialog::new()
                    .add_filter("R16 heightmap", &["r16"])
                    .set_file_name("terrain.r16")
                    .save_file();
                if let Some(path) = path {
                    if let Some(hm) = self.renderer.terrain.as_ref() {
                        let bytes = hm.to_r16();
                        match std::fs::write(&path, &bytes) {
                            Ok(()) => log::info!("Terrain: R16 saved to {}", path.display()),
                            Err(e) => log::error!("Terrain: save failed: {}", e),
                        }
                    }
                }
            }
            EditorAction::TerrainSavePNG => {
                let path = rfd::FileDialog::new()
                    .add_filter("PNG 16-bit", &["png"])
                    .set_file_name("terrain.png")
                    .save_file();
                if let Some(path) = path {
                    if let Some(hm) = self.renderer.terrain.as_ref() {
                        match save_heightmap_png(hm, &path) {
                            Ok(()) => log::info!("Terrain: PNG saved to {}", path.display()),
                            Err(e) => log::error!("Terrain: PNG save failed: {}", e),
                        }
                    }
                }
            }
            EditorAction::TerrainLoadFile => {
                let path = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .add_filter("R16", &["r16"])
                    .pick_file();
                let Some(path) = path else { return };
                let ext = path.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                let result: std::io::Result<crate::render::terrain::Heightmap> = if ext == "png" {
                    std::fs::read(&path).and_then(|b| {
                        crate::render::terrain::Heightmap::from_png_bytes(&b, 2000.0, 100.0)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                    })
                } else {
                    std::fs::read(&path).and_then(|b| {
                        let n = b.len() / 2;
                        let res = (n as f64).sqrt().round() as u32;
                        crate::render::terrain::Heightmap::from_r16(&b, res, 2000.0, 100.0)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                    })
                };
                match result {
                    Ok(mut hm) => {
                        hm.auto_bake_splat();
                        let mesh = crate::render::terrain::generate_terrain_mesh(
                            &self.renderer.device, &hm,
                        );
                        self.renderer.add_mesh("terrain", mesh);
                        let tex = crate::render::terrain::generate_terrain_texture(&hm, 2048);
                        let _ = self.renderer.load_texture_rgba("terrain_tex", &tex, 2048, 2048);
                        if let Some(mat) = self.renderer.materials.get("terrain_mat").cloned() {
                            self.renderer.update_material("terrain_mat", mat);
                        }
                        self.renderer.terrain = Some(hm);
                        self.pickable = None;
                        log::info!("Terrain: loaded from {}", path.display());
                    }
                    Err(e) => log::error!("Terrain: load failed: {}", e),
                }
            }
        }
    }

    fn on_window_event(&mut self, elwt: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let consumed = self.editor.on_window_event(&self.window, &event);
        let wants_keyboard = self.editor.egui_ctx.wants_keyboard_input();
        let in_play = self.editor.state.play.active;
        let in_menu = self.mode.is_menu();

        match event {
            WindowEvent::CloseRequested => elwt.exit(),
            WindowEvent::Resized(size) => {
                self.renderer.resize(size);
                self.game.camera_mut().set_viewport(size.width, size.height);
            }
            WindowEvent::KeyboardInput { ref event, .. } if !wants_keyboard && !in_menu => {
                self.input.on_key(event);
                if self.editor.state.flying
                    && event.state == ElementState::Pressed
                    && event.physical_key == winit::keyboard::PhysicalKey::Code(KeyCode::Escape)
                {
                    self.game.camera_mut().exit_fly();
                    self.editor.state.flying = false;
                    self.input.editor_flying = false;
                    let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                    self.window.set_cursor_visible(true);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if in_menu { return; }
                self.input.on_mouse_button(button, state);
                if !in_play {
                    let in_active_drag = self.editor.state.gizmo.drag.is_some()
                        || self.editor.state.box_select.is_some();
                    let allow = !consumed || in_active_drag;
                    let (mx, my) = self.input.mouse_pos;
                    let in_vp = self.in_viewport(mx, my);

                    if allow && button == MouseButton::Left && in_vp {
                        match state {
                            ElementState::Pressed => {
                                if self.editor.state.terrain.active {
                                    self.editor.state.terrain.dragging = true;
                                    let cam = self.game.camera();
                                    let (o, d) = cam.ray_from_screen(
                                        mx, my,
                                        self.renderer.size.width as f32,
                                        self.renderer.size.height as f32,
                                    );
                                    if let Some(hm) = self.renderer.terrain.as_mut() {
                                        if self.editor.state.terrain.update_cursor(hm, o, d) {
                                            let pos = self.editor.state.terrain.cursor_world.unwrap();
                                            self.editor.state.undo_requested = true;
                                            self.editor.state.terrain.apply(hm, pos);
                                        }
                                    }
                                    self.mouse_press_pos = None;
                                } else if self.editor.state.palette.active.is_some() {
                                    if let Some(item) = self.editor.state.palette.active {
                                        if let Some(pos) = self.editor.state.palette.preview_pos {
                                            let ctrl = self.input.key_down(KeyCode::ControlLeft)
                                                || self.input.key_down(KeyCode::ControlRight);
                                            let snap = ctrl || self.editor.state.palette.snap_to_grid;
                                            let step = self.editor.state.palette.grid_step;
                                            let final_pos = if snap { placement::snap_to_grid(pos, step) } else { pos };
                                            self.spawn_palette_item(item, final_pos);
                                            if !self.editor.state.palette.keep_active {
                                                self.editor.state.palette.active = None;
                                            }
                                        }
                                    }
                                    self.mouse_press_pos = None;
                                } else {
                                    let started_gizmo = if !self.editor.state.selected.is_empty() {
                                        let ax = gizmo::pick_axis(
                                            &self.world, &self.editor.state.selected,
                                            self.game.camera(), self.editor.state.gizmo.mode,
                                            &self.renderer, mx, my,
                                        );
                                        if let Some(axis) = ax {
                                            let alt = self.input.key_down(KeyCode::AltLeft)
                                                || self.input.key_down(KeyCode::AltRight);
                                            if alt {
                                                let gs = self.game.save_game_state();
                                                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                                                self.duplicate_selected();
                                            }
                                            if let Some(drag) = gizmo::begin_drag(
                                                &self.world, &self.editor.state.selected, axis,
                                                self.editor.state.gizmo.mode, self.game.camera(),
                                                &self.renderer, mx, my,
                                            ) {
                                                let gs = self.game.save_game_state();
                                                self.editor.state.undo.push_forced(&self.world, &self.renderer, gs);
                                                self.editor.state.gizmo.drag = Some(drag);
                                                self.editor.state.gizmo.hovered = Some(axis);
                                                true
                                            } else { false }
                                        } else { false }
                                    } else { false };

                                    if !started_gizmo {
                                        self.editor.state.box_select = Some(BoxSelect {
                                            start: (mx, my), current: (mx, my),
                                        });
                                        self.mouse_press_pos = None;
                                    }
                                }
                            }
                            ElementState::Released => {
                                if self.editor.state.terrain.active
                                    && self.editor.state.terrain.dragging
                                {
                                    self.editor.state.terrain.dragging = false;
                                    self.editor.state.terrain.last_apply_pos = None;
                                    self.mouse_press_pos = None;
                                    return;
                                }
                                self.editor.state.gizmo.drag = None;
                                if let Some(bs) = self.editor.state.box_select.take() {
                                    let dx = bs.current.0 - bs.start.0;
                                    let dy = bs.current.1 - bs.start.1;
                                    if dx * dx + dy * dy < 9.0 {
                                        self.try_pick((mx, my));
                                    } else {
                                        let entities = crate::editor::picking::entities_in_screen_rect(
                                            &self.world, &self.renderer, self.game.camera(),
                                            (bs.start.0, bs.start.1, bs.current.0, bs.current.1),
                                        );
                                        let shift = self.input.key_down(KeyCode::ShiftLeft)
                                            || self.input.key_down(KeyCode::ShiftRight);
                                        if shift {
                                            for e in entities {
                                                if !self.editor.state.selected.contains(&e) {
                                                    self.editor.state.selected.push(e);
                                                }
                                            }
                                        } else { self.editor.state.selected = entities; }
                                    }
                                    self.mouse_press_pos = None;
                                } else if let Some((px, py)) = self.mouse_press_pos.take() {
                                    let dx = mx - px;
                                    let dy = my - py;
                                    if dx * dx + dy * dy < 9.0 { self.try_pick((mx, my)); }
                                }
                            }
                        }
                    } else if allow && button == MouseButton::Right {
                        match state {
                            ElementState::Pressed => {
                                self.rmb_press_time = Some(Instant::now());
                                self.rmb_press_pos = Some(self.input.mouse_pos);
                                self.rmb_dragged = false;
                            }
                            ElementState::Released => {
                                let was_drag = self.rmb_dragged;
                                if !was_drag && in_vp {
                                    if let Some(t) = self.rmb_press_time {
                                        if t.elapsed().as_millis() < 300 {
                                            self.editor.state.context_menu_pos = Some(self.input.mouse_pos);
                                        }
                                    }
                                }
                                self.rmb_press_time = None;
                                self.rmb_press_pos = None;
                                self.rmb_dragged = false;
                            }
                        }
                    } else if !allow
                        && button == MouseButton::Right
                        && state == ElementState::Released
                    {
                        self.rmb_press_time = None;
                        self.rmb_press_pos = None;
                        self.rmb_dragged = false;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if in_menu { return; }
                self.input.on_mouse_move(position.x as f32, position.y as f32);
                let in_active_drag = self.editor.state.gizmo.drag.is_some()
                    || self.editor.state.box_select.is_some();
                if consumed && !in_active_drag {
                } else {
                    let (mx, my) = self.input.mouse_pos;
                    let in_vp = self.in_viewport(mx, my);
                    if let Some(bs) = &mut self.editor.state.box_select { bs.current = (mx, my); }

                    if self.editor.state.terrain.active
                        && self.editor.state.terrain.dragging
                        && in_vp
                    {
                        let (o, d) = self.game.camera().ray_from_screen(
                            mx, my,
                            self.renderer.size.width as f32,
                            self.renderer.size.height as f32,
                        );
                        if let Some(hm) = self.renderer.terrain.as_mut() {
                            if self.editor.state.terrain.update_cursor(hm, o, d) {
                                let pos = self.editor.state.terrain.cursor_world.unwrap();
                                if !self.editor.state.terrain.should_throttle(pos) {
                                    self.editor.state.terrain.apply(hm, pos);
                                }
                            }
                        }
                    }

                    if self.input.mouse_down(MouseButton::Right) {
                        if let Some((px, py)) = self.rmb_press_pos {
                            if !self.rmb_dragged {
                                let dx = mx - px;
                                let dy = my - py;
                                if dx * dx + dy * dy > 25.0 { self.rmb_dragged = true; }
                            }
                        }
                    }
                    if !in_play
                        && !self.editor.state.flying
                        && !self.editor.state.selected.is_empty()
                        && self.editor.state.box_select.is_none()
                        && !self.editor.state.terrain.dragging
                    {
                        if let Some(drag) = self.editor.state.gizmo.drag.clone() {
                            let snap = self.input.key_down(KeyCode::ControlLeft)
                                || self.input.key_down(KeyCode::ControlRight)
                                || self.editor.state.gizmo.snap_enabled;
                            gizmo::apply_drag_with_mode(
                                &mut self.world, &drag, self.editor.state.gizmo.mode, snap,
                                self.game.camera(), &self.renderer, mx, my,
                            );
                        } else if in_vp {
                            let h = gizmo::pick_axis(
                                &self.world, &self.editor.state.selected,
                                self.game.camera(), self.editor.state.gizmo.mode,
                                &self.renderer, mx, my,
                            );
                            self.editor.state.gizmo.hovered = h;
                        } else { self.editor.state.gizmo.hovered = None; }
                    }
                }
            }
            WindowEvent::CursorEntered { .. } => { self.input.on_cursor_enter(); }
            WindowEvent::CursorLeft { .. } => { self.input.on_cursor_enter(); }
            WindowEvent::MouseWheel { delta, .. } if !in_play && !in_menu => {
                if !consumed {
                    let (mx, my) = self.input.mouse_pos;
                    if self.in_viewport(mx, my) {
                        let d = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y,
                            MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.0,
                        };
                        self.input.on_scroll(d);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw(elwt);
                self.window.request_redraw();
            }
            _ => {}
        }
    }

    fn on_device_event(&mut self, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.input.on_mouse_motion_device(delta.0 as f32, delta.1 as f32);
        }
    }
}

// ============================================================
// Terrain PNG export helper
// ============================================================

fn save_heightmap_png(
    hm: &crate::render::terrain::Heightmap,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    use image::{ImageBuffer, Luma};
    let res = hm.resolution;
    let (mn, mx) = hm.min_max();
    let range = (mx - mn).max(1e-4);
    let mut img: ImageBuffer<Luma<u16>, Vec<u16>> = ImageBuffer::new(res, res);
    for z in 0..res {
        for x in 0..res {
            let h = hm.heights[(z * res + x) as usize];
            let n = ((h - mn) / range).clamp(0.0, 1.0);
            let v = (n * 65535.0) as u16;
            img.put_pixel(x, z, Luma([v]));
        }
    }
    img.save(path)?;
    Ok(())
}

struct AppHandler<G: Game> {
    app: Option<App<G>>,
    game_init: Option<G>,
}

impl<G: Game> ApplicationHandler for AppHandler<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() { return; }
        let game = self.game_init.take().expect("resumed called twice with no Game");
        let app = App::new(event_loop, game);
        self.app = Some(app);
        event_loop.set_control_flow(ControlFlow::Poll);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        if let Some(app) = self.app.as_mut() { app.on_window_event(event_loop, window_id, event); }
    }
    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: winit::event::DeviceId, event: DeviceEvent) {
        if let Some(app) = self.app.as_mut() { app.on_device_event(event); }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Some(app) = self.app.as_ref() { app.window.request_redraw(); }
    }
}

pub fn run<G: Game>(game: G) {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("rust_engine=info,warn")
    ).init();
    let event_loop = EventLoop::new().expect("failed to create EventLoop");
    let mut handler = AppHandler::<G> { app: None, game_init: Some(game) };
    event_loop.run_app(&mut handler).expect("EventLoop::run_app failed");
}