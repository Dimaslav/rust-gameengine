use glam::Mat4;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlendSample {
    pub clip: String,
    pub axis: f32,
    #[serde(default = "default_time_scale")]
    pub time_scale: f32,
}

fn default_time_scale() -> f32 { 1.0 }

impl BlendSample {
    pub fn new(clip: impl Into<String>, axis: f32, time_scale: f32) -> Self {
        Self {
            clip: clip.into(),
            axis,
            time_scale: time_scale.max(0.01),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendOutput {
    pub sample_idx: usize,
    /// Вес клипа (сумма всех output.weight == 1.0).
    pub weight: f32,
    /// Эффективный playback speed.
    pub time_scale: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlendTree1D {
    #[serde(default)]
    pub samples: Vec<BlendSample>,
}

impl BlendTree1D {
    pub fn new(mut samples: Vec<BlendSample>) -> Self {
        samples.sort_by(|a, b| {
            a.axis.partial_cmp(&b.axis).unwrap_or(std::cmp::Ordering::Equal)
        });
        Self { samples }
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn evaluate(&self, axis: f32) -> Vec<BlendOutput> {
        let n = self.samples.len();
        if n == 0 {
            return Vec::new();
        }
        if n == 1 {
            return vec![BlendOutput {
                sample_idx: 0,
                weight: 1.0,
                time_scale: self.samples[0].time_scale,
            }];
        }

        // Clamp по краям.
        if axis <= self.samples[0].axis {
            return vec![BlendOutput {
                sample_idx: 0,
                weight: 1.0,
                time_scale: self.samples[0].time_scale,
            }];
        }
        let last = n - 1;
        if axis >= self.samples[last].axis {
            return vec![BlendOutput {
                sample_idx: last,
                weight: 1.0,
                time_scale: self.samples[last].time_scale,
            }];
        }

        let mut lo = 0;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.samples[mid].axis <= axis {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        let a = &self.samples[lo];
        let b = &self.samples[hi];
        let span = b.axis - a.axis;
        let t = if span.abs() < 1e-6 {
            0.0
        } else {
            ((axis - a.axis) / span).clamp(0.0, 1.0)
        };

        let weight_a = 1.0 - t;
        let weight_b = t;
        let ts = a.time_scale * weight_a + b.time_scale * weight_b;

        vec![
            BlendOutput { sample_idx: lo, weight: weight_a, time_scale: ts },
            BlendOutput { sample_idx: hi, weight: weight_b, time_scale: ts },
        ]
    }

    pub fn total_weight(&self, axis: f32) -> f32 {
        self.evaluate(axis).iter().map(|o| o.weight).sum()
    }
}

#[derive(Debug, Clone)]
pub struct AnimationBlendPlayer {
    pub tree: BlendTree1D,
    pub axis: f32,
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
}

impl AnimationBlendPlayer {
    pub fn new(tree: BlendTree1D) -> Self {
        Self {
            tree,
            axis: 0.0,
            time: 0.0,
            speed: 1.0,
            looping: true,
        }
    }

    pub fn set_axis(&mut self, axis: f32) {
        self.axis = axis;
    }

    pub fn reset_phase(&mut self) {
        self.time = 0.0;
    }
}

impl Default for AnimationBlendPlayer {
    fn default() -> Self {
        Self::new(BlendTree1D::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_3() -> BlendTree1D {
        BlendTree1D::new(vec![
            BlendSample::new("idle", 0.0, 1.0),
            BlendSample::new("walk", 3.0, 1.0),
            BlendSample::new("run", 6.0, 1.4),
        ])
    }

    #[test]
    fn empty_tree_returns_empty() {
        let t = BlendTree1D::new(vec![]);
        assert!(t.evaluate(0.0).is_empty());
    }

    #[test]
    fn single_sample_always_weight_1() {
        let t = BlendTree1D::new(vec![BlendSample::new("idle", 0.0, 1.0)]);
        let out = t.evaluate(100.0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sample_idx, 0);
        assert!((out[0].weight - 1.0).abs() < 1e-6);
    }

    #[test]
    fn below_first_clamps_to_first() {
        let t = tree_3();
        let out = t.evaluate(-5.0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sample_idx, 0);
        assert!((out[0].weight - 1.0).abs() < 1e-6);
    }

    #[test]
    fn above_last_clamps_to_last() {
        let t = tree_3();
        let out = t.evaluate(10.0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sample_idx, 2);
        assert!((out[0].weight - 1.0).abs() < 1e-6);
        assert!((out[0].time_scale - 1.4).abs() < 1e-6);
    }

    #[test]
    fn exactly_at_sample_weight_1() {
        let t = tree_3();
        let out = t.evaluate(3.0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].sample_idx, 1);
        assert!((out[0].weight - 1.0).abs() < 1e-6);
        assert!((out[1].weight - 0.0).abs() < 1e-6);
    }

    #[test]
    fn midpoint_walk_run() {
        let t = tree_3();
        let out = t.evaluate(4.5);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].sample_idx, 1);
        assert_eq!(out[1].sample_idx, 2);
        assert!((out[0].weight - 0.5).abs() < 1e-5);
        assert!((out[1].weight - 0.5).abs() < 1e-5);
        assert!((out[0].time_scale - 1.2).abs() < 1e-5);
        assert!((out[1].time_scale - 1.2).abs() < 1e-5);
    }

    #[test]
    fn weights_sum_to_1() {
        let t = tree_3();
        for axis in [-1.0, 0.0, 1.5, 3.0, 4.5, 6.0, 100.0] {
            assert!(
                (t.total_weight(axis) - 1.0).abs() < 1e-5,
                "axis={} weights_sum={}",
                axis,
                t.total_weight(axis),
            );
        }
    }

    #[test]
    fn new_sorts_samples() {
        let t = BlendTree1D::new(vec![
            BlendSample::new("run", 6.0, 1.4),
            BlendSample::new("idle", 0.0, 1.0),
            BlendSample::new("walk", 3.0, 1.0),
        ]);
        assert_eq!(t.samples[0].clip, "idle");
        assert_eq!(t.samples[1].clip, "walk");
        assert_eq!(t.samples[2].clip, "run");
    }

    #[test]
    fn duplicate_axis_does_not_panic() {
        let t = BlendTree1D::new(vec![
            BlendSample::new("a", 3.0, 1.0),
            BlendSample::new("b", 3.0, 1.0),
        ]);
        let out = t.evaluate(3.0);
        assert!(!out.is_empty());
        let sum: f32 = out.iter().map(|o| o.weight).sum();
        assert!((sum - 1.0).abs() < 1e-5);
    }
}