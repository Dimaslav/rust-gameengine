//! Валидация сцены: поиск битых ссылок, дубликатов и других проблем.

use std::collections::{HashMap, HashSet};

use crate::ecs::{Entity, World};
use crate::game::ai::AiAgent;
use crate::game::audio::AudioSource;
use crate::game::components::{
    MaterialHandle, MeshHandle, Name, Parent, Transform, Trigger, TriggerAction,
};
use crate::game::lights::{DirectionalLight, PointLight};
use crate::physics::{Collider, RigidBody};
use crate::render::Renderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneIssueKind {
    BrokenParent,
    CyclicParent,
    MissingMesh,
    MissingMaterial,
    DuplicateName,
    EmptyEntity,
    TriggerWithoutTransform,
    TriggerBadElevatorRef,
    AiAgentWithoutTransform,
    RigidBodyWithoutCollider,
    ColliderWithoutTransform,
}

#[derive(Debug, Clone)]
pub struct SceneIssue {
    pub kind: SceneIssueKind,
    pub severity: IssueSeverity,
    pub message: String,
    pub entities: Vec<Entity>,
}

pub fn validate_scene(world: &World, renderer: &Renderer) -> Vec<SceneIssue> {
    let mut issues = Vec::new();

    let all_entities: HashSet<Entity> = world.entities().iter().copied().collect();

    for &e in world.entities() {
        // 1. Parent checks
        if let Some(&Parent(p)) = world.get::<Parent>(e) {
            if !all_entities.contains(&p) {
                issues.push(SceneIssue {
                    kind: SceneIssueKind::BrokenParent,
                    severity: IssueSeverity::Error,
                    message: format!("Entity #{} has Parent({}) that doesn't exist", e, p),
                    entities: vec![e],
                });
            } else if p == e {
                issues.push(SceneIssue {
                    kind: SceneIssueKind::CyclicParent,
                    severity: IssueSeverity::Error,
                    message: format!("Entity #{} is its own parent", e),
                    entities: vec![e],
                });
            } else {
                let mut cur = p;
                let mut depth = 0u32;
                let mut cyclic = false;
                while depth < 64 {
                    if cur == e {
                        cyclic = true;
                        break;
                    }
                    match world.get::<Parent>(cur).copied() {
                        Some(Parent(next)) => {
                            cur = next;
                            depth += 1;
                        }
                        None => break,
                    }
                }
                if cyclic {
                    issues.push(SceneIssue {
                        kind: SceneIssueKind::CyclicParent,
                        severity: IssueSeverity::Error,
                        message: format!("Entity #{} has a cyclic Parent chain", e),
                        entities: vec![e],
                    });
                }
            }
        }

        // 2. Mesh
        if let Some(mh) = world.get::<MeshHandle>(e)
            && !renderer.meshes.contains_key(&mh.0) {
                issues.push(SceneIssue {
                    kind: SceneIssueKind::MissingMesh,
                    severity: IssueSeverity::Error,
                    message: format!("Entity #{} references mesh '{}' (not loaded)", e, mh.0),
                    entities: vec![e],
                });
            }

        // 3. Material
        if let Some(mh) = world.get::<MaterialHandle>(e)
            && renderer.materials.get(&mh.0).is_none() {
                issues.push(SceneIssue {
                    kind: SceneIssueKind::MissingMaterial,
                    severity: IssueSeverity::Error,
                    message: format!("Entity #{} references material '{}' (not loaded)", e, mh.0),
                    entities: vec![e],
                });
            }

        // 4. Components without Transform
        if world.get::<Transform>(e).is_none() {
            if world.has::<Collider>(e) {
                issues.push(SceneIssue {
                    kind: SceneIssueKind::ColliderWithoutTransform,
                    severity: IssueSeverity::Error,
                    message: format!("Entity #{} has Collider but no Transform", e),
                    entities: vec![e],
                });
            }
            if world.has::<AiAgent>(e) {
                issues.push(SceneIssue {
                    kind: SceneIssueKind::AiAgentWithoutTransform,
                    severity: IssueSeverity::Error,
                    message: format!("Entity #{} has AiAgent but no Transform", e),
                    entities: vec![e],
                });
            }
            if world.has::<Trigger>(e) {
                issues.push(SceneIssue {
                    kind: SceneIssueKind::TriggerWithoutTransform,
                    severity: IssueSeverity::Warning,
                    message: format!("Entity #{} has Trigger but no Transform", e),
                    entities: vec![e],
                });
            }
        }

        // 5. RigidBody без Collider
        if world.has::<RigidBody>(e) && !world.has::<Collider>(e) {
            issues.push(SceneIssue {
                kind: SceneIssueKind::RigidBodyWithoutCollider,
                severity: IssueSeverity::Warning,
                message: format!("Entity #{} has RigidBody but no Collider", e),
                entities: vec![e],
            });
        }

        // 6. Trigger::CallElevator → битая ссылка
        if let Some(trigger) = world.get::<Trigger>(e)
            && let TriggerAction::CallElevator { elevator, .. } = &trigger.action
                && !all_entities.contains(elevator) {
                    issues.push(SceneIssue {
                        kind: SceneIssueKind::TriggerBadElevatorRef,
                        severity: IssueSeverity::Error,
                        message: format!(
                            "Entity #{} (Trigger) references elevator #{} (doesn't exist)",
                            e, elevator
                        ),
                        entities: vec![e],
                    });
                }

        // 7. Empty entity
        let has_anything = world.has::<Name>(e)
            || world.has::<MeshHandle>(e)
            || world.has::<MaterialHandle>(e)
            || world.has::<Collider>(e)
            || world.has::<RigidBody>(e)
            || world.has::<Trigger>(e)
            || world.has::<AiAgent>(e)
            || world.has::<PointLight>(e)
            || world.has::<DirectionalLight>(e)
            || world.has::<AudioSource>(e);
        if !has_anything {
            issues.push(SceneIssue {
                kind: SceneIssueKind::EmptyEntity,
                severity: IssueSeverity::Info,
                message: format!("Entity #{} is empty (no meaningful components)", e),
                entities: vec![e],
            });
        }
    }

    // 8. Дубликаты Name
    let mut by_name: HashMap<String, Vec<Entity>> = HashMap::new();
    for (e, name) in world.query::<Name>() {
        by_name.entry(name.0.clone()).or_default().push(e);
    }
    for (name, entities) in by_name {
        if entities.len() > 1 {
            issues.push(SceneIssue {
                kind: SceneIssueKind::DuplicateName,
                severity: IssueSeverity::Warning,
                message: format!(
                    "{} entities share the name '{}' (lookup by name will break)",
                    entities.len(), name
                ),
                entities,
            });
        }
    }

    issues
}