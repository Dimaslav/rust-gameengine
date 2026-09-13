Rust Engine:
3D-игровой движок на Rust + wgpu. Собственная ECS, deferred shading, PBR,
IBL из HDRI, CSM, SSAO, bloom, скелетная анимация из glTF.

Что работает:
- ECS — плотные хранилища, `query`/`query2`/`for_each_pair`, events
- Deferred rendering — G-buffer (albedo/металл, нормаль/глубина, emissive/roughness)
- PBR — GGX + normal mapping (без tangents)
- IBL из HDRI — irradiance + prefiltered + BRDF LUT
- Тени — CSM (3 каскада 2048²) + cube shadow для point-light
- SSAO — 8 taps + blur 4×4
- Bloom — bright-pass + separable Gaussian
- ACES tonemap с exposure
- Инстансинг + frustum culling — 2000 объектов = 6 draw calls
- glTF — меши, материалы, текстуры, skin, animation, иерархия нод
- Скелетная анимация — 4-костный skinning, linear + slerp
- Debug views F1–F6 — SSAO, G-buffer, HDR, CSM

Чего нет:
Alpha mode, double-sided, morph targets, MSAA, SSR, прозрачность, аудио,
UI, физика, save/load, hot reload, многопоточный ECS.

Запуск:
cargo run --release
Rust 1.75+. Первая сборка 3–10 минут.

Ассеты (опционально):
assets/sky.hdr — HDRI с polyhaven.com
assets/animated.glb — модель с анимацией (Khronos Sample Models)

Управление:
Клавиша	Действие
ЛКМ + мышь	Орбита
Колесо	Zoom
WASD	Панорама
G / C	Сетка / culling
F1–F6	Debug views
[ ] , . ; '	Bloom / exposure
Z / X	SSAO strength
Esc	Выход

Структура:
src/
├── main.rs                 # DemoGame
├── ecs/                    # World, ComponentStorage, Events, System
├── engine/                 # App (Game trait), Input, Time
├── game/components.rs      # Transform, MeshHandle, AnimationPlayer, ...
└── render/
    ├── camera.rs, csm.rs, ibl.rs, mesh.rs, material.rs,
    │   skinning.rs, gltf_loader.rs, line.rs, texture.rs,
    │   shadow_cube.rs, debug.rs
    ├── renderer/           # mod, gpu_types, size_dep, passes
    └── shaders/            # 15 WGSL шейдеров

Пайплайн:
1. Shadow   — CSM ×3 + Cube ×6
2. G-buffer — 3 MRT + depth
3. SSAO + blur
4. Lighting — fullscreen, PBR + IBL + тени + sky
5. Forward  — debug-линии
6. Post     — bright → blur H → blur V → composite (ACES)

Зависимости:
wgpu 0.20, winit 0.30, glam 0.27, gltf 1.4, image 0.25,
bytemuck, pollster, anyhow, env_logger.
