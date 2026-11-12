use crate::game::crafting::{CraftingRegistry, CraftingSession, RecipeDef};
use crate::game::items::{Inventory, ItemRegistry};
use crate::ui::UiLayer;

// ============================================================
// Layout
// ============================================================

const PANEL_W: f32 = 820.0;
const PANEL_H: f32 = 560.0;
const PAD: f32 = 20.0;
const LIST_W: f32 = 380.0;
const ROW_H: f32 = 40.0;
const ROW_GAP: f32 = 3.0;

// ============================================================
// Действия
// ============================================================

pub enum CraftingAction {
    None,
    Select(String),
    Craft(String),
    Close,
}

// ============================================================
// Публичный вход
// ============================================================

pub fn draw(
    ui: &mut UiLayer,
    session: &mut CraftingSession,
    registry: &CraftingRegistry,
    item_registry: &ItemRegistry,
    inventory: &Inventory,
    gold: u32,
    player_level: u32,
) -> CraftingAction {
    let sw = ui.screen_w();
    let sh = ui.screen_h();
    let px = (sw - PANEL_W) * 0.5;
    let py = (sh - PANEL_H) * 0.5;

    // Фон.
    ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.5]);
    ui.rect(px - 2.0, py - 2.0, PANEL_W + 4.0, PANEL_H + 4.0, [0.55, 0.60, 0.75, 1.0]);
    ui.rect(px, py, PANEL_W, PANEL_H, [0.13, 0.15, 0.20, 0.98]);

    // Шапка.
    let title = format!("FORGE  ·  {} recipes", registry.len());
    ui.text_centered(sw * 0.5, py + 20.0, &title, 22.0, [1.0, 0.9, 0.6, 1.0]);
    ui.text_centered(
        sw * 0.5, py + 46.0,
        &format!("Gold: {}  ·  Level: {}", gold, player_level),
        12.0, [1.0, 0.85, 0.4, 1.0],
    );

    // Закрытие.
    let close_size = 26.0;
    let close_x = px + PANEL_W - close_size - 10.0;
    let close_y = py + 10.0;
    let mut action = CraftingAction::None;
    if ui.button(close_x, close_y, close_size, close_size, "X") {
        action = CraftingAction::Close;
    }

    let content_y = py + 80.0;

    // ============ Список рецептов ============
    let lx = px + PAD;
    ui.text(lx, content_y, "RECIPES", 14.0, [0.9, 0.9, 0.5, 1.0]);
    ui.rect(lx, content_y + 20.0, LIST_W, 1.0, [0.4, 0.45, 0.55, 0.8]);

    let sorted = registry.sorted();
    let mut y = content_y + 30.0;
    let mut current_category: Option<&str> = None;

    for recipe in &sorted {
        // Заголовок категории.
        let cat = recipe.category.as_str();
        if current_category != Some(cat) {
            if current_category.is_some() {
                y += 4.0;
            }
            current_category = Some(cat);
            ui.text(lx + 4.0, y, cat, 11.0, [0.7, 0.75, 0.9, 1.0]);
            y += 16.0;
        }

        let is_selected = session
            .selected_recipe_id
            .as_deref() == Some(recipe.id.as_str());
        let row_rect = (lx, y, LIST_W, ROW_H);
        let hovered = point_in_rect(ui.input().mouse_pos, row_rect);
        let can_craft = can_craft(recipe, inventory, gold, player_level);

        draw_recipe_row(ui, row_rect, recipe, is_selected, hovered, can_craft);

        if hovered && ui.input().mouse_clicked {
            action = CraftingAction::Select(recipe.id.clone());
        }

        y += ROW_H + ROW_GAP;
        if y > py + PANEL_H - 60.0 { break; }
    }

    // ============ Детали выбранного рецепта ============
    let dx = lx + LIST_W + PAD;
    let dw = px + PANEL_W - PAD - dx;
    draw_details(
        ui, dx, content_y, dw, py + PANEL_H - 40.0 - content_y,
        session, registry, item_registry, inventory, gold, player_level, &mut action,
    );

    // Подсказка.
    ui.text_centered(
        sw * 0.5, py + PANEL_H - 18.0,
        "LMB recipe to select · LMB Craft to produce · X/Esc to close",
        11.0, [0.6, 0.65, 0.75, 0.8],
    );

    action
}

// ============================================================
// Детали
// ============================================================

#[allow(clippy::too_many_arguments)]
fn draw_details(
    ui: &mut UiLayer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    session: &CraftingSession,
    registry: &CraftingRegistry,
    item_registry: &ItemRegistry,
    inventory: &Inventory,
    gold: u32,
    player_level: u32,
    action: &mut CraftingAction,
) {
    ui.rect(x, y, w, h, [0.08, 0.10, 0.14, 0.95]);
    ui.rect_outline(x, y, w, h, 1.0, [0.35, 0.4, 0.5, 1.0]);

    let Some(recipe_id) = session.selected_recipe_id.as_deref() else {
        ui.text_centered(x + w * 0.5, y + h * 0.5,
            "Select a recipe", 14.0, [0.6, 0.65, 0.75, 0.9]);
        return;
    };
    let Some(recipe) = registry.get(recipe_id) else {
        ui.text_centered(x + w * 0.5, y + h * 0.5,
            "Recipe not found", 14.0, [0.9, 0.3, 0.3, 0.9]);
        return;
    };

    let pad = 16.0;
    let mut cy = y + pad;

    // Заголовок рецепта.
    ui.text(x + pad, cy, &recipe.title, 18.0, [1.0, 0.95, 0.7, 1.0]);
    cy += 26.0;
    if !recipe.category.is_empty() {
        ui.text(x + pad, cy, &format!("Category: {}", recipe.category), 11.0,
            [0.7, 0.75, 0.85, 1.0]);
        cy += 20.0;
    }

    ui.rect(x + pad, cy, w - pad * 2.0, 1.0, [0.4, 0.45, 0.55, 0.8]);
    cy += 10.0;

    // Level requirement.
    let level_ok = player_level >= recipe.required_level;
    let level_color = if level_ok { [0.5, 1.0, 0.5, 1.0] } else { [1.0, 0.4, 0.4, 1.0] };
    ui.text(x + pad, cy,
        &format!("Level: {} / {}", player_level, recipe.required_level),
        12.0, level_color);
    cy += 22.0;

    // Gold requirement (если есть).
    if recipe.gold_cost > 0 {
        let gold_ok = gold >= recipe.gold_cost;
        let gold_color = if gold_ok { [1.0, 0.85, 0.4, 1.0] } else { [1.0, 0.4, 0.4, 1.0] };
        ui.text(x + pad, cy,
            &format!("Gold: {} / {}", gold, recipe.gold_cost),
            12.0, gold_color);
        cy += 22.0;
    }

    // Inputs.
    ui.text(x + pad, cy, "INPUTS", 11.0, [0.9, 0.9, 0.5, 1.0]);
    cy += 16.0;
    for input in &recipe.inputs {
        let have = inventory.count_of(&input.item_id);
        let enough = have >= input.count;
        let color = if enough { [0.5, 1.0, 0.5, 1.0] } else { [1.0, 0.4, 0.4, 1.0] };
        let name = item_registry
            .get_by_id(&input.item_id)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| input.item_id.clone());
        ui.text(x + pad + 8.0, cy,
            &format!("• {}  {}/{}", name, have, input.count), 12.0, color);
        cy += 18.0;
    }

    cy += 6.0;

    // Outputs.
    ui.text(x + pad, cy, "OUTPUTS", 11.0, [0.9, 0.9, 0.5, 1.0]);
    cy += 16.0;
    for output in &recipe.outputs {
        let name = item_registry
            .get_by_id(&output.item_id)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| output.item_id.clone());
        ui.text(x + pad + 8.0, cy,
            &format!("• {} ×{}", name, output.count), 12.0, [0.9, 0.95, 1.0, 1.0]);
        cy += 18.0;
    }

    // Кнопка Craft.
    let can = can_craft(recipe, inventory, gold, player_level);
    let btn_w = 160.0;
    let btn_h = 40.0;
    let btn_x = x + (w - btn_w) * 0.5;
    let btn_y = y + h - btn_h - 16.0;

    let clicked = ui.button(btn_x, btn_y, btn_w, btn_h,
        if can { "Craft" } else { "Cannot craft" });
    if clicked && can {
        *action = CraftingAction::Craft(recipe.id.clone());
    }
}

// ============================================================
// Утилиты
// ============================================================

fn can_craft(
    recipe: &RecipeDef,
    inventory: &Inventory,
    gold: u32,
    player_level: u32,
) -> bool {
    if player_level < recipe.required_level { return false; }
    if gold < recipe.gold_cost { return false; }
    for input in &recipe.inputs {
        if inventory.count_of(&input.item_id) < input.count {
            return false;
        }
    }
    true
}

fn draw_recipe_row(
    ui: &mut UiLayer,
    rect: (f32, f32, f32, f32),
    recipe: &RecipeDef,
    is_selected: bool,
    hovered: bool,
    can_craft: bool,
) {
    let (x, y, w, h) = rect;
    let bg = if is_selected {
        [0.22, 0.28, 0.38, 0.98]
    } else if hovered {
        [0.16, 0.18, 0.24, 0.95]
    } else {
        [0.08, 0.10, 0.14, 0.95]
    };
    ui.rect(x, y, w, h, bg);
    let border = if is_selected {
        [1.0, 0.9, 0.5, 1.0]
    } else {
        [0.35, 0.4, 0.5, 1.0]
    };
    ui.rect_outline(x, y, w, h, 1.0, border);

    let title_color = if can_craft { [1.0, 0.95, 0.85, 1.0] } else { [0.6, 0.6, 0.6, 0.9] };
    ui.text(x + 10.0, y + 6.0, &recipe.title, 13.0, title_color);

    let detail = format!(
        "{} in → {} out{}",
        recipe.inputs.len(),
        recipe.outputs.len(),
        if recipe.gold_cost > 0 { format!(" · {}g", recipe.gold_cost) } else { String::new() },
    );
    ui.text(x + 10.0, y + 24.0, &detail, 10.0, [0.6, 0.65, 0.75, 1.0]);

    if can_craft {
        ui.text(x + w - 20.0, y + h * 0.5 - 4.0, "✓", 16.0, [0.5, 1.0, 0.5, 1.0]);
    }
}

#[inline]
fn point_in_rect(p: (f32, f32), r: (f32, f32, f32, f32)) -> bool {
    p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3
}