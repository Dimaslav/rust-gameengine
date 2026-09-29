use std::collections::HashSet;
use winit::event::{ElementState, KeyEvent, MouseButton};
use winit::keyboard::KeyCode;

#[derive(Default)]
pub struct Input {
    keys_down: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    keys_released: HashSet<KeyCode>,
    mouse_buttons_down: HashSet<MouseButton>,
    pub mouse_pos: (f32, f32),
    /// Смещение курсора за кадр — для orbit-камеры.
    pub mouse_delta: (f32, f32),
    /// Колесо прокрутки за кадр.
    pub scroll_delta: f32,
    /// Первое событие CursorMoved после входа/выхода курсора.
    /// Пока true — не добавляем в delta (иначе первый кадр даст скачок
    /// от старой позиции к текущей).
    first_move: bool,
}

impl Input {
    pub fn new() -> Self {
        Self {
            first_move: true,
            ..Default::default()
        }
    }

    pub fn on_key(&mut self, event: &KeyEvent) {
        let code = match event.physical_key {
            winit::keyboard::PhysicalKey::Code(c) => c,
            _ => return,
        };
        match event.state {
            ElementState::Pressed => {
                if !self.keys_down.contains(&code) {
                    self.keys_pressed.insert(code);
                }
                self.keys_down.insert(code);
            }
            ElementState::Released => {
                self.keys_down.remove(&code);
                self.keys_released.insert(code);
            }
        }
    }

    pub fn on_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        match state {
            ElementState::Pressed => {
                self.mouse_buttons_down.insert(button);
            }
            ElementState::Released => {
                self.mouse_buttons_down.remove(&button);
            }
        }
    }

    pub fn on_mouse_move(&mut self, x: f32, y: f32) {
        if self.first_move {
            // Просто запоминаем позицию, delta = 0.
            self.first_move = false;
        } else {
            self.mouse_delta.0 += x - self.mouse_pos.0;
            self.mouse_delta.1 += y - self.mouse_pos.1;
        }
        self.mouse_pos = (x, y);
    }

    /// Вызывать на `CursorEntered`/`CursorLeft`, чтобы следующий move
    /// не дал скачок относительно «старой» позиции.
    pub fn on_cursor_enter(&mut self) {
        self.first_move = true;
    }

    pub fn on_scroll(&mut self, delta: f32) {
        self.scroll_delta += delta;
    }

    pub fn key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.keys_pressed.contains(&key)
    }
    pub fn key_released(&self, key: KeyCode) -> bool {
        self.keys_released.contains(&key)
    }
    pub fn mouse_down(&self, button: MouseButton) -> bool {
        self.mouse_buttons_down.contains(&button)
    }

    pub fn end_frame(&mut self) {
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = 0.0;
    }
}