//! UiLayer — immediate-mode API для построения игрового UI.
//!
//! Работает в screen-space пикселях (top-left origin). Игровой код
//! вызывает `ui.rect(...)`, `ui.text(...)`, `ui.button(...)` из
//! `Game::collect_ui` — каждая функция добавляет квады и (для
//! интерактивных элементов) возвращает interaction-статус.

use super::font;
use super::quad::UiQuad;

/// Состояние мыши, нужное для interaction.
#[derive(Debug, Clone, Copy)]
pub struct UiInput {
    pub mouse_pos: (f32, f32),
    pub mouse_clicked: bool,
    pub mouse_down: bool,
}

pub struct UiLayer {
    quads: Vec<UiQuad>,
    screen_w: f32,
    screen_h: f32,
    input: UiInput,
    /// Было ли interaction-событие (клик) потреблено UI в этом кадре.
    /// Игровой код может использовать это, чтобы не дублировать клик
    /// в 3D-сцену.
    pub consumed_click: bool,
    /// Уникальный id для текущего интерактивного виджета — пригодится
    /// для отладки и будущего widget tree.
    next_widget_id: u32,
}

impl UiLayer {
    pub fn new(screen_w: f32, screen_h: f32, input: UiInput) -> Self {
        Self {
            quads: Vec::with_capacity(256),
            screen_w,
            screen_h,
            input,
            consumed_click: false,
            next_widget_id: 0,
        }
    }

    pub fn screen_w(&self) -> f32 { self.screen_w }
    pub fn screen_h(&self) -> f32 { self.screen_h }
    pub fn input(&self) -> UiInput { self.input }

    pub fn quads(&self) -> &[UiQuad] { &self.quads }
    pub fn into_quads(self) -> Vec<UiQuad> { self.quads }

    pub fn clear(&mut self) { self.quads.clear(); }

    // ============================================================
    // Примитивы
    // ============================================================

    /// Сплошной прямоугольник.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        if w <= 0.0 || h <= 0.0 || color[3] <= 0.0 { return; }
        self.quads.push(UiQuad {
            rect: [x, y, w, h],
            color,
            uv: font::white_uv(),
        });
    }

    /// Рамка (4 прямоугольника).
    pub fn rect_outline(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: [f32; 4]) {
        let t = thickness.max(0.5);
        self.rect(x, y, w, t, color);
        self.rect(x, y + h - t, w, t, color);
        self.rect(x, y + t, t, h - 2.0 * t, color);
        self.rect(x + w - t, y + t, t, h - 2.0 * t, color);
    }

    /// Текст. `size_px` — высота глифа. Оригинал 5×7.
    pub fn text(&mut self, x: f32, y: f32, text: &str, size_px: f32, color: [f32; 4]) {
        if size_px <= 0.0 || color[3] <= 0.0 { return; }
        let glyph_w = size_px * font::GLYPH_W as f32 / font::GLYPH_H as f32;
        let mut cx = x;
        for c in text.chars() {
            if c == ' ' {
                cx += glyph_w;
                continue;
            }
            let uv = font::glyph_uv(c);
            self.quads.push(UiQuad {
                rect: [cx, y, glyph_w, size_px],
                color,
                uv,
            });
            cx += glyph_w;
        }
    }

    /// Текст, центрированный по `(cx, cy)`.
    pub fn text_centered(&mut self, cx: f32, cy: f32, text: &str, size_px: f32, color: [f32; 4]) {
        if size_px <= 0.0 { return; }
        let glyph_w = size_px * font::GLYPH_W as f32 / font::GLYPH_H as f32;
        let n = text.chars().count() as f32;
        let total_w = n * glyph_w;
        let x = cx - total_w * 0.5;
        let y = cy - size_px * 0.5;
        self.text(x, y, text, size_px, color);
    }

    /// Ширина текста в пикселях.
    pub fn text_width(&self, text: &str, size_px: f32) -> f32 {
        let glyph_w = size_px * font::GLYPH_W as f32 / font::GLYPH_H as f32;
        text.chars().count() as f32 * glyph_w
    }

    // ============================================================
    // Интерактивные элементы
    // ============================================================

    fn is_hovered(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let (mx, my) = self.input.mouse_pos;
        mx >= x && mx < x + w && my >= y && my < y + h
    }

    /// Кнопка. Возвращает `true` если кликнута в этом кадре.
    pub fn button(&mut self, x: f32, y: f32, w: f32, h: f32, label: &str) -> bool {
        self.next_widget_id += 1;

        let hovered = self.is_hovered(x, y, w, h);
        let pressed = hovered && self.input.mouse_down;

        let bg = if pressed {
            [0.12, 0.12, 0.18, 0.95]
        } else if hovered {
            [0.28, 0.30, 0.36, 0.95]
        } else {
            [0.18, 0.20, 0.26, 0.95]
        };

        self.rect(x, y, w, h, bg);
        self.rect_outline(x, y, w, h, 1.5, [0.55, 0.60, 0.75, 1.0]);

        let text_size = h.min(24.0) * 0.55;
        self.text_centered(x + w * 0.5, y + h * 0.5, label, text_size, [1.0, 1.0, 1.0, 1.0]);

        let clicked = hovered && self.input.mouse_clicked;
        if clicked {
            self.consumed_click = true;
        }
        clicked
    }

    /// Прогресс-бар. `frac` в [0, 1].
    pub fn bar(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        frac: f32,
        fg: [f32; 4],
        bg: [f32; 4],
    ) {
        let f = frac.clamp(0.0, 1.0);
        self.rect(x, y, w, h, bg);
        self.rect(x, y, w * f, h, fg);
        self.rect_outline(x, y, w, h, 1.0, [0.0, 0.0, 0.0, 0.7]);
    }

    /// Прицел — четыре линии крестом.
    pub fn crosshair(&mut self, cx: f32, cy: f32, size: f32, thickness: f32, color: [f32; 4]) {
        let t = thickness.max(1.0);
        self.rect(cx - size, cy - t * 0.5, size * 2.0, t, color);
        self.rect(cx - t * 0.5, cy - size, t, size * 2.0, color);
    }
}