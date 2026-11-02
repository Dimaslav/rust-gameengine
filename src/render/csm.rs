use glam::{Mat4, Vec3, Vec4};

use super::camera::{look_at_rh, orthographic_rh};

pub const CASCADE_COUNT: usize = 3;
pub const CASCADE_SIZE: u32 = 2048;

pub fn split_distances(near: f32, far: f32, lambda: f32) -> [f32; CASCADE_COUNT + 1] {
    let mut result = [0.0f32; CASCADE_COUNT + 1];
    result[0] = near;
    for i in 1..=CASCADE_COUNT {
        let p = i as f32 / CASCADE_COUNT as f32;
        let log = near * (far / near).powf(p);
        let uni = near + (far - near) * p;
        result[i] = lambda * log + (1.0 - lambda) * uni;
    }
    result
}

pub fn build_cascades(
    camera_view: Mat4,
    camera_proj: Mat4,
    camera_near: f32,
    camera_far: f32,
    sun_dir: Vec3,
    splits: &[f32; CASCADE_COUNT + 1],
) -> [Mat4; CASCADE_COUNT] {
    let inv_view = camera_view.inverse();
    let inv_proj = camera_proj.inverse();

    let mut result = [Mat4::IDENTITY; CASCADE_COUNT];

    for i in 0..CASCADE_COUNT {
        let near_i = splits[i];
        let far_i = splits[i + 1];

        let ndc_z_near = if near_i > camera_near {
            camera_far * (near_i - camera_near) / (near_i * (camera_far - camera_near))
        } else { 0.0 };
        let ndc_z_far = if far_i > camera_near {
            camera_far * (far_i - camera_near) / (far_i * (camera_far - camera_near))
        } else { 1.0 };

        let corners_ndc = [
            Vec4::new(-1.0, -1.0, ndc_z_near, 1.0),
            Vec4::new( 1.0, -1.0, ndc_z_near, 1.0),
            Vec4::new(-1.0,  1.0, ndc_z_near, 1.0),
            Vec4::new( 1.0,  1.0, ndc_z_near, 1.0),
            Vec4::new(-1.0, -1.0, ndc_z_far,  1.0),
            Vec4::new( 1.0, -1.0, ndc_z_far,  1.0),
            Vec4::new(-1.0,  1.0, ndc_z_far,  1.0),
            Vec4::new( 1.0,  1.0, ndc_z_far,  1.0),
        ];

        let mut corners_world = [Vec3::ZERO; 8];
        for (j, c) in corners_ndc.iter().enumerate() {
            let world = inv_view * inv_proj * *c;
            corners_world[j] = world.truncate() / world.w;
        }

        let mut center = Vec3::ZERO;
        for c in &corners_world { center += *c; }
        center /= 8.0;

        let mut radius = 0.0f32;
        for c in &corners_world { radius = radius.max((*c - center).length()); }
        let radius = (radius * 16.0).ceil() / 16.0;

        let sun = sun_dir.normalize_or_zero();
        let eye = center + sun * radius * 2.0;

        let v = look_at_rh(eye, center, Vec3::Y);
        let proj = orthographic_rh(-radius, radius, -radius, radius, 0.1, radius * 4.0);

        result[i] = proj * v;
    }

    result
}