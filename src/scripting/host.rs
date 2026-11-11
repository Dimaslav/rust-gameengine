use std::cell::RefCell;
use std::rc::Rc;

use glam::Vec3;
use crate::ecs::{Entity, World};
use crate::engine::Input;
use crate::game::components::{Health, Name, Transform, Velocity};
use super::event::ScriptEvent;

pub type SharedHost = Rc<RefCell<ScriptHost>>;
#[derive(Debug, Clone)]
pub enum ScriptCommand {
    PlaySound { sound: String, position: Option<Vec3> },
    Log { level: LogLevel, message: String },
    SendEvent { name: String },
}

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

pub struct ScriptHost {
    world_ptr: *mut World,
    input_ptr: *const Input,
    entity: Entity,
    dt: f32,
    elapsed: f32,
    pub commands: RefCell<Vec<ScriptCommand>>,
    current_event: Option<ScriptEvent>,
}

impl ScriptHost {
    pub(crate) unsafe fn new(
        world: *mut World,
        input: *const Input,
        entity: Entity,
        dt: f32,
        elapsed: f32,
    ) -> Self {
        Self {
            world_ptr: world,
            input_ptr: input,
            entity,
            dt,
            elapsed,
            commands: RefCell::new(Vec::new()),
            current_event: None,
        }
    }

    pub fn set_visible(&self, e: i64, v: bool) {
        if !self.entity_exists(e) { return; }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if v {
            w.remove::<crate::game::components::Visible>(ent);
        } else {
            w.insert(ent, crate::game::components::Visible(false));
        }
    }

    pub fn deal_damage(&self, e: i64, amount: f64) {
        if !self.entity_exists(e) { return; }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if let Some(h) = w.get_mut::<Health>(ent) {
            h.current = (h.current - amount as f32).max(0.0);
        }
    }

    pub(crate) fn with_event(mut self, ev: ScriptEvent) -> Self {
        self.current_event = Some(ev);
        self
    }

    #[inline]
    fn world(&self) -> &World {
        unsafe { &*self.world_ptr }
    }
    #[inline]
    fn world_mut(&self) -> &mut World {
        unsafe { &mut *self.world_ptr }
    }
    #[inline]
    fn input(&self) -> &Input {
        unsafe { &*self.input_ptr }
    }

    #[inline]
    fn entity_from_rhai(e: i64) -> Entity {
        Entity::from_bits(e as u64)
    }

    fn entity_exists(&self, e: i64) -> bool {
        let ent = Self::entity_from_rhai(e);
        self.world().is_alive(ent)
    }

    // ============================================================
    // Идентификация
    // ============================================================

    pub fn self_entity(&self) -> i64 {
        self.entity.to_bits() as i64
    }

    pub fn entity_name(&self, e: i64) -> String {
        if !self.entity_exists(e) {
            return String::new();
        }
        let ent = Self::entity_from_rhai(e);
        self.world()
            .get::<Name>(ent)
            .map(|n| n.0.clone())
            .unwrap_or_default()
    }

    pub fn find_by_name(&self, name: &str) -> i64 {
        for (e, n) in self.world().query::<Name>() {
            if n.0 == name {
                return e.to_bits() as i64;
            }
        }
        -1
    }

    // ============================================================
    // Transform
    // ============================================================

    pub fn position(&self, e: i64) -> Vec3 {
        if !self.entity_exists(e) {
            return Vec3::ZERO;
        }
        let ent = Self::entity_from_rhai(e);
        self.world()
            .get::<Transform>(ent)
            .map(|t| t.position)
            .unwrap_or(Vec3::ZERO)
    }

    pub fn set_position(&self, e: i64, x: f64, y: f64, z: f64) {
        if !self.entity_exists(e) {
            return;
        }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if let Some(t) = w.get_mut::<Transform>(ent) {
            t.position = Vec3::new(x as f32, y as f32, z as f32);
        }
    }

    pub fn translate(&self, e: i64, dx: f64, dy: f64, dz: f64) {
        if !self.entity_exists(e) {
            return;
        }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if let Some(t) = w.get_mut::<Transform>(ent) {
            t.position += Vec3::new(dx as f32, dy as f32, dz as f32);
        }
    }

    pub fn scale(&self, e: i64) -> f64 {
        if !self.entity_exists(e) {
            return 1.0;
        }
        let ent = Self::entity_from_rhai(e);
        self.world()
            .get::<Transform>(ent)
            .map(|t| t.scale.x as f64)
            .unwrap_or(1.0)
    }

    pub fn set_scale(&self, e: i64, s: f64) {
        if !self.entity_exists(e) {
            return;
        }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if let Some(t) = w.get_mut::<Transform>(ent) {
            t.scale = Vec3::splat(s as f32);
        }
    }

    // ============================================================
    // Health / Velocity
    // ============================================================

    pub fn health(&self, e: i64) -> f64 {
        if !self.entity_exists(e) {
            return 0.0;
        }
        let ent = Self::entity_from_rhai(e);
        self.world()
            .get::<Health>(ent)
            .map(|h| h.current as f64)
            .unwrap_or(0.0)
    }

    pub fn set_health(&self, e: i64, v: f64) {
        if !self.entity_exists(e) {
            return;
        }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if let Some(h) = w.get_mut::<Health>(ent) {
            h.current = (v as f32).clamp(0.0, h.max);
        }
    }

    pub fn damage(&self, e: i64, v: f64) {
        if !self.entity_exists(e) {
            return;
        }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if let Some(h) = w.get_mut::<Health>(ent) {
            h.current = (h.current - v as f32).max(0.0);
        }
    }

    pub fn set_velocity(&self, e: i64, x: f64, y: f64, z: f64) {
        if !self.entity_exists(e) {
            return;
        }
        let ent = Self::entity_from_rhai(e);
        let w = self.world_mut();
        if let Some(v) = w.get_mut::<Velocity>(ent) {
            v.value = Vec3::new(x as f32, y as f32, z as f32);
        }
    }

    // ============================================================
    // Input
    // ============================================================

    pub fn action_down(&self, action: &str) -> bool {
        self.input().down(action)
    }
    pub fn action_pressed(&self, action: &str) -> bool {
        self.input().pressed(action)
    }
    pub fn action_released(&self, action: &str) -> bool {
        self.input().released(action)
    }
    pub fn key_down(&self, key: &str) -> bool {
        parse_key(key)
            .map(|k| self.input().key_down(k))
            .unwrap_or(false)
    }
    pub fn key_pressed(&self, key: &str) -> bool {
        parse_key(key)
            .map(|k| self.input().key_pressed(k))
            .unwrap_or(false)
    }

    // ============================================================
    // World
    // ============================================================

    pub fn spawn(
        &self,
        name: &str,
        mesh: &str,
        material: &str,
        x: f64,
        y: f64,
        z: f64,
    ) -> i64 {
        let w = self.world_mut();
        let e = w.spawn();
        w.insert(e, Name(name.to_string()));
        w.insert(e, Transform::at(Vec3::new(x as f32, y as f32, z as f32)));
        w.insert(e, crate::game::components::MeshHandle(mesh.to_string()));
        w.insert(
            e,
            crate::game::components::MaterialHandle(material.to_string()),
        );
        e.to_bits() as i64
    }

    pub fn despawn(&self, e: i64) {
        if !self.entity_exists(e) {
            return;
        }
        let ent = Self::entity_from_rhai(e);
        self.world_mut().despawn(ent);
    }

    pub fn query_name_prefix(&self, prefix: &str) -> rhai::Array {
        let mut arr = rhai::Array::new();
        for (e, n) in self.world().query::<Name>() {
            if n.0.starts_with(prefix) {
                arr.push(rhai::Dynamic::from(e.to_bits() as i64));
            }
        }
        arr
    }

    pub fn has_component(&self, e: i64, kind: &str) -> bool {
        if !self.entity_exists(e) {
            return false;
        }
        let ent = Self::entity_from_rhai(e);
        let w = self.world();
        match kind {
            "Health" => w.has::<Health>(ent),
            "Name" => w.has::<Name>(ent),
            "Velocity" => w.has::<Velocity>(ent),
            _ => false,
        }
    }

    // ============================================================
    // Time / math
    // ============================================================

    pub fn dt(&self) -> f64 {
        self.dt as f64
    }
    pub fn elapsed(&self) -> f64 {
        self.elapsed as f64
    }

    pub fn random(&self) -> f64 {
        rand01() as f64
    }
    pub fn random_range(&self, lo: f64, hi: f64) -> f64 {
        lo + rand01() as f64 * (hi - lo)
    }

    // ============================================================
    // Events
    // ============================================================

    pub fn event_name(&self) -> String {
        self.current_event
            .as_ref()
            .map(|e| e.name.clone())
            .unwrap_or_default()
    }

    /// Было ли вызвано из `on_event` (а не `on_start` / `on_update`)?
    pub fn event_has(&self) -> bool {
        self.current_event.is_some()
    }

    pub fn event_source(&self) -> i64 {
        self.current_event
            .as_ref()
            .and_then(|e| e.source)
            .map(|e| e.to_bits() as i64)
            .unwrap_or(i64::MIN)
    }

    pub fn event_get(&self, key: &str) -> f64 {
        self.current_event
            .as_ref()
            .and_then(|e| e.payload.get(key).copied())
            .unwrap_or(0.0)
    }

    pub fn emit_event(&self, name: &str) {
        self.commands
            .borrow_mut()
            .push(ScriptCommand::SendEvent {
                name: name.to_string(),
            });
    }

    // ============================================================
    // Команды «наружу»
    // ============================================================

    pub fn log_info(&self, msg: &str) {
        self.commands.borrow_mut().push(ScriptCommand::Log {
            level: LogLevel::Info,
            message: msg.to_string(),
        });
    }
    pub fn log_warn(&self, msg: &str) {
        self.commands.borrow_mut().push(ScriptCommand::Log {
            level: LogLevel::Warn,
            message: msg.to_string(),
        });
    }

    pub fn play_sound(&self, name: &str) {
        self.commands
            .borrow_mut()
            .push(ScriptCommand::PlaySound {
                sound: name.to_string(),
                position: None,
            });
    }
    pub fn play_sound_at(&self, name: &str, x: f64, y: f64, z: f64) {
        self.commands
            .borrow_mut()
            .push(ScriptCommand::PlaySound {
                sound: name.to_string(),
                position: Some(Vec3::new(x as f32, y as f32, z as f32)),
            });
    }
}

// ============================================================
// Helpers
// ============================================================

fn parse_key(s: &str) -> Option<winit::keyboard::KeyCode> {
    use winit::keyboard::KeyCode::*;
    Some(match s {
        "Space" => Space,
        "Escape" => Escape,
        "Enter" => Enter,
        "Tab" => Tab,
        "W" => KeyW,
        "A" => KeyA,
        "S" => KeyS,
        "D" => KeyD,
        "E" => KeyE,
        "Q" => KeyQ,
        "R" => KeyR,
        "F" => KeyF,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "Digit1" => Digit1,
        "Digit2" => Digit2,
        "Digit3" => Digit3,
        "Digit4" => Digit4,
        _ => return None,
    })
}

fn rand01() -> f32 {
    use std::cell::Cell;
    thread_local! {
        static S: Cell<u32> = const { Cell::new(0x5EED_FACE) };
    }
    S.with(|s| {
        let mut v = s.get();
        v = v.wrapping_mul(1664525).wrapping_add(1013904223);
        s.set(v);
        ((v >> 8) & 0xFFFFFF) as f32 / 16777215.0
    })
}