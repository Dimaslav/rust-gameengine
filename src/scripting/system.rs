//! ScriptSystem — тик всех entity со ScriptComponent.
//!
//! Порядок в кадре (вызывать из `App::redraw` после physics и ai):
//!   1. Собрать список entity со ScriptComponent (один проход по World).
//!   2. Для каждого запустить on_start (один раз) и on_update.
//!   3. Собрать команды (sound, log, event) и вернуть их наружу.

use std::rc::Rc;
use std::cell::RefCell;

use crate::ecs::{Entity, World};
use crate::engine::Input;

use super::component::ScriptComponent;
use super::engine::ScriptEngine;
use super::host::{LogLevel, ScriptCommand, ScriptHost, SharedHost};

pub struct ScriptSystem {
    pub engine: ScriptEngine,
    reload_timer: f32,
    /// Кэш "какие скрипты уже загружены" — чтобы не дёргать `ensure_loaded`
    /// каждый кадр.
    known: std::collections::HashSet<String>,
}

const RELOAD_INTERVAL: f32 = 0.5;

impl ScriptSystem {
    pub fn new(root: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let mut engine = ScriptEngine::new(root);
        engine.load_all()?;
        let known = engine.loaded_names().into_iter().collect();
        Ok(Self {
            engine,
            reload_timer: 0.0,
            known,
        })
    }

    /// Тик. Все команды (`play_sound`, `log`) собираются и возвращаются.
    /// App применит их после — там, где доступны AudioSystem и т.д.
    pub fn update(
        &mut self,
        world: &mut World,
        input: &Input,
        dt: f32,
        elapsed: f32,
    ) -> Vec<ScriptCommand> {
        // --- Hot reload ---
        self.reload_timer += dt;
        if self.reload_timer >= RELOAD_INTERVAL {
            self.reload_timer = 0.0;
            let reloaded = self.engine.reload_changed();
            if !reloaded.is_empty() {
                // Сбрасываем started у затронутых entity,
                // чтобы on_start выполнился заново.
                let reloaded_set: std::collections::HashSet<&str> =
                    reloaded.iter().map(|s| s.as_str()).collect();
                let to_reset: Vec<Entity> = world.query::<ScriptComponent>()
                    .filter(|(_, c)| reloaded_set.contains(c.script.as_str()))
                    .map(|(e, _)| e)
                    .collect();
                for e in to_reset {
                    if let Some(c) = world.get_mut::<ScriptComponent>(e) {
                        c.started = false;
                    }
                }
                for name in reloaded {
                    self.known.insert(name);
                }
            }
        }

        // --- Lazy-load скриптов, которые появились в компонентах ---
        let scripts_in_use: Vec<String> = world.query::<ScriptComponent>()
            .map(|(_, c)| c.script.clone())
            .collect();
        for s in scripts_in_use {
            if !self.known.contains(&s) {
                if self.engine.ensure_loaded(&s).is_ok() {
                    self.known.insert(s);
                }
            }
        }

        // --- Собрать список entity со скриптами ---
        let scripted: Vec<(Entity, String, bool, bool)> = world
            .query::<ScriptComponent>()
            .filter(|(_, c)| c.enabled)
            .map(|(e, c)| (e, c.script.clone(), c.started, c.enabled))
            .collect();

        let mut all_commands: Vec<ScriptCommand> = Vec::new();

        for (entity, script_name, started, _) in scripted {
            // Проверяем, что entity ещё существует (мог быть удалён
            // предыдущим скриптом в этом же кадре).
            if !world.entities().contains(&entity) { continue; }

            // Создаём хост.
            // SAFETY: указатели на World и Input живут весь вызов
            // `ScriptSystem::update`, потому что `App::redraw` держит
            // оба на стеке. Мы не выходим за пределы функции.
            let host = unsafe {
                ScriptHost::new(
                    world as *mut World,
                    input as *const Input,
                    entity,
                    dt,
                    elapsed,
                )
            };
            let shared: SharedHost = Rc::new(RefCell::new(host));

            // on_start — один раз.
            if !started {
                self.engine.call(&script_name, "on_start", shared.clone());
                if let Some(c) = world.get_mut::<ScriptComponent>(entity) {
                    c.started = true;
                }
            }

            // on_update — каждый кадр.
            self.engine.call(&script_name, "on_update", shared.clone());

            // Собрать команды.
            let cmds = shared.borrow().commands.borrow().clone();
            for c in cmds {
                match &c {
                    ScriptCommand::Log { level, message } => match level {
                        LogLevel::Info  => log::info!("[script {}] {}", script_name, message),
                        LogLevel::Warn  => log::warn!("[script {}] {}", script_name, message),
                        LogLevel::Error => log::error!("[script {}] {}", script_name, message),
                    },
                    _ => all_commands.push(c),
                }
            }
        }

        all_commands
    }
}