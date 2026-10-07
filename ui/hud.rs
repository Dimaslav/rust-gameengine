//! Высокоуровневые HUD-хелперы: health bar, ammo, notifications, crosshair.

use super::layer::UiLayer;

pub struct Notification {
    pub text: String,
    pub ttl: f32,
    pub max_ttl: f32,
}

pub struct Hud {
    pub notifications: Vec<Notification>,
}

impl Hud {
    pub fn new() -> Self {
        Self { notifications: Vec::new() }
    }

    pub fn push(&mut self, text: impl Into<String>) {
        self.notifications.push(Notification {
            text: text.into(),
            ttl: 3.0,
            max_ttl: 3.0,
        });
    }

    pub fn tick(&mut self, dt: f32) {
        for n in &mut self.notifications {
            n.ttl -= dt;
        }
        self.notifications.retain(|n| n.ttl > 0.0);
    }

    /// Health bar в **ЛЕВОМ** нижнем углу. `x = 24.0`.
    pub fn health_bar(&self, ui: &mut UiLayer, current: f32, max: f32) {
        let w = 220.0;
        let h = 22.0;
        let x = 24.0;                              // ← ЛЕВЫЙ КРАЙ
        let y = ui.screen_h() - 24.0 - h;

        let frac = if max > 0.0 { (current / max).clamp(0.0, 1.0) } else { 0.0 };
        let color = if frac > 0.5 { [0.2, 0.85, 0.3, 1.0] }
                    else if frac > 0.25 { [0.95, 0.8, 0.2, 1.0] }
                    else { [0.9, 0.25, 0.2, 1.0] };

        ui.rect(x - 6.0, y - 6.0, w + 12.0, h + 12.0, [0.0, 0.0, 0.0, 0.55]);
        ui.bar(x, y, w, h, frac, color, [0.1, 0.1, 0.1, 0.9]);

        let label = format!("HP  {:.0} / {:.0}", current, max);
        ui.text_centered(x + w * 0.5, y + h * 0.5, &label, 16.0, [1.0, 1.0, 1.0, 1.0]);
    }

    /// Legacy метод. **Не вызывать** в FortressDemo — он рисует
    /// старую ammo-панель без резерва. Оставлен для совместимости.
    pub fn ammo(&self, ui: &mut UiLayer, current: u32, max: u32) {
        let text = format!("AMMO {:>3} / {:<3}", current, max);
        let size = 22.0;
        let text_w = ui.text_width(&text, size);
        let padding = 10.0;
        let box_w = text_w + padding * 2.0;
        let box_h = size + padding * 1.4;
        let x = ui.screen_w() - 24.0 - box_w;
        let y = ui.screen_h() - 24.0 - box_h;

        ui.rect(x, y, box_w, box_h, [0.0, 0.0, 0.0, 0.55]);
        ui.text(x + padding, y + (box_h - size) * 0.5, &text, size, [1.0, 1.0, 0.85, 1.0]);
    }

    pub fn crosshair(&self, ui: &mut UiLayer, highlight: bool) {
        let cx = ui.screen_w() * 0.5;
        let cy = ui.screen_h() * 0.5;
        let color = if highlight { [1.0, 0.85, 0.3, 0.9] }
                    else { [1.0, 1.0, 1.0, 0.85] };
        let size = if highlight { 10.0 } else { 8.0 };
        ui.crosshair(cx, cy, size, 1.5, color);
    }

    pub fn notifications(&self, ui: &mut UiLayer) {
        let size = 16.0;
        let padding = 8.0;
        let spacing = 6.0;
        let mut y = 24.0;

        for n in &self.notifications {
            let alpha = (n.ttl / n.max_ttl).min(1.0);
            let text_w = ui.text_width(&n.text, size);
            let box_w = text_w + padding * 2.0;
            let box_h = size + padding * 1.4;
            let x = ui.screen_w() - 24.0 - box_w;

            ui.rect(x, y, box_w, box_h, [0.0, 0.0, 0.0, 0.6 * alpha]);
            ui.text(x + padding, y + (box_h - size) * 0.5,
                &n.text, size, [1.0, 1.0, 0.9, alpha]);

            y += box_h + spacing;
        }
    }

    pub fn debug_overlay(&self, ui: &mut UiLayer, fps: f32, entities: usize,
                         camera_pos: glam::Vec3) {
        let size = 14.0;
        let mut y = 24.0;
        let x = 24.0;

        let lines = [
            format!("FPS {:.1}", fps),
            format!("Entities {}", entities),
            format!("Pos {:.1} {:.1} {:.1}", camera_pos.x, camera_pos.y, camera_pos.z),
        ];

        let box_w = 200.0;
        let box_h = lines.len() as f32 * (size + 4.0) + 12.0;
        ui.rect(x - 6.0, y - 6.0, box_w, box_h, [0.0, 0.0, 0.0, 0.55]);

        for line in &lines {
            ui.text(x, y, line, size, [0.9, 1.0, 0.9, 1.0]);
            y += size + 4.0;
        }
    }
}

impl Default for Hud {
    fn default() -> Self { Self::new() }
}