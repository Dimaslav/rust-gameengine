use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::fmt;

use super::component::{AnyStorage, ComponentStorage};
use super::events::Events;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Entity {
    pub index: u32,
    pub generation: u32,
}

impl Entity {
    pub const INVALID: Entity = Entity {
        index: u32::MAX,
        generation: u32::MAX,
    };

    pub const fn from_raw(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub const fn to_bits(self) -> u64 {
        ((self.generation as u64) << 32) | (self.index as u64)
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self {
            index: bits as u32,
            generation: (bits >> 32) as u32,
        }
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.generation == 0 {
            write!(f, "#{}", self.index)
        } else {
            write!(f, "#{}v{}", self.index, self.generation)
        }
    }
}

#[derive(Default)]
pub struct World {
    next_index: u32,
    generations: Vec<u32>,
    free_indices: Vec<u32>,
    entities: Vec<Entity>,
    entity_pos: HashMap<u32, usize>,
    storages: HashMap<TypeId, Box<dyn AnyStorage>>,
    events: Events,
    scratch_entities: Vec<Entity>,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn(&mut self) -> Entity {
        let index = if let Some(recycled) = self.free_indices.pop() {
            recycled
        } else {
            let idx = self.next_index;
            self.next_index = self.next_index.wrapping_add(1);
            idx
        };

        let idx_usize = index as usize;
        if idx_usize >= self.generations.len() {
            self.generations.resize(idx_usize + 1, 0);
        }
        let generation = self.generations[idx_usize];

        let entity = Entity { index, generation };
        let pos = self.entities.len();
        self.entities.push(entity);
        self.entity_pos.insert(index, pos);
        entity
    }

    pub fn despawn(&mut self, entity: Entity) {
        let idx = entity.index as usize;
        if idx >= self.generations.len() || self.generations[idx] != entity.generation {
            return;
        }
        let Some(pos) = self.entity_pos.remove(&entity.index) else {
            return;
        };
        self.entities.swap_remove(pos);
        if pos < self.entities.len() {
            let moved = self.entities[pos].index;
            self.entity_pos.insert(moved, pos);
        }
        for store in self.storages.values_mut() {
            store.remove_entity(entity);
        }
        self.generations[idx] = entity.generation.wrapping_add(1);
        self.free_indices.push(entity.index);
    }

    pub fn is_alive(&self, entity: Entity) -> bool {
        let idx = entity.index as usize;
        idx < self.generations.len()
            && self.generations[idx] == entity.generation
            && self.entity_pos.contains_key(&entity.index)
    }

    pub fn sync_next_id(&mut self) {
        if self.entities.is_empty() {
            return;
        }
        let max_idx = self.entities.iter().map(|e| e.index).max().unwrap_or(0);
        self.next_index = self.next_index.max(max_idx.saturating_add(1));
        let need_len = self.next_index as usize;
        if need_len > self.generations.len() {
            self.generations.resize(need_len, 0);
        }

        for e in &self.entities {
            let idx = e.index as usize;
            if idx < self.generations.len() {
                self.generations[idx] = self.generations[idx].max(e.generation);
            }
        }

        // Rebuild free list.
        let live: HashSet<u32> = self.entities.iter().map(|e| e.index).collect();
        self.free_indices.clear();
        for i in 0..self.next_index {
            if !live.contains(&i) {
                self.free_indices.push(i);
            }
        }
    }

    pub fn insert<T: Send + Sync + 'static>(&mut self, entity: Entity, component: T) {
        debug_assert!(
            self.is_alive(entity),
            "World::insert on dead entity {:?}",
            entity,
        );
        if !self.is_alive(entity) {
            log::error!(
                "World::insert on dead entity {} (release, ignored)",
                entity
            );
            return;
        }
        let storage = self
            .storages
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(ComponentStorage::<T>::default()));
        storage
            .as_any_mut()
            .downcast_mut::<ComponentStorage<T>>()
            .unwrap()
            .insert(entity, component);
    }

    pub fn get<T: Send + Sync + 'static>(&self, entity: Entity) -> Option<&T> {
        let storage = self.storages.get(&TypeId::of::<T>())?;
        storage
            .as_any()
            .downcast_ref::<ComponentStorage<T>>()?
            .get(entity)
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self, entity: Entity) -> Option<&mut T> {
        let storage = self.storages.get_mut(&TypeId::of::<T>())?;
        storage
            .as_any_mut()
            .downcast_mut::<ComponentStorage<T>>()?
            .get_mut(entity)
    }

    pub fn remove<T: Send + Sync + 'static>(&mut self, entity: Entity) -> Option<T> {
        let storage = self.storages.get_mut(&TypeId::of::<T>())?;
        storage
            .as_any_mut()
            .downcast_mut::<ComponentStorage<T>>()?
            .remove(entity)
    }

    pub fn has<T: Send + Sync + 'static>(&self, entity: Entity) -> bool {
        self.storages
            .get(&TypeId::of::<T>())
            .map(|s| s.contains_entity(entity))
            .unwrap_or(false)
    }

    // ============================================================
    // Запросы
    // ============================================================

    pub fn query<T: Send + Sync + 'static>(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.storages
            .get(&TypeId::of::<T>())
            .and_then(|s| s.as_any().downcast_ref::<ComponentStorage<T>>())
            .map(|s| s.iter())
            .into_iter()
            .flatten()
    }

    pub fn query_mut<T: Send + Sync + 'static>(
        &mut self,
    ) -> impl Iterator<Item = (Entity, &mut T)> {
        self.storages
            .get_mut(&TypeId::of::<T>())
            .and_then(|s| s.as_any_mut().downcast_mut::<ComponentStorage<T>>())
            .map(|s| s.iter_mut())
            .into_iter()
            .flatten()
    }

    pub fn query2<A, B>(&self) -> impl Iterator<Item = (Entity, &A, &B)>
    where
        A: Send + Sync + 'static,
        B: Send + Sync + 'static,
    {
        let a_store = self
            .storages
            .get(&TypeId::of::<A>())
            .and_then(|s| s.as_any().downcast_ref::<ComponentStorage<A>>());
        let b_store = self
            .storages
            .get(&TypeId::of::<B>())
            .and_then(|s| s.as_any().downcast_ref::<ComponentStorage<B>>());

        a_store.into_iter().flat_map(move |a| {
            a.iter().filter_map(move |(e, av)| {
                b_store.and_then(|b| b.get(e)).map(|bv| (e, av, bv))
            })
        })
    }

    pub fn for_each_pair<A, B, F>(&mut self, mut f: F)
    where
        A: Send + Sync + 'static,
        B: Send + Sync + 'static,
        F: FnMut(Entity, &mut A, &B),
    {
        let a_type = TypeId::of::<A>();
        let b_type = TypeId::of::<B>();
        assert_ne!(
            a_type, b_type,
            "for_each_pair requires distinct component types"
        );

        let Self {
            scratch_entities,
            storages,
            ..
        } = self;

        let taken = std::mem::take(scratch_entities);
        let mut guard = ScratchRestore {
            slot: scratch_entities,
            data: Some(taken),
        };
        let buf = guard.data.as_mut().unwrap();
        buf.clear();

        if let Some(s) = storages.get(&a_type)
            && let Some(storage) = s.as_any().downcast_ref::<ComponentStorage<A>>()
        {
            buf.extend(storage.entities_iter().copied());
        }

        if guard.data.as_ref().unwrap().is_empty() {
            return;
        }

        let a_ptr: *mut ComponentStorage<A> = storages
            .get_mut(&a_type)
            .and_then(|s| s.as_any_mut().downcast_mut::<ComponentStorage<A>>())
            .map(|s| s as *mut _)
            .unwrap();
        let b_ptr: *const ComponentStorage<B> = match storages.get(&b_type) {
            Some(s) => s
                .as_any()
                .downcast_ref::<ComponentStorage<B>>()
                .map(|s| s as *const _)
                .unwrap(),
            None => return,
        };

        let entities = guard.data.as_ref().unwrap();
        unsafe {
            let a_store = &mut *a_ptr;
            let b_store = &*b_ptr;
            for &e in entities {
                if let (Some(a), Some(b)) = (a_store.get_mut(e), b_store.get(e)) {
                    f(e, a, b);
                }
            }
        }
    }

    pub fn send<E: 'static + Send + Sync>(&mut self, event: E) {
        self.events.send(event);
    }

    pub fn read_events<E: 'static + Send + Sync>(&self) -> impl Iterator<Item = &E> {
        self.events.read::<E>()
    }

    pub fn read_events_current<E: 'static + Send + Sync>(&self) -> impl Iterator<Item = &E> {
        self.events.read_current::<E>()
    }

    pub fn update_events(&mut self) {
        self.events.update();
    }

    // ============================================================
    // Прочее
    // ============================================================

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }
    pub fn len(&self) -> usize {
        self.entities.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

struct ScratchRestore<'a> {
    slot: &'a mut Vec<Entity>,
    data: Option<Vec<Entity>>,
}

impl Drop for ScratchRestore<'_> {
    fn drop(&mut self) {
        if let Some(v) = self.data.take() {
            *self.slot = v;
        }
    }
}

// ============================================================
// Тесты
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Pos(i32);
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Vel(i32);

    #[test]
    fn spawn_and_despawn() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        let c = w.spawn();
        assert_eq!(w.len(), 3);
        assert!(w.is_alive(a) && w.is_alive(b) && w.is_alive(c));
        w.despawn(b);
        assert_eq!(w.len(), 2);
        assert!(!w.is_alive(b));
    }

    #[test]
    fn recycled_slot_bumps_generation() {
        let mut w = World::new();
        let a = w.spawn();
        w.despawn(a);
        let b = w.spawn();
        assert_eq!(a.index, b.index);
        assert_ne!(a.generation, b.generation);
        assert!(!w.is_alive(a));
        assert!(w.is_alive(b));
    }

    #[test]
    fn despawn_stale_is_noop() {
        let mut w = World::new();
        let a = w.spawn();
        w.despawn(a);
        let b = w.spawn();
        w.despawn(a);
        assert!(w.is_alive(b));
    }

    #[test]
    fn for_each_pair_visits_intersection() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        let c = w.spawn();
        w.insert(a, Pos(0));
        w.insert(a, Vel(1));
        w.insert(b, Pos(0));
        w.insert(c, Vel(1));
        let mut visited = Vec::new();
        w.for_each_pair::<Pos, Vel, _>(|e, _, _| visited.push(e));
        assert_eq!(visited, vec![a]);
    }

    #[test]
    fn for_each_pair_returns_scratch_capacity() {
        let mut w = World::new();
        for i in 0..100 {
            let e = w.spawn();
            w.insert(e, Pos(i));
            w.insert(e, Vel(1));
        }
        let mut c1 = 0;
        w.for_each_pair::<Pos, Vel, _>(|_, _, _| c1 += 1);
        assert_eq!(c1, 100);
        let mut c2 = 0;
        w.for_each_pair::<Pos, Vel, _>(|_, _, _| c2 += 1);
        assert_eq!(c2, 100);
    }

    #[test]
    fn for_each_pair_restores_scratch_on_panic() {
        let mut w = World::new();
        for _ in 0..16 {
            let e = w.spawn();
            w.insert(e, Pos(1));
            w.insert(e, Vel(1));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.for_each_pair::<Pos, Vel, _>(|_, _, _| panic!("boom"));
        }));
        assert!(result.is_err());
        let mut count = 0;
        w.for_each_pair::<Pos, Vel, _>(|_, _, _| count += 1);
        assert_eq!(count, 16);
    }

    #[test]
    fn sync_next_id_after_load() {
        let mut w = World::new();
        w.entities.push(Entity::from_raw(100, 0));
        w.entity_pos.insert(100, 0);
        w.sync_next_id();
        assert!(w.next_index > 100);
        assert_eq!(w.free_indices.len(), 100);
        let new = w.spawn();
        assert_eq!(new.index, 99);
        assert_eq!(new.generation, 0);
        assert!(w.is_alive(new));
        assert!(!w.is_alive(Entity::from_raw(99, 1)));
    }

    #[test]
    fn sync_next_id_rebuilds_free_list() {
        let mut w = World::new();
        w.entities.push(Entity::from_raw(0, 0));
        w.entities.push(Entity::from_raw(2, 0));
        w.entity_pos.insert(0, 0);
        w.entity_pos.insert(2, 1);
        w.generations = vec![0, 1, 0];
        w.sync_next_id();

        assert_eq!(w.free_indices, vec![1]);

        let e = w.spawn();
        assert_eq!(e.index, 1);
        assert_eq!(e.generation, 1);
    }

    #[test]
    fn sync_next_id_handles_empty_world() {
        let mut w = World::new();
        w.sync_next_id();
        assert!(w.free_indices.is_empty());
        assert_eq!(w.next_index, 0);
    }

    #[test]
    fn sync_next_id_high_generation_is_alive() {
        let mut w = World::new();
        w.entities.push(Entity::from_raw(7, 5));
        w.entity_pos.insert(7, 0);
        w.sync_next_id();

        let e = Entity::from_raw(7, 5);
        assert!(w.is_alive(e), "entity at generation 5 must be alive after sync");
        assert!(w.generations[7] >= 5);
    }

    #[test]
    fn save_load_cycle_keeps_indices_bounded() {
        let mut w = World::new();
        let e0 = w.spawn();
        let e1 = w.spawn();
        let _ = w.spawn();
        let e3 = w.spawn();
        let _ = w.spawn();
        w.despawn(e1);
        w.despawn(e3);
        assert_eq!(w.free_indices.len(), 2);
        w.sync_next_id();
        assert_eq!(w.free_indices.len(), 2);
        let _a = w.spawn();
        let _b = w.spawn();
        assert_eq!(w.next_index, 5, "next_index не должен вырасти");
        let _ = e0;
    }

    #[test]
    fn entity_bits_roundtrip() {
        let e = Entity::from_raw(12345, 42);
        assert_eq!(Entity::from_bits(e.to_bits()), e);
    }

    #[test]
    fn entity_display() {
        assert_eq!(format!("{}", Entity::from_raw(5, 0)), "#5");
        assert_eq!(format!("{}", Entity::from_raw(5, 2)), "#5v2");
    }
}