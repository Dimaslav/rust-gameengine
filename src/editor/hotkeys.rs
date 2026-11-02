//! Горячие клавиши редактора.
//!
//! ## Что это
//!
//! Централизованный маппинг `HotkeyAction → HotkeyBinding`. Пользователь
//! может менять привязки через Settings → Hotkeys; всё сохраняется в
//! `editor.ron`.
//!
//! ## Как работает
//!
//! `HotkeyMap::poll(&input)` вызывается раз в кадр (из `App::redraw`).
//! Он возвращает `Vec<HotkeyAction>` — все действия, чьи привязки
//! сработали **в этом кадре** (учитывая `key_pressed`, не `key_down`).
//!
//! Модификаторы проверяются строго: если привязка задана как
//! `Ctrl+Shift+S`, то `Ctrl+S` её не активирует.
//!
//! ## Capture
//!
//! При «нажми клавишу» (`state.hotkey_capture = Some(action)`),
//! `App::on_window_event` перехватывает следующее `KeyboardInput`
//! **до** `Input::on_key` — иначе клавиша «нажмётся» в игре. Модификаторы
//! (Ctrl/Shift/Alt) игнорируются как «клавиша» — они накапливаются,
//! ждём основную.

use serde::{Deserialize, Serialize};

use crate::engine::input::Input;
use crate::engine::input_keys::Key;
use winit::keyboard::KeyCode;

// ============================================================
// HotkeyAction
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    // --- File ---
    Save,
    LoadScene,
    NewScene,
    QuickSave, // <-- ДОБАВЛЕНО
    QuickLoad, // <-- ДОБАВЛЕНО

    // --- Edit ---
    Undo,
    Redo,
    Copy,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    InvertSelection,
    DeselectAll,

    // --- Camera ---
    FocusSelected,

    // --- Play ---
    TogglePlay,

    // --- Gizmo ---
    GizmoTranslate,
    GizmoRotate,
    GizmoScale,
    ToggleSnap,

    // --- UI ---
    CommandPalette,
    ToggleSettings,
    ToggleContentBrowser,

    // --- Bookmarks ---
    SaveBookmark(u8), // 0..=8 (UI показывает 1..=9)
    GotoBookmark(u8), // 0..=8
}

impl HotkeyAction {
    /// Все действия в стабильном порядке (для UI).
    pub const ALL: &'static [HotkeyAction] = &[
        // File
        HotkeyAction::Save,
        HotkeyAction::LoadScene,
        HotkeyAction::NewScene,
        HotkeyAction::QuickSave,
        HotkeyAction::QuickLoad,
        // Edit
        HotkeyAction::Undo,
        HotkeyAction::Redo,
        HotkeyAction::Copy,
        HotkeyAction::Paste,
        HotkeyAction::Duplicate,
        HotkeyAction::Delete,
        HotkeyAction::SelectAll,
        HotkeyAction::InvertSelection,
        HotkeyAction::DeselectAll,
        // Camera
        HotkeyAction::FocusSelected,
        // Play
        HotkeyAction::TogglePlay,
        // Gizmo
        HotkeyAction::GizmoTranslate,
        HotkeyAction::GizmoRotate,
        HotkeyAction::GizmoScale,
        HotkeyAction::ToggleSnap,
        // UI
        HotkeyAction::CommandPalette,
        HotkeyAction::ToggleSettings,
        HotkeyAction::ToggleContentBrowser,
        // Bookmarks — 9 слотов
        HotkeyAction::SaveBookmark(0),
        HotkeyAction::SaveBookmark(1),
        HotkeyAction::SaveBookmark(2),
        HotkeyAction::SaveBookmark(3),
        HotkeyAction::SaveBookmark(4),
        HotkeyAction::SaveBookmark(5),
        HotkeyAction::SaveBookmark(6),
        HotkeyAction::SaveBookmark(7),
        HotkeyAction::SaveBookmark(8),
        HotkeyAction::GotoBookmark(0),
        HotkeyAction::GotoBookmark(1),
        HotkeyAction::GotoBookmark(2),
        HotkeyAction::GotoBookmark(3),
        HotkeyAction::GotoBookmark(4),
        HotkeyAction::GotoBookmark(5),
        HotkeyAction::GotoBookmark(6),
        HotkeyAction::GotoBookmark(7),
        HotkeyAction::GotoBookmark(8),
    ];

    /// Категория для группировки в UI.
    pub fn category(self) -> &'static str {
        match self {
            HotkeyAction::Save
            | HotkeyAction::LoadScene
            | HotkeyAction::NewScene
            | HotkeyAction::QuickSave
            | HotkeyAction::QuickLoad => "File",
            HotkeyAction::Undo | HotkeyAction::Redo
            | HotkeyAction::Copy | HotkeyAction::Paste
            | HotkeyAction::Duplicate | HotkeyAction::Delete
            | HotkeyAction::SelectAll | HotkeyAction::InvertSelection
            | HotkeyAction::DeselectAll => "Edit",
            HotkeyAction::FocusSelected => "Camera",
            HotkeyAction::TogglePlay => "Play",
            HotkeyAction::GizmoTranslate | HotkeyAction::GizmoRotate
            | HotkeyAction::GizmoScale | HotkeyAction::ToggleSnap => "Gizmo",
            HotkeyAction::CommandPalette | HotkeyAction::ToggleSettings
            | HotkeyAction::ToggleContentBrowser => "UI",
            HotkeyAction::SaveBookmark(_) | HotkeyAction::GotoBookmark(_) => "Bookmarks",
        }
    }

    /// Человекочитаемое имя для UI.
    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::Save => "Save Scene",
            HotkeyAction::LoadScene => "Load Scene",
            HotkeyAction::NewScene => "New Scene",
            HotkeyAction::QuickSave => "Quick Save",
            HotkeyAction::QuickLoad => "Quick Load",
            HotkeyAction::Undo => "Undo",
            HotkeyAction::Redo => "Redo",
            HotkeyAction::Copy => "Copy",
            HotkeyAction::Paste => "Paste",
            HotkeyAction::Duplicate => "Duplicate",
            HotkeyAction::Delete => "Delete",
            HotkeyAction::SelectAll => "Select All",
            HotkeyAction::InvertSelection => "Invert Selection",
            HotkeyAction::DeselectAll => "Deselect All",
            HotkeyAction::FocusSelected => "Focus Selected",
            HotkeyAction::TogglePlay => "Play / Stop",
            HotkeyAction::GizmoTranslate => "Gizmo: Translate",
            HotkeyAction::GizmoRotate => "Gizmo: Rotate",
            HotkeyAction::GizmoScale => "Gizmo: Scale",
            HotkeyAction::ToggleSnap => "Toggle Snap",
            HotkeyAction::CommandPalette => "Command Palette",
            HotkeyAction::ToggleSettings => "Toggle Settings",
            HotkeyAction::ToggleContentBrowser => "Toggle Content Browser",
            HotkeyAction::SaveBookmark(_) => "Save Bookmark",
            HotkeyAction::GotoBookmark(_) => "Go Bookmark",
        }
    }

    /// Суффикс для bookmark-действий (`Save Bookmark 3`).
    pub fn bookmark_slot(self) -> Option<u8> {
        match self {
            HotkeyAction::SaveBookmark(i) => Some(i),
            HotkeyAction::GotoBookmark(i) => Some(i),
            _ => None,
        }
    }
}

// ============================================================
// HotkeyBinding
// ============================================================

/// Одна привязка: `Ctrl+Shift+S`, `F9`, `Delete`.
///
/// `Key` — сериализуемый аналог `KeyCode` (см. `engine::input_keys`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub key: Key,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

impl HotkeyBinding {
    pub fn new(key: Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: false }
    }

    pub fn ctrl(key: Key) -> Self {
        Self { key, ctrl: true, shift: false, alt: false }
    }

    pub fn ctrl_shift(key: Key) -> Self {
        Self { key, ctrl: true, shift: true, alt: false }
    }

    pub fn alt(key: Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: true }
    }

    pub fn plain(key: Key) -> Self {
        Self::new(key)
    }

    /// Строковое представление для UI: `Ctrl+Shift+Z`.
    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.ctrl { s.push_str("Ctrl+"); }
        if self.shift { s.push_str("Shift+"); }
        if self.alt { s.push_str("Alt+"); }
        s.push_str(self.key.display_name());
        s
    }
}

// ============================================================
// HotkeyMap
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyMap {
    /// Пара `(действие, привязка)`. Может быть несколько привязок
    /// на одно действие; `set` заменяет первую.
    #[serde(default)]
    pub bindings: Vec<(HotkeyAction, HotkeyBinding)>,
}

impl Default for HotkeyMap {
    fn default() -> Self {
        Self { bindings: Self::default_bindings() }
    }
}

impl HotkeyMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Стандартный набор.
    pub fn default_bindings() -> Vec<(HotkeyAction, HotkeyBinding)> {
        use HotkeyAction::*;
        use winit::keyboard::KeyCode as KC;

        vec![
            // File
            (Save, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyS))),
            (LoadScene, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyO))),
            (NewScene, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyN))),
            (QuickSave, HotkeyBinding::plain(Key::from_keycode(KC::F5))),
            (QuickLoad, HotkeyBinding::plain(Key::from_keycode(KC::F9))),

            // Edit
            (Undo, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyZ))),
            (Redo, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyY))),
            (Copy, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyC))),
            (Paste, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyV))),
            (Duplicate, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyD))),
            (Delete, HotkeyBinding::plain(Key::from_keycode(KC::Delete))),
            (SelectAll, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyA))),
            (InvertSelection, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyI))),
            (DeselectAll, HotkeyBinding::ctrl_shift(Key::from_keycode(KC::KeyA))),

            // Camera
            (FocusSelected, HotkeyBinding::plain(Key::from_keycode(KC::KeyF))),

            // Play (F9 сдвинут на F10, так как F9 теперь QuickLoad)
            (TogglePlay, HotkeyBinding::plain(Key::from_keycode(KC::F10))),

            // Gizmo
            (GizmoTranslate, HotkeyBinding::plain(Key::from_keycode(KC::Digit1))),
            (GizmoRotate, HotkeyBinding::plain(Key::from_keycode(KC::Digit2))),
            (GizmoScale, HotkeyBinding::plain(Key::from_keycode(KC::Digit3))),
            (ToggleSnap, HotkeyBinding::plain(Key::from_keycode(KC::KeyX))),

            // UI
            (CommandPalette, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyP))),
            (ToggleSettings, HotkeyBinding::ctrl(Key::from_keycode(KC::Comma))),
            (ToggleContentBrowser, HotkeyBinding::ctrl(Key::from_keycode(KC::KeyB))),

            // Bookmarks — Ctrl+1..9, Alt+1..9
            (SaveBookmark(0), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit1))),
            (SaveBookmark(1), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit2))),
            (SaveBookmark(2), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit3))),
            (SaveBookmark(3), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit4))),
            (SaveBookmark(4), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit5))),
            (SaveBookmark(5), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit6))),
            (SaveBookmark(6), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit7))),
            (SaveBookmark(7), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit8))),
            (SaveBookmark(8), HotkeyBinding::ctrl(Key::from_keycode(KC::Digit9))),
            (GotoBookmark(0), HotkeyBinding::alt(Key::from_keycode(KC::Digit1))),
            (GotoBookmark(1), HotkeyBinding::alt(Key::from_keycode(KC::Digit2))),
            (GotoBookmark(2), HotkeyBinding::alt(Key::from_keycode(KC::Digit3))),
            (GotoBookmark(3), HotkeyBinding::alt(Key::from_keycode(KC::Digit4))),
            (GotoBookmark(4), HotkeyBinding::alt(Key::from_keycode(KC::Digit5))),
            (GotoBookmark(5), HotkeyBinding::alt(Key::from_keycode(KC::Digit6))),
            (GotoBookmark(6), HotkeyBinding::alt(Key::from_keycode(KC::Digit7))),
            (GotoBookmark(7), HotkeyBinding::alt(Key::from_keycode(KC::Digit8))),
            (GotoBookmark(8), HotkeyBinding::alt(Key::from_keycode(KC::Digit9))),
        ]
    }

    // ============================================================
    // Query
    // ============================================================

    /// Первая привязка для действия (или `None`).
    pub fn get(&self, action: HotkeyAction) -> Option<HotkeyBinding> {
        self.bindings.iter().find(|(a, _)| *a == action).map(|(_, b)| *b)
    }

    /// Все привязки для действия.
    pub fn get_all(&self, action: HotkeyAction) -> Vec<HotkeyBinding> {
        self.bindings.iter()
            .filter(|(a, _)| *a == action)
            .map(|(_, b)| *b)
            .collect()
    }

    // ============================================================
    // Mutation
    // ============================================================

    /// Заменить первую привязку действия. Если действие не привязано —
    /// добавить.
    pub fn set(&mut self, action: HotkeyAction, binding: HotkeyBinding) {
        // Удаляем дубликаты binding'а у других действий — иначе
        // нажатие одной клавиши вызовет два конфликтующих действия.
        self.bindings.retain(|(a, b)| *a == action || *b != binding);

        // Заменяем первую привязку действия.
        for (a, b) in self.bindings.iter_mut() {
            if *a == action {
                *b = binding;
                return;
            }
        }
        // Если не нашли — добавляем.
        self.bindings.push((action, binding));
    }

    /// Удалить все привязки действия.
    pub fn remove(&mut self, action: HotkeyAction) {
        self.bindings.retain(|(a, _)| *a != action);
    }

    /// Сбросить одно действие к default.
    pub fn reset(&mut self, action: HotkeyAction) {
        self.remove(action);
        for (a, b) in Self::default_bindings() {
            if a == action {
                self.bindings.push((a, b));
            }
        }
    }

    /// Сбросить всё.
    pub fn reset_all(&mut self) {
        self.bindings = Self::default_bindings();
    }

    // ============================================================
    // Poll
    // ============================================================

    /// Все действия, чьи привязки сработали в этом кадре.
    pub fn poll(&self, input: &Input) -> Vec<HotkeyAction> {
        let mut out = Vec::new();
        for (action, binding) in &self.bindings {
            if Self::matches(binding, input) {
                out.push(*action);
            }
        }
        out
    }

    fn matches(binding: &HotkeyBinding, input: &Input) -> bool {
        let Some(kc) = binding.key.to_keycode() else {
            return false;
        };
        if !input.key_pressed(kc) {
            return false;
        }
        let ctrl = input.key_down(KeyCode::ControlLeft)
            || input.key_down(KeyCode::ControlRight);
        let shift = input.key_down(KeyCode::ShiftLeft)
            || input.key_down(KeyCode::ShiftRight);
        let alt = input.key_down(KeyCode::AltLeft) || input.key_down(KeyCode::AltRight);
        ctrl == binding.ctrl && shift == binding.shift && alt == binding.alt
    }
}