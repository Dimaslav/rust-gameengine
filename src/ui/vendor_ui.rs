//! UI окна вендора: список товаров + инвентарь игрока + золото.
//!
//! Управление:
//! - **ЛКМ на товар** — купить (если хватает золота и есть stock).
//! - **ЛКМ на предмет инвентаря** — продать (по `item.sell_price`).
//! - **Закрыть** — `X` справа сверху, `Esc` снаружи (в `fortress.rs`).

use crate::game::items::{Equipment, Inventory, ItemRegistry};
use crate::game::vendors::{VendorDef, VendorSession};
use crate::ui::UiLayer;

// ============================================================
// Layout
// ============================================================

const PANEL_W: f32 = 760.0;
const PANEL_H: f32 = 520.0;
const PAD: f32 = 20.0;
const ROW_H: f32 = 44.0;
const ROW_GAP: f32 = 4.0;
const COL_HALF_W: f32 = (PANEL_W - PAD * 2.0 - PAD) * 0.5;

// ============================================================
// Действия
// ============================================================

pub enum VendorAction {
    None,
    Buy(usize),
    Sell(usize),
    Close,
}

// ============================================================
// Публичный вход
// ============================================================

pub fn draw(
    ui: &mut UiLayer,
    session: &VendorSession,
    def: &VendorDef,
    registry: &ItemRegistry,
    inventory: &Inventory,
    _equipment: &Equipment,
    gold: u32,
) -> VendorAction {
    let sw = ui.screen_w();
    let sh = ui.screen_h();
    let px = (sw - PANEL_W) * 0.5;
    let py = (sh - PANEL_H) * 0.5;

    // Фон + рамка.
    ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.5]);
    ui.rect(px - 2.0, py - 2.0, PANEL_W + 4.0, PANEL_H + 4.0, [0.55, 0.60, 0.75, 1.0]);
    ui.rect(px, py, PANEL_W, PANEL_H, [0.13, 0.15, 0.20, 0.98]);

    // Шапка.
    let title = format!("{}  ·  {} items", def.title, def.items.len());
    ui.text_centered(sw * 0.5, py + 20.0, &title, 22.0, [1.0, 0.9, 0.6, 1.0]);

    let gold_text = format!("Gold: {}  ·  spent {}  ·  earned {}", gold, session.spent, session.earned);
    ui.text_centered(sw * 0.5, py + 46.0, &gold_text, 12.0, [1.0, 0.85, 0.4, 1.0]);

    // Кнопка закрытия.
    let close_size = 26.0;
    let close_x = px + PANEL_W - close_size - 10.0;
    let close_y = py + 10.0;
    let mut action = VendorAction::None;
    if ui.button(close_x, close_y, close_size, close_size, "X") {
        action = VendorAction::Close;
    }

    let content_y = py + 80.0;

    // ============ Левая колонка: сток ============
    let lx = px + PAD;
    ui.text(lx, content_y, "BUY", 14.0, [0.9, 0.9, 0.5, 1.0]);
    ui.rect(lx, content_y + 20.0, COL_HALF_W, 1.0, [0.4, 0.45, 0.55, 0.8]);

    let mut y = content_y + 30.0;
    for (idx, vitem) in def.items.iter().enumerate() {
        let row_rect = (lx, y, COL_HALF_W, ROW_H);
        let hovered = point_in_rect(ui.input().mouse_pos, row_rect);
        let stock = session.stock.get(idx).copied().unwrap_or(0);
        let sold_out = stock == 0;
        let item = registry.get_by_id(&vitem.item_id);
        let name = item.map(|i| i.name.clone()).unwrap_or_else(|| vitem.item_id.clone());
        let rarity_color = item.map(|i| i.rarity.color()).unwrap_or([0.75, 0.75, 0.75, 1.0]);
        let afford = gold >= vitem.price;

        draw_stock_row(
            ui, row_rect, &name, vitem.price, stock, sold_out, afford, hovered, rarity_color,
        );

        if hovered
            && ui.input().mouse_clicked
            && !sold_out
            && afford
        {
            action = VendorAction::Buy(idx);
        }

        y += ROW_H + ROW_GAP;
    }

    // ============ Правая колонка: инвентарь ============
    let rx = lx + COL_HALF_W + PAD;
    ui.text(rx, content_y, "SELL", 14.0, [0.9, 0.9, 0.5, 1.0]);
    ui.rect(rx, content_y + 20.0, COL_HALF_W, 1.0, [0.4, 0.45, 0.55, 0.8]);

    let mut y = content_y + 30.0;
    for (idx, slot) in inventory.slots.iter().enumerate() {
        let Some(stack) = slot.as_ref() else { continue };
        let item = registry.get_by_id(&stack.item_id);
        let name = item.map(|i| i.name.clone()).unwrap_or_else(|| stack.item_id.clone());
        let price = item.map(|i| i.sell_price).unwrap_or(0);
        let rarity_color = item.map(|i| i.rarity.color()).unwrap_or([0.75, 0.75, 0.75, 1.0]);
        if price == 0 {
            // Нельзя продать — показываем серым, но занимаем место.
            ui.text(rx, y + 12.0, &format!("  {} ×{} — no price", name, stack.count), 11.0,
                [0.4, 0.4, 0.4, 0.8]);
            y += 20.0;
            continue;
        }
        let row_rect = (rx, y, COL_HALF_W, ROW_H);
        let hovered = point_in_rect(ui.input().mouse_pos, row_rect);
        let can_sell = price > 0;

        draw_sell_row(
            ui, row_rect, &name, stack.count, price, can_sell, hovered, rarity_color,
        );

        if hovered
            && ui.input().mouse_clicked
            && can_sell
        {
            action = VendorAction::Sell(idx);
        }

        y += ROW_H + ROW_GAP;
        if y > py + PANEL_H - 40.0 { break; }
    }

    // Подсказка внизу.
    ui.text_centered(
        sw * 0.5, py + PANEL_H - 18.0,
        "LMB on wares to buy · LMB on inventory to sell · X/Esc to close",
        11.0, [0.6, 0.65, 0.75, 0.8],
    );

    action
}

// ============================================================
// Отрисовка строк
// ============================================================

#[allow(clippy::too_many_arguments)]
fn draw_stock_row(
    ui: &mut UiLayer,
    rect: (f32, f32, f32, f32),
    name: &str,
    price: u32,
    stock: i32,
    sold_out: bool,
    afford: bool,
    hovered: bool,
    rarity_color: [f32; 4],
) {
    let (x, y, w, h) = rect;
    let bg = if sold_out {
        [0.10, 0.10, 0.10, 0.9]
    } else if hovered && afford {
        [0.22, 0.28, 0.22, 0.95]
    } else if hovered {
        [0.30, 0.15, 0.15, 0.95]
    } else {
        [0.08, 0.10, 0.14, 0.95]
    };
    ui.rect(x, y, w, h, bg);
    ui.rect_outline(x, y, w, h, 1.0, [0.35, 0.4, 0.5, 1.0]);

    let text_color = if sold_out {
        [0.4, 0.4, 0.4, 0.8]
    } else {
        rarity_color
    };
    ui.text(x + 10.0, y + 8.0, name, 13.0, text_color);

    let stock_text = if stock < 0 {
        "∞".to_string()
    } else {
        format!("x{}", stock)
    };
    ui.text(x + 10.0, y + 26.0, &stock_text, 10.0, [0.6, 0.65, 0.75, 1.0]);

    let price_color = if sold_out {
        [0.4, 0.4, 0.4, 0.8]
    } else if afford {
        [1.0, 0.85, 0.4, 1.0]
    } else {
        [0.9, 0.35, 0.35, 1.0]
    };
    let price_text = format!("{} g", price);
    let pw = ui.text_width(&price_text, 14.0);
    ui.text(x + w - pw - 10.0, y + 14.0, &price_text, 14.0, price_color);
}

fn draw_sell_row(
    ui: &mut UiLayer,
    rect: (f32, f32, f32, f32),
    name: &str,
    count: u32,
    price: u32,
    can_sell: bool,
    hovered: bool,
    rarity_color: [f32; 4],
) {
    let (x, y, w, h) = rect;
    let bg = if hovered && can_sell {
        [0.22, 0.28, 0.22, 0.95]
    } else {
        [0.08, 0.10, 0.14, 0.95]
    };
    ui.rect(x, y, w, h, bg);
    ui.rect_outline(x, y, w, h, 1.0, [0.35, 0.4, 0.5, 1.0]);

    ui.text(x + 10.0, y + 8.0, name, 13.0, rarity_color);
    if count > 1 {
        ui.text(x + 10.0, y + 26.0, &format!("x{}", count), 10.0, [0.6, 0.65, 0.75, 1.0]);
    }

    let price_text = format!("+{} g", price);
    let pw = ui.text_width(&price_text, 14.0);
    let price_color = if can_sell { [0.5, 1.0, 0.5, 1.0] } else { [0.4, 0.4, 0.4, 0.8] };
    ui.text(x + w - pw - 10.0, y + 14.0, &price_text, 14.0, price_color);
}

#[inline]
fn point_in_rect(p: (f32, f32), r: (f32, f32, f32, f32)) -> bool {
    p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3
}