//! Gizmo: 3D-манипулятор для translate / rotate / scale.
//!
//! Hit-test делается в **экранном пространстве**: положение каждой оси
//! проецируется через view-projection в пиксели, и расстояние до
//! соответствующего отрезка сравнивается с порогом в пикселях.
//!
//! ## Hierarchy-aware drag
//!
//! Раньше `apply_drag_with_mode` наивно прибавлял мировой offset к
//! локальному `Transform.position` / умножал на локальную `rotation` /
//! `scale`. Это работало для root-entity, но ломалось для parented:
//!   * Translate: мировой offset ≠ локальный offset при повёрнутом родителе.
//!   * Rotate: ось вращения мировая, а rotation — локальная.
//!   * Scale: world-axis scale ≠ local-axis scale.
//!
//! Теперь `begin_drag` кэширует `world_position`, `world_rotation` и
//! `inverse(parent_world)` каждой entity на момент старта drag'а.
//! `apply_drag` вычисляет новую world-трансформацию, а затем конвертирует
//! её в local через `inverse(parent_world_at_start)`.
//!
//! Сущности, чей предок тоже выделен, **пропускаются**: движение/поворот
//! предка автоматически распространяется на потомка через иерархию.
//! Иначе offset применился бы дважды.

use std::collections::HashSet;

use glam::{Mat4, Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::components::{MeshHandle, Parent, Transform};
use crate::render::{Camera3D, LineBatch, Renderer};

const PIXEL_THRESHOLD: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum GizmoMode {
    #[default]
    Translate,
    Rotate,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn to_vec(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }

    pub fn color(self) -> [f32; 4] {
        match self {
            Axis::X => [1.0, 0.20, 0.20, 1.0],
            Axis::Y => [0.20, 1.0, 0.20, 1.0],
            Axis::Z => [0.20, 0.40, 1.0, 1.0],
        }
    }

    pub fn color_hl(self) -> [f32; 4] {
        match self {
            Axis::X => [1.0, 0.7, 0.2, 1.0],
            Axis::Y => [0.9, 1.0, 0.2, 1.0],
            Axis::Z => [0.5, 0.8, 1.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GizmoState {
    pub mode: GizmoMode,
    pub hovered: Option<Axis>,
    pub drag: Option<GizmoDrag>,
    pub snap_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct GizmoDrag {
    pub axis: Axis,
    pub start_states: Vec<(Entity, Transform)>,
    /// World-space position каждой entity в момент старта drag'а.
    pub start_world_positions: Vec<Vec3>,
    /// World-space rotation каждой entity в момент старта drag'а.
    pub start_world_rotations: Vec<Quat>,
    /// `inverse(parent_world)` на момент старта drag'а. `None` для root.
    pub start_parent_worlds_inv: Vec<Option<Mat4>>,
    /// World-rotation родителя на момент старта. `None` для root.
    pub start_parent_rotations: Vec<Option<Quat>>,
    pub start_center: Vec3,
    pub start_ray_t: f32,
    pub start_point: Vec3,
    pub start_scale_along_axis: f32,
}

// ============================================================
// Центр группы — учитывает Parent.
// ============================================================

pub fn group_center(world: &World, selected: &[Entity]) -> Option<Vec3> {
    let mut sum = Vec3::ZERO;
    let mut n = 0;
    for &e in selected {
        if let Some(p) = crate::game::world_position(world, e) {
            sum += p;
            n += 1;
        }
    }
    if n == 0 { None } else { Some(sum / n as f32) }
}

pub fn group_radius(world: &World, selected: &[Entity], renderer: &Renderer) -> f32 {
    let Some(center) = group_center(world, selected) else {
        return 1.0;
    };
    let mut r: f32 = 0.5;
    for &e in selected {
        let Some(mh) = world.get::<MeshHandle>(e) else { continue };
        let Some(mesh) = renderer.meshes.get(&mh.0) else { continue };
        let model = crate::game::world_matrix(world, e);
        let (c, mr) = mesh.world_bounds(&model);
        let d = (c - center).length() + mr;
        if d > r { r = d; }
    }
    r
}

fn gizmo_scale(camera: &Camera3D, center: Vec3) -> f32 {
    let dist = (camera.position() - center).length();
    (dist * 0.12).clamp(0.3, 20.0)
}

// ============================================================
// Проекция
// ============================================================

fn project_to_screen(
    view_proj: &glam::Mat4,
    screen_w: f32,
    screen_h: f32,
    p: Vec3,
) -> Option<(f32, f32)> {
    let clip = *view_proj * p.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    let x = (ndc.x * 0.5 + 0.5) * screen_w;
    let y = (1.0 - (ndc.y * 0.5 + 0.5)) * screen_h;
    Some((x, y))
}

fn dist_point_segment_px(m: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (mx, my) = m;
    let (ax, ay) = a;
    let (bx, by) = b;
    let abx = bx - ax;
    let aby = by - ay;
    let apx = mx - ax;
    let apy = my - ay;
    let ab2 = abx * abx + aby * aby;
    let t = if ab2 < 1e-6 {
        0.0
    } else {
        ((apx * abx + apy * aby) / ab2).clamp(0.0, 1.0)
    };
    let cx = ax + abx * t;
    let cy = ay + aby * t;
    let dx = mx - cx;
    let dy = my - cy;
    (dx * dx + dy * dy).sqrt()
}

// ============================================================
// Hit testing (в пикселях экрана)
// ============================================================

pub fn pick_axis(
    world: &World,
    selected: &[Entity],
    camera: &Camera3D,
    mode: GizmoMode,
    renderer: &Renderer,
    sx: f32,
    sy: f32,
) -> Option<Axis> {
    let center = group_center(world, selected)?;
    let scale = gizmo_scale(camera, center);

    let view_proj = camera.view_projection();
    let screen_w = renderer.size.width as f32;
    let screen_h = renderer.size.height as f32;

    let m = (sx, sy);
    let mut best: Option<(Axis, f32)> = None;

    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let a = axis.to_vec();
        let d_px: f32 = match mode {
            GizmoMode::Translate | GizmoMode::Scale => {
                let p0 = match project_to_screen(&view_proj, screen_w, screen_h, center) {
                    Some(p) => p,
                    None => continue,
                };
                let p1 = match project_to_screen(
                    &view_proj,
                    screen_w,
                    screen_h,
                    center + a * scale,
                ) {
                    Some(p) => p,
                    None => continue,
                };
                dist_point_segment_px(m, p0, p1)
            }
            GizmoMode::Rotate => {
                let (t1, t2) = ring_basis(a);
                let segs = 32usize;
                let mut best_d = f32::INFINITY;
                let mut prev: Option<(f32, f32)> = None;
                for i in 0..=segs {
                    let ang = i as f32 / segs as f32 * std::f32::consts::TAU;
                    let world_p = center + (t1 * ang.cos() + t2 * ang.sin()) * scale;
                    let p = match project_to_screen(
                        &view_proj, screen_w, screen_h, world_p,
                    ) {
                        Some(p) => p,
                        None => { prev = None; continue; }
                    };
                    if let Some(prev_p) = prev {
                        let d = dist_point_segment_px(m, prev_p, p);
                        if d < best_d { best_d = d; }
                    }
                    prev = Some(p);
                }
                best_d
            }
        };

        if d_px <= PIXEL_THRESHOLD && best.is_none_or(|(_, bd)| d_px < bd) {
            best = Some((axis, d_px));
        }
    }

    best.map(|(a, _)| a)
}

fn ring_basis(normal: Vec3) -> (Vec3, Vec3) {
    let up = if normal.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let t1 = up.cross(normal).normalize();
    let t2 = normal.cross(t1).normalize();
    (t1, t2)
}

// ============================================================
// Математика
// ============================================================

fn closest_point_on_axis(
    ro: Vec3,
    rd: Vec3,
    center: Vec3,
    axis: Vec3,
) -> Option<(Vec3, f32)> {
    let d1 = rd.normalize_or_zero();
    let d2 = axis.normalize_or_zero();
    if d1.length_squared() < 1e-6 || d2.length_squared() < 1e-6 {
        return None;
    }
    let w0 = ro - center;

    let a = d1.dot(d1);
    let b = d1.dot(d2);
    let c = d2.dot(d2);
    let d = d1.dot(w0);
    let e = d2.dot(w0);

    let denom = a * c - b * b;
    if denom.abs() < 1e-6 {
        return None;
    }

    let ray_t = (b * e - c * d) / denom;
    let axis_t = (a * e - b * d) / denom;

    if ray_t < 0.0 {
        return None;
    }

    Some((center + d2 * axis_t, axis_t))
}

fn intersect_plane(
    ro: Vec3,
    rd: Vec3,
    center: Vec3,
    normal: Vec3,
) -> Option<(Vec3, f32)> {
    let n = normal.normalize_or_zero();
    if n.length_squared() < 1e-6 { return None; }
    let denom = n.dot(rd);
    if denom.abs() < 1e-6 { return None; }
    let t = (center - ro).dot(n) / denom;
    if t < 0.0 { return None; }
    Some((ro + rd * t, t))
}

// ============================================================
// Рисование
// ============================================================

pub fn draw_gizmo(
    batch: &mut LineBatch,
    world: &World,
    selected: &[Entity],
    camera: &Camera3D,
    state: &GizmoState,
) {
    let Some(center) = group_center(world, selected) else { return; };
    let scale = gizmo_scale(camera, center);

    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let a = axis.to_vec();
        let hl = state.hovered == Some(axis)
            || state.drag.as_ref().map(|d| d.axis == axis).unwrap_or(false);
        let color = if hl { axis.color_hl() } else { axis.color() };

        match state.mode {
            GizmoMode::Translate => {
                let end = center + a * scale;
                batch.line(center, end, color);
                let up = if axis == Axis::Y { Vec3::X } else { Vec3::Y };
                let side = a.cross(up).normalize_or_zero() * scale * 0.08;
                let tip = end - a * scale * 0.15;
                batch.line(tip - side, end, color);
                batch.line(tip + side, end, color);
            }
            GizmoMode::Rotate => draw_ring(batch, center, a, scale, color),
            GizmoMode::Scale => {
                let end = center + a * scale;
                batch.line(center, end, color);
                draw_box(batch, end, scale * 0.08, color);
            }
        }
    }
}

fn draw_ring(
    batch: &mut LineBatch,
    center: Vec3,
    normal: Vec3,
    radius: f32,
    color: [f32; 4],
) {
    let (t1, t2) = ring_basis(normal);
    let segs = 32;
    for i in 0..segs {
        let a1 = i as f32 / segs as f32 * std::f32::consts::TAU;
        let a2 = (i + 1) as f32 / segs as f32 * std::f32::consts::TAU;
        let p1 = center + (t1 * a1.cos() + t2 * a1.sin()) * radius;
        let p2 = center + (t1 * a2.cos() + t2 * a2.sin()) * radius;
        batch.line(p1, p2, color);
    }
}

fn draw_box(batch: &mut LineBatch, center: Vec3, half: f32, color: [f32; 4]) {
    let h = Vec3::splat(half);
    let corners = [
        center + Vec3::new(-h.x, -h.y, -h.z),
        center + Vec3::new( h.x, -h.y, -h.z),
        center + Vec3::new( h.x,  h.y, -h.z),
        center + Vec3::new(-h.x,  h.y, -h.z),
        center + Vec3::new(-h.x, -h.y,  h.z),
        center + Vec3::new( h.x, -h.y,  h.z),
        center + Vec3::new( h.x,  h.y,  h.z),
        center + Vec3::new(-h.x,  h.y,  h.z),
    ];
    let edges = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];
    for (a, b) in edges {
        batch.line(corners[a], corners[b], color);
    }
}

// ============================================================
// Drag
// ============================================================

pub fn begin_drag(
    world: &World,
    selected: &[Entity],
    axis: Axis,
    mode: GizmoMode,
    camera: &Camera3D,
    renderer: &Renderer,
    sx: f32,
    sy: f32,
) -> Option<GizmoDrag> {
    let center = group_center(world, selected)?;
    let a = axis.to_vec();

    let (origin, dir) = camera.ray_from_screen(
        sx,
        sy,
        renderer.size.width as f32,
        renderer.size.height as f32,
    );

    let (start_point, start_ray_t) = match mode {
        GizmoMode::Translate | GizmoMode::Scale => {
            closest_point_on_axis(origin, dir, center, a)?
        }
        GizmoMode::Rotate => {
            intersect_plane(origin, dir, center, a)?
        }
    };

    let start_states: Vec<(Entity, Transform)> = selected
        .iter()
        .filter_map(|&e| world.get::<Transform>(e).map(|t| (e, *t)))
        .collect();
    if start_states.is_empty() {
        return None;
    }

    // Кэш world position/rotation + inverse(parent_world) для hierarchy-aware drag.
    let mut start_world_positions = Vec::with_capacity(start_states.len());
    let mut start_world_rotations = Vec::with_capacity(start_states.len());
    let mut start_parent_worlds_inv = Vec::with_capacity(start_states.len());
    let mut start_parent_rotations = Vec::with_capacity(start_states.len());

    for (e, _t) in &start_states {
        let world_mat = crate::game::world_matrix(world, *e);
        let (_s, r, p) = world_mat.to_scale_rotation_translation();
        start_world_positions.push(p);
        start_world_rotations.push(r);

        if let Some(Parent(parent)) = world.get::<Parent>(*e).copied() {
            let parent_world = crate::game::world_matrix(world, parent);
            let (_ps, pr, _pt) = parent_world.to_scale_rotation_translation();
            start_parent_worlds_inv.push(Some(parent_world.inverse()));
            start_parent_rotations.push(Some(pr));
        } else {
            start_parent_worlds_inv.push(None);
            start_parent_rotations.push(None);
        }
    }

    let mut scale_along = 0.0_f32;
    let mut n = 0;
    for (_, t) in &start_states {
        scale_along += match axis {
            Axis::X => t.scale.x.abs(),
            Axis::Y => t.scale.y.abs(),
            Axis::Z => t.scale.z.abs(),
        };
        n += 1;
    }
    let start_scale_along_axis = (scale_along / n.max(1) as f32).max(0.0001);

    Some(GizmoDrag {
        axis,
        start_states,
        start_world_positions,
        start_world_rotations,
        start_parent_worlds_inv,
        start_parent_rotations,
        start_center: center,
        start_ray_t,
        start_point,
        start_scale_along_axis,
    })
}

/// Предикат: у `entity` есть выделенный предок (движение предка распространяется
/// на потомка автоматически, поэтому `entity` нужно пропустить).
fn has_selected_ancestor(
    world: &World,
    entity: Entity,
    selected: &HashSet<Entity>,
) -> bool {
    let mut cur = entity;
    for _ in 0..64 {
        match world.get::<Parent>(cur).copied() {
            Some(Parent(p)) => {
                if selected.contains(&p) {
                    return true;
                }
                cur = p;
            }
            None => return false,
        }
    }
    // Цикл — считаем «есть предок», чтобы не давить двойной offset.
    true
}

pub fn apply_drag_with_mode(
    world: &mut World,
    drag: &GizmoDrag,
    mode: GizmoMode,
    snap: bool,
    camera: &Camera3D,
    renderer: &Renderer,
    sx: f32,
    sy: f32,
) {
    let axis = drag.axis.to_vec();
    let center = drag.start_center;

    let (origin, dir) = camera.ray_from_screen(
        sx,
        sy,
        renderer.size.width as f32,
        renderer.size.height as f32,
    );

    // Множество выделенных entity — для пропуска «потомков выделенных предков».
    let selected_set: HashSet<Entity> = drag.start_states.iter().map(|(e, _)| *e).collect();

    match mode {
        GizmoMode::Translate => {
            let Some((point, _)) = closest_point_on_axis(origin, dir, center, axis) else {
                return;
            };
            let mut delta = (point - drag.start_point).dot(axis);
            if snap {
                delta = (delta * 2.0).round() * 0.5;
            }
            let offset_world = axis * delta;

            for (i, (e, start_t)) in drag.start_states.iter().enumerate() {
                if has_selected_ancestor(world, *e, &selected_set) {
                    continue;
                }
                let start_world_pos = drag.start_world_positions[i];
                let new_world_pos = start_world_pos + offset_world;

                let new_local_pos = if let Some(parent_inv) = drag.start_parent_worlds_inv[i] {
                    parent_inv.transform_point3(new_world_pos)
                } else {
                    new_world_pos
                };

                let mut t = *start_t;
                t.position = new_local_pos;
                world.insert(*e, t);
            }
        }
        GizmoMode::Scale => {
            // Скалирование работает в локальном space: ось gizmo
            // совпадает с локальной осью scale только для неповёрнутых
            // и не-родительских entity. Для повёрнутых/в иерархии
            // это упрощение — полноценный fix требует SVD-разложения
            // матрицы (по сути — сферической интерполяции scale).
            let Some((point, _)) = closest_point_on_axis(origin, dir, center, axis) else {
                return;
            };
            let delta = (point - drag.start_point).dot(axis);
            let sensitivity = gizmo_scale(camera, center).max(0.001);
            let mut mul = 1.0 + delta / sensitivity;
            if snap {
                mul = (mul * 10.0).round() / 10.0;
            }
            mul = mul.clamp(0.01, 100.0);

            for (e, start_t) in &drag.start_states {
                if has_selected_ancestor(world, *e, &selected_set) {
                    continue;
                }
                let mut t = *start_t;
                match drag.axis {
                    Axis::X => t.scale.x = (start_t.scale.x * mul).max(0.01),
                    Axis::Y => t.scale.y = (start_t.scale.y * mul).max(0.01),
                    Axis::Z => t.scale.z = (start_t.scale.z * mul).max(0.01),
                }
                world.insert(*e, t);
            }
        }
        GizmoMode::Rotate => {
            let Some((point, _)) = intersect_plane(origin, dir, center, axis) else {
                return;
            };
            let v0 = (drag.start_point - center).normalize_or_zero();
            let v1 = (point - center).normalize_or_zero();

            if v0.length_squared() < 1e-6 || v1.length_squared() < 1e-6 {
                return;
            }

            let sin = axis.dot(v0.cross(v1));
            let cos = v0.dot(v1);
            let mut angle = sin.atan2(cos);
            if snap {
                let step = std::f32::consts::PI / 12.0;
                angle = (angle / step).round() * step;
            }
            let q_world = Quat::from_axis_angle(axis, angle);

            for (i, (e, start_t)) in drag.start_states.iter().enumerate() {
                if has_selected_ancestor(world, *e, &selected_set) {
                    continue;
                }
                let start_world_pos = drag.start_world_positions[i];
                let start_world_rot = drag.start_world_rotations[i];

                let new_world_pos = center + q_world * (start_world_pos - center);
                let new_world_rot = (q_world * start_world_rot).normalize();

                let (new_local_pos, new_local_rot) =
                    if let (Some(parent_inv), Some(parent_rot)) = (
                        drag.start_parent_worlds_inv[i],
                        drag.start_parent_rotations[i],
                    ) {
                        let local_pos = parent_inv.transform_point3(new_world_pos);
                        let local_rot = (parent_rot.inverse() * new_world_rot).normalize();
                        (local_pos, local_rot)
                    } else {
                        (new_world_pos, new_world_rot)
                    };

                let t = Transform {
                    position: new_local_pos,
                    rotation: new_local_rot,
                    scale: start_t.scale,
                };
                world.insert(*e, t);
            }
        }
    }
}