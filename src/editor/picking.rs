//! Picking: луч → BVH-accelerated raycast по треугольникам, box select.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::MeshHandle;
use crate::render::{Camera3D, Renderer};

pub fn pick_entity(
    world: &World,
    renderer: &Renderer,
    camera: &Camera3D,
    screen_x: f32,
    screen_y: f32,
) -> Option<Entity> {
    let (origin, dir) = camera.ray_from_screen(
        screen_x,
        screen_y,
        renderer.size.width as f32,
        renderer.size.height as f32,
    );
    pick_ray(world, renderer, origin, dir).map(|(e, _)| e)
}

/// Возвращает (entity, t_world) — ближайшее попадание луча.
///
/// `t_world` — расстояние в мировых единицах вдоль `dir` (который
/// должен быть нормализован). Это позволяет использовать результат
/// для сравнения с `interact_distance` / `gun_range`.
pub fn pick_ray(
    world: &World,
    renderer: &Renderer,
    origin: Vec3,
    dir: Vec3,
) -> Option<(Entity, f32)> {
    let mut best: Option<(Entity, f32)> = None;

    for &e in world.entities() {
        let Some(mh) = world.get::<MeshHandle>(e) else { continue };
        let Some(mesh) = renderer.meshes.get(&mh.0) else { continue };
        // Ленивое построение BVH: первый raycast по этому мешу
        // запустит Bvh::build, дальше результат закэширован.
        if mesh.bvh().is_empty() {
            continue;
        }

        let model = crate::game::world_matrix(world, e);

        // Broad phase — bounding sphere.
        let (center, radius) = mesh.world_bounds(&model);
        let Some(_) = ray_sphere_hit(origin, dir, center, radius) else {
            continue;
        };

        // Переводим луч в локальное пространство меша.
        let inv_model = model.inverse();
        let local_o = inv_model.transform_point3(origin);
        let local_d_unnorm = inv_model.transform_vector3(dir);
        let local_d_len = local_d_unnorm.length();
        if local_d_len < 1e-12 {
            continue;
        }
        let local_d = local_d_unnorm / local_d_len;

        // BVH raycast.
        // `t_local` — расстояние вдоль `local_d` в локальных единицах.
        // Пропорция: t_world = t_local / |M⁻¹ · dir|.
        if let Some((_tri_idx, t_local)) =
            mesh.bvh().raycast(local_o, local_d, f32::INFINITY, &mesh.triangles)
        {
            let t_world = t_local / local_d_len;
            if best.map_or(true, |(_, bt)| t_world < bt) {
                best = Some((e, t_world));
            }
        }
    }

    best
}

/// Box-select: вернуть все сущности, чья проекция bounding sphere
/// пересекается с экранным прямоугольником `rect`.
///
/// `rect` = (x0, y0, x1, y1) в физических пикселях, порядок любой.
pub fn entities_in_screen_rect(
    world: &World,
    renderer: &Renderer,
    camera: &Camera3D,
    rect: (f32, f32, f32, f32),
) -> Vec<Entity> {
    let (xa, ya, xb, yb) = rect;
    let (xmin, xmax) = if xa <= xb { (xa, xb) } else { (xb, xa) };
    let (ymin, ymax) = if ya <= yb { (ya, yb) } else { (yb, ya) };

    let w = renderer.size.width as f32;
    let h = renderer.size.height as f32;

    let mut result = Vec::new();

    for &e in world.entities() {
        let Some(mh) = world.get::<MeshHandle>(e) else { continue };
        let Some(mesh) = renderer.meshes.get(&mh.0) else { continue };

        let model = crate::game::world_matrix(world, e);
        let (center, radius) = mesh.world_bounds(&model);

        let Some((sx, sy)) = camera.project_to_screen(center, w, h) else { continue };

        // Оценка экранного радиуса сферы: проецируем точку,
        // отстоящую на `radius` вправо от центра, и берём расстояние.
        let right = camera.right();
        let edge_world = center + right * radius;
        let px_radius = match camera.project_to_screen(edge_world, w, h) {
            Some((ex, ey)) => ((ex - sx).powi(2) + (ey - sy).powi(2)).sqrt(),
            None => 0.0,
        };

        // Пересечение окружности (sx, sy, px_radius) с прямоугольником:
        // ближайшая точка прямоугольника к центру не должна быть
        // дальше, чем px_radius.
        let nearest_x = sx.clamp(xmin, xmax);
        let nearest_y = sy.clamp(ymin, ymax);
        let dx = sx - nearest_x;
        let dy = sy - nearest_y;
        if dx * dx + dy * dy <= px_radius * px_radius {
            result.push(e);
        }
    }

    result
}

fn ray_sphere_hit(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 { return None; }
    let t = -b - disc.sqrt();
    if t < 0.0 {
        let t2 = -b + disc.sqrt();
        if t2 < 0.0 { return None; }
        Some(t2)
    } else {
        Some(t)
    }
}