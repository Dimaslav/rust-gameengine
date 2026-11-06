use crate::game::items::{
    EquipSlot, Equipment, Inventory, ItemRegistry, SortKey,
};
use crate::ui::UiLayer;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvLocation {
    Inventory(usize),
    Equipment(EquipSlot),
}

#[derive(Default)]
pub struct InvDragState {
    pub from: Option<InvLocation>,
    pub was_mouse_down: bool,
}

impl InvDragState {
    pub fn is_dragging(&self) -> bool {
        self.from.is_some()
    }
    pub fn cancel(&mut self) {
        self.from = None;
    }
}

pub enum InventoryAction {
    None,
    StartDrag(InvLocation),
    Swap(InvLocation, InvLocation),
    Split(usize),
    Use(usize),
    EquipFromInv(usize),
    Unequip(EquipSlot),
    Drop(usize),
    SortBy(SortKey),
}

const PANEL_W: f32 = 720.0;
const PANEL_H: f32 = 520.0;
const PAD: f32 = 20.0;
const CELL: f32 = 64.0;
const GAP: f32 = 6.0;
const COLS: usize = 6;
const EQ_SLOT_W: f32 = 220.0;
const EQ_SLOT_H: f32 = 44.0;

pub fn draw(
    ui: &mut UiLayer,
    inv: &Inventory,
    equip: &Equipment,
    registry: &ItemRegistry,
    drag: &mut InvDragState,
) -> InventoryAction {
    let sw = ui.screen_w();
    let sh = ui.screen_h();
    let px = (sw - PANEL_W) * 0.5;
    let py = (sh - PANEL_H) * 0.5;

    let input = ui.input();
    let (mx, my) = input.mouse_pos;

    // Фон + рамка.
    ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.5]);
    ui.rect(px - 2.0, py - 2.0, PANEL_W + 4.0, PANEL_H + 4.0, [0.55, 0.60, 0.75, 1.0]);
    ui.rect(px, py, PANEL_W, PANEL_H, [0.13, 0.15, 0.20, 0.98]);

    // Заголовок.
    ui.text_centered(sw * 0.5, py + 22.0, "INVENTORY", 24.0, [1.0, 0.9, 0.6, 1.0]);
    let used = inv.slots.iter().filter(|s| s.is_some()).count();
    ui.text_centered(
        sw * 0.5, py + 50.0,
        &format!("{} / {} slots used", used, inv.capacity),
        12.0, [0.75, 0.8, 0.9, 1.0],
    );

    // Кнопки сортировки.
    let sort_y = py + 64.0;
    let sort_x0 = px + PAD;
    let mut action = InventoryAction::None;
    if ui.button(sort_x0, sort_y, 80.0, 24.0, "Rarity") {
        action = InventoryAction::SortBy(SortKey::Rarity);
    }
    if ui.button(sort_x0 + 88.0, sort_y, 80.0, 24.0, "Kind") {
        action = InventoryAction::SortBy(SortKey::Kind);
    }
    if ui.button(sort_x0 + 176.0, sort_y, 80.0, 24.0, "Name") {
        action = InventoryAction::SortBy(SortKey::Name);
    }

    let content_y = py + 100.0;
    ui.rect(px + PAD, content_y - 8.0, PANEL_W - PAD * 2.0, 1.0, [0.4, 0.45, 0.55, 0.8]);

    // === Экипировка (слева) ===
    let eq_x = px + PAD;
    let eq_y = content_y + 8.0;
    ui.text(eq_x, eq_y - 2.0, "EQUIPMENT", 12.0, [0.9, 0.9, 0.5, 1.0]);

    let eq_slots: [(EquipSlot, &Option<String>); 3] = [
        (EquipSlot::Weapon, &equip.weapon_id),
        (EquipSlot::Armor, &equip.armor_id),
        (EquipSlot::Trinket, &equip.trinket_id),
    ];
    let mut hovered: Option<InvLocation> = None;
    let mut eq_y_cursor = eq_y + 18.0;
    for (slot, id_opt) in eq_slots {
        let loc = InvLocation::Equipment(slot);
        let rect = (eq_x, eq_y_cursor, EQ_SLOT_W, EQ_SLOT_H);
        if point_in_rect(mx, my, rect) { hovered = Some(loc); }
        let is_drop_target = drag.from.is_some() && drag.from != Some(loc);
        draw_eq_slot(ui, rect, slot, id_opt.as_deref(), registry, is_drop_target);
        eq_y_cursor += EQ_SLOT_H + GAP;
    }

    // === Инвентарь (сетка) ===
    let grid_x = eq_x + EQ_SLOT_W + PAD;
    let grid_y = content_y + 8.0;
    ui.text(grid_x, grid_y - 2.0, "ITEMS", 12.0, [0.9, 0.9, 0.5, 1.0]);

    let grid_top = grid_y + 18.0;
    for i in 0..inv.capacity {
        let col = (i % COLS) as f32;
        let row = (i / COLS) as f32;
        let cx = grid_x + col * (CELL + GAP);
        let cy = grid_top + row * (CELL + GAP);
        let rect = (cx, cy, CELL, CELL);
        let loc = InvLocation::Inventory(i);

        if point_in_rect(mx, my, rect) {
            hovered = Some(loc);
        }
        let is_drop_target = drag.from.is_some() && drag.from != Some(loc);
        draw_inv_cell(
            ui, rect, i, inv.slots.get(i).and_then(|s| s.as_ref()),
            registry, is_drop_target,
        );
    }

    // === Обработка клика / drag ===
    if input.mouse_clicked
        && let Some(loc) = hovered {
            let ctrl = input.modifiers.ctrl;
            match loc {
                InvLocation::Inventory(idx) => {
                    if ctrl {
                        // Ctrl+click — split, не начинаем drag.
                        action = InventoryAction::Split(idx);
                        drag.cancel();
                    } else if drag.from.is_none() {
                        drag.from = Some(loc);
                        drag.was_mouse_down = true;
                        action = InventoryAction::StartDrag(loc);
                    } else if let Some(from) = drag.from {
                        if from != loc {
                            action = InventoryAction::Swap(from, loc);
                        }
                        drag.from = None;
                    }
                }
                InvLocation::Equipment(_) => {
                    if ctrl {
                        // Ctrl+click на слоте экипировки — снять.
                        if let InvLocation::Equipment(slot) = loc {
                            action = InventoryAction::Unequip(slot);
                        }
                        drag.cancel();
                    } else if drag.from.is_none() {
                        drag.from = Some(loc);
                        drag.was_mouse_down = true;
                        action = InventoryAction::StartDrag(loc);
                    } else if let Some(from) = drag.from {
                        if from != loc {
                            action = InventoryAction::Swap(from, loc);
                        }
                        drag.from = None;
                    }
                }
            }
        }

    // Отпускание мыши — если тащили.
    if drag.was_mouse_down && !input.mouse_down {
        if let Some(from) = drag.from {
            if let Some(to) = hovered {
                if to != from {
                    action = InventoryAction::Swap(from, to);
                }
            }
            drag.from = None;
        }
        drag.was_mouse_down = false;
    }

    // === Tooltip ===
    if drag.from.is_none()
        && let Some(loc) = hovered {
            let item = match loc {
                InvLocation::Inventory(idx) => inv
                    .slots
                    .get(idx)
                    .and_then(|s| s.as_ref())
                    .and_then(|s| registry.get_by_id(&s.item_id)),
                InvLocation::Equipment(slot) => {
                    let id = match slot {
                        EquipSlot::Weapon => equip.weapon_id.as_deref(),
                        EquipSlot::Armor => equip.armor_id.as_deref(),
                        EquipSlot::Trinket => equip.trinket_id.as_deref(),
                    };
                    id.and_then(|i| registry.get_by_id(i))
                }
            };
            if let Some(item) = item {
                draw_tooltip(ui, mx, my, item);
            }
        }

    // === Призрак drag ===
    if let Some(from) = drag.from {
        let item = match from {
            InvLocation::Inventory(idx) => inv
                .slots
                .get(idx)
                .and_then(|s| s.as_ref())
                .and_then(|s| registry.get_by_id(&s.item_id)),
            InvLocation::Equipment(slot) => {
                let id = match slot {
                    EquipSlot::Weapon => equip.weapon_id.as_deref(),
                    EquipSlot::Armor => equip.armor_id.as_deref(),
                    EquipSlot::Trinket => equip.trinket_id.as_deref(),
                };
                id.and_then(|i| registry.get_by_id(i))
            }
        };
        if let Some(item) = item {
            let ghost_w = ui.text_width(&item.name, 12.0) + 16.0;
            let ghost_h = 22.0;
            let gx = mx + 12.0;
            let gy = my + 12.0;
            ui.rect(gx, gy, ghost_w, ghost_h, [0.0, 0.0, 0.0, 0.75]);
            ui.rect_outline(gx, gy, ghost_w, ghost_h, 1.0, item.rarity.color());
            ui.text(gx + 8.0, gy + 5.0, &item.name, 12.0, [1.0, 1.0, 1.0, 1.0]);
        }
    }

    // Подсказка внизу.
    ui.text_centered(
        sw * 0.5, py + PANEL_H - 18.0,
        "LMB drag: move/merge · Ctrl+click: split · Ctrl+click on equip: unequip · I: close",
        11.0, [0.6, 0.65, 0.75, 0.8],
    );

    action
}

fn draw_inv_cell(
    ui: &mut UiLayer,
    rect: (f32, f32, f32, f32),
    _idx: usize,
    stack: Option<&crate::game::items::ItemStack>,
    registry: &ItemRegistry,
    is_drop_target: bool,
) {
    let (x, y, w, h) = rect;
    ui.rect(x, y, w, h, [0.08, 0.10, 0.14, 0.95]);
    let border = if is_drop_target {
        [0.5, 1.0, 0.5, 1.0]
    } else {
        [0.35, 0.4, 0.5, 1.0]
    };
    ui.rect_outline(x, y, w, h, 1.0, border);

    if let Some(stack) = stack {
        let item = registry.get_by_id(&stack.item_id);
        let (icon, name, color) = match item {
            Some(it) => (it.kind.icon(), it.name.clone(), it.rarity.color()),
            None => ("?", stack.item_id.clone(), [0.5, 0.5, 0.5, 1.0]),
        };
        ui.text_centered(x + w * 0.5, y + 22.0, icon, 22.0, color);
        let short: String = name.chars().take(10).collect();
        ui.text_centered(x + w * 0.5, y + h - 12.0, &short, 9.0, [0.9, 0.9, 0.9, 1.0]);
        if stack.count > 1 {
            ui.text(x + w - 18.0, y + 4.0, &format!("x{}", stack.count), 10.0,
                [1.0, 0.9, 0.5, 1.0]);
        }
    }
}

fn draw_eq_slot(
    ui: &mut UiLayer,
    rect: (f32, f32, f32, f32),
    slot: EquipSlot,
    id: Option<&str>,
    registry: &ItemRegistry,
    is_drop_target: bool,
) {
    let (x, y, w, h) = rect;
    ui.rect(x, y, w, h, [0.08, 0.10, 0.14, 0.95]);
    let border = if is_drop_target {
        [0.5, 1.0, 0.5, 1.0]
    } else {
        [0.35, 0.4, 0.5, 1.0]
    };
    ui.rect_outline(x, y, w, h, 1.0, border);
    ui.text(x + 8.0, y + 14.0, slot.label(), 10.0, [0.7, 0.75, 0.85, 1.0]);

    let name = id
        .and_then(|i| registry.get_by_id(i))
        .map(|i| (i.name.clone(), i.rarity.color()))
        .unwrap_or_else(|| ("— empty —".to_string(), [0.5, 0.5, 0.5, 1.0]));
    ui.text(x + 70.0, y + 14.0, &name.0, 12.0, name.1);
}

fn draw_tooltip(
    ui: &mut UiLayer,
    mx: f32,
    my: f32,
    item: &crate::game::items::Item,
) {
    let title = format!("{}  [{}]", item.name, item.rarity.label());
    let kind = format!("Type: {}", item.kind.label());
    let desc = item.description.clone();
    let bonus = item.bonus_summary();
    let price = if item.sell_price > 0 {
        format!("Sell: {} g", item.sell_price)
    } else {
        String::new()
    };

    let w = 260.0_f32.max(ui.text_width(&title, 13.0) + 24.0);
    let line_h = 16.0;
    let mut lines: Vec<(String, f32, [f32; 4])> = vec![
        (title, 13.0, item.rarity.color()),
        (kind, 11.0, [0.8, 0.85, 0.95, 1.0]),
    ];
    if !desc.is_empty() {
        lines.push((desc, 11.0, [0.75, 0.8, 0.9, 1.0]));
    }
    if !bonus.is_empty() {
        lines.push((bonus, 11.0, [0.5, 1.0, 0.5, 1.0]));
    }
    if !price.is_empty() {
        lines.push((price, 11.0, [1.0, 0.85, 0.4, 1.0]));
    }
    let h = 12.0 + lines.len() as f32 * line_h;

    let mut tx = mx + 16.0;
    let mut ty = my + 16.0;
    if tx + w > ui.screen_w() - 8.0 { tx = mx - w - 16.0; }
    if ty + h > ui.screen_h() - 8.0 { ty = my - h - 16.0; }
    tx = tx.max(8.0);
    ty = ty.max(8.0);

    ui.rect(tx, ty, w, h, [0.05, 0.06, 0.10, 0.96]);
    ui.rect_outline(tx, ty, w, h, 1.0, item.rarity.color());

    let mut y = ty + 8.0;
    for (text, size, color) in lines {
        ui.text(tx + 12.0, y, &text, size, color);
        y += line_h;
    }
}

#[inline]
fn point_in_rect(mx: f32, my: f32, r: (f32, f32, f32, f32)) -> bool {
    mx >= r.0 && mx < r.0 + r.2 && my >= r.1 && my < r.1 + r.3
}