//! Процедурный генератор игрового мира.

use std::time::Instant;

use glam::{Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::ai::{AiAgent, Enemy, PatrolPath};
use crate::game::components::{
    Health, MaterialHandle, MeshHandle, Name, Spinner, Tint, Transform, Trigger, TriggerAction,
};
use crate::game::lights::{DirectionalLight, PointLight};
use crate::game::rpg::GoldValue;
use crate::physics::{Collider, RigidBody};
use crate::render::material::Material;
use crate::render::mesh::Mesh;
use crate::render::terrain::{generate_terrain_mesh, generate_terrain_texture, Heightmap};
use crate::render::Renderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Biome {
    Ruins,
    Forest,
    Dungeon,
}

impl Biome {
    pub const ALL: &'static [Biome] = &[Biome::Ruins, Biome::Forest, Biome::Dungeon];

    pub fn label(self) -> &'static str {
        match self {
            Biome::Ruins => "Ancient Ruins",
            Biome::Forest => "Dark Forest",
            Biome::Dungeon => "Dungeon Crypt",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Biome::Ruins => "Broken columns, gravestones, patrolling undead",
            Biome::Forest => "Dense trees, scattered rocks, hidden enemies",
            Biome::Dungeon => "Stone corridors, torches, lurking horrors",
        }
    }

    fn uses_hilly_terrain(self) -> bool {
        matches!(self, Biome::Ruins | Biome::Forest)
    }
}

#[derive(Debug, Clone)]
pub struct WorldGenConfig {
    pub seed: u64,
    pub biome: Biome,
    pub size: f32,
    pub prop_density: f32,
    pub enemy_count: u32,
    pub loot_count: u32,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            biome: Biome::Ruins,
            size: 300.0,
            prop_density: 0.5,
            enemy_count: 8,
            loot_count: 6,
        }
    }
}

pub struct WorldGenResult {
    pub player_spawn: Vec3,
    pub terrain_resolution: u32,
    pub entity_count: usize,
    pub elapsed_ms: u32,
}

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        if s == 0 {
            s = 1;
        }
        Self { state: s }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    #[inline]
    fn f32_01(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.f32_01() * (hi - lo)
    }

    fn chance(&mut self, p: f32) -> bool {
        self.f32_01() < p
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next_u32() as usize) % items.len()]
    }
}

pub struct WorldGen;

impl WorldGen {
    pub fn generate(
        world: &mut World,
        renderer: &mut Renderer,
        config: &WorldGenConfig,
    ) -> WorldGenResult {
        let t0 = Instant::now();
        let mut rng = Lcg::new(config.seed);

        log::info!(
            "WorldGen: start generation with seed={} biome={:?}",
            config.seed,
            config.biome
        );

        Self::ensure_assets(renderer);
        
        // Для каждого биома генерируем свой базовый размер, если пользователь не задал жестко
        let actual_size = match config.biome {
            Biome::Dungeon => rng.range(60.0, 120.0),
            Biome::Ruins => rng.range(150.0, 400.0),
            Biome::Forest => rng.range(200.0, 500.0),
        };
        
        Self::create_terrain(world, renderer, config, actual_size, &mut rng);
        Self::spawn_sun(world);

        let hm = renderer.terrain.as_ref().expect("terrain must be set");

        let player_spawn = match config.biome {
            Biome::Ruins => Self::build_ruins(world, hm, config, &mut rng),
            Biome::Forest => Self::build_forest(world, hm, config, &mut rng),
            Biome::Dungeon => Self::build_dungeon(world, hm, config, actual_size, &mut rng),
        };

        Self::spawn_enemies(world, hm, config, &mut rng, player_spawn);
        Self::spawn_loot(world, hm, config, &mut rng, player_spawn);

        WorldGenResult {
            player_spawn,
            terrain_resolution: hm.resolution,
            entity_count: world.len(),
            elapsed_ms: t0.elapsed().as_millis() as u32,
        }
    }

    fn ensure_assets(renderer: &mut Renderer) {
        use crate::render::props;

        let mesh_creators: &[(&str, &dyn Fn(&wgpu::Device) -> Mesh)] = &[
            ("crate_detail", &props::crate_detail),
            ("barrel_detail", &props::barrel_detail),
            ("column_fluted", &props::column_fluted),
            ("pillar_ruined_a", &|d| props::pillar_ruined(d, 1)),
            ("pillar_ruined_b", &|d| props::pillar_ruined(d, 2)),
            ("pillar_ruined_c", &|d| props::pillar_ruined(d, 3)),
            ("chest_detail", &props::chest_detail),
            ("torch_stand", &props::torch_stand),
            ("brazier_detail", &props::brazier_detail),
            ("tree_pine_a", &|d| props::tree_pine(d, 8.0)),
            ("tree_pine_b", &|d| props::tree_pine(d, 6.0)),
            ("tree_oak_a", &|d| props::tree_oak(d, 7.0, 42)),
            ("tree_oak_b", &|d| props::tree_oak(d, 6.0, 99)),
            ("rock_cluster_a", &|d| props::rock_cluster(d, 0.8, 11)),
            ("rock_cluster_b", &|d| props::rock_cluster(d, 0.5, 22)),
            ("rock_cluster_c", &|d| props::rock_cluster(d, 1.2, 33)),
            ("gravestone", &props::gravestone),
            ("bench", &props::bench),
            ("table", &props::table),
            ("fence_post", &props::fence_post),
            ("weapon_rack", &props::weapon_rack),
            ("skull", &props::skull),
        ];

        for (name, create) in mesh_creators {
            if !renderer.meshes.contains_key(*name) {
                let mesh = create(&renderer.device);
                renderer.add_mesh(*name, mesh);
            }
        }

        if !renderer.meshes.contains_key("capsule") {
            renderer.add_mesh("capsule", Mesh::capsule(&renderer.device, 0.4, 0.8, 6, 20));
        }

        let mats: &[(&str, Material)] = &[
            ("arena_floor", Material::new([0.42, 0.46, 0.50, 1.0]).with_metallic_roughness(0.0, 1.0)),
            ("arena_wall", Material::new([0.38, 0.40, 0.44, 1.0]).with_metallic_roughness(0.0, 0.88)),
            ("arena_column", Material::new([0.68, 0.65, 0.58, 1.0]).with_metallic_roughness(0.1, 0.7)),
            ("arena_crate", Material::new([0.65, 0.45, 0.25, 1.0]).with_metallic_roughness(0.0, 0.85)),
            ("arena_barrel", Material::new([0.55, 0.30, 0.15, 1.0]).with_metallic_roughness(0.3, 0.6)),
            ("arena_marker", Material::new([1.0, 0.9, 0.4, 1.0]).with_metallic_roughness(0.0, 0.5).with_emissive([2.5, 2.2, 0.6])),
            ("emissive_warm", Material::new([1.0, 0.7, 0.4, 1.0]).with_metallic_roughness(0.0, 0.5).with_emissive([3.0, 1.5, 0.4])),
            ("emissive_cold", Material::new([0.5, 0.75, 1.0, 1.0]).with_metallic_roughness(0.0, 0.5).with_emissive([0.8, 1.5, 3.0])),
            ("flat_red", Material::new([1.0, 0.35, 0.35, 1.0]).with_metallic_roughness(0.0, 0.5)),
            ("gold", Material::new([1.0, 0.85, 0.3, 1.0]).with_metallic_roughness(1.0, 0.25)),
            ("rpg_dungeon", Material::new([0.13, 0.11, 0.16, 1.0]).with_metallic_roughness(0.0, 0.95)),
            ("foliage", Material::new([0.20, 0.45, 0.15, 1.0]).with_metallic_roughness(0.0, 0.95)),
            ("bark", Material::new([0.30, 0.20, 0.12, 1.0]).with_metallic_roughness(0.0, 0.9)),
            ("ground", Material::new([0.35, 0.38, 0.30, 1.0]).with_metallic_roughness(0.0, 0.95)),
        ];

        for (name, mat) in mats {
            if !renderer.has_material(name) {
                renderer.add_material(*name, mat.clone());
            }
        }
    }

    fn create_terrain(
        world: &mut World,
        renderer: &mut Renderer,
        config: &WorldGenConfig,
        size: f32,
        _rng: &mut Lcg,
    ) {
        let (flat_r, max_h) = if config.biome.uses_hilly_terrain() {
            match config.biome {
                Biome::Ruins => (size * 0.15, 25.0),
                Biome::Forest => (0.0, 35.0),
                _ => (0.0, 10.0),
            }
        } else {
            (0.0, 0.0)
        };

        let hm = Heightmap::new_procedural_with_options(
            256,
            size,
            config.seed as u32,
            flat_r,
            max_h,
        );

        let mesh = generate_terrain_mesh(&renderer.device, &hm);
        renderer.add_mesh("terrain", mesh);

        let tex = generate_terrain_texture(&hm, 2048);
        renderer
            .load_texture_rgba("terrain_tex", &tex, 2048, 2048)
            .expect("terrain texture");

        let tint = if config.biome == Biome::Dungeon {
            [0.20, 0.18, 0.22, 1.0]
        } else {
            [1.0, 1.0, 1.0, 1.0]
        };
        let mat = Material::new(tint)
            .with_metallic_roughness(0.0, 0.92)
            .with_texture("terrain_tex");
        if renderer.has_material("terrain_mat") {
            renderer.update_material("terrain_mat", mat);
        } else {
            renderer.add_material("terrain_mat", mat);
        }

        renderer.terrain = Some(hm);

        let e = world.spawn();
        world.insert(e, Name("Terrain".into()));
        world.insert(e, Transform::at(Vec3::ZERO));
        world.insert(e, MeshHandle("terrain".into()));
        world.insert(e, MaterialHandle("terrain_mat".into()));
    }

    fn spawn_sun(world: &mut World) {
        let e = world.spawn();
        world.insert(e, Name("Sun".into()));
        world.insert(e, Transform::at(Vec3::new(0.0, 40.0, 0.0)));
        let mut l = DirectionalLight::sun();
        l.direction = Vec3::new(0.4, 1.0, 0.3);
        l.intensity = 1.3;
        l.color = [1.0, 0.95, 0.85];
        world.insert(e, l);
    }

    fn spawn_prop(
        world: &mut World,
        name: &str,
        mesh: &str,
        material: &str,
        pos: Vec3,
        rot_y: f32,
        scale: f32,
        half_extents: Vec3,
    ) -> Entity {
        let e = world.spawn();
        world.insert(e, Name(name.into()));
        world.insert(
            e,
            Transform {
                position: pos,
                rotation: Quat::from_rotation_y(rot_y),
                scale: Vec3::splat(scale),
            },
        );
        world.insert(e, MeshHandle(mesh.into()));
        world.insert(e, MaterialHandle(material.into()));
        world.insert(e, RigidBody::static_body());
        world.insert(e, Collider::aabb(half_extents));
        e
    }

    fn spawn_torch(world: &mut World, pos: Vec3) -> Entity {
        let stand = world.spawn();
        world.insert(stand, Name("Torch".into()));
        world.insert(stand, Transform::at(pos).with_scale(1.0));
        world.insert(stand, MeshHandle("torch_stand".into()));
        world.insert(stand, MaterialHandle("arena_column".into()));
        world.insert(stand, RigidBody::static_body());
        world.insert(stand, Collider::aabb(Vec3::new(0.1, 0.8, 0.1)));

        let light = world.spawn();
        world.insert(light, Name("TorchLight".into()));
        world.insert(
            light,
            Transform::at(pos + Vec3::Y * 1.55).with_scale(0.20),
        );
        world.insert(light, MeshHandle("sphere".into()));
        world.insert(light, MaterialHandle("emissive_warm".into()));
        world.insert(light, PointLight::new([1.0, 0.55, 0.2], 5.0, 14.0));
        world.insert(light, Spinner::new(Vec3::Y, 0.6));

        stand
    }

    fn build_ruins(
        world: &mut World,
        hm: &Heightmap,
        config: &WorldGenConfig,
        rng: &mut Lcg,
    ) -> Vec3 {
        let n_cols = (rng.range(4.0, 16.0) as u32).max(3);
        let ring_r = rng.range(15.0, 40.0);
        
        for i in 0..n_cols {
            let a = (i as f32 / n_cols as f32) * std::f32::consts::TAU;
            let x = a.cos() * ring_r;
            let z = a.sin() * ring_r;
            let y = hm.sample(x, z);

            let mesh: &'static str = *rng.pick(&[
                "pillar_ruined_a",
                "pillar_ruined_b",
                "pillar_ruined_c",
            ]);
            Self::spawn_prop(
                world,
                &format!("Pillar_{}", i),
                mesh,
                "arena_column",
                Vec3::new(x, y + 0.6, z),
                a + rng.range(-0.3, 0.3),
                rng.range(0.8, 1.5),
                Vec3::new(0.4, 0.5, 0.4),
            );
        }

        let num_walls = rng.range(0.0, 4.0) as u32;
        for i in 0..num_walls {
            let angle = i as f32 * std::f32::consts::FRAC_PI_2 + rng.range(-0.3, 0.3);
            let wall_len = rng.range(8.0, 20.0);
            let x = angle.cos() * 30.0;
            let z = angle.sin() * 30.0;
            let y = hm.sample(x, z);
            
            let e = world.spawn();
            world.insert(e, Name(format!("Wall_{}", i)));
            world.insert(
                e,
                Transform {
                    position: Vec3::new(x, y + 2.0, z),
                    rotation: Quat::from_rotation_y(angle),
                    scale: Vec3::new(wall_len, 4.0, 0.6),
                },
            );
            world.insert(e, MeshHandle("cube".into()));
            world.insert(e, MaterialHandle("arena_wall".into()));
            world.insert(e, RigidBody::static_body());
            world.insert(e, Collider::aabb(Vec3::splat(0.5)));
        }

        if rng.chance(0.6) {
            let gx = rng.range(-20.0, 20.0);
            let gz = rng.range(-20.0, 20.0);
            for i in 0..3 {
                for j in 0..3 {
                    let x = gx + i as f32 * 1.5;
                    let z = gz + j as f32 * 1.5;
                    let y = hm.sample(x, z);
                    Self::spawn_prop(
                        world,
                        &format!("Grave_{}_{}", i, j),
                        "gravestone",
                        "arena_column",
                        Vec3::new(x, y + 0.4, z),
                        rng.range(-0.2, 0.2),
                        0.7,
                        Vec3::splat(0.25),
                    );
                }
            }
        }

        let num_torches = rng.range(2.0, 6.0) as u32;
        for _ in 0..num_torches {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.range(5.0, 25.0);
            let x = a.cos() * r;
            let z = a.sin() * r;
            Self::spawn_torch(world, Vec3::new(x, hm.sample(x, z), z));
        }

        Self::scatter_props(
            world,
            hm,
            config,
            rng,
            30.0,
            &[
                ("tree_pine_a", "bark", 1.0),
                ("tree_pine_b", "bark", 0.8),
                ("tree_oak_a", "bark", 0.9),
                ("rock_cluster_a", "arena_column", 1.0),
                ("rock_cluster_b", "arena_column", 0.7),
                ("rock_cluster_c", "arena_column", 1.2),
            ],
        );

        Vec3::new(0.0, hm.sample(0.0, 0.0) + 2.0, 0.0)
    }

    fn build_forest(
        world: &mut World,
        hm: &Heightmap,
        config: &WorldGenConfig,
        rng: &mut Lcg,
    ) -> Vec3 {
        let clearing_r = rng.range(4.0, 15.0);
        let num_clearings = rng.range(1.0, 3.0) as u32;
        
        Self::scatter_props(
            world,
            hm,
            config,
            rng,
            clearing_r,
            &[
                ("tree_pine_a", "bark", 1.0),
                ("tree_pine_b", "bark", 0.85),
                ("tree_pine_a", "bark", 1.1),
                ("tree_oak_a", "bark", 0.95),
                ("tree_oak_b", "bark", 0.9),
                ("rock_cluster_a", "arena_column", 0.9),
                ("rock_cluster_b", "arena_column", 0.7),
                ("rock_cluster_c", "arena_column", 1.1),
            ],
        );

        for _ in 0..num_clearings {
            let cx = rng.range(-config.size * 0.3, config.size * 0.3);
            let cz = rng.range(-config.size * 0.3, config.size * 0.3);
            let r = rng.range(3.0, 8.0);
            
            // Очищаем зону от деревьев, спавня пни
            let num_stumps = rng.range(1.0, 4.0) as u32;
            for i in 0..num_stumps {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let dist = rng.range(0.0, r);
                let x = cx + a.cos() * dist;
                let z = cz + a.sin() * dist;
                let y = hm.sample(x, z);
                Self::spawn_prop(
                    world,
                    &format!("Stump_{}_{}", i, cx as i32),
                    "crate_detail",
                    "arena_crate",
                    Vec3::new(x, y + 0.3, z),
                    rng.range(0.0, std::f32::consts::TAU),
                    0.6,
                    Vec3::splat(0.4),
                );
            }
        }

        Vec3::new(0.0, hm.sample(0.0, 0.0) + 2.0, 0.0)
    }

    fn build_dungeon(
        world: &mut World,
        hm: &Heightmap,
        _config: &WorldGenConfig,
        size: f32,
        rng: &mut Lcg,
    ) -> Vec3 {
        let room_h = 6.0;
        let wall_t = 0.6;
        let half = size * 0.5;

        // 1. Внешние стены
        let walls = [
            (Vec3::new(0.0, room_h * 0.5, -half), Vec3::new(size, room_h, wall_t)),
            (Vec3::new(0.0, room_h * 0.5, half), Vec3::new(size, room_h, wall_t)),
            (Vec3::new(-half, room_h * 0.5, 0.0), Vec3::new(wall_t, room_h, size)),
            (Vec3::new(half, room_h * 0.5, 0.0), Vec3::new(wall_t, room_h, size)),
        ];

        for (i, (pos, sz)) in walls.iter().enumerate() {
            let e = world.spawn();
            world.insert(e, Name(format!("OuterWall_{}", i)));
            world.insert(
                e,
                Transform {
                    position: Vec3::new(pos.x, hm.sample(pos.x, pos.z) + pos.y, pos.z),
                    rotation: Quat::IDENTITY,
                    scale: *sz,
                },
            );
            world.insert(e, MeshHandle("cube".into()));
            world.insert(e, MaterialHandle("rpg_dungeon".into()));
            world.insert(e, RigidBody::static_body());
            world.insert(e, Collider::aabb(Vec3::splat(0.5)));
        }

        // 2. Процедурный лабиринт (сетка комнат)
        let grid_size = rng.range(2.0, 5.0) as u32;
        let cell_w = size / grid_size as f32;
        
        for gx in 0..grid_size {
            for gz in 0..grid_size {
                let cx = -half + (gx as f32 + 0.5) * cell_w;
                let cz = -half + (gz as f32 + 0.5) * cell_w;
                
                // Случайно ставим стены между комнатами
                if gx < grid_size - 1 && rng.chance(0.7) {
                    let wx = cx + cell_w * 0.5;
                    let e = world.spawn();
                    world.insert(e, Name(format!("InnerWallX_{}_{}", gx, gz)));
                    world.insert(
                        e,
                        Transform {
                            position: Vec3::new(wx, hm.sample(wx, cz) + room_h * 0.5, cz),
                            rotation: Quat::IDENTITY,
                            scale: Vec3::new(wall_t, room_h, cell_w * 0.8),
                        },
                    );
                    world.insert(e, MeshHandle("cube".into()));
                    world.insert(e, MaterialHandle("rpg_dungeon".into()));
                    world.insert(e, RigidBody::static_body());
                    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
                }
                
                if gz < grid_size - 1 && rng.chance(0.7) {
                    let wz = cz + cell_w * 0.5;
                    let e = world.spawn();
                    world.insert(e, Name(format!("InnerWallZ_{}_{}", gx, gz)));
                    world.insert(
                        e,
                        Transform {
                            position: Vec3::new(cx, hm.sample(cx, wz) + room_h * 0.5, wz),
                            rotation: Quat::IDENTITY,
                            scale: Vec3::new(cell_w * 0.8, room_h, wall_t),
                        },
                    );
                    world.insert(e, MeshHandle("cube".into()));
                    world.insert(e, MaterialHandle("rpg_dungeon".into()));
                    world.insert(e, RigidBody::static_body());
                    world.insert(e, Collider::aabb(Vec3::splat(0.5)));
                }
                
                // Случайная колонна или факел в комнате
                if rng.chance(0.5) {
                    Self::spawn_prop(
                        world,
                        &format!("Column_{}_{}", gx, gz),
                        "column_fluted",
                        "arena_column",
                        Vec3::new(cx, hm.sample(cx, cz) + room_h * 0.5, cz),
                        rng.range(0.0, std::f32::consts::TAU),
                        1.0,
                        Vec3::splat(0.5),
                    );
                } else if rng.chance(0.3) {
                    Self::spawn_torch(world, Vec3::new(cx, hm.sample(cx, cz), cz));
                }
                
                // Ящики
                if rng.chance(0.4) {
                    let x = cx + rng.range(-cell_w * 0.3, cell_w * 0.3);
                    let z = cz + rng.range(-cell_w * 0.3, cell_w * 0.3);
                    let mesh = if rng.chance(0.5) { "crate_detail" } else { "barrel_detail" };
                    let mat = if rng.chance(0.5) { "arena_crate" } else { "arena_barrel" };
                    Self::spawn_prop(
                        world,
                        &format!("Crate_{}_{}", gx, gz),
                        mesh,
                        mat,
                        Vec3::new(x, hm.sample(x, z) + 0.5, z),
                        rng.range(0.0, std::f32::consts::TAU),
                        1.0,
                        Vec3::splat(0.5),
                    );
                }
            }
        }

        Vec3::new(0.0, hm.sample(0.0, 0.0) + 2.0, 0.0)
    }

    fn scatter_props(
        world: &mut World,
        hm: &Heightmap,
        config: &WorldGenConfig,
        rng: &mut Lcg,
        min_r: f32,
        palette: &[(&'static str, &'static str, f32)],
    ) {
        let cell = rng.range(6.0, 12.0);
        let half = config.size * 0.5;

        let mut x = -half + cell * 0.5;
        while x < half {
            let mut z = -half + cell * 0.5;
            while z < half {
                let px = x + rng.range(-cell * 0.4, cell * 0.4);
                let pz = z + rng.range(-cell * 0.4, cell * 0.4);

                let dist_to_center = (px * px + pz * pz).sqrt();
                if dist_to_center < min_r {
                    z += cell;
                    continue;
                }

                if rng.chance(config.prop_density * 0.5) {
                    let n = hm.normal_at(px, pz);
                    if n.y < 0.85 {
                        z += cell;
                        continue;
                    }
                    let py = hm.sample(px, pz);

                    let (mesh, mat, scale) = *rng.pick(palette);
                    let s = scale * rng.range(0.8, 1.3);

                    let half_ext = if mesh.starts_with("tree") {
                        Vec3::new(0.25, 1.5, 0.25)
                    } else {
                        Vec3::new(0.6, 0.6, 0.6)
                    };

                    Self::spawn_prop(
                        world,
                        &format!("Prop_{}_{}", mesh, (px * 100.0) as i32),
                        mesh,
                        mat,
                        Vec3::new(px, py, pz),
                        rng.range(0.0, std::f32::consts::TAU),
                        s,
                        half_ext,
                    );
                }
                z += cell;
            }
            x += cell;
        }
    }

    fn spawn_enemies(
        world: &mut World,
        hm: &Heightmap,
        config: &WorldGenConfig,
        rng: &mut Lcg,
        player_spawn: Vec3,
    ) {
        let max_r = config.size * 0.45;
        let mut spawned = 0;
        let mut attempts = 0;
        
        let actual_enemy_count = config.enemy_count + rng.range(0.0, 6.0) as u32;

        while spawned < actual_enemy_count && attempts < actual_enemy_count * 20 {
            attempts += 1;
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.range(15.0, max_r);
            let x = a.cos() * r;
            let z = a.sin() * r;
            let y = hm.sample(x, z);

            if (Vec3::new(x, y, z) - player_spawn).length() < 15.0 {
                continue;
            }

            let e = world.spawn();
            world.insert(e, Name(format!("Enemy_{}", spawned)));
            world.insert(e, Transform::at(Vec3::new(x, y + 0.9, z)).with_scale(rng.range(0.6, 1.2)));
            world.insert(e, MeshHandle("capsule".into()));
            world.insert(
                e,
                MaterialHandle(
                    if rng.chance(0.3) { "flat_red" } else { "rpg_dungeon" }.into(),
                ),
            );
            world.insert(e, Collider::capsule(0.5, 1.8));
            world.insert(e, Health::new(rng.range(40.0, 100.0)));
            world.insert(e, Enemy);

            let agent = AiAgent::new()
                .with_speed(rng.range(2.0, 6.0))
                .with_vision(rng.range(10.0, 25.0), rng.range(30.0, 70.0).to_radians())
                .with_hearing(rng.range(10.0, 30.0))
                .with_attack(rng.range(1.0, 2.5), rng.range(5.0, 15.0), rng.range(0.5, 1.5));
            world.insert(e, agent);

            let num_pts = rng.range(2.0, 5.0) as u32;
            let mut patrol_pts = Vec::new();
            for _ in 0..num_pts {
                let dx = rng.range(-8.0, 8.0);
                let dz = rng.range(-8.0, 8.0);
                let px = x + dx;
                let pz = z + dz;
                patrol_pts.push(Vec3::new(px, hm.sample(px, pz), pz));
            }
            world.insert(e, PatrolPath::new(patrol_pts));

            spawned += 1;
        }
    }

    fn spawn_loot(
        world: &mut World,
        hm: &Heightmap,
        config: &WorldGenConfig,
        rng: &mut Lcg,
        player_spawn: Vec3,
    ) {
        let mut items: Vec<(&'static str, &'static str, u32, [f32; 4])> = vec![
            ("health_potion", "emissive_cold", 40, [0.5, 1.0, 0.5, 1.0]),
            ("antidote", "emissive_cold", 25, [0.4, 0.9, 0.7, 1.0]),
            ("gold_coin", "gold", 50, [1.0, 0.85, 0.3, 1.0]),
            ("ruby_gem", "gold", 100, [1.0, 0.4, 0.4, 1.0]),
        ];

        let max_r = config.size * 0.4;
        let mut spawned = 0;
        let mut attempts = 0;
        
        let actual_loot_count = config.loot_count + rng.range(0.0, 8.0) as u32;

        while spawned < actual_loot_count && attempts < actual_loot_count * 20 {
            attempts += 1;
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.range(5.0, max_r);
            let x = a.cos() * r;
            let z = a.sin() * r;
            let y = hm.sample(x, z);

            if (Vec3::new(x, y, z) - player_spawn).length() < 5.0 {
                continue;
            }

            let (item_id, mat, _value, tint) = *rng.pick(&items);

            let e = world.spawn();
            world.insert(e, Name(format!("Pickup_{}_{}", item_id, spawned)));
            world.insert(e, Transform::at(Vec3::new(x, y + 0.5, z)).with_scale(0.4));
            world.insert(e, MeshHandle("cube".into()));
            world.insert(e, MaterialHandle(mat.into()));
            world.insert(e, Tint(tint));
            world.insert(e, Spinner::new(Vec3::Y, 2.0));
            world.insert(
                e,
                Trigger::repeatable(1.5, TriggerAction::PlaySound("pickup".into())),
            );
            world.insert(e, GoldValue(1));

            items.rotate_left(1);
            spawned += 1;
        }
    }
}