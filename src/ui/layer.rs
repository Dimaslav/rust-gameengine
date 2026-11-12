use std::sync::Arc;

use super::font::Font;
use super::quad::UiQuad;

#[derive(Debug, Clone, Copy, Default)]
pub struct UiModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UiInput {
    pub mouse_pos: (f32, f32),
    pub mouse_clicked: bool,
    pub mouse_down: bool,
    pub modifiers: UiModifiers,
}

pub struct UiLayer {
    quads: Vec<UiQuad>,
    screen_w: f32,
    screen_h: f32,
    input: UiInput,
    font: Arc<Font>,
    pub consumed_click: bool,
    next_widget_id: u32,
}

impl UiLayer {
    pub fn new(
        screen_w: f32,
        screen_h: f32,
        input: UiInput,
        font: Arc<Font>,
    ) -> Self {
        Self {
            quads: Vec::with_capacity(256),
            screen_w,
            screen_h,
            input,
            font,
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

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        if w <= 0.0 || h <= 0.0 || color[3] <= 0.0 { return; }
        self.quads.push(UiQuad {
            rect: [x, y, w, h],
            color,
            uv: self.font.white_uv,
        });
    }

    pub fn rect_outline(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: [f32; 4]) {
        let t = thickness.max(0.5);
        self.rect(x, y, w, t, color);
        self.rect(x, y + h - t, w, t, color);
        self.rect(x, y + t, t, h - 2.0 * t, color);
        self.rect(x + w - t, y + t, t, h - 2.0 * t, color);
    }

    pub fn text(&mut self, x: f32, y: f32, text: &str, size_px: f32, color: [f32; 4]) {
        if size_px <= 0.0 || color[3] <= 0.0 { return; }
        let line_h = self.font.line_height(size_px);
        let mut cx = x;
        let mut cy = y;
        for c in text.chars() {
            if c == '\n' {
                cx = x;
                cy += line_h;
                continue;
            }
            match self.font.glyph(c, size_px) {
                Some(g) => {
                    if g.draw_w > 0.0 && g.draw_h > 0.0 && c != ' ' && c != '\t' {
                        self.quads.push(UiQuad {
                            rect: [cx + g.offset_x, cy + g.offset_y, g.draw_w, g.draw_h],
                            color,
                            uv: g.uv,
                        });
                    }
                    cx += g.advance;
                }
                None => {
                    // Неизвестный символ — сдвигаем на «среднюю» ширину.
                    cx += size_px * 0.4;
                }
            }
        }
    }

    pub fn text_centered(&mut self, cx: f32, cy: f32, text: &str, size_px: f32, color: [f32; 4]) {
        if size_px <= 0.0 { return; }
        let (w, h) = self.font.measure(text, size_px);
        let x = cx - w * 0.5;
        let y = cy - h * 0.5;
        self.text(x, y, text, size_px, color);
    }

    pub fn text_width(&self, text: &str, size_px: f32) -> f32 {
        self.font.measure(text, size_px).0
    }

    // ============================================================
    // Интерактивные элементы (без изменений)
    // ============================================================

    fn is_hovered(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let (mx, my) = self.input.mouse_pos;
        mx >= x && mx < x + w && my >= y && my < y + h
    }

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
        if clicked { self.consumed_click = true; }
        clicked
    }

    pub fn bar(
        &mut self, x: f32, y: f32, w: f32, h: f32,
        frac: f32, fg: [f32; 4], bg: [f32; 4],
    ) {
        let f = frac.clamp(0.0, 1.0);
        self.rect(x, y, w, h, bg);
        self.rect(x, y, w * f, h, fg);
        self.rect_outline(x, y, w, h, 1.0, [0.0, 0.0, 0.0, 0.7]);
    }

    pub fn crosshair(&mut self, cx: f32, cy: f32, size: f32, thickness: f32, color: [f32; 4]) {
        let t = thickness.max(1.0);
        self.rect(cx - size, cy - t * 0.5, size * 2.0, t, color);
        self.rect(cx - t * 0.5, cy - size, t, size * 2.0, color);
    }
}