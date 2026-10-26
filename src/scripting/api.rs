//! Регистрация всего скриптового API в Rhai Engine.

use glam::Vec3;
use rhai::Engine;

use super::host::{SharedHost, ScriptHost};

pub fn register_all(engine: &mut Engine) {
    register_vec3(engine);
    register_host(engine);
    register_math(engine);
}

// ============================================================
// Vec3
// ============================================================

fn register_vec3(engine: &mut Engine) {
    engine.register_type_with_name::<Vec3>("Vec3");

    engine.register_get("x", |v: &mut Vec3| v.x as f64);
    engine.register_get("y", |v: &mut Vec3| v.y as f64);
    engine.register_get("z", |v: &mut Vec3| v.z as f64);

    engine.register_get_set(
        "x", |v: &mut Vec3| v.x as f64, |v: &mut Vec3, val: f64| v.x = val as f32,
    );
    engine.register_get_set(
        "y", |v: &mut Vec3| v.y as f64, |v: &mut Vec3, val: f64| v.y = val as f32,
    );
    engine.register_get_set(
        "z", |v: &mut Vec3| v.z as f64, |v: &mut Vec3, val: f64| v.z = val as f32,
    );

    engine.register_fn("length", |v: &mut Vec3| v.length() as f64);
    engine.register_fn("vec3", |x: f64, y: f64, z: f64| {
        Vec3::new(x as f32, y as f32, z as f32)
    });
}

// ============================================================
// Host (World + Input + Time + Events)
// ============================================================
//
// Все функции принимают `SharedHost` (= Rc<RefCell<ScriptHost>>),
// клонируют Rc и бросают `borrow()` / `borrow_mut()`.
// Так Rhai может держать `Dynamic` с клонируемой ссылкой на хост.

fn register_host(engine: &mut Engine) {
    engine.register_type_with_name::<SharedHost>("Host");

    // --- Идентификация ---
    engine.register_fn("self_entity", |h: SharedHost| h.borrow().self_entity());
    engine.register_fn("entity_name", |h: SharedHost, e: i64| h.borrow().entity_name(e));
    engine.register_fn("find_by_name", |h: SharedHost, n: &str| h.borrow().find_by_name(n));

    // --- Transform ---
    engine.register_fn("position", |h: SharedHost, e: i64| h.borrow().position(e));
    engine.register_fn("set_position", |h: SharedHost, e: i64, x: f64, y: f64, z: f64| {
        h.borrow().set_position(e, x, y, z);
    });
    engine.register_fn("translate", |h: SharedHost, e: i64, dx: f64, dy: f64, dz: f64| {
        h.borrow().translate(e, dx, dy, dz);
    });
    engine.register_fn("scale", |h: SharedHost, e: i64| h.borrow().scale(e));
    engine.register_fn("set_scale", |h: SharedHost, e: i64, s: f64| {
        h.borrow().set_scale(e, s);
    });

    // --- Health / Velocity ---
    engine.register_fn("health", |h: SharedHost, e: i64| h.borrow().health(e));
    engine.register_fn("set_health", |h: SharedHost, e: i64, v: f64| {
        h.borrow().set_health(e, v);
    });
    engine.register_fn("damage", |h: SharedHost, e: i64, v: f64| {
        h.borrow().damage(e, v);
    });
    engine.register_fn("set_velocity", |h: SharedHost, e: i64, x: f64, y: f64, z: f64| {
        h.borrow().set_velocity(e, x, y, z);
    });

    // --- Input ---
    engine.register_fn("action_down", |h: SharedHost, a: &str| h.borrow().action_down(a));
    engine.register_fn("action_pressed", |h: SharedHost, a: &str| h.borrow().action_pressed(a));
    engine.register_fn("action_released", |h: SharedHost, a: &str| h.borrow().action_released(a));
    engine.register_fn("key_down", |h: SharedHost, k: &str| h.borrow().key_down(k));
    engine.register_fn("key_pressed", |h: SharedHost, k: &str| h.borrow().key_pressed(k));

    // --- World ---
    engine.register_fn(
        "spawn",
        |h: SharedHost, name: &str, mesh: &str, mat: &str, x: f64, y: f64, z: f64| -> i64 {
            h.borrow().spawn(name, mesh, mat, x, y, z)
        },
    );
    engine.register_fn("despawn", |h: SharedHost, e: i64| h.borrow().despawn(e));
    engine.register_fn("query_name_prefix", |h: SharedHost, p: &str| {
        h.borrow().query_name_prefix(p)
    });
    engine.register_fn("has_component", |h: SharedHost, e: i64, kind: &str| {
        h.borrow().has_component(e, kind)
    });

    // --- Time / math ---
    engine.register_fn("dt", |h: SharedHost| h.borrow().dt());
    engine.register_fn("elapsed", |h: SharedHost| h.borrow().elapsed());
    engine.register_fn("random", |h: SharedHost| h.borrow().random());
    engine.register_fn("random_range", |h: SharedHost, lo: f64, hi: f64| {
        h.borrow().random_range(lo, hi)
    });

    // --- Events ---
    engine.register_fn("event_has", |h: SharedHost| h.borrow().event_has());
    engine.register_fn("event_name", |h: SharedHost| h.borrow().event_name());
    engine.register_fn("event_source", |h: SharedHost| h.borrow().event_source());
    engine.register_fn("event_get", |h: SharedHost, key: &str| {
        h.borrow().event_get(key)
    });
    engine.register_fn("emit_event", |h: SharedHost, name: &str| {
        h.borrow().emit_event(name)
    });

    // --- Commands ---
    engine.register_fn("log_info", |h: SharedHost, msg: &str| h.borrow().log_info(msg));
    engine.register_fn("log_warn", |h: SharedHost, msg: &str| h.borrow().log_warn(msg));
    engine.register_fn("play_sound", |h: SharedHost, n: &str| h.borrow().play_sound(n));
    engine.register_fn("play_sound_at", |h: SharedHost, n: &str, x: f64, y: f64, z: f64| {
        h.borrow().play_sound_at(n, x, y, z);
    });
}

// ============================================================
// Math (global)
// ============================================================

fn register_math(engine: &mut Engine) {
    engine.register_fn("clamp", |x: f64, lo: f64, hi: f64| x.clamp(lo, hi));
    engine.register_fn("lerp", |a: f64, b: f64, t: f64| a + (b - a) * t);
    engine.register_fn("min", |a: f64, b: f64| a.min(b));
    engine.register_fn("max", |a: f64, b: f64| a.max(b));
    engine.register_fn("abs", |x: f64| x.abs());
    engine.register_fn("sqrt", |x: f64| x.sqrt());
    engine.register_fn("sin", |x: f64| x.sin());
    engine.register_fn("cos", |x: f64| x.cos());
    engine.register_fn("tan", |x: f64| x.tan());
    engine.register_fn("pi", || std::f64::consts::PI);
}