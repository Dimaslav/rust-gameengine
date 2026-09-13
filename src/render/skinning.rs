//! Скелетная анимация: скелет, клипы, интерполяция.
//!
//! Skeleton хранит иерархию костей. `local_pose[i]` = локальная трансформация
//! кости в текущем кадре. `joint_matrices` = world_pose × inverse_bind —
//! то, что шейдер применяет к вершинам.

use glam::{Mat4, Quat, Vec3};

pub const MAX_JOINTS: usize = 64;
pub const PARENT_NONE: usize = usize::MAX;

/// Трек одного свойства кости (translation / rotation / scale).
#[derive(Debug, Clone)]
pub struct Track {
    /// Ключевые кадры по времени.
    pub times: Vec<f32>,
    /// Значения. Translation/Scale — xyz, Rotation — xyzw.
    pub values: Vec<[f32; 4]>,
}

impl Track {
    /// Линейная интерполяция (для rotation — slerp).
    pub fn sample(&self, time: f32, is_rotation: bool) -> [f32; 4] {
        if self.times.is_empty() {
            return [0.0, 0.0, 0.0, 1.0];
        }
        if time <= self.times[0] {
            return self.values[0];
        }
        if time >= *self.times.last().unwrap() {
            return *self.values.last().unwrap();
        }
        // Бинарный поиск.
        let mut lo = 0usize;
        let mut hi = self.times.len() - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.times[mid] <= time {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let t0 = self.times[lo];
        let t1 = self.times[hi];
        let alpha = ((time - t0) / (t1 - t0)).clamp(0.0, 1.0);
        let a = self.values[lo];
        let b = self.values[hi];

        if is_rotation {
            let qa = Quat::from_xyzw(a[0], a[1], a[2], a[3]);
            let qb = Quat::from_xyzw(b[0], b[1], b[2], b[3]);
            let q = qa.slerp(qb, alpha);
            [q.x, q.y, q.z, q.w]
        } else {
            [
                a[0] + (b[0] - a[0]) * alpha,
                a[1] + (b[1] - a[1]) * alpha,
                a[2] + (b[2] - a[2]) * alpha,
                a[3] + (b[3] - a[3]) * alpha,
            ]
        }
    }
}

/// Анимационный клип. `bones[i]` — треки для кости i.
#[derive(Debug, Clone, Default)]
pub struct AnimationClip {
    pub name: String,
    pub duration: f32,
    pub translations: Vec<Option<Track>>,
    pub rotations: Vec<Option<Track>>,
    pub scales: Vec<Option<Track>>,
}

impl AnimationClip {
    /// Считает локальные позы костей для момента времени `time`.
    /// Для костей без треков берёт bind-pose.
    pub fn local_pose(&self, time: f32, base_local: &[Mat4]) -> Vec<Mat4> {
        let n = base_local.len();
        let mut result = base_local.to_vec();

        // Разбираем bind-pose на компоненты для смешивания с треками.
        for i in 0..n {
            let (base_scale, base_rot, base_pos) =
                base_local[i].to_scale_rotation_translation();

            let trans = self
                .translations
                .get(i)
                .and_then(|t| t.as_ref())
                .map(|t| {
                    let v = t.sample(time, false);
                    Vec3::new(v[0], v[1], v[2])
                })
                .unwrap_or(base_pos);

            let rot = self
                .rotations
                .get(i)
                .and_then(|t| t.as_ref())
                .map(|t| {
                    let v = t.sample(time, true);
                    Quat::from_xyzw(v[0], v[1], v[2], v[3]).normalize()
                })
                .unwrap_or(base_rot);

            let scale = self
                .scales
                .get(i)
                .and_then(|t| t.as_ref())
                .map(|t| {
                    let v = t.sample(time, false);
                    Vec3::new(v[0], v[1], v[2])
                })
                .unwrap_or(base_scale);

            result[i] = Mat4::from_scale_rotation_translation(scale, rot, trans);
        }

        result
    }
}

#[derive(Debug, Clone)]
pub struct Skeleton {
    pub names: Vec<String>,
    /// Индекс узла glTF для каждой кости — нужен для маппинга animation channels.
    pub node_indices: Vec<usize>,
    pub parents: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
    pub local_bind: Vec<Mat4>,
}

impl Skeleton {
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn bind_pose(&self) -> Vec<Mat4> {
        self.local_bind.clone()
    }

    pub fn world_matrices(&self, local_pose: &[Mat4]) -> Vec<Mat4> {
        let n = self.names.len();
        let mut world = vec![Mat4::IDENTITY; n];
        for i in 0..n {
            let parent = self.parents[i];
            let local = if i < local_pose.len() {
                local_pose[i]
            } else {
                Mat4::IDENTITY
            };
            world[i] = if parent == PARENT_NONE {
                local
            } else {
                world[parent] * local
            };
        }
        world
    }

    pub fn joint_matrices(&self, local_pose: &[Mat4]) -> Vec<Mat4> {
        let world = self.world_matrices(local_pose);
        world
            .iter()
            .zip(&self.inverse_bind)
            .map(|(w, ib)| *w * *ib)
            .collect()
    }
}