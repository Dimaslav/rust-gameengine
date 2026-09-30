# Rust Engine 3D

Небольшой 3D-движок на Rust + wgpu + winit с редактором сцены,
Play-режимом от первого лица и hot-reload шейдеров.

![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)
![wgpu](https://img.shields.io/badge/wgpu-0.20-blue)
![winit](https://img.shields.io/badge/winit-0.29-blue)
![egui](https://img.shields.io/badge/egui-0.28-purple)

---

## Что это

Хобби-движок с полноценным deferred-рендером (G-buffer + SSAO + IBL +
CSM + cube shadow), ECS, glTF-загрузчиком со скелетной анимацией,
встроенным редактором сцены с gizmò, undo/redo и Play-режимом от
первого лица с коллизиями.

Всё на чистом Rust без сторонних игровых фреймворков (Bevy, Fyrox
не используются). Только низкоуровневые библиотеки — wgpu для GPU,
winit для окна, egui для UI.

---

## Возможности

### Рендер
- Deferred pipeline: G-buffer (3 MRT) → SSAO + blur → lighting
- Каскадные тени (CSM 3 каскада) для directional light
- Cube shadow map для точечного источника
- PBR (metallic-roughness) с IBL (irradiance + prefiltered + BRDF LUT)
- Normal mapping через cotangent frame (без тангентов в вершинах)
- Bloom + ACES tonemapping
- Alpha modes (Opaque / Mask / Blend) с отдельным forward-проходом
- Double-sided материалы (пайплайн-вариант без culling)
- Правильный colorspace: baseColor/emissive → sRGB, normal/MR → linear

### ECS
- Dense-storage компоненты в `Vec<T>` с параллельным `Vec<Entity>` и
  обратным `HashMap`
- Итерация по 1 и 2 компонентам (`query`, `query2`, `for_each_pair`)
- Event bus с двойной буферизацией
- Иерархия сущностей через `Parent(Entity)`, world-матрицы через
  обход цепочки

### Редактор
- Окна: **Hierarchy**, **Inspector**, **Stats**, **Renderer**
- Выделение: клик, Ctrl+клик, Shift+клик (диапазон), multi-select
- Gizmò: translate / rotate / scale, оси X/Y/Z, Ctrl = snap
- Точный picking: Möller–Trumbore raycast по треугольникам
- Undo/redo (снапшоты сцены в RON, 50 шагов)
- Save / Load сцены в `.ron`
- Копирование / вставка сущностей (Ctrl+C / Ctrl+V)
- Make Unique материал (клон для одного объекта)
- Поиск в Hierarchy по имени
- Контекстное меню (Focus / Duplicate / Delete)

### Fly (UE5-style)
- **RMB** — захват курсора, режим свободного полёта
- **WASD** — движение, **E / Q** — вверх/вниз, **Space** — тоже вверх
- **Shift** — ×3, **Ctrl** — ×0.3
- **Scroll** во время полёта — менять скорость
- Выход — отпустить **RMB** или **Esc**

### Play-режим
- **F9** или **▶ Play** — войти/выйти
- FPS-контроллер: **WASD** + мышь + **Space** (прыжок) + **Shift** (бег)
- Гравитация, AABB-коллизии с объектами сцены
- Escape-логика: если игрок застрял в коллайдере — можно выбраться
- Crosshair + HUD (FPS, позиция, on ground / airborne)
- Head bob (опционально, по пройденному пути)
- Спавн там, где сейчас стоит editor-камера

### Hot-reload шейдеров
- В debug-сборке шейдеры читаются с диска
- **F12** — пересобрать все пайплайны заново
- Ошибки компиляции шейдеров показываются в консоли, старый
  пайплайн продолжает работать

---

## Сборка и запуск

### Требования
- **Rust** 1.75+ (`rustup default stable`)
- **Windows**: MSVC Build Tools для VS 2022 (workload «Desktop
  development with C++»)
- **Linux**: `libx11-dev libxkbcommon-dev libwayland-dev`
- **macOS**: Xcode Command Line Tools
- GPU с Vulkan / DX12 / Metal

### Запуск

```bash
# Debug — работает hot-reload шейдеров, но медленный рендер
cargo run

# Release — быстро (300–2000 FPS в демо-сцене)
cargo run --release
