//! UI рендеринг диалогового окна.

use crate::game::dialogs::DialogSession;
use crate::ui::UiLayer;

/// Sentinel: пользователь нажал "Close", а не выбрал опцию.
pub const CLOSE_SENTINEL: usize = usize::MAX;

pub struct DialogUi;

impl DialogUi {
    /// Рисует окно диалога внизу экрана.
    ///
    /// Возвращает:
    ///   * `None` — игрок ещё ничего не выбрал;
    ///   * `Some(idx)` — выбрана опция `idx`;
    ///   * `Some(CLOSE_SENTINEL)` — нажат крестик (закрыть диалог).
    pub fn draw(ui: &mut UiLayer, session: &DialogSession) -> Option<usize> {
        let Some(node) = session.node() else {
            return Some(CLOSE_SENTINEL);
        };

        let sw = ui.screen_w();
        let sh = ui.screen_h();

        // Размеры панели.
        let panel_w = (sw * 0.75).min(920.0);
        let panel_h = 230.0;
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

        // Кнопка закрытия (X) справа сверху.
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

        let opt_h = 30.0;
        for (i, opt) in node.options.iter().enumerate() {
            let btn_x = panel_x + 20.0;
            let btn_w = panel_w - 40.0;

            let label = format!("{}.  {}", i + 1, opt.text);
            if ui.button(btn_x, y, btn_w, opt_h, &label) {
                clicked = Some(i);
            }
            y += opt_h + 4.0;
        }

        // Подсказка.
        ui.text_centered(
            sw * 0.5,
            panel_y + panel_h - 14.0,
            "ЛКМ — выбор · X — закрыть",
            10.0,
            [0.6, 0.65, 0.75, 0.8],
        );

        clicked
    }
}