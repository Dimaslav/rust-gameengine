use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::component::{AnyStorage, ComponentStorage};
use super::events::Events;

pub type Entity = u32;

#[derive(Default)]
pub struct World {
    next_id: Entity,
    entities: Vec<Entity>,
    /// Entity → индекс в `entities`. Нужен для O(1) despawn.
    entity_pos: HashMap<Entity, usize>,
    storages: HashMap<TypeId, Box<dyn AnyStorage>>,
    events: Events,
}

impl World {
    pub fn new() -> Self { Self::default() }

    // ============================================================
    // Жизненный цикл сущностей
    // ============================================================

    pub fn spawn(&mut self) -> Entity {
        let id = self.next_id;
        self.next_id += 1;
        let idx = self.entities.len();
        self.entities.push(id);
        self.entity_pos.insert(id, idx);
        id
    }

    /// O(1) удаление через swap_remove + обновление обратного индекса.
    pub fn despawn(&mut self, entity: Entity) {
        let Some(pos) = self.entity_pos.remove(&entity) else {
            return;
        };
        self.entities.swap_remove(pos);
        // Если swap_remove переместил последний элемент — обновляем его индекс.
        if pos < self.entities.len() {
            self.entity_pos.insert(self.entities[pos], pos);
        }
        for store in self.storages.values_mut() {
            store.remove_entity(entity);
        }
    }

    /// Гарантирует, что `next_id` больше любого существующего entity.
    /// Вызывать после загрузки сцены / десериализации, если id вставлялись
    /// напрямую.
    pub fn sync_next_id(&mut self) {
        if let Some(&max) = self.entities.iter().max() {
            if max >= self.next_id {
                self.next_id = max.saturating_add(1);
            }
        }
    }

    // ============================================================
    // Компоненты
    // ============================================================

    pub fn insert<T: Send + Sync + 'static>(&mut self, entity: Entity, component: T) {
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
        storage.as_any().downcast_ref::<ComponentStorage<T>>()?.get(entity)
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self, entity: Entity) -> Option<&mut T> {
        let storage = self.storages.get_mut(&TypeId::of::<T>())?;
        storage.as_any_mut().downcast_mut::<ComponentStorage<T>>()?.get_mut(entity)
    }

    pub fn remove<T: Send + Sync + 'static>(&mut self, entity: Entity) -> Option<T> {
        let storage = self.storages.get_mut(&TypeId::of::<T>())?;
        storage.as_any_mut().downcast_mut::<ComponentStorage<T>>()?.remove(entity)
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
        assert_ne!(a_type, b_type, "for_each_pair requires distinct component types");

        let entities: Vec<Entity> = match self.storages.get(&a_type) {
            Some(s) => s
                .as_any()
                .downcast_ref::<ComponentStorage<A>>()
                .map(|s| s.entity_list())
                .unwrap_or_default(),
            None => return,
        };

        let a_ptr: *mut ComponentStorage<A> = self
            .storages
            .get_mut(&a_type)
            .and_then(|s| s.as_any_mut().downcast_mut::<ComponentStorage<A>>())
            .map(|s| s as *mut _)
            .unwrap();

        let b_ptr: *const ComponentStorage<B> = match self.storages.get(&b_type) {
            Some(s) => s
                .as_any()
                .downcast_ref::<ComponentStorage<B>>()
                .map(|s| s as *const _)
                .unwrap(),
            None => return,
        };

        // SAFETY: два разных TypeId → две разные Box-аллокации → два
        // непересекающихся участка памяти. HashMap не модифицируется.
        unsafe {
            let a_store = &mut *a_ptr;
            let b_store = &*b_ptr;
            for e in entities {
                if let (Some(a), Some(b)) = (a_store.get_mut(e), b_store.get(e)) {
                    f(e, a, b);
                }
            }
        }
    }

    // ============================================================
    // Events
    // ============================================================

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

    pub fn entities(&self) -> &[Entity] { &self.entities }
    pub fn len(&self) -> usize { self.entities.len() }
    pub fn is_empty(&self) -> bool { self.entities.is_empty() }
}