//! Gizmo: 3D-манипулятор для translate / rotate / scale.

use std::collections::HashSet;
use glam::{Mat4, Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::components::{MeshHandle, Parent, Transform};
use crate::render::{Camera3D, LineBatch, Renderer};

const PIXEL_THRESHOLD: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GizmoMode {
    #[default]
    Translate,
    Rotate,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X, Y, Z,
    XY, XZ, YZ,
}

impl Axis {
    pub fn to_vec(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
            _ => Vec3::ZERO,
        }
    }
    pub fn plane_axes(self) -> (Vec3, Vec3) {
        match self {
            Axis::XY => (Vec3::X, Vec3::Y),
            Axis::XZ => (Vec3::X, Vec3::Z),
            Axis::YZ => (Vec3::Y, Vec3::Z),
            _ => (Vec3::ZERO, Vec3::ZERO),
        }
    }
    pub fn normal(self) -> Vec3 {
        match self {
            Axis::XY => Vec3::Z,
            Axis::XZ => Vec3::Y,
            Axis::YZ => Vec3::X,
            _ => Vec3::ZERO,
        }
    }
    pub fn color(self) -> [f32; 4] {
        match self {
            Axis::X | Axis::YZ => [1.0, 0.20, 0.20, 1.0],
            Axis::Y | Axis::XZ => [0.20, 1.0, 0.20, 1.0],
            Axis::Z | Axis::XY => [0.20, 0.40, 1.0, 1.0],
        }
    }
    pub fn color_hl(self) -> [f32; 4] {
        match self {
            Axis::X | Axis::YZ => [1.0, 0.7, 0.2, 1.0],
            Axis::Y | Axis::XZ => [0.9, 1.0, 0.2, 1.0],
            Axis::Z | Axis::XY => [0.5, 0.8, 1.0, 1.0],
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
    pub start_world_positions: Vec<Vec3>,
    pub start_world_rotations: Vec<Quat>,
    pub start_parent_worlds_inv: Vec<Option<Mat4>>,
    pub start_parent_rotations: Vec<Option<Quat>>,
    pub start_center: Vec3,
    pub start_ray_t: f32,
    pub start_point: Vec3,
    pub start_scale_along_axis: f32,
}

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
    let Some(center) = group_center(world, selected) else { return 1.0; };
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

fn project_to_screen(view_proj: &glam::Mat4, screen_w: f32, screen_h: f32, p: Vec3) -> Option<(f32, f32)> {
    let clip = *view_proj * p.extend(1.0);
    if clip.w <= 1e-6 { return None; }
    let ndc = clip.truncate() / clip.w;
    Some(((ndc.x * 0.5 + 0.5) * screen_w, (1.0 - (ndc.y * 0.5 + 0.5)) * screen_h))
}

fn dist_point_segment_px(m: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (mx, my) = m; let (ax, ay) = a; let (bx, by) = b;
    let abx = bx - ax; let aby = by - ay; let apx = mx - ax; let apy = my - ay;
    let ab2 = abx * abx + aby * aby;
    let t = if ab2 < 1e-6 { 0.0 } else { ((apx * abx + apy * aby) / ab2).clamp(0.0, 1.0) };
    let cx = ax + abx * t; let cy = ay + aby * t;
    let dx = mx - cx; let dy = my - cy;
    (dx * dx + dy * dy).sqrt()
}

pub fn pick_axis(world: &World, selected: &[Entity], camera: &Camera3D, mode: GizmoMode, renderer: &Renderer, sx: f32, sy: f32) -> Option<Axis> {
    let center = group_center(world, selected)?;
    let scale = gizmo_scale(camera, center);
    let view_proj = camera.view_projection();
    let screen_w = renderer.size.width as f32;
    let screen_h = renderer.size.height as f32;
    let m = (sx, sy);
    let mut best: Option<(Axis, f32)> = None;

    for axis in [Axis::X, Axis::Y, Axis::Z, Axis::XY, Axis::XZ, Axis::YZ] {
        let (a1, a2) = axis.plane_axes();
        let d_px: f32 = match mode {
            GizmoMode::Translate | GizmoMode::Scale => {
                if a1 != Vec3::ZERO {
                    let p_center = center + (a1 + a2) * scale * 0.3;
                    match project_to_screen(&view_proj, screen_w, screen_h, p_center) {
                        Some(p) => ((m.0 - p.0).powi(2) + (m.1 - p.1).powi(2)).sqrt(),
                        None => continue,
                    }
                } else {
                    let a = axis.to_vec();
                    let p0 = match project_to_screen(&view_proj, screen_w, screen_h, center) { Some(p) => p, None => continue };
                    let p1 = match project_to_screen(&view_proj, screen_w, screen_h, center + a * scale) { Some(p) => p, None => continue };
                    dist_point_segment_px(m, p0, p1)
                }
            }
            GizmoMode::Rotate => {
                let a = axis.to_vec();
                if a == Vec3::ZERO { continue; }
                let (t1, t2) = ring_basis(a);
                let segs = 32usize;
                let mut best_d = f32::INFINITY;
                let mut prev: Option<(f32, f32)> = None;
                for i in 0..=segs {
                    let ang = i as f32 / segs as f32 * std::f32::consts::TAU;
                    let world_p = center + (t1 * ang.cos() + t2 * ang.sin()) * scale;
                    let p = match project_to_screen(&view_proj, screen_w, screen_h, world_p) {
                        Some(p) => p, None => { prev = None; continue; }
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

        let threshold = if a1 != Vec3::ZERO { PIXEL_THRESHOLD * 1.5 } else { PIXEL_THRESHOLD };
        if d_px <= threshold && best.is_none_or(|(_, bd)| d_px < bd) {
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

fn closest_point_on_axis(ro: Vec3, rd: Vec3, center: Vec3, axis: Vec3) -> Option<(Vec3, f32)> {
    let d1 = rd.normalize_or_zero(); let d2 = axis.normalize_or_zero();
    if d1.length_squared() < 1e-6 || d2.length_squared() < 1e-6 { return None; }
    let w0 = ro - center;
    let a = d1.dot(d1); let b = d1.dot(d2); let c = d2.dot(d2); let d = d1.dot(w0); let e = d2.dot(w0);
    let denom = a * c - b * b;
    if denom.abs() < 1e-6 { return None; }
    let ray_t = (b * e - c * d) / denom;
    let axis_t = (a * e - b * d) / denom;
    if ray_t < 0.0 { return None; }
    Some((center + d2 * axis_t, axis_t))
}

fn intersect_plane(ro: Vec3, rd: Vec3, center: Vec3, normal: Vec3) -> Option<(Vec3, f32)> {
    let n = normal.normalize_or_zero();
    if n.length_squared() < 1e-6 { return None; }
    let denom = n.dot(rd);
    if denom.abs() < 1e-6 { return None; }
    let t = (center - ro).dot(n) / denom;
    if t < 0.0 { return None; }
    Some((ro + rd * t, t))
}

pub fn draw_gizmo(batch: &mut LineBatch, world: &World, selected: &[Entity], camera: &Camera3D, state: &GizmoState) {
    let Some(center) = group_center(world, selected) else { return; };
    let scale = gizmo_scale(camera, center);

    for axis in [Axis::X, Axis::Y, Axis::Z, Axis::XY, Axis::XZ, Axis::YZ] {
        let (a1, a2) = axis.plane_axes();
        let hl = state.hovered == Some(axis) || state.drag.as_ref().map(|d| d.axis == axis).unwrap_or(false);
        let color = if hl { axis.color_hl() } else { axis.color() };

        if a1 != Vec3::ZERO {
            if state.mode == GizmoMode::Translate {
                let p0 = center + (a1 * 0.2 + a2 * 0.2) * scale;
                let p1 = center + (a1 * 0.4 + a2 * 0.2) * scale;
                let p2 = center + (a1 * 0.4 + a2 * 0.4) * scale;
                let p3 = center + (a1 * 0.2 + a2 * 0.4) * scale;
                batch.line(p0, p1, color); batch.line(p1, p2, color);
                batch.line(p2, p3, color); batch.line(p3, p0, color);
            }
            continue;
        }

        let a = axis.to_vec();
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
            GizmoMode::Rotate => {
                let (t1, t2) = ring_basis(a);
                let segs = 32;
                for i in 0..segs {
                    let a1 = i as f32 / segs as f32 * std::f32::consts::TAU;
                    let a2 = (i + 1) as f32 / segs as f32 * std::f32::consts::TAU;
                    batch.line(center + (t1 * a1.cos() + t2 * a1.sin()) * scale, center + (t1 * a2.cos() + t2 * a2.sin()) * scale, color);
                }
            }
            GizmoMode::Scale => {
                let end = center + a * scale;
                batch.line(center, end, color);
                let h = Vec3::splat(scale * 0.08);
                let box_center = end;
                for i in 0..8 {
                    let sign = Vec3::new(if i&1==0 {1.0} else {-1.0}, if i&2==0 {1.0} else {-1.0}, if i&4==0 {1.0} else {-1.0});
                    batch.line(box_center - h * sign, box_center - h * Vec3::new(sign.x, -sign.y, -sign.z), color);
                }
            }
        }
    }
}

pub fn begin_drag(world: &World, selected: &[Entity], axis: Axis, mode: GizmoMode, camera: &Camera3D, renderer: &Renderer, sx: f32, sy: f32) -> Option<GizmoDrag> {
    let center = group_center(world, selected)?;
    let (origin, dir) = camera.ray_from_screen(sx, sy, renderer.size.width as f32, renderer.size.height as f32);

    let (start_point, start_ray_t) = match mode {
        GizmoMode::Translate | GizmoMode::Scale => {
            if axis.plane_axes().0 != Vec3::ZERO {
                intersect_plane(origin, dir, center, axis.normal())?
            } else {
                closest_point_on_axis(origin, dir, center, axis.to_vec())?
            }
        }
        GizmoMode::Rotate => intersect_plane(origin, dir, center, axis.to_vec())?,
    };

    let start_states: Vec<(Entity, Transform)> = selected.iter().filter_map(|&e| world.get::<Transform>(e).map(|t| (e, *t))).collect();
    if start_states.is_empty() { return None; }

    let mut start_world_positions = Vec::with_capacity(start_states.len());
    let mut start_world_rotations = Vec::with_capacity(start_states.len());
    let mut start_parent_worlds_inv = Vec::with_capacity(start_states.len());
    let mut start_parent_rotations = Vec::with_capacity(start_states.len());

    for (e, _t) in &start_states {
        let world_mat = crate::game::world_matrix(world, *e);
        let (_s, r, p) = world_mat.to_scale_rotation_translation();
        start_world_positions.push(p); start_world_rotations.push(r);
        if let Some(Parent(parent)) = world.get::<Parent>(*e).copied() {
            let parent_world = crate::game::world_matrix(world, parent);
            let (_ps, pr, _pt) = parent_world.to_scale_rotation_translation();
            start_parent_worlds_inv.push(Some(parent_world.inverse())); start_parent_rotations.push(Some(pr));
        } else { start_parent_worlds_inv.push(None); start_parent_rotations.push(None); }
    }

    let mut scale_along = 0.0_f32; let mut n = 0;
    for (_, t) in &start_states {
        let a = axis.to_vec();
        scale_along += if a == Vec3::ZERO { 1.0 } else { a.abs().dot(t.scale.abs()) };
        n += 1;
    }
    let start_scale_along_axis = (scale_along / n.max(1) as f32).max(0.0001);

    Some(GizmoDrag { axis, start_states, start_world_positions, start_world_rotations, start_parent_worlds_inv, start_parent_rotations, start_center: center, start_ray_t, start_point, start_scale_along_axis })
}

fn has_selected_ancestor(world: &World, entity: Entity, selected: &HashSet<Entity>) -> bool {
    let mut cur = entity;
    for _ in 0..64 {
        match world.get::<Parent>(cur).copied() {
            Some(Parent(p)) => { if selected.contains(&p) { return true; } cur = p; }
            None => return false,
        }
    }
    true
}

pub fn apply_drag_with_mode(
    world: &mut World, drag: &GizmoDrag, mode: GizmoMode, snap: bool, snap_size: f32,
    camera: &Camera3D, renderer: &Renderer, sx: f32, sy: f32,
) {
    let center = drag.start_center;
    let (origin, dir) = camera.ray_from_screen(sx, sy, renderer.size.width as f32, renderer.size.height as f32);
    let selected_set: HashSet<Entity> = drag.start_states.iter().map(|(e, _)| *e).collect();

    match mode {
        GizmoMode::Translate => {
            let point = if drag.axis.plane_axes().0 != Vec3::ZERO {
                match intersect_plane(origin, dir, center, drag.axis.normal()) { Some(p) => p.0, None => return }
            } else {
                match closest_point_on_axis(origin, dir, center, drag.axis.to_vec()) { Some(p) => p.0, None => return }
            };
            
            let mut delta = point - drag.start_point;
            if drag.axis.plane_axes().0 == Vec3::ZERO {
                delta = drag.axis.to_vec() * delta.dot(drag.axis.to_vec());
            }
            
            if snap {
                if drag.axis.plane_axes().0 == Vec3::ZERO {
                    let d = (delta.length() / snap_size.max(0.001)).round() * snap_size;
                    delta = delta.normalize_or_zero() * d;
                } else {
                    delta.x = (delta.x / snap_size.max(0.001)).round() * snap_size;
                    delta.y = (delta.y / snap_size.max(0.001)).round() * snap_size;
                    delta.z = (delta.z / snap_size.max(0.001)).round() * snap_size;
                }
            }

            for (i, (e, start_t)) in drag.start_states.iter().enumerate() {
                if has_selected_ancestor(world, *e, &selected_set) { continue; }
                let new_world_pos = drag.start_world_positions[i] + delta;
                let new_local_pos = if let Some(parent_inv) = drag.start_parent_worlds_inv[i] {
                    parent_inv.transform_point3(new_world_pos)
                } else { new_world_pos };
                let mut t = *start_t; t.position = new_local_pos; world.insert(*e, t);
            }
        }
        GizmoMode::Scale => {
            let point = if drag.axis.plane_axes().0 != Vec3::ZERO {
                match intersect_plane(origin, dir, center, drag.axis.normal()) { Some(p) => p.0, None => return }
            } else {
                match closest_point_on_axis(origin, dir, center, drag.axis.to_vec()) { Some(p) => p.0, None => return }
            };
            let delta = (point - drag.start_point).dot(drag.axis.to_vec());
            let sensitivity = gizmo_scale(camera, center).max(0.001);
            let mut mul = 1.0 + delta / sensitivity;
            if snap { mul = (mul * 10.0).round() / 10.0; }
            mul = mul.clamp(0.01, 100.0);

            for (e, start_t) in &drag.start_states {
                if has_selected_ancestor(world, *e, &selected_set) { continue; }
                let mut t = *start_t;
                if drag.axis.to_vec() != Vec3::ZERO {
                    match drag.axis {
                        Axis::X => t.scale.x = (start_t.scale.x * mul).max(0.01),
                        Axis::Y => t.scale.y = (start_t.scale.y * mul).max(0.01),
                        Axis::Z => t.scale.z = (start_t.scale.z * mul).max(0.01),
                        _ => {}
                    }
                } else {
                    t.scale = (start_t.scale * mul).max(Vec3::splat(0.01));
                }
                world.insert(*e, t);
            }
        }
        GizmoMode::Rotate => {
            let point = match intersect_plane(origin, dir, center, drag.axis.to_vec()) { Some(p) => p.0, None => return };
            let v0 = (drag.start_point - center).normalize_or_zero();
            let v1 = (point - center).normalize_or_zero();
            if v0.length_squared() < 1e-6 || v1.length_squared() < 1e-6 { return; }
            let sin = drag.axis.to_vec().dot(v0.cross(v1));
            let cos = v0.dot(v1);
            let mut angle = sin.atan2(cos);
            if snap { angle = (angle / (std::f32::consts::PI / 12.0)).round() * (std::f32::consts::PI / 12.0); }
            let q_world = Quat::from_axis_angle(drag.axis.to_vec(), angle);

            for (i, (e, start_t)) in drag.start_states.iter().enumerate() {
                if has_selected_ancestor(world, *e, &selected_set) { continue; }
                let new_world_pos = center + q_world * (drag.start_world_positions[i] - center);
                let new_world_rot = (q_world * drag.start_world_rotations[i]).normalize();
                let (new_local_pos, new_local_rot) = if let (Some(parent_inv), Some(parent_rot)) = (drag.start_parent_worlds_inv[i], drag.start_parent_rotations[i]) {
                    (parent_inv.transform_point3(new_world_pos), (parent_rot.inverse() * new_world_rot).normalize())
                } else { (new_world_pos, new_world_rot) };
                world.insert(*e, Transform { position: new_local_pos, rotation: new_local_rot, scale: start_t.scale });
            }
        }
    }
}