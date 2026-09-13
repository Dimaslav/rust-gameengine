use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::component::{AnyStorage, ComponentStorage};
use super::events::Events;

pub type Entity = u32;

#[derive(Default)]
pub struct World {
    next_id: Entity,
    entities: Vec<Entity>,
    storages: HashMap<TypeId, Box<dyn AnyStorage>>,
    events: Events,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    // ============================================================
    // Жизненный цикл сущностей
    // ============================================================

    pub fn spawn(&mut self) -> Entity {
        let id = self.next_id;
        self.next_id += 1;
        self.entities.push(id);
        id
    }

    pub fn despawn(&mut self, entity: Entity) {
        self.entities.retain(|&e| e != entity);
        for store in self.storages.values_mut() {
            store.remove_entity(entity);
        }
    }

    // ============================================================
    // Операции с компонентами
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
    // Итерация по одному компоненту
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

    // ============================================================
    // Итерация по двум компонентам
    // ============================================================

    /// Иммутабельная итерация: все сущности с A и B.
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
                b_store
                    .and_then(|b| b.get(e))
                    .map(|bv| (e, av, bv))
            })
        })
    }

    /// Мутабельная итерация по A + чтение B.
    ///
    /// Внутри один контролируемый `unsafe`: два разных `TypeId` гарантируют
    /// разные Box-аллокации в HashMap, значит `&mut A` и `&B` не пересекаются
    /// по памяти. HashMap во время цикла не мутируется — указатели стабильны.
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

        // Копируем список сущностей — чтобы не держать borrow во время цикла.
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

        // SAFETY:
        // 1. a_type != b_type => две разные записи в HashMap<TypeId, Box<dyn ...>>.
        // 2. Две разные записи => две разные Box-аллокации => два непересекающихся
        //    участка памяти. Одна мутабельная, вторая константная — допустимо.
        // 3. HashMap не модифицируется во время цикла — указатели валидны.
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

    /// Читать события текущего и предыдущего кадра.
    pub fn read_events<E: 'static + Send + Sync>(&self) -> impl Iterator<Item = &E> {
        self.events.read::<E>()
    }

    /// Читать только события этого кадра (без предыдущего).
    pub fn read_events_current<E: 'static + Send + Sync>(&self) -> impl Iterator<Item = &E> {
        self.events.read_current::<E>()
    }

    /// Вызывается движком в начале кадра: current → previous.
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