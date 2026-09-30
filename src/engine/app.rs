use std::sync::Arc;
use winit::event::{DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::keyboard::KeyCode;
use winit::window::{CursorGrabMode, Window, WindowBuilder};

use crate::ecs::{Entity, World};
use crate::editor::gizmo::{self, GizmoMode};
use crate::editor::ui::{self as editor_ui, Stats, UiAssets, UiState};
use crate::editor::{Editor, EditorAction};
use crate::game::components::{
    MaterialHandle, MeshHandle, Parent, SkeletonHandle, Spinner, Transform, Velocity,
};
use crate::render::{
    Camera3D, EguiFrameData, GpuLight, GpuPointLight, LineBatch, LineVertex, MeshDraw, PostFx,
    Renderer,
};
use glam::Vec3;
use super::collision::{self, PlayerBox};
use super::input::Input;
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

    fn dir_lights(&self) -> Vec<GpuLight> { Vec::new() }
    fn point_lights(&self) -> Vec<GpuPointLight> { Vec::new() }
    fn ambient(&self) -> [f32; 3] { [0.18, 0.20, 0.26] }
    fn postfx(&self) -> PostFx { PostFx::default() }
    fn apply_postfx(&mut self, _postfx: PostFx) {}
    fn camera(&self) -> &Camera3D;
    fn camera_mut(&mut self) -> &mut Camera3D;
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
}

impl<G: Game> App<G> {
    fn update_player(&mut self, dt: f32) {
        {
            let sens = self.editor.state.play.look_sensitivity;
            let (mdx, mdy) = self.input.mouse_motion;
            self.game.camera_mut().fps_look(mdx * sens, mdy * sens);
        }

        let eye_height = self.editor.state.play.eye_height;
        let player_radius = self.editor.state.play.player_radius;
        let player_height = self.editor.state.play.player_height;
        let walk_speed = self.editor.state.play.walk_speed;
        let run_speed = self.editor.state.play.run_speed;
        let jump_speed = self.editor.state.play.jump_speed;
        let gravity = self.editor.state.play.gravity;
        let floor_y = self.editor.state.play.floor_y;
        let bob_enabled = self.editor.state.play.bob_enabled;
        let bob_amp = self.editor.state.play.bob_amplitude;

        let f = self.game.camera().forward();
        let fwd_xz = Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
        let right_xz = Vec3::new(-fwd_xz.z, 0.0, fwd_xz.x);

        let running = self.input.key_down(KeyCode::ShiftLeft);
        let speed = if running { run_speed } else { walk_speed };

        let mut motion = Vec3::ZERO;
        if self.input.key_down(KeyCode::KeyW) { motion += fwd_xz; }
        if self.input.key_down(KeyCode::KeyS) { motion -= fwd_xz; }
        if self.input.key_down(KeyCode::KeyD) { motion += right_xz; }
        if self.input.key_down(KeyCode::KeyA) { motion -= right_xz; }

        let moving = motion.length_squared() > 1e-8;
        if moving {
            motion = motion.normalize() * speed * dt;
        }

        let mut vvel = self.editor.state.play.vertical_velocity;
        let mut on_ground = self.editor.state.play.on_ground;

        if self.input.key_pressed(KeyCode::Space) && on_ground {
            vvel = jump_speed;
            on_ground = false;
        }
        vvel -= gravity * dt;
        let dy = vvel * dt;

        let eye_pos = self.game.camera().first_person_pos;
        let feet = eye_pos - Vec3::Y * eye_height;
        let pbox = PlayerBox {
            radius: player_radius,
            height: player_height,
        };
        let delta = motion + Vec3::new(0.0, dy, 0.0);
        let (new_feet, landed) = collision::resolve_movement(
            &self.world,
            &self.renderer,
            feet,
            delta,
            &pbox,
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

        let new_eye = new_feet + Vec3::Y * (eye_height + bob_offset);
        self.game.camera_mut().first_person_pos = new_eye;

        let play = &mut self.editor.state.play;
        play.vertical_velocity = vvel;
        play.on_ground = on_ground;
        play.bob_distance = bob_dist;
        play.bob_current = bob_cur;
        play.saved_position = new_eye;
    }

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
        if self.input.key_down(KeyCode::KeyQ) { delta -= Vec3::Y; }

        if delta.length_squared() > 1e-6 {
            let mv = delta.normalize() * speed;
            self.game.camera_mut().fly_move(mv);
        }

        if self.input.scroll_delta.abs() > 0.01 {
            let fs = &mut self.editor.state.fly_speed;
            *fs = (*fs * (1.0 + self.input.scroll_delta * 0.1)).clamp(0.5, 200.0);
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

        // === RMB + fly ===
        let rmb = self.input.mouse_down(MouseButton::Right);
        let want_fly = rmb && !self.editor.state.play.active;
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

        let continue_running = self
            .game
            .update(&mut self.world, &self.input, &mut self.renderer, dt);
        if !continue_running {
            elwt.exit();
            return;
        }

        if let Some(action @ EditorAction::TogglePlay) = self.editor.state.pending_action {
            self.editor.state.pending_action = None;
            self.handle_editor_action(action);
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

        let assets = UiAssets {
            mesh_names: &mesh_names,
            material_names: &material_names,
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

        for (name, mat) in self.editor.state.dirty_materials.drain(..) {
            self.renderer.update_material(&name, mat);
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
                "Rust Engine 3D [{}] | FPS {:>5.1} | Frame {:.2}/{:.2} ms | Hitches {} | Entities {} | Sel {}",
                mode_str,
                self.time.fps(),
                self.time.frame_time_avg_ms(),
                self.time.frame_time_max_ms(),
                self.time.hitches,
                self.world.len(),
                self.editor.state.selected.len(),
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
        if originals.is_empty() { return; }

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
            // Parent не переносим — он ссылается на старый id.

            new_selected.push(new_e);
        }

        self.editor.state.selected = new_selected;
        log::info!("Duplicated {} entities", originals.len());
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

                    let cam_eye = self.game.camera().position();
                    let mut spawn = cam_eye;
                    if spawn.y < play.floor_y + play.eye_height {
                        spawn.y = play.floor_y + play.eye_height;
                    }
                    self.game.camera_mut().enter_fps(spawn);
                    play.saved_position = spawn;

                    let _ = self.window.set_cursor_grab(CursorGrabMode::Locked);
                    self.window.set_cursor_visible(false);
                    self.input.on_cursor_enter();
                    self.input.mouse_motion = (0.0, 0.0);
                    self.input.skip_motion_frames = 4;

                    log::info!("Entered play mode");
                } else {
                    play.saved_position = self.game.camera().first_person_pos;
                    self.game.camera_mut().exit_fps();

                    let _ = self.window.set_cursor_grab(CursorGrabMode::None);
                    self.window.set_cursor_visible(true);

                    log::info!("Exited play mode");
                }
            }
            EditorAction::SpawnPlayerHere => {
                let Some(e) = self.editor.state.primary() else { return };
                let Some(t) = self.world.get::<Transform>(e) else { return };
                let mut pos = t.position;
                pos.y += self.editor.state.play.eye_height;
                self.editor.state.play.saved_position = pos;
                log::info!(
                    "Player spawn set to ({:.2}, {:.2}, {:.2})",
                    pos.x, pos.y, pos.z
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
                if snaps.is_empty() { return; }
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
                self.editor.state.selected = new_selected;
                log::info!("Pasted entities");
            }
            EditorAction::MakeMaterialUnique => {
                let Some(e) = self.editor.state.primary() else { return };
                let Some(mh) = self.world.get::<MaterialHandle>(e).cloned() else { return };
                let Some(mat) = self.renderer.materials.get(&mh.0).cloned() else { return };

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
                if let Err(e) = crate::scene::save_scene_to_file(&self.world, &path) {
                    log::error!("Save failed: {}", e);
                }
            }
            EditorAction::Load => {
                let path = self.editor.state.save_path.clone();
                match crate::scene::load_scene_from_file(&path) {
                    Ok(new_world) => {
                        self.world = new_world;
                        self.editor.state.selected.clear();
                        self.editor.state.undo.clear();
                    }
                    Err(e) => log::error!("Load failed: {}", e),
                }
            }
            EditorAction::AddCube | EditorAction::AddSphere => {
                self.editor.state.undo.push_forced(&self.world);

                let (mesh, mat, base_name) = match action {
                    EditorAction::AddCube => ("cube", "checker_blue", "Cube"),
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

                self.editor.state.select_single(e);
            }
            EditorAction::DeleteSelected => {
                if self.editor.state.selected.is_empty() { return; }
                self.editor.state.undo.push_forced(&self.world);
                // Детей выделенных тоже удаляем, чтобы не осталось
                // висячих Parent.
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
                if self.editor.state.selected.is_empty() { return; }
                let Some(center) =
                    gizmo::group_center(&self.world, &self.editor.state.selected)
                else { return; };
                let radius =
                    gizmo::group_radius(&self.world, &self.editor.state.selected, &self.renderer);
                self.game.camera_mut().focus_on(center, radius);
            }
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
                                                    } else { false }
                                                } else { false }
                                            } else { false };

                                        if !started_gizmo {
                                            app.mouse_press_pos = Some(app.input.mouse_pos);
                                        }
                                    }
                                    ElementState::Released => {
                                        app.editor.state.gizmo.drag = None;

                                        if let Some((px, py)) = app.mouse_press_pos.take() {
                                            let (mx, my) = app.input.mouse_pos;
                                            let dx = mx - px;
                                            let dy = my - py;
                                            if dx * dx + dy * dy < 9.0 {
                                                app.try_pick((mx, my));
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        WindowEvent::CursorMoved { position, .. } if !consumed => {
                            app.input
                                .on_mouse_move(position.x as f32, position.y as f32);

                            if !in_play
                                && !app.editor.state.flying
                                && !app.editor.state.selected.is_empty()
                            {
                                let (mx, my) = app.input.mouse_pos;
                                if let Some(drag) = app.editor.state.gizmo.drag.clone() {
                                    let snap = app.input.key_down(KeyCode::ControlLeft)
                                        || app.input.key_down(KeyCode::ControlRight);
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