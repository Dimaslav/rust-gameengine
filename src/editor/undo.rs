use std::collections::VecDeque;
use std::time::Instant;

use crate::ecs::World;
use crate::render::Renderer;

const MAX_UNDO: usize = 50;
/// Cooldown защищает от шторма правок (drag). Если пользователь
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

    /// Проверка, истёк ли cooldown — используется в `App::redraw`,
    /// чтобы решить, снимать ли дорогой snapshot до egui.
    pub fn can_push_now(&self) -> bool {
        self.last_push.elapsed().as_millis() >= PUSH_COOLDOWN_MS
    }

    /// Cooldown'ится (для drag/inspector). Возвращает false, если
    /// снапшот **не** был положен.
    ///
    /// Сохраняет и `World`, и материалы/текстуры из `Renderer` —
    /// чтобы Ctrl+Z откатывал всё сразу.
    pub fn push(&mut self, world: &World, renderer: &Renderer) -> bool {
        if !self.can_push_now() {
            return false;
        }
        self.push_forced(world, renderer)
    }

    /// Принудительный снапшот без cooldown. Для явных операций:
    /// AddCube, Delete, Duplicate, Paste, MakeUnique, …
    pub fn push_forced(&mut self, world: &World, renderer: &Renderer) -> bool {
        match crate::scene::save_scene_with_assets_to_string(world, renderer, None) {
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

    /// Готовый снапшот (например, снятый до UI в `App::redraw`).
    pub fn push_snapshot(&mut self, snapshot: String) -> bool {
        if !self.can_push_now() {
            return false;
        }
        self.push_snapshot_internal(snapshot);
        true
    }

    fn push_snapshot_internal(&mut self, snapshot: String) {
        if self.undo.len() >= MAX_UNDO {
            self.undo.pop_front();
        }
        self.undo.push_back(snapshot);
        self.redo.clear();
        self.last_push = Instant::now();
    }

    /// Восстановить предыдущее состояние. Возвращает загруженный World
    /// и синхронно восстанавливает материалы в `Renderer`.
    pub fn undo(&mut self, world: &World, renderer: &mut Renderer) -> Option<World> {
        let prev = self.undo.pop_back()?;

        // Текущее (с материалами) — в redo.
        if let Ok(current) = crate::scene::save_scene_with_assets_to_string(world, renderer, None) {
            self.redo.push(current);
        }

        match crate::scene::load_scene_with_assets_from_str(&prev, renderer) {
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

    pub fn redo(&mut self, world: &World, renderer: &mut Renderer) -> Option<World> {
        let next = self.redo.pop()?;

        if let Ok(current) = crate::scene::save_scene_with_assets_to_string(world, renderer, None) {
            if self.undo.len() >= MAX_UNDO {
                self.undo.pop_front();
            }
            self.undo.push_back(current);
        }

        match crate::scene::load_scene_with_assets_from_str(&next, renderer) {
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