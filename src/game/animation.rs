use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::ecs::{Entity, World};
use crate::game::components::{AnimationPlayer, RootMotion, SkeletonHandle, Transform};
use crate::render::skinning::{AnimationClip, Skeleton};
use crate::render::Renderer;
use glam::{Mat4, Quat, Vec3};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationEvent {
    pub time: f32,
    pub name: String,
    #[serde(default)]
    pub payload: Option<String>,
}

impl AnimationEvent {
    pub fn new(time: f32, name: impl Into<String>) -> Self {
        Self { time, name: name.into(), payload: None }
    }
    pub fn with_payload(mut self, p: impl Into<String>) -> Self {
        self.payload = Some(p.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct AnimationEventTriggered {
    pub entity: Entity,
    pub clip: String,
    pub name: String,
    pub payload: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AnimationEvents {
    by_clip: HashMap<String, Vec<AnimationEvent>>,
}

impl AnimationEvents {
    pub fn new() -> Self { Self::default() }
    pub fn add(&mut self, clip: impl Into<String>, event: AnimationEvent) {
        let list = self.by_clip.entry(clip.into()).or_default();
        list.push(event);
        list.sort_by(|a, b| {
            a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    pub fn with(mut self, clip: impl Into<String>, event: AnimationEvent) -> Self {
        self.add(clip, event);
        self
    }

    pub fn get(&self, clip: &str) -> &[AnimationEvent] {
        self.by_clip.get(clip).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool { self.by_clip.is_empty() }
    pub fn clip_count(&self) -> usize { self.by_clip.len() }
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("read {}", path.as_ref().display()))?;
        let file: AnimationEventsFile = ron::from_str(&text)
            .with_context(|| format!("parse {}", path.as_ref().display()))?;
        let mut events = Self::new();
        for (clip, list) in file.by_clip {
            for ev in list {
                events.add(clip.clone(), ev);
            }
        }
        Ok(events)
    }

    /// Сохранить в RON-файл.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let file = AnimationEventsFile { by_clip: self.by_clip.clone() };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .context("serialize AnimationEvents")?;
        std::fs::write(path.as_ref(), text)
            .with_context(|| format!("write {}", path.as_ref().display()))?;
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AnimationEventsFile {
    #[serde(default)]
    pub by_clip: HashMap<String, Vec<AnimationEvent>>,
}

pub struct AnimationRuntime {
    prev_times: HashMap<Entity, f32>,
}

impl AnimationRuntime {
    pub fn new() -> Self {
        Self { prev_times: HashMap::new() }
    }

    pub fn advance_all(
        &mut self,
        world: &mut World,
        renderer: &mut Renderer,
        clips: &HashMap<String, AnimationClip>,
        skeletons: &HashMap<String, Skeleton>,
        events: &AnimationEvents,
        dt: f32,
    ) -> Vec<AnimationEventTriggered> {
        let plain_entities: Vec<Entity> = world
            .query::<AnimationPlayer>()
            .map(|(e, _)| e)
            .collect();
        let blend_entities: Vec<Entity> = world
            .query::<crate::game::animation_blend::AnimationBlendPlayer>()
            .map(|(e, _)| e)
            .collect();

        let mut triggered: Vec<AnimationEventTriggered> = Vec::new();
        let mut root_motions: Vec<(Entity, Vec3, Quat)> = Vec::new();

        for e in &plain_entities {
            let e = *e;
            let (clip_name, looping, new_time) = {
                let Some(player) = world.get_mut::<AnimationPlayer>(e) else {
                    continue;
                };
                player.time += dt * player.speed;
                (player.clip.clone(), player.looping, player.time)
            };
            let prev_time = self.prev_times.get(&e).copied().unwrap_or(0.0);
            let Some(clip) = clips.get(&clip_name) else {
                self.prev_times.insert(e, new_time);
                continue;
            };
            let duration = clip.duration;
            let from_time = if looping && duration > 0.0 {
                prev_time % duration
            } else {
                prev_time.min(duration)
            };
            let to_time = if looping && duration > 0.0 {
                new_time % duration
            } else {
                new_time.min(duration)
            };

            for ev in events.get(&clip_name) {
                if event_in_range(ev.time, from_time, to_time, looping, duration) {
                    triggered.push(AnimationEventTriggered {
                        entity: e,
                        clip: clip_name.clone(),
                        name: ev.name.clone(),
                        payload: ev.payload.clone(),
                    });
                }
            }

            let root_motion_enabled = world
                .get::<RootMotion>(e)
                .map(|r| r.enabled)
                .unwrap_or(false)
                && !looping;

            // 7. Обновляем скелет + считаем root delta.
            let skel_handle = world.get::<SkeletonHandle>(e).cloned();
            if let Some(handle) = skel_handle
                && let Some(skel) = skeletons.get(&handle.0)
            {
                let final_time = if looping && duration > 0.0 {
                    to_time
                } else {
                    new_time.min(duration)
                };

                let root_idx = if root_motion_enabled {
                    skel.find_root_bone()
                } else {
                    None
                };
                let (local_pose, root_to) =
                    skel.local_pose_with_root(clip, final_time, root_idx);
                let joint_matrices = skel.joint_matrices(&local_pose);
                renderer.update_skeleton(&handle.0, &joint_matrices);
                if let (Some(idx), Some((trans_to, rot_to))) = (root_idx, root_to) {
                    let pose_from = clip.local_pose(from_time, &skel.local_bind);
                    if idx < pose_from.len() {
                        let (_, rot_from, trans_from) =
                            pose_from[idx].to_scale_rotation_translation();
                        let delta_pos = trans_to - trans_from;
                        let delta_rot = rot_to * rot_from.inverse();
                        if delta_pos.length_squared() > 1e-8
                            || (delta_rot.w.abs() - 1.0).abs() > 1e-4
                        {
                            root_motions.push((e, delta_pos, delta_rot));
                        }
                    }
                }
            }

            // 8. Запоминаем new_time для следующего кадра.
            self.prev_times.insert(e, new_time);
        }

        // === Step 11.3: AnimationBlendPlayer ===
        for e in &blend_entities {
            let e = *e;

            // 1. Продвигаем глобальное время.
            let (tree, axis, looping, new_time) = {
                let Some(bp) = world.get_mut::<crate::game::animation_blend::AnimationBlendPlayer>(e) else {
                    continue;
                };
                bp.time += dt * bp.speed;
                (bp.tree.clone(), bp.axis, bp.looping, bp.time)
            };

            if tree.samples.is_empty() {
                self.prev_times.insert(e, new_time);
                continue;
            }

            let prev_time = self.prev_times.get(&e).copied().unwrap_or(0.0);

            // 2. Оценить дерево.
            let outputs = tree.evaluate(axis);
            if outputs.is_empty() {
                self.prev_times.insert(e, new_time);
                continue;
            }

            // 3. Найти доминирующий клип (для events).
            let mut dominant_idx: Option<usize> = None;
            let mut dominant_weight: f32 = 0.0;
            for out in &outputs {
                if out.weight > dominant_weight {
                    dominant_weight = out.weight;
                    dominant_idx = Some(out.sample_idx);
                }
            }

            // 4. Собрать позы активных клипов.
            let skel_handle = world.get::<SkeletonHandle>(e).cloned();
            let Some(handle) = skel_handle else {
                self.prev_times.insert(e, new_time);
                continue;
            };
            let Some(skel) = skeletons.get(&handle.0) else {
                self.prev_times.insert(e, new_time);
                continue;
            };

            let mut poses: Vec<(Vec<Mat4>, f32)> = Vec::with_capacity(outputs.len());
            let mut total_weight = 0.0f32;

            for out in &outputs {
                if out.weight < 1e-4 {
                    continue;
                }
                let Some(sample) = tree.samples.get(out.sample_idx) else {
                    continue;
                };
                let Some(clip) = clips.get(&sample.clip) else {
                    continue;
                };
                let duration = clip.duration;
                if duration <= 1e-4 {
                    continue;
                }
                let local_time = if looping {
                    (new_time * out.time_scale) % duration
                } else {
                    (new_time * out.time_scale).min(duration)
                };
                let pose = clip.local_pose(local_time, &skel.local_bind);
                poses.push((pose, out.weight));
                total_weight += out.weight;
            }

            if poses.is_empty() || total_weight < 1e-4 {
                self.prev_times.insert(e, new_time);
                continue;
            }

            let num_joints = poses.iter().map(|(p, _)| p.len()).max().unwrap_or(0);
            let mut blended: Vec<Mat4> = Vec::with_capacity(num_joints);
            for j in 0..num_joints {
                let mut first = true;
                let mut pos = Vec3::ZERO;
                let mut scale = Vec3::ONE;
                let mut rot = Quat::IDENTITY;
                let mut cum = 0.0f32;
                for (pose, w) in &poses {
                    let Some(m) = pose.get(j) else { continue };
                    let (s, r, t) = m.to_scale_rotation_translation();
                    if first {
                        pos = t;
                        scale = s;
                        rot = r;
                        cum = *w;
                        first = false;
                    } else {
                        let total = cum + *w;
                        let alpha = if total > 1e-6 { *w / total } else { 0.0 };
                        pos = pos.lerp(t, alpha);
                        scale = scale.lerp(s, alpha);
                        rot = rot.slerp(r, alpha);
                        cum = total;
                    }
                }
                blended.push(Mat4::from_scale_rotation_translation(scale, rot, pos));
            }

            let joint_matrices = skel.joint_matrices(&blended);
            renderer.update_skeleton(&handle.0, &joint_matrices);
            if let Some(didx) = dominant_idx
                && let Some(sample) = tree.samples.get(didx)
                && let Some(clip) = clips.get(&sample.clip)
            {
                let duration = clip.duration;
                if duration > 1e-4 {
                    let from_t = if looping {
                        (prev_time * 0.0)
                    } else {
                        0.0
                    };
                    let _ = from_t;
                    let prev_local = if looping {
                        (prev_time * sample.time_scale) % duration
                    } else {
                        (prev_time * sample.time_scale).min(duration)
                    };
                    let to_local = if looping {
                        (new_time * sample.time_scale) % duration
                    } else {
                        (new_time * sample.time_scale).min(duration)
                    };
                    for ev in events.get(&sample.clip) {
                        if event_in_range(ev.time, prev_local, to_local, looping, duration) {
                            triggered.push(AnimationEventTriggered {
                                entity: e,
                                clip: sample.clip.clone(),
                                name: ev.name.clone(),
                                payload: ev.payload.clone(),
                            });
                        }
                    }
                }
            }

            self.prev_times.insert(e, new_time);
        }

        for (e, delta_pos, delta_rot) in root_motions {
            if let Some(t) = world.get_mut::<Transform>(e) {
                let world_delta = t.rotation * delta_pos;
                t.position += world_delta;
                t.rotation = (t.rotation * delta_rot).normalize();
            }
        }
        let mut alive_set: std::collections::HashSet<Entity> =
            std::collections::HashSet::with_capacity(plain_entities.len() + blend_entities.len());
        alive_set.extend(plain_entities.iter().copied());
        alive_set.extend(blend_entities.iter().copied());
        self.prev_times.retain(|k, _| alive_set.contains(k));

        triggered
    }

    pub fn reset(&mut self, entity: Entity) {
        self.prev_times.remove(&entity);
    }

    pub fn clear(&mut self) {
        self.prev_times.clear();
    }

    pub fn tracked_count(&self) -> usize {
        self.prev_times.len()
    }
}

impl Default for AnimationRuntime {
    fn default() -> Self { Self::new() }
}

fn event_in_range(
    event_time: f32,
    from: f32,
    to: f32,
    looping: bool,
    duration: f32,
) -> bool {
    if duration <= 0.0 { return false; }

    if from <= to {
        event_time > from && event_time <= to
    } else if looping {
        event_time > from || event_time <= to
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_in_range_simple() {
        assert!( event_in_range(0.5, 0.0, 1.0, false, 2.0));
        assert!( event_in_range(1.0, 0.0, 1.0, false, 2.0));
        assert!(!event_in_range(0.0, 0.0, 1.0, false, 2.0)); // граница слева
        assert!(!event_in_range(1.5, 0.0, 1.0, false, 2.0));
    }

    #[test]
    fn event_in_range_wrap() {
        // Клип 2.0s, prev = 1.8, new = 2.3 → wrap: to = 0.3.
        assert!(event_in_range(1.9, 1.8, 0.3, true, 2.0));
        assert!(event_in_range(2.0, 1.8, 0.3, true, 2.0));
        assert!(event_in_range(0.1, 1.8, 0.3, true, 2.0));
        assert!(!event_in_range(1.0, 1.8, 0.3, true, 2.0));
        assert!(!event_in_range(0.5, 1.8, 0.3, true, 2.0));
    }

    #[test]
    fn events_registry_sorts() {
        let mut ev = AnimationEvents::new();
        ev.add("Walk", AnimationEvent::new(0.8, "b"));
        ev.add("Walk", AnimationEvent::new(0.3, "a"));
        ev.add("Walk", AnimationEvent::new(1.2, "c"));
        let list = ev.get("Walk");
        assert_eq!(list[0].name, "a");
        assert_eq!(list[1].name, "b");
        assert_eq!(list[2].name, "c");
    }

    #[test]
    fn events_registry_empty_clip() {
        let ev = AnimationEvents::new();
        assert!(ev.get("Nonexistent").is_empty());
        assert!(ev.is_empty());
    }
}