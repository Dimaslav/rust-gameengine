# Rust Engine 3D

Полноценный 3D-движок на Rust с wgpu: ECS, деферальный рендер, физика,
редактор с gizmo и undo, импорт/экспорт FBX и glTF.

![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange)
![wgpu](https://img.shields.io/badge/wgpu-0.20-blue)
![License](https://img.shields.io/badge/license-MIT-green)

---

## Что это

Учебно-боевой 3D-движок, написанный с нуля без игровых фреймворков.
Включает:

- **Собственный ECS** — плотное хранение компонентов, type-erased storages, event bus.
- **Деферальный рендер** — G-buffer + forward transparent pass.
- **PBR** — metallic/roughness, IBL (env cubemap + irradiance + prefilter + BRDF LUT).
- **CSM** — 3-каскадные тени с PCF Poisson 12 taps, slope-scaled bias и cascade blend.
- **SSAO** — 16 сэмплов + bilateral blur + temporal rotation.
- **Bloom mip chain** — 5 уровней, 13-tap downsample + 3-tap tent upsample.
- **Физика** — rigid bodies (static / dynamic / kinematic), sphere/AABB/capsule коллайдеры, impulse solver с трением.
- **BVH raycast** — picking по треугольникам, стрельба, interact.
- **LOD** — авто-генерация через vertex clustering, distance-based выбор.
- **Редактор** — hierarchy, inspector, gizmo, undo/redo, prefabs, палитра примитивов.
- **Command Palette** — Ctrl+P с fuzzy-поиском по всем действиям.
- **Camera bookmarks** — Ctrl+1..9 сохранить, Alt+1..9 перейти.
- **FBX** — импорт ASCII и binary (7.0–7.7), экспорт ASCII 7.4.
- **glTF 2.0** — загрузка с PBR, skinning и анимациями.
- **Сериализация сцены** — RON.
- **Play-режим** — FPS-контроллер с коллизиями, стрельбой, триггерами.
