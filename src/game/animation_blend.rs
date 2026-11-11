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
    pub weight: f32,
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

#[derive(Debug, Clone, Default)]
pub struct BoneMask {
    bones: Option<Vec<usize>>,
}

impl BoneMask {
    pub fn all() -> Self {
        Self { bones: None }
    }

    pub fn from_indices(indices: Vec<usize>) -> Self {
        Self { bones: Some(indices) }
    }

    pub fn upper_body(skeleton: &crate::render::skinning::Skeleton) -> Self {
        let lower_keywords = [
            "leg", "thigh", "calf", "shin", "foot", "toe",
            "hip", "pelvis",
        ];
        let mut bones = Vec::new();
        for (i, name) in skeleton.names.iter().enumerate() {
            let n = name.to_lowercase();
            let is_lower = lower_keywords.iter().any(|kw| n.contains(kw));
            let is_upper = n.contains("spine")
                || n.contains("chest")
                || n.contains("neck")
                || n.contains("head")
                || n.contains("arm")
                || n.contains("hand")
                || n.contains("shoulder")
                || n.contains("clavicle")
                || n.contains("forearm");
            if is_upper && !is_lower {
                bones.push(i);
            }
        }
        Self { bones: Some(bones) }
    }

    pub fn contains(&self, idx: usize) -> bool {
        match &self.bones {
            None => true,
            Some(v) => v.contains(&idx),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(&self.bones, Some(v) if v.is_empty())
    }

    pub fn len(&self) -> usize {
        match &self.bones {
            None => usize::MAX, // «все»
            Some(v) => v.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnimationLayer {
    pub tree: BlendTree1D,
    pub axis: f32,
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
    pub weight: f32,
    pub bone_mask: BoneMask,
}

impl AnimationLayer {
    pub fn new(tree: BlendTree1D, weight: f32, bone_mask: BoneMask) -> Self {
        Self {
            tree,
            axis: 0.0,
            time: 0.0,
            speed: 1.0,
            looping: true,
            weight: weight.clamp(0.0, 1.0),
            bone_mask,
        }
    }

    pub fn set_axis(&mut self, axis: f32) {
        self.axis = axis;
    }

    pub fn set_weight(&mut self, weight: f32) {
        self.weight = weight.clamp(0.0, 1.0);
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnimationLayeredPlayer {
    pub layers: Vec<AnimationLayer>,
    pub looping: bool,
}

impl AnimationLayeredPlayer {
    pub fn new_base(tree: BlendTree1D) -> Self {
        let base = AnimationLayer::new(tree, 1.0, BoneMask::all());
        Self {
            layers: vec![base],
            looping: true,
        }
    }

    pub fn push_overlay(&mut self, tree: BlendTree1D, mask: BoneMask, weight: f32) {
        self.layers.push(AnimationLayer::new(tree, weight, mask));
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
    Ne,
}

impl Comparison {
    pub fn check(self, left: f32, right: f32) -> bool {
        match self {
            Comparison::Lt => left < right,
            Comparison::Le => left <= right,
            Comparison::Eq => (left - right).abs() < 1e-5,
            Comparison::Ge => left >= right,
            Comparison::Gt => left > right,
            Comparison::Ne => (left - right).abs() >= 1e-5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub parameter: String,
    pub cmp: Comparison,
    pub threshold: f32,
}

impl Condition {
    pub fn new(parameter: impl Into<String>, cmp: Comparison, threshold: f32) -> Self {
        Self { parameter: parameter.into(), cmp, threshold }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub condition: Option<Condition>,
    #[serde(default = "default_transition_duration")]
    pub duration: f32,
    #[serde(default)]
    pub priority: i32,
}

fn default_transition_duration() -> f32 { 0.2 }

impl Transition {
    pub fn new(from: impl Into<String>, to: impl Into<String>, duration: f32) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            condition: None,
            duration: duration.max(0.0),
            priority: 0,
        }
    }

    pub fn with_condition(mut self, cond: Condition) -> Self {
        self.condition = Some(cond);
        self
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    fn condition_met(&self, params: &std::collections::HashMap<String, f32>) -> bool {
        match &self.condition {
            Some(c) => {
                let value = params.get(&c.parameter).copied().unwrap_or(0.0);
                c.cmp.check(value, c.threshold)
            }
            None => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateDef {
    pub name: String,
    pub tree: BlendTree1D,
}

impl StateDef {
    pub fn new(name: impl Into<String>, tree: BlendTree1D) -> Self {
        Self { name: name.into(), tree }
    }
}

#[derive(Debug, Clone)]
struct ActiveTransition {
    from: String,
    to: String,
    elapsed: f32,
    duration: f32,
    priority: i32,
}

#[derive(Debug, Clone)]
pub struct AnimationStateMachine {
    pub states: std::collections::HashMap<String, StateDef>,
    pub transitions: Vec<Transition>,
    pub parameters: std::collections::HashMap<String, f32>,
    pub current_state: String,
    pub state_time: f32,
    pub looping: bool,
    active: Option<ActiveTransition>,
}

impl AnimationStateMachine {
    pub fn new(initial: impl Into<String>, initial_tree: BlendTree1D) -> Self {
        let name = initial.into();
        let mut states = std::collections::HashMap::new();
        states.insert(name.clone(), StateDef::new(name.clone(), initial_tree));
        Self {
            states,
            transitions: Vec::new(),
            parameters: std::collections::HashMap::new(),
            current_state: name,
            state_time: 0.0,
            looping: true,
            active: None,
        }
    }

    pub fn add_state(&mut self, name: impl Into<String>, tree: BlendTree1D) {
        let n = name.into();
        self.states.insert(n.clone(), StateDef::new(n, tree));
    }

    pub fn add_transition(&mut self, t: Transition) {
        self.transitions.push(t);
    }

    pub fn set_parameter(&mut self, name: impl Into<String>, value: f32) {
        self.parameters.insert(name.into(), value);
    }

    pub fn force_state(&mut self, name: impl Into<String>) {
        let n = name.into();
        if self.states.contains_key(&n) {
            self.current_state = n;
            self.state_time = 0.0;
            self.active = None;
        }
    }

    pub fn active_transition(&self) -> Option<(&str, &str, f32, f32)> {
        self.active.as_ref().map(|a| {
            (a.from.as_str(), a.to.as_str(), a.elapsed, a.duration)
        })
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let mut state_changed = false;

        if let Some(a) = self.active.as_mut() {
            a.elapsed += dt;
            if a.elapsed >= a.duration {
                self.current_state = a.to.clone();
                self.state_time = 0.0;
                self.active = None;
                state_changed = true;
            }
        }

        self.state_time += dt;
        let active_priority = self.active.as_ref().map(|a| a.priority).unwrap_or(i32::MIN);
        let current = self.current_state.clone();
        for t in &self.transitions {
            if t.from != current && t.from != "*" {
                continue;
            }
            if t.priority < active_priority {
                continue;
            }
            if !t.condition_met(&self.parameters) {
                continue;
            }
            if t.to == current {
                continue;
            }
            if !self.states.contains_key(&t.to) {
                continue;
            }
            let is_same_target = self
                .active
                .as_ref()
                .map(|a| a.to == t.to)
                .unwrap_or(false);
            if is_same_target {
                continue;
            }
            self.active = Some(ActiveTransition {
                from: current.clone(),
                to: t.to.clone(),
                elapsed: 0.0,
                duration: t.duration,
                priority: t.priority,
            });
            if t.duration <= 1e-4 {
                self.current_state = t.to.clone();
                self.state_time = 0.0;
                self.active = None;
                state_changed = true;
            }
            break;
        }

        state_changed
    }

    pub fn current_pose<F>(
        &self,
        evaluate_tree: F,
    ) -> Option<Vec<Mat4>>
    where
        F: Fn(&BlendTree1D, f32, bool) -> Vec<Mat4>,
    {
        let from_state = self.states.get(&self.current_state)?;
        let pose_a = evaluate_tree(
            &from_state.tree,
            self.state_time,
            self.looping,
        );

        if let Some(a) = self.active.as_ref() {
            let target_state = self.states.get(&a.to)?;
            let pose_b = evaluate_tree(
                &target_state.tree,
                self.state_time,
                self.looping,
            );
            let alpha = if a.duration <= 1e-6 {
                1.0
            } else {
                (a.elapsed / a.duration).clamp(0.0, 1.0)
            };
            let n = pose_a.len().min(pose_b.len());
            let mut out = Vec::with_capacity(n);
            for j in 0..n {
                let (bs, br, bt) = pose_a[j].to_scale_rotation_translation();
                let (os, or_, ot) = pose_b[j].to_scale_rotation_translation();
                let pos = bt.lerp(ot, alpha);
                let scale = bs.lerp(os, alpha);
                let rot = br.slerp(or_, alpha);
                out.push(Mat4::from_scale_rotation_translation(scale, rot, pos));
            }
            Some(out)
        } else {
            Some(pose_a)
        }
    }
}

impl Default for AnimationStateMachine {
    fn default() -> Self {
        Self::new("default", BlendTree1D::default())
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

    #[test]
    fn bone_mask_all_contains_everything() {
        let m = BoneMask::all();
        assert!(m.contains(0));
        assert!(m.contains(999));
        assert!(!m.is_empty());
    }

    #[test]
    fn bone_mask_from_indices() {
        let m = BoneMask::from_indices(vec![1, 3, 5]);
        assert!(!m.contains(0));
        assert!(m.contains(1));
        assert!(m.contains(3));
        assert!(m.contains(5));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn bone_mask_empty_is_empty() {
        let m = BoneMask::from_indices(vec![]);
        assert!(m.is_empty());
        assert!(!m.contains(0));
    }

    #[test]
    fn layer_weight_clamped() {
        let l = AnimationLayer::new(BlendTree1D::default(), 5.0, BoneMask::all());
        assert!((l.weight - 1.0).abs() < 1e-6);
        let mut l = AnimationLayer::new(BlendTree1D::default(), -1.0, BoneMask::all());
        assert!((l.weight - 0.0).abs() < 1e-6);
        l.set_weight(0.5);
        assert!((l.weight - 0.5).abs() < 1e-6);
    }

    #[test]
    fn layered_player_base_weight_1() {
        let lp = AnimationLayeredPlayer::new_base(BlendTree1D::default());
        assert_eq!(lp.layers.len(), 1);
        assert!((lp.layers[0].weight - 1.0).abs() < 1e-6);
    }

    #[test]
    fn layered_player_push_overlay() {
        let mut lp = AnimationLayeredPlayer::new_base(BlendTree1D::default());
        lp.push_overlay(
            BlendTree1D::default(),
            BoneMask::from_indices(vec![1, 2]),
            0.5,
        );
        assert_eq!(lp.layers.len(), 2);
        assert_eq!(lp.layers[1].weight, 0.5);
        assert!(lp.layers[1].bone_mask.contains(1));
    }

    #[test]
    fn sm_initial_state_set() {
        let sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        assert_eq!(sm.current_state, "idle");
        assert!(sm.active_transition().is_none());
    }

    #[test]
    fn sm_force_state_works() {
        let mut sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        sm.add_state("run", BlendTree1D::default());
        sm.force_state("run");
        assert_eq!(sm.current_state, "run");
    }

    #[test]
    fn sm_force_state_unknown_ignored() {
        let mut sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        sm.force_state("nonexistent");
        assert_eq!(sm.current_state, "idle");
    }

    #[test]
    fn sm_transition_triggers_on_condition() {
        let mut sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        sm.add_state("run", BlendTree1D::default());
        sm.add_transition(
            Transition::new("idle", "run", 0.3).with_condition(
                Condition::new("speed", Comparison::Gt, 3.0),
            ),
        );
        sm.set_parameter("speed", 0.0);
        sm.tick(0.1);
        assert_eq!(sm.current_state, "idle");
        sm.set_parameter("speed", 5.0);
        sm.tick(0.1);
        assert!(sm.active_transition().is_some());
        sm.tick(0.3);
        assert_eq!(sm.current_state, "run");
        assert!(sm.active_transition().is_none());
    }

    #[test]
    fn sm_instant_transition_zero_duration() {
        let mut sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        sm.add_state("run", BlendTree1D::default());
        sm.add_transition(
            Transition::new("idle", "run", 0.0).with_condition(
                Condition::new("speed", Comparison::Gt, 3.0),
            ),
        );
        sm.set_parameter("speed", 5.0);
        sm.tick(0.01);
        assert_eq!(sm.current_state, "run");
        assert!(sm.active_transition().is_none());
    }

    #[test]
    fn sm_no_self_transition() {
        let mut sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        sm.add_transition(
            Transition::new("idle", "idle", 0.2).with_condition(
                Condition::new("x", Comparison::Gt, 0.0),
            ),
        );
        sm.set_parameter("x", 1.0);
        sm.tick(0.05);
        assert!(sm.active_transition().is_none());
    }

    #[test]
    fn sm_priority_interrupt() {
        let mut sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        sm.add_state("walk", BlendTree1D::default());
        sm.add_state("attack", BlendTree1D::default());
        sm.add_transition(
            Transition::new("idle", "walk", 1.0).with_condition(
                Condition::new("speed", Comparison::Gt, 1.0),
            ),
        );
        sm.add_transition(
            Transition::new("idle", "attack", 0.1)
                .with_condition(Condition::new("attacking", Comparison::Gt, 0.5))
                .with_priority(10),
        );
        sm.set_parameter("speed", 2.0);
        sm.tick(0.05);
        assert!(sm.active_transition().is_some());
        sm.set_parameter("attacking", 1.0);
        sm.tick(0.05);
        let (_, to, _, _) = sm.active_transition().unwrap();
        assert_eq!(to, "attack");

        sm.tick(0.2);
        assert_eq!(sm.current_state, "attack");
    }

    #[test]
    fn sm_wildcard_from() {
        let mut sm = AnimationStateMachine::new("a", BlendTree1D::default());
        sm.add_state("b", BlendTree1D::default());
        sm.add_transition(
            Transition::new("*", "b", 0.0).with_condition(
                Condition::new("force_b", Comparison::Gt, 0.0),
            ),
        );
        sm.set_parameter("force_b", 1.0);
        sm.tick(0.01);
        assert_eq!(sm.current_state, "b");
    }

    #[test]
    fn sm_missing_parameter_treated_as_zero() {
        let mut sm = AnimationStateMachine::new("idle", BlendTree1D::default());
        sm.add_state("run", BlendTree1D::default());
        sm.add_transition(
            Transition::new("idle", "run", 0.0).with_condition(
                Condition::new("speed", Comparison::Gt, 3.0),
            ),
        );
        sm.tick(0.01);
        assert_eq!(sm.current_state, "idle");
    }

    #[test]
    fn comparison_ops() {
        assert!(Comparison::Lt.check(1.0, 2.0));
        assert!(!Comparison::Lt.check(2.0, 2.0));
        assert!(Comparison::Le.check(2.0, 2.0));
        assert!(Comparison::Eq.check(2.0, 2.0));
        assert!(Comparison::Ge.check(2.0, 2.0));
        assert!(Comparison::Gt.check(3.0, 2.0));
        assert!(Comparison::Ne.check(1.0, 2.0));
    }
}