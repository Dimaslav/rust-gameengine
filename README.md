# Rust Engine 3D

Полноценный игровой движок на Rust с редактором уровней, ECS-архитектурой,
PBR-рендером и полным asset pipeline. Готов как фундамент для разработки
собственных игр — пиши `impl Game for MyGame`, и у тебя своя игра.
╔═══════════════════════════════════════════════════╗
║ ██████╗ ██╗ ██╗███████╗████████╗ ║
║ ██╔══██╗██║ ██║██╔════╝╚══██╔══╝ ║
║ ██████╔╝██║ ██║███████╗ ██║ ║
║ ██╔══██╗██║ ██║╚════██║ ██║ ║
║ ██║ ██║╚██████╔╝███████║ ██║ ║
║ ╚═╝ ╚═╝ ╚═════╝ ╚══════╝ ╚═╝ ENGINE 3D ║
╚═══════════════════════════════════════════════════╝


---

## Содержание

- [Что это](#что-это)
- [Возможности](#возможности)
- [Быстрый старт](#быстрый-старт)
- [Как сделать свою игру](#как-сделать-свою-игру)
- [Управление](#управление)
- [Архитектура](#архитектура)
- [Структура проекта](#структура-проекта)
- [Демо-сцена "The Fallen Citadel"](#демо-сцена-the-fallen-citadel)
- [Зависимости](#зависимости)
- [Сборка](#сборка)
- [Roadmap](#roadmap)
- [Лицензия](#лицензия)

---

## Что это

**Rust Engine 3D** — это самодостаточный игровой движок, написанный на Rust,
который можно использовать как **фундамент для собственной игры**. Он даёт:

- **ECS** (Entity Component System) с плотным хранилищем компонентов
- **Редактор в стиле Unity**: gizmo, инспектор, палитра, иерархия, undo/redo
- **Deferred PBR рендер**: shadow mapping (CSM + cube), IBL, SSAO, TAA, bloom, volumetric fog
- **Полный asset pipeline**: `.meta`-файлы, UUID, hot-reload текстур, Content Browser
- **Runtime UI**: HUD, кнопки, прогресс-бары, пиксельный шрифт (процедурный)
- **Физика**: rigid bodies, коллайдеры, raycast, navmesh, A*
- **AI**: FSM, зрение, слух, патрули, восприятие
- **Сериализация**: сцены, префабы, FBX (import/export), RON

Разработчик игры подключает движок как библиотеку и пишет **только свою игру** —
не движок.

---

## Возможности

| Система | Что есть |
|---|---|
| **ECS** | `World`, `Entity`, компоненты через `HashMap<TypeId, AnyStorage>`, системы через трейт `System` |
| **Рендер** | Deferred PBR (metallic/roughness), CSM (3 каскада), cube shadow (4 point-light), SSAO, IBL (HDRI), TAA, bloom, volumetric fog, decals, particles, sky box |
| **Материалы** | PBR + alpha modes (Opaque/Mask/Blend), нормал-мапы, MR-текстуры, emissive, double-sided, кастомные sampler'ы |
| **Освещение** | Directional (sun/fill), point lights (до 16 в кадр), IBL ambient, emissive surfaces |
| **Камера** | Orbit / FirstPerson / Fly. Frustum culling, LOD |
| **Физика** | RigidBody (Static/Dynamic/Kinematic), Collider (Sphere/AABB/Capsule), raycast, overlap queries, сериализация |
| **Navmesh** | Grid-based bake из коллайдеров, A* pathfinding, string-pulling smoothing |
| **AI** | `AiAgent` с FSM (Idle/Patrol/Investigate/Chase/Attack/Dead), vision cone, hearing range, patrol paths |
| **Анимация** | Skeleton (до 64 костей), AnimationClip, anim events, interpolation (slerp), glTF loading |
| **Аудио** | Spatial (distance attenuation), occlusion (raycast), шины (Master/SFX/Music/Voice/UI), procedural + файлы |
| **Runtime UI** | Immediate-mode `UiLayer`, `Hud` (health, ammo, crosshair, notifications, objectives), пауза-меню |
| **Assets** | `.meta`-файлы с UUID, Content Browser, hot-reload текстур, content hash, dependencies |
| **Сериализация** | RON-сцены, префабы (`*.prefab.ron`), save/load, версионирование |
| **FBX** | Import (ASCII 7.x + binary 7.0–7.5), Export (ASCII 7.4) |
| **Редактор** | Gizmo (translate/rotate/scale), box-select, picking через BVH, инспектор по компонентам, Command Palette, undo-стек |
| **Режим Play** | Full snapshot scene → play → restore (Play-in-Editor как в Unity) |
| **Производительность** | Frustum culling, LOD (3 уровня через vertex clustering), instance batching, ленивая BVH |

---

## Быстрый старт

**Требования:**
- Rust 1.75+ (edition 2021)
- GPU с поддержкой Vulkan / Metal / DX12
- Windows / Linux / macOS

```bash
git clone <repo> rust-engine
cd rust-engine
cargo run --release
