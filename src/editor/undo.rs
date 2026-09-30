//! Undo/redo стек — снимки всей сцены (World) в RON.
//!
//! Снимок делается через `scene::save_scene_to_string`, восстановление —
//! через `scene::load_scene_from_str`. Восстановление создаёт **новый**
//! World, старый молча заменяется.
//!
//! Ограничение: изменения `Renderer` (материалы, текстуры, скелеты)
//! не входят в снимок. Откатывается только состояние ECS-мира.

use std::collections::VecDeque;
use std::time::Instant;

use crate::ecs::World;

const MAX_UNDO: usize = 50;
/// Минимальный интервал между пуш-снимками в миллисекундах.
/// Защищает от «цунами» при быстром изменении DragValue/Slider.
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
            // Чтобы самый первый push не отсеивался cooldown'ом.
            last_push: Instant::now() - std::time::Duration::from_secs(3600),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Укладывает текущее состояние сцены в undo-стек.
    ///
    /// Возвращает `true`, если снимок сделан.
    /// Может отсеять запрос, если с прошлого push прошло < `PUSH_COOLDOWN_MS`.
    /// Форсировать можно через `push_forced`.
    pub fn push(&mut self, world: &World) -> bool {
        if self.last_push.elapsed().as_millis() < PUSH_COOLDOWN_MS {
            return false;
        }
        self.push_forced(world)
    }

    /// Кладёт снимок, игнорируя cooldown. Используется для явных действий
    /// (Delete / Add / Duplicate / drag-start), где потеря снимка недопустима.
    pub fn push_forced(&mut self, world: &World) -> bool {
        match crate::scene::save_scene_to_string(world) {
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

    /// Откат. Возвращает новый World или None, если стека нет.
    pub fn undo(&mut self, world: &World) -> Option<World> {
        let prev = self.undo.pop_back()?;

        // Текущее состояние — в redo.
        if let Ok(current) = crate::scene::save_scene_to_string(world) {
            self.redo.push(current);
        }

        match crate::scene::load_scene_from_str(&prev) {
            Ok(w) => Some(w),
            Err(e) => {
                log::warn!("undo load failed: {}", e);
                None
            }
        }
    }

    /// Возврат. Возвращает новый World или None, если redo пуст.
    pub fn redo(&mut self, world: &World) -> Option<World> {
        let next = self.redo.pop()?;

        if let Ok(current) = crate::scene::save_scene_to_string(world) {
            if self.undo.len() >= MAX_UNDO {
                self.undo.pop_front();
            }
            self.undo.push_back(current);
        }

        match crate::scene::load_scene_from_str(&next) {
            Ok(w) => Some(w),
            Err(e) => {
                log::warn!("redo load failed: {}", e);
                None
            }
        }
    }

    /// Полная очистка стека. Вызывается после Load сцены из файла.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}