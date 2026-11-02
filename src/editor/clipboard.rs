//! Буфер обмена для компонентов: копирование одного компонента с одной
//! сущности и вставка на другую.

use crate::ecs::{Entity, World};
use crate::game::ai::{AiAgent, PatrolPath};
use crate::game::audio::AudioSource;
use crate::game::components::*;
use crate::game::decals::Decal;
use crate::game::lights::{DirectionalLight, PointLight};
use crate::physics::{Collider, PhysicsMaterial, RigidBody};

// ============================================================
// ComponentKind — публичный, чтобы UI мог ссылаться
// ============================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComponentKind {
    Transform,
    Mesh,
    Material,
    Skeleton,
    Animation,
    Spinner,
    Velocity,
    Health,
    Chase,
    Interactable,
    Trigger,
    Parent,
    Tint,
    Visible,
    TextureTiling,
    Elevator,
    SlidingDoor,
    RigidBody,
    Collider,
    PhysicsMaterial,
    DirectionalLight,
    PointLight,
    Decal,
    AudioSource,
    AiAgent,
    PatrolPath,
    Enemy,
    AiTarget,
}

impl ComponentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Transform => "Transform",
            Self::Mesh => "Mesh",
            Self::Material => "Material",
            Self::Skeleton => "Skeleton",
            Self::Animation => "Animation",
            Self::Spinner => "Spinner",
            Self::Velocity => "Velocity",
            Self::Health => "Health",
            Self::Chase => "Chase",
            Self::Interactable => "Interactable",
            Self::Trigger => "Trigger",
            Self::Parent => "Parent",
            Self::Tint => "Tint",
            Self::Visible => "Visible",
            Self::TextureTiling => "Texture Tiling",
            Self::Elevator => "Elevator",
            Self::SlidingDoor => "SlidingDoor",
            Self::RigidBody => "RigidBody",
            Self::Collider => "Collider",
            Self::PhysicsMaterial => "PhysicsMaterial",
            Self::DirectionalLight => "DirectionalLight",
            Self::PointLight => "PointLight",
            Self::Decal => "Decal",
            Self::AudioSource => "AudioSource",
            Self::AiAgent => "AiAgent",
            Self::PatrolPath => "PatrolPath",
            Self::Enemy => "Enemy",
            Self::AiTarget => "AiTarget",
        }
    }

    /// Можно ли скопировать этот компонент?
    pub fn is_copyable(self) -> bool {
        !matches!(self, Self::Parent | Self::Skeleton)
    }
}

// ============================================================
// ClipboardComponent — сериализуемая копия компонента
// ============================================================

#[derive(Clone)]
pub enum ClipboardComponent {
    Transform(Transform),
    Mesh(MeshHandle),
    Material(MaterialHandle),
    Tint(Tint),
    Visible(Visible),
    TextureTiling(TextureTiling),
    Health(Health),
    Chase(Chase),
    Interactable(Interactable),
    Trigger(Trigger),
    Spinner(Spinner),
    Velocity(Velocity),
    RigidBody(RigidBody),
    Collider(Collider),
    PhysicsMaterial(PhysicsMaterial),
    DirectionalLight(DirectionalLight),
    PointLight(PointLight),
    AudioSource(AudioSource),
    AiAgent(AiAgent),
    PatrolPath(PatrolPath),
    Decal(Decal),
    Elevator(Elevator),
    SlidingDoor(SlidingDoor),
    Animation(AnimationPlayer),
}

impl ClipboardComponent {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Transform(_) => "Transform",
            Self::Mesh(_) => "Mesh",
            Self::Material(_) => "Material",
            Self::Tint(_) => "Tint",
            Self::Visible(_) => "Visible",
            Self::TextureTiling(_) => "Texture Tiling",
            Self::Health(_) => "Health",
            Self::Chase(_) => "Chase",
            Self::Interactable(_) => "Interactable",
            Self::Trigger(_) => "Trigger",
            Self::Spinner(_) => "Spinner",
            Self::Velocity(_) => "Velocity",
            Self::RigidBody(_) => "RigidBody",
            Self::Collider(_) => "Collider",
            Self::PhysicsMaterial(_) => "PhysicsMaterial",
            Self::DirectionalLight(_) => "DirectionalLight",
            Self::PointLight(_) => "PointLight",
            Self::AudioSource(_) => "AudioSource",
            Self::AiAgent(_) => "AiAgent",
            Self::PatrolPath(_) => "PatrolPath",
            Self::Decal(_) => "Decal",
            Self::Elevator(_) => "Elevator",
            Self::SlidingDoor(_) => "Sliding Door",
            Self::Animation(_) => "Animation",
        }
    }

    pub fn from_entity(world: &World, e: Entity, kind: ComponentKind) -> Option<Self> {
        Some(match kind {
            ComponentKind::Transform => Self::Transform(*world.get::<Transform>(e)?),
            ComponentKind::Mesh => Self::Mesh(world.get::<MeshHandle>(e)?.clone()),
            ComponentKind::Material => Self::Material(world.get::<MaterialHandle>(e)?.clone()),
            ComponentKind::Tint => Self::Tint(*world.get::<Tint>(e)?),
            ComponentKind::Visible => Self::Visible(*world.get::<Visible>(e)?),
            ComponentKind::TextureTiling => Self::TextureTiling(*world.get::<TextureTiling>(e)?),
            ComponentKind::Health => Self::Health(*world.get::<Health>(e)?),
            ComponentKind::Chase => Self::Chase(*world.get::<Chase>(e)?),
            ComponentKind::Interactable => Self::Interactable(*world.get::<Interactable>(e)?),
            ComponentKind::Trigger => Self::Trigger(world.get::<Trigger>(e)?.clone()),
            ComponentKind::Spinner => Self::Spinner(*world.get::<Spinner>(e)?),
            ComponentKind::Velocity => Self::Velocity(*world.get::<Velocity>(e)?),
            ComponentKind::RigidBody => Self::RigidBody(*world.get::<RigidBody>(e)?),
            ComponentKind::Collider => Self::Collider(*world.get::<Collider>(e)?),
            ComponentKind::PhysicsMaterial => Self::PhysicsMaterial(*world.get::<PhysicsMaterial>(e)?),
            ComponentKind::DirectionalLight => Self::DirectionalLight(*world.get::<DirectionalLight>(e)?),
            ComponentKind::PointLight => Self::PointLight(*world.get::<PointLight>(e)?),
            ComponentKind::AudioSource => Self::AudioSource(world.get::<AudioSource>(e)?.clone()),
            ComponentKind::AiAgent => Self::AiAgent(world.get::<AiAgent>(e)?.clone()),
            ComponentKind::PatrolPath => Self::PatrolPath(world.get::<PatrolPath>(e)?.clone()),
            ComponentKind::Decal => Self::Decal(world.get::<Decal>(e)?.clone()),
            ComponentKind::Elevator => Self::Elevator(world.get::<Elevator>(e)?.clone()),
            ComponentKind::SlidingDoor => Self::SlidingDoor(*world.get::<SlidingDoor>(e)?),
            ComponentKind::Animation => Self::Animation(world.get::<AnimationPlayer>(e)?.clone()),
            _ => return None,
        })
    }

    /// Вставить в entity. Runtime-поля сбрасываются.
    pub fn paste_into(&self, world: &mut World, e: Entity) {
        match self {
            Self::Transform(t) => {
                // Сохраняем позицию текущего, применяем rot/scale
                if let Some(cur) = world.get::<Transform>(e).copied() {
                    let mut new_t = *t;
                    new_t.position = cur.position;
                    world.insert(e, new_t);
                } else {
                    world.insert(e, *t);
                }
            }
            Self::Mesh(m) => { world.insert(e, m.clone()); }
            Self::Material(m) => { world.insert(e, m.clone()); }
            Self::Tint(t) => { world.insert(e, *t); }
            Self::Visible(v) => { world.insert(e, *v); }
            Self::TextureTiling(t) => { world.insert(e, *t); }
            Self::Health(h) => { world.insert(e, *h); }
            Self::Chase(c) => { world.insert(e, *c); }
            Self::Interactable(i) => { world.insert(e, *i); }
            Self::Trigger(t) => {
                let mut t = t.clone();
                t.fired = false;
                world.insert(e, t);
            }
            Self::Spinner(s) => { world.insert(e, *s); }
            Self::Velocity(v) => { world.insert(e, *v); }
            Self::RigidBody(rb) => {
                let mut rb = *rb;
                rb.velocity = glam::Vec3::ZERO;
                rb.angular_velocity = glam::Vec3::ZERO;
                rb.force = glam::Vec3::ZERO;
                rb.torque = glam::Vec3::ZERO;
                world.insert(e, rb);
            }
            Self::Collider(c) => { world.insert(e, *c); }
            Self::PhysicsMaterial(p) => { world.insert(e, *p); }
            Self::DirectionalLight(l) => { world.insert(e, *l); }
            Self::PointLight(l) => { world.insert(e, *l); }
            Self::AudioSource(s) => {
                let mut s = s.clone();
                s.playing = false;
                world.insert(e, s);
            }
            Self::AiAgent(a) => {
                let mut a = a.clone();
                a.path.clear();
                a.path_index = 0;
                a.repath_timer = 0.0;
                a.attack_timer = 0.0;
                a.last_seen_pos = None;
                a.time_since_seen = 999.0;
                a.state = crate::game::ai::AiState::Idle;
                a.state_timer = 0.0;
                world.insert(e, a);
            }
            Self::PatrolPath(p) => { world.insert(e, p.clone()); }
            Self::Decal(d) => { world.insert(e, d.clone()); }
            Self::Elevator(el) => {
                let mut el = el.clone();
                el.player_inside = false;
                world.insert(e, el);
            }
            Self::SlidingDoor(sd) => { world.insert(e, *sd); }
            Self::Animation(a) => {
                let mut a = a.clone();
                a.time = 0.0;
                world.insert(e, a);
            }
        }
    }
}