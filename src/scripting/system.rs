use std::cell::RefCell;
use std::rc::Rc;

use crate::ecs::{Entity, World};
use crate::engine::Input;

use super::component::ScriptComponent;
use super::engine::ScriptEngine;
use super::event::ScriptEvent;
use super::host::{LogLevel, ScriptCommand, ScriptHost, SharedHost};

pub struct ScriptSystem {
    pub engine: ScriptEngine,
    reload_timer: f32,
    known: std::collections::HashSet<String>,
    pending_events: Vec<ScriptEvent>,
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
            pending_events: Vec::new(),
        })
    }

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
                let reloaded_set: std::collections::HashSet<&str> =
                    reloaded.iter().map(|s| s.as_str()).collect();
                let to_reset: Vec<Entity> = world
                    .query::<ScriptComponent>()
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

        let scripts_in_use: Vec<String> = world
            .query::<ScriptComponent>()
            .map(|(_, c)| c.script.clone())
            .collect();
        for s in scripts_in_use {
            if !self.known.contains(&s) && self.engine.ensure_loaded(&s).is_ok() {
                self.known.insert(s);
            }
        }

        let mut events: Vec<ScriptEvent> = self.pending_events.drain(..).collect();
        events.extend(
            world
                .read_events_current::<ScriptEvent>()
                .cloned(),
        );

        // --- Собрать список entity со скриптами ---
        let scripted: Vec<(Entity, String, bool)> = world
            .query::<ScriptComponent>()
            .filter(|(_, c)| c.enabled)
            .map(|(e, c)| (e, c.script.clone(), c.started))
            .collect();

        let mut all_commands: Vec<ScriptCommand> = Vec::new();

        for (entity, script_name, started) in scripted {
            if !world.entities().contains(&entity) {
                continue;
            }

            if !started {
                {
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
                    self.engine.call(&script_name, "on_start", shared.clone());
                    drain_commands(
                        &shared,
                        &script_name,
                        "on_start",
                        &mut all_commands,
                    );
                }
                if let Some(c) = world.get_mut::<ScriptComponent>(entity) {
                    c.started = true;
                }
            }

            {
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
                self.engine.call(&script_name, "on_update", shared.clone());
                drain_commands(
                    &shared,
                    &script_name,
                    "on_update",
                    &mut all_commands,
                );
            }

            for ev in &events {
                let host = unsafe {
                    ScriptHost::new(
                        world as *mut World,
                        input as *const Input,
                        entity,
                        dt,
                        elapsed,
                    )
                };
                let shared: SharedHost =
                    Rc::new(RefCell::new(host.with_event(ev.clone())));
                self.engine.call(&script_name, "on_event", shared.clone());
                let ctx = format!("on_event/{}", ev.name);
                drain_commands(
                    &shared,
                    &script_name,
                    &ctx,
                    &mut all_commands,
                );
            }
        }

        let mut external_commands: Vec<ScriptCommand> = Vec::new();
        for c in all_commands {
            match c {
                ScriptCommand::SendEvent { name } => {
                    self.pending_events.push(ScriptEvent::new(name));
                }
                _ => external_commands.push(c),
            }
        }
        external_commands
    }
}

fn drain_commands(
    shared: &SharedHost,
    script_name: &str,
    ctx: &str,
    all: &mut Vec<ScriptCommand>,
) {
    let cmds = shared.borrow().commands.borrow().clone();
    for c in cmds {
        match &c {
            ScriptCommand::Log { level, message } => match level {
                LogLevel::Info => {
                    log::info!("[script {}:{}] {}", script_name, ctx, message)
                }
                LogLevel::Warn => {
                    log::warn!("[script {}:{}] {}", script_name, ctx, message)
                }
                LogLevel::Error => {
                    log::error!("[script {}:{}] {}", script_name, ctx, message)
                }
            },
            _ => all.push(c),
        }
    }
}