//! Picking: луч → точный raycast по треугольникам, box select.

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

/// Возвращает (entity, t) — ближайшее попадание луча.
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

        let model = crate::game::world_matrix(world, e);
        let (center, radius) = mesh.world_bounds(&model);

        let Some(_t_sphere) = ray_sphere_hit(origin, dir, center, radius) else {
            continue;
        };

        let inv_model = model.inverse();
        let local_o = inv_model.transform_point3(origin);
        let local_d = inv_model.transform_vector3(dir).normalize();

        for tri in &mesh.triangles {
            if let Some(t) = ray_triangle(local_o, local_d, tri[0], tri[1], tri[2]) {
                let scale = (model.transform_vector3(local_d).length()).max(0.0001);
                let t_world = t / scale;
                if best.map_or(true, |(_, bt)| t_world < bt) {
                    best = Some((e, t_world));
                }
                break;
            }
        }
    }

    best
}

/// Box-select: вернуть все сущности, чей центр (в мировых координатах)
/// проецируется внутрь экранного прямоугольника `rect`.
///
/// `rect` = (x0, y0, x1, y1) в физических пикселях, порядок любой.
/// Проекция центра AABB — компромисс: работает быстро и не пропускает
/// крупные объекты, у которых центр за экраном, но объект виден
/// (для них центр обычно всё равно попадает в rect, если объект
/// достаточно крупный).
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
        let (center, _radius) = mesh.world_bounds(&model);

        let Some((sx, sy)) = camera.project_to_screen(center, w, h) else { continue };

        if sx >= xmin && sx <= xmax && sy >= ymin && sy <= ymax {
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

fn ray_triangle(ro: Vec3, rd: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let p = rd.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 { return None; }
    let inv_det = 1.0 / det;
    let tvec = ro - v0;
    let u = tvec.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) { return None; }
    let q = tvec.cross(e1);
    let v = rd.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 { return None; }
    let t = e2.dot(q) * inv_det;
    if t > 1e-5 { Some(t) } else { None }
}