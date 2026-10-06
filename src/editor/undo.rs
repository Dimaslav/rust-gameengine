//! Undo/redo стек.
//!
//! Каждый снапшот — пара `(scene_ron, game_state_ron)`. `scene_ron`
//! собирается движком (`save_scene_with_assets_to_string`), а
//! `game_state_ron` — это непрозрачная для движка RON-строка
//! (`Game::save_game_state`). Раньше game-state не сохранялся, и
//! `Ctrl+Z` откатывал мир, но не RPG-прогрессию (золото, ключи,
//! выполненные квесты) — что давало рассинхрон между миром и HUD.
//!
//! Если `Game::save_game_state` возвращает `None` — снапшот содержит
//! `None`, и при откате `App` ничего не передаёт в `load_game_state`.

use std::collections::VecDeque;
use std::time::Instant;

use crate::ecs::World;
use crate::render::Renderer;

const MAX_UNDO: usize = 50;
/// Cooldown защищает от шторма правок (drag). Если пользователь
/// явно вызвал операцию (AddCube, Delete) — она идёт через push_forced.
const PUSH_COOLDOWN_MS: u128 = 300;

/// Один снапшот: сериализованный мир + game-specific состояние.
struct Snapshot {
    scene_ron: String,
    game_state_ron: Option<String>,
}

pub struct UndoStack {
    undo: VecDeque<Snapshot>,
    redo: Vec<Snapshot>,
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
    /// Сохраняет и `World`, и материалы/текстуры из `Renderer`,
    /// и game-state (непрозрачную строку от игры).
    pub fn push(
        &mut self,
        world: &World,
        renderer: &Renderer,
        game_state_ron: Option<String>,
    ) -> bool {
        if !self.can_push_now() {
            return false;
        }
        self.push_forced(world, renderer, game_state_ron)
    }

    /// Принудительный снапшот без cooldown. Для явных операций:
    /// AddCube, Delete, Duplicate, Paste, MakeUnique, …
    pub fn push_forced(
        &mut self,
        world: &World,
        renderer: &Renderer,
        game_state_ron: Option<String>,
    ) -> bool {
        match crate::scene::save_scene_with_game_state_to_string(
            world, renderer, None, game_state_ron,
        ) {
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

    /// Готовый снапшот сцены (например, снятый до UI в `App::redraw`),
    /// плюс game_state.
    ///
    /// NB: `scene_ron` уже содержит внутри `game_state_ron` — если он
    /// пришёл из `save_scene_with_game_state_to_string`. Но чтобы не
    /// усложнять контракт, второй аргумент здесь — отдельная
    /// копия, которую `undo()` вернёт вызывающему. Если они
    /// разойдутся — берётся та, что в аргументе.
    pub fn push_snapshot(
        &mut self,
        snapshot: String,
        game_state_ron: Option<String>,
    ) -> bool {
        if !self.can_push_now() {
            return false;
        }
        self.push_snapshot_internal(snapshot);
        // Обновляем game_state у только что добавленного снапшота.
        if let Some(last) = self.undo.back_mut() {
            last.game_state_ron = game_state_ron;
        }
        true
    }

    fn push_snapshot_internal(&mut self, snapshot: String) {
        if self.undo.len() >= MAX_UNDO {
            self.undo.pop_front();
        }
        self.undo.push_back(Snapshot {
            scene_ron: snapshot,
            game_state_ron: None,
        });
        self.redo.clear();
        self.last_push = Instant::now();
    }

    /// Восстановить предыдущее состояние. Возвращает `(World, game_state)`
    /// и синхронно восстанавливает материалы в `Renderer`.
    ///
    /// `current_game_state` — game-state **текущего** мира, нужен для
    /// помещения в redo.
    pub fn undo(
        &mut self,
        world: &World,
        renderer: &mut Renderer,
        current_game_state: Option<String>,
    ) -> Option<(World, Option<String>)> {
        let prev = self.undo.pop_back()?;

        // Текущее (с материалами и game-state) — в redo.
        if let Ok(current) = crate::scene::save_scene_with_game_state_to_string(
            world, renderer, None, current_game_state.clone(),
        ) {
            self.redo.push(Snapshot {
                scene_ron: current,
                game_state_ron: current_game_state,
            });
        }

        match crate::scene::load_scene_with_assets_from_str(&prev.scene_ron, renderer) {
            Ok((mut w, _spawn)) => {
                w.sync_next_id();
                Some((w, prev.game_state_ron))
            }
            Err(e) => {
                log::warn!("undo load failed: {}", e);
                None
            }
        }
    }

    pub fn redo(
        &mut self,
        world: &World,
        renderer: &mut Renderer,
        current_game_state: Option<String>,
    ) -> Option<(World, Option<String>)> {
        let next = self.redo.pop()?;

        if let Ok(current) = crate::scene::save_scene_with_game_state_to_string(
            world, renderer, None, current_game_state.clone(),
        ) {
            if self.undo.len() >= MAX_UNDO {
                self.undo.pop_front();
            }
            self.undo.push_back(Snapshot {
                scene_ron: current,
                game_state_ron: current_game_state,
            });
        }

        match crate::scene::load_scene_with_assets_from_str(&next.scene_ron, renderer) {
            Ok((mut w, _spawn)) => {
                w.sync_next_id();
                Some((w, next.game_state_ron))
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