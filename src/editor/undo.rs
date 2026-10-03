use std::collections::VecDeque;
use std::time::Instant;

use crate::ecs::World;

const MAX_UNDO: usize = 50;
/// Cooldown защищает от шторма правок (drag). Но если пользователь
/// явно вызвал операцию (AddCube, Delete) — она идёт через push_forced.
const PUSH_COOLDOWN_MS: u128 = 300;

pub struct UndoStack {
    undo: VecDeque<String>,
    redo: Vec<String>,
    last_push: Instant,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo: VecDeque::with_capacity(MAX_UNDO),
            redo: Vec::new(),
            last_push: Instant::now() - std::time::Duration::from_secs(3600),
        }
    }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }

    /// Cooldown'ится (для drag/inspector). Возвращает false, если снапшот
    /// **не** был положен.
    pub fn push(&mut self, world: &World) -> bool {
        if self.last_push.elapsed().as_millis() < PUSH_COOLDOWN_MS {
            return false;
        }
        self.push_forced(world)
    }

    pub fn push_forced(&mut self, world: &World) -> bool {
        match crate::scene::save_scene_to_string(world, None) {
            Ok(s) => {
                if self.undo.len() >= MAX_UNDO {
                    self.undo.pop_front();
                }
                self.undo.push_back(s);
                self.redo.clear();
                self.last_push = Instant::now();
                true
            }
            Err(e) => {
                log::warn!("undo push failed: {}", e);
                false
            }
        }
    }

    pub fn undo(&mut self, world: &World) -> Option<World> {
        let prev = self.undo.pop_back()?;
        if let Ok(current) = crate::scene::save_scene_to_string(world, None) {
            self.redo.push(current);
        }
        match crate::scene::load_scene_from_str(&prev) {
            Ok((mut w, _spawn)) => {
                w.sync_next_id();
                Some(w)
            }
            Err(e) => {
                log::warn!("undo load failed: {}", e);
                None
            }
        }
    }

    pub fn redo(&mut self, world: &World) -> Option<World> {
        let next = self.redo.pop()?;
        if let Ok(current) = crate::scene::save_scene_to_string(world, None) {
            if self.undo.len() >= MAX_UNDO {
                self.undo.pop_front();
            }
            self.undo.push_back(current);
        }
        match crate::scene::load_scene_from_str(&next) {
            Ok((mut w, _spawn)) => {
                w.sync_next_id();
                Some(w)
            }
            Err(e) => {
                log::warn!("redo load failed: {}", e);
                None
            }
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

impl Default for UndoStack {
    fn default() -> Self { Self::new() }
}