use std::sync::Arc;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::keyboard::KeyCode;
use winit::window::{Window, WindowBuilder};

use crate::ecs::{Entity, World};
use crate::editor::gizmo::{self, GizmoMode};
use crate::editor::ui::{self as editor_ui, Stats, UiAssets, UiState};
use crate::editor::{Editor, EditorAction};
use crate::game::components::{
    MaterialHandle, MeshHandle, SkeletonHandle, Spinner, Transform, Velocity,
};
use crate::render::{
    Camera3D, EguiFrameData, GpuLight, GpuPointLight, LineBatch, LineVertex, MeshDraw, PostFx,
    Renderer,
};
use glam::Vec3;
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
    fn redraw(&mut self, elwt: &EventLoopWindowTarget<()>) {
        self.time.tick();
        let dt = self.time.delta;

        self.world.update_events();

        let (mx, my) = self.input.mouse_pos;
        self.input.editor_captured = self.editor.state.gizmo.drag.is_some()
            || !self.in_viewport(mx, my);

        let continue_running = self
            .game
            .update(&mut self.world, &self.input, &mut self.renderer, dt);
        if !continue_running {
            elwt.exit();
            return;
        }

        let ctrl = self.input.key_down(KeyCode::ControlLeft)
            || self.input.key_down(KeyCode::ControlRight);
        if ctrl && self.input.key_pressed(KeyCode::KeyZ) {
            self.editor.state.pending_action = Some(EditorAction::Undo);
        }
        if ctrl && self.input.key_pressed(KeyCode::KeyY) {
            self.editor.state.pending_action = Some(EditorAction::Redo);
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

        // Проверим, что выделенные существуют (могли исчезнуть при undo).
        self.editor.state.prune_selection(&self.world);

        if let Some(action) = self.editor.state.pending_action.take() {
            self.handle_editor_action(action);
        }

        // Draw data
        let selected = self.editor.state.selected.clone();
        let draws = self.game.collect_draws(&mut self.world, &self.renderer);
        let mut lines = self
            .game
            .collect_lines(&mut self.world, &self.renderer, &selected);

        if !selected.is_empty() {
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

        if self.time.frame_count % 10 == 0 {
            self.window.set_title(&format!(
                "Rust Engine 3D | FPS: {:>5.1} | Entities: {:>6} | Selected: {}",
                self.time.fps(),
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
                self.world
                    .insert(new_e, crate::game::components::Name(format!("{}_copy", n.0)));
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

            new_selected.push(new_e);
        }

        self.editor.state.selected = new_selected;
        log::info!("Duplicated {} entities", originals.len());
    }

    fn handle_editor_action(&mut self, action: EditorAction) {
        use crate::game::components::{MaterialHandle, MeshHandle, Transform};

        match action {
            EditorAction::Undo => {
                if let Some(new_world) = self.editor.state.undo.undo(&self.world) {
                    self.world = new_world;
                    self.editor.state.prune_selection(&self.world);
                    log::info!("Undo");
                }
            }
            EditorAction::Redo => {
                if let Some(new_world) = self.editor.state.undo.redo(&self.world) {
                    self.world = new_world;
                    self.editor.state.prune_selection(&self.world);
                    log::info!("Redo");
                }
            }
            EditorAction::Save => {
                let path = self.editor.state.save_path.clone();
                match crate::scene::save_scene_to_file(&self.world, &path) {
                    Ok(()) => log::info!("Scene saved to {}", path),
                    Err(e) => log::error!("Save failed: {}", e),
                }
            }
            EditorAction::Load => {
                let path = self.editor.state.save_path.clone();
                match crate::scene::load_scene_from_file(&path) {
                    Ok(new_world) => {
                        let n = new_world.len();
                        self.world = new_world;
                        self.editor.state.selected.clear();
                        self.editor.state.undo.clear();
                        log::info!("Scene loaded from {}: {} entities", path, n);
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
                log::info!("Added {} (#{})", mesh, e);
            }
            EditorAction::DeleteSelected => {
                if self.editor.state.selected.is_empty() {
                    return;
                }
                self.editor.state.undo.push_forced(&self.world);
                let count = self.editor.state.selected.len();
                for e in self.editor.state.selected.drain(..) {
                    self.world.despawn(e);
                }
                log::info!("Deleted {} entities", count);
            }
            EditorAction::Duplicate => {
                self.editor.state.undo.push_forced(&self.world);
                self.duplicate_selected();
            }
            EditorAction::FocusSelected => {
                if self.editor.state.selected.is_empty() {
                    return;
                }
                let center =
                    match gizmo::group_center(&self.world, &self.editor.state.selected) {
                        Some(c) => c,
                        None => return,
                    };
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

                    let (px, py) = app.input.mouse_pos;
                    let in_vp = app.in_viewport(px, py);

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
                        }

                        WindowEvent::MouseInput { state, button, .. }
                            if !consumed && in_vp =>
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
                                                        app.editor.state.undo.push_forced(&app.world);
                                                        app.editor.state.gizmo.drag = Some(drag);
                                                        app.editor.state.gizmo.hovered = Some(axis);
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

                            if !app.editor.state.selected.is_empty() {
                                let (mx, my) = app.input.mouse_pos;
                                if let Some(drag) = app.editor.state.gizmo.drag.clone() {
                                    gizmo::apply_drag_with_mode(
                                        &mut app.world,
                                        &drag,
                                        app.editor.state.gizmo.mode,
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
                            if !consumed && in_vp =>
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

                Event::AboutToWait => {
                    app.window.request_redraw();
                }

                _ => {}
            }
        })
        .unwrap();
}