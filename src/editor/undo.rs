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

    /// Публичная проверка: можно ли сейчас положить снапшот
    /// (не истёк ли cooldown).
    ///
    /// Используется в `App::redraw` — чтобы решить, делать ли
    /// дорогостоящий snapshot ДО egui. Если cooldown активен,
    /// снимок заведомо не будет принят `push`'ем, и тратить
    /// время на сериализацию сцены бессмысленно.
    pub fn can_push_now(&self) -> bool {
        self.last_push.elapsed().as_millis() >= PUSH_COOLDOWN_MS
    }

    /// Cooldown'ится (для drag/inspector). Возвращает false, если снапшот
    /// **не** был положен.
    pub fn push(&mut self, world: &World) -> bool {
        if !self.can_push_now() {
            return false;
        }
        self.push_forced(world)
    }

    /// Принудительно положить снапшот текущего `world` без учёта
    /// cooldown. Используется для операций, которые пользователь
    /// вызвал явно (AddCube, Delete, Duplicate, Paste, ...).
    pub fn push_forced(&mut self, world: &World) -> bool {
        match crate::scene::save_scene_to_string(world, None) {
            Ok(s) => {
                self.push_snapshot_internal(s);
                true
            }
            Err(e) => {
                log::warn!("undo push failed: {}", e);
                false
            }
        }
    }

    /// Положить уже сериализованный снапшот. Идентичен `push()`,
    /// но не сериализует — используется для снимков, взятых РАНЕЕ
    /// (например, до UI в `App::redraw`). Возвращает `false`,
    /// если cooldown активен.
    pub fn push_snapshot(&mut self, snapshot: String) -> bool {
        if !self.can_push_now() {
            return false;
        }
        self.push_snapshot_internal(snapshot);
        true
    }

    /// Общая запись в стек. Без проверок cooldown — вызывающий
    /// обязан проверить сам.
    fn push_snapshot_internal(&mut self, snapshot: String) {
        if self.undo.len() >= MAX_UNDO {
            self.undo.pop_front();
        }
        self.undo.push_back(snapshot);
        self.redo.clear();
        self.last_push = Instant::now();
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