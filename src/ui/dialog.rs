//! UI рендеринг диалогового окна (с поддержкой условий и локализации).
//!
//! ИЗМЕНЕНО: высота панели теперь зависит от числа доступных опций.
//! Раньше было жёстко 250 px, а у `elder.en.ron` в `root` бывает 7–8
//! опций → кнопки уезжали за панель. Теперь высота = min(нужно, sh-100).

use crate::game::dialogs::{DialogContext, DialogSession};
use crate::t;
use crate::ui::UiLayer;

pub const CLOSE_SENTINEL: usize = usize::MAX;

pub struct DialogUi;

impl DialogUi {
    /// Возвращает `Some(original_idx)` при клике на опцию,
    /// `Some(CLOSE_SENTINEL)` — при закрытии, `None` — если ничего не нажато.
    pub fn draw(
        ui: &mut UiLayer,
        session: &DialogSession,
        ctx: &DialogContext,
    ) -> Option<usize> {
        let Some(node) = session.node() else {
            return Some(CLOSE_SENTINEL);
        };

        let sw = ui.screen_w();
        let sh = ui.screen_h();

        // Опции считаем заранее — от них зависит высота панели.
        let options = session.available_options(ctx);
        let opt_count = options.len().max(1);

        let panel_w = (sw * 0.75).min(920.0);

        // 92 (шапка+текст) + опции + 30 (подсказка) + отступ.
        let needed = 92.0 + opt_count as f32 * 32.0 + 30.0;
        let panel_h = needed.clamp(220.0, sh - 100.0);
        let panel_x = (sw - panel_w) * 0.5;
        let panel_y = sh - panel_h - 40.0;

        // Затемнение фона.
        ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.30]);

        // Внешняя рамка.
        ui.rect(
            panel_x - 2.0, panel_y - 2.0,
            panel_w + 4.0, panel_h + 4.0,
            [0.55, 0.60, 0.75, 1.0],
        );
        // Внутренняя панель.
        ui.rect(panel_x, panel_y, panel_w, panel_h, [0.10, 0.12, 0.16, 0.98]);

        // Спикер.
        ui.text(panel_x + 20.0, panel_y + 14.0, &node.speaker, 20.0,
            [1.0, 0.9, 0.5, 1.0]);

        // Кнопка закрытия.
        let close_size = 26.0;
        let close_x = panel_x + panel_w - close_size - 10.0;
        let close_y = panel_y + 10.0;
        if ui.button(close_x, close_y, close_size, close_size, "X") {
            return Some(CLOSE_SENTINEL);
        }

        // Разделитель.
        ui.rect(panel_x + 16.0, panel_y + 46.0, panel_w - 32.0, 1.0,
            [0.4, 0.45, 0.55, 0.8]);

        // Текст реплики.
        ui.text(panel_x + 20.0, panel_y + 60.0, &node.text, 15.0,
            [1.0, 1.0, 1.0, 1.0]);

        // Опции.
        let mut y = panel_y + 92.0;
        let mut clicked: Option<usize> = None;
        let opt_h = 28.0;
        let list_bottom = panel_y + panel_h - 30.0;

        for (display_i, (original_idx, opt)) in options.iter().enumerate() {
            // Защита от overflow: если не влезает, дальше не рисуем.
            // При большом числе опций это значит, что часть не видна —
            // но не «уезжает за панель» и не ломает клики по панели.
            if y + opt_h > list_bottom {
                break;
            }
            let btn_x = panel_x + 20.0;
            let btn_w = panel_w - 40.0;
            let label = format!("{}.  {}", display_i + 1, opt.text);
            if ui.button(btn_x, y, btn_w, opt_h, &label) {
                clicked = Some(*original_idx);
            }
            y += opt_h + 4.0;
        }

        if options.is_empty() {
            ui.text_centered(
                sw * 0.5,
                panel_y + 120.0,
                t!("dialog.no_options"),
                12.0,
                [0.7, 0.7, 0.7, 0.9],
            );
        }

        ui.text_centered(
            sw * 0.5,
            panel_y + panel_h - 14.0,
            t!("dialog.close_hint"),
            10.0,
            [0.6, 0.65, 0.75, 0.8],
        );

        clicked
    }
}