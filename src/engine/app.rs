use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::ecs::{System, World};
use crate::render::{
    Camera3D, GpuLight, GpuPointLight, LineVertex, MeshDraw, PostFx, Renderer,
};
use super::input::Input;
use super::time::Time;

pub trait Game: 'static {
    fn init(&mut self, _world: &mut World, _renderer: &mut Renderer) {}
    fn update(&mut self, _world: &mut World, _input: &Input, _renderer: &mut Renderer, _dt: f32) {}

    fn collect_draws(&mut self, _world: &mut World, _renderer: &Renderer) -> Vec<MeshDraw> {
        Vec::new()
    }

    fn collect_lines(&mut self, _world: &mut World, _renderer: &Renderer) -> Vec<LineVertex> {
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

    /// Настройки постобработки (bloom + exposure).
    fn postfx(&self) -> PostFx {
        PostFx::default()
    }

    fn camera(&self) -> &Camera3D;
    fn camera_mut(&mut self) -> &mut Camera3D;
}

struct App<G: Game> {
    game: G,
    world: World,
    input: Input,
    time: Time,
    systems: Vec<Box<dyn System>>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl<G: Game> ApplicationHandler for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("Rust Engine 3D")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let mut renderer = pollster::block_on(Renderer::new(window.clone()));

        self.game
            .camera_mut()
            .set_viewport(renderer.size.width, renderer.size.height);

        self.game.init(&mut self.world, &mut renderer);
        self.renderer = Some(renderer);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size);
                    self.game.camera_mut().set_viewport(size.width, size.height);
                }
            }

            WindowEvent::KeyboardInput { event, .. } => self.input.on_key(&event),

            WindowEvent::MouseInput { state, button, .. } => {
                self.input.on_mouse_button(button, state);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.input
                    .on_mouse_move(position.x as f32, position.y as f32);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.0,
                };
                self.input.on_scroll(d);
            }

            WindowEvent::RedrawRequested => {
                self.time.tick();
                let dt = self.time.delta;

                self.world.update_events();

                // Игровая логика (получает &mut Renderer для обновления skeleton).
                if let Some(renderer) = &mut self.renderer {
                    self.game.update(&mut self.world, &self.input, renderer, dt);
                }
                for sys in self.systems.iter_mut() {
                    sys.update(&mut self.world, dt);
                }

                let draws = match &self.renderer {
                    Some(r) => self.game.collect_draws(&mut self.world, r),
                    None => Vec::new(),
                };
                let lines = match &self.renderer {
                    Some(r) => self.game.collect_lines(&mut self.world, r),
                    None => Vec::new(),
                };
                let dir_lights = self.game.dir_lights();
                let point_lights = self.game.point_lights();
                let ambient = self.game.ambient();
                let postfx = self.game.postfx();

                let draw_count = draws.len();
                let instance_count: usize = draws.iter().map(|d| d.instances.len()).sum();

                if let Some(renderer) = &mut self.renderer {
                    let res = renderer.render(
                        self.game.camera(),
                        &draws,
                        &lines,
                        &dir_lights,
                        &point_lights,
                        ambient,
                        postfx,
                    );
                    match res {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            renderer.resize(renderer.size);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => log::warn!("Surface error: {:?}", e),
                    }
                }

                if self.time.frame_count % 10 == 0 {
                    if let Some(w) = &self.window {
                        w.set_title(&format!(
                            "Rust Engine 3D | FPS: {:>5.1} | Entities: {:>6} | Draws: {:>4} | Instances: {:>6} | Lights: {}d {}p | Bloom: {:.2}",
                            self.time.fps(),
                            self.world.len(),
                            draw_count,
                            instance_count,
                            dir_lights.len(),
                            point_lights.len(),
                            postfx.bloom_strength,
                        ));
                    }
                }

                self.input.end_frame();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        _event: DeviceEvent,
    ) {
    }
}

pub fn run<G: Game>(game: G) {
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        game,
        world: World::new(),
        input: Input::new(),
        time: Time::new(),
        systems: Vec::new(),
        window: None,
        renderer: None,
    };
    event_loop.run_app(&mut app).unwrap();
}