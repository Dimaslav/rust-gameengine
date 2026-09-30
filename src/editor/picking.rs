//! Picking: луч из камеры → точный raycast по треугольникам мешей.
//!
//! Двухфазный: сначала быстрый AABB (bounding-сфера), потом —
//! точный Möller–Trumbore по треугольникам. Для 2000+ объектов
//! с узким отбором это работает за миллисекунды.

use glam::{Mat4, Vec3};

use crate::ecs::{Entity, World};
use crate::game::components::{MeshHandle, Parent, Transform};
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

    let mut best: Option<(Entity, f32)> = None;

    for &e in world.entities() {
        let Some(mh) = world.get::<MeshHandle>(e) else {
            continue;
        };
        let Some(mesh) = renderer.meshes.get(&mh.0) else {
            continue;
        };

        let model = super::super::game::world_matrix(world, e);
        let (center, radius) = mesh.world_bounds(&model);

        // Быстрый отсев по bounding-сфере.
        let Some(t_sphere) = ray_sphere_hit(origin, dir, center, radius) else {
            continue;
        };

        // Точный тест по треугольникам в локальном пространстве.
        let inv_model = model.inverse();
        let local_o = inv_model.transform_point3(origin);
        let local_d = inv_model.transform_vector3(dir).normalize();

        for tri in &mesh.triangles {
            if let Some(t) = ray_triangle(local_o, local_d, tri[0], tri[1], tri[2]) {
                // Пересчёт t в мировое пространство: длина local_d отличается,
                // но нам достаточно относительного порядка — оба t вдоль луча.
                // Используем масштаб по обратной матрице.
                let scale = (model.transform_vector3(local_d).length()).max(0.0001);
                let t_world = t / scale;
                let _ = t_sphere;
                if best.map_or(true, |(_, bt)| t_world < bt) {
                    best = Some((e, t_world));
                }
                break; // одного попадания в меш достаточно для t
            }
        }
    }

    best.map(|(e, _)| e)
}

fn ray_sphere_hit(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t < 0.0 {
        let t2 = -b + disc.sqrt();
        if t2 < 0.0 { return None; }
        Some(t2)
    } else {
        Some(t)
    }
}

/// Möller–Trumbore.
fn ray_triangle(ro: Vec3, rd: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let p = rd.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = ro - v0;
    let u = tvec.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = rd.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv_det;
    if t > 1e-5 { Some(t) } else { None }
}