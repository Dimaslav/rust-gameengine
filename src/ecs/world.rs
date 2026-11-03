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

    /// ИЗМЕНЕНО (#9): скретч-буфер для `for_each_pair`.
    ///
    /// Раньше при каждом вызове `for_each_pair` создавался новый
    /// `Vec<Entity>` через `ComponentStorage::entity_list()` (который
    /// делал `.clone()`). В `MovementSystem::update` это происходило
    /// каждый кадр — 60+ аллокаций/сек для сцены средней величины.
    ///
    /// Теперь: один `Vec<Entity>` живёт в `World`, `for_each_pair`
    /// вынимает его через `mem::take`, заполняет из `entities_iter()`
    /// A-стораджа, обходит, возвращает обратно. Аллокация происходит
    /// только при первом вызове (или если буфер вырос).
    ///
    /// `Default` даёт `Vec::new()` — первая итерация аллоцирует, дальше
    /// только переиспользуется.
    scratch_entities: Vec<Entity>,
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
        if let Some(&max) = self.entities.iter().max()
            && max >= self.next_id {
                self.next_id = max.saturating_add(1);
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

    /// ИЗМЕНЕНО (#9): переписано на переиспользуемый скретч-буфер.
    ///
    /// До этого `entity_list()` клонировал `Vec<Entity>` из A-стораджа
    /// **на каждый вызов**. `MovementSystem::update` дёргает этот метод
    /// каждый кадр → 60+ аллокаций/сек + memcpy. Теперь буфер живёт
    /// в `World` и переиспользуется между вызовами.
    ///
    /// Рекурсивный вызов `for_each_pair` из `f` технически возможен
    /// (хотя требует `unsafe` или передачи `World` через `RefCell`),
    /// и он тоже безопасен: `mem::take` вынимает буфер локально, а
    /// вложенный вызов получит `Vec::new()` (аллоцирует свой), потом
    /// вернёт его. Внешний вызов не увидит внутренний буфер и вернёт
    /// свой — никаких гонок, никакого UB.
    ///
    /// Все ранние `return` возвращают скретч обратно, чтобы следующая
    /// итерация не потеряла capacity.
    pub fn for_each_pair<A, B, F>(&mut self, mut f: F)
    where
        A: Send + Sync + 'static,
        B: Send + Sync + 'static,
        F: FnMut(Entity, &mut A, &B),
    {
        let a_type = TypeId::of::<A>();
        let b_type = TypeId::of::<B>();
        assert_ne!(a_type, b_type, "for_each_pair requires distinct component types");

        // ИЗМЕНЕНО (#9): `mem::take` — вынимаем скретч локально.
        // `self.scratch_entities` остаётся пустым `Vec::new()` на время
        // работы метода; вернём заполненный в конце.
        let mut entities = std::mem::take(&mut self.scratch_entities);
        entities.clear();

        // Заполняем буфер entity из A-стораджа без промежуточной
        // аллокации (`entities_iter()` возвращает `slice::Iter`,
        // `.copied()` даёт `u32`, `extend` пишет прямо в буфер).
        if let Some(s) = self.storages.get(&a_type)
            && let Some(storage) = s.as_any().downcast_ref::<ComponentStorage<A>>() {
                entities.extend(storage.entities_iter().copied());
            }

        if entities.is_empty() {
            // A-сторадж пуст или отсутствует — работа сделана,
            // возвращаем capacity-буфер в World.
            self.scratch_entities = entities;
            return;
        }

        // Raw pointers — обходим borrow checker, т.к. оба стораджа
        // берутся из одной HashMap. Разные TypeId → разные Box →
        // непересекающиеся регионы памяти.
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
            None => {
                // B-стораджа нет — пересечения быть не может.
                // Возвращаем скретч обратно в World.
                self.scratch_entities = entities;
                return;
            }
        };

        // SAFETY: два разных TypeId → две разные Box-аллокации → два
        // непересекающихся участка памяти. HashMap (`self.storages`)
        // не модифицируется во время цикла. `a_ptr` мутабельный, но
        // `b_ptr` указывает на другой объект.
        unsafe {
            let a_store = &mut *a_ptr;
            let b_store = &*b_ptr;
            for &e in &entities {
                if let (Some(a), Some(b)) = (a_store.get_mut(e), b_store.get(e)) {
                    f(e, a, b);
                }
            }
        }

        // ИЗМЕНЕНО (#9): возвращаем capacity-буфер в World.
        self.scratch_entities = entities;
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

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Tag(u32);

    #[test]
    fn spawn_and_despawn() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        let c = w.spawn();
        assert_eq!(w.len(), 3);
        assert_eq!(w.entities(), &[a, b, c]);

        w.despawn(b);
        assert_eq!(w.len(), 2);
        assert!(!w.entities().contains(&b));
        // a и c остались.
        assert!(w.entities().contains(&a));
        assert!(w.entities().contains(&c));
    }

    #[test]
    fn despawn_removes_components() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Pos(1));
        w.insert(e, Vel(2));
        assert!(w.has::<Pos>(e));
        assert!(w.has::<Vel>(e));

        w.despawn(e);
        assert!(!w.has::<Pos>(e));
        assert!(!w.has::<Vel>(e));
    }

    #[test]
    fn insert_get_remove() {
        let mut w = World::new();
        let e = w.spawn();
        assert!(w.get::<Pos>(e).is_none());

        w.insert(e, Pos(10));
        assert_eq!(w.get::<Pos>(e), Some(&Pos(10)));

        // Перезапись.
        w.insert(e, Pos(20));
        assert_eq!(w.get::<Pos>(e), Some(&Pos(20)));

        let removed = w.remove::<Pos>(e);
        assert_eq!(removed, Some(Pos(20)));
        assert!(w.get::<Pos>(e).is_none());
    }

    #[test]
    fn query_filters_by_component() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        let c = w.spawn();
        w.insert(a, Pos(1));
        w.insert(b, Vel(2));
        w.insert(c, Pos(3));

        let mut found: Vec<(Entity, i32)> = w.query::<Pos>()
            .map(|(e, p)| (e, p.0))
            .collect();
        found.sort_by_key(|(_, v)| *v);
        assert_eq!(found, vec![(a, 1), (c, 3)]);
    }

    #[test]
    fn query2_requires_both() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        let c = w.spawn();
        w.insert(a, Pos(1));
        w.insert(a, Vel(10));
        w.insert(b, Pos(2));
        // b без Vel.
        w.insert(c, Vel(30));
        // c без Pos.

        let found: Vec<(Entity, i32, i32)> = w.query2::<Pos, Vel>()
            .map(|(e, p, v)| (e, p.0, v.0))
            .collect();
        assert_eq!(found, vec![(a, 1, 10)]);
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
        // b без Vel — не должен попасть.
        w.insert(c, Vel(1));
        // c без Pos — не должен попасть.

        let mut visited: Vec<Entity> = Vec::new();
        w.for_each_pair::<Pos, Vel, _>(|e, _p, _v| visited.push(e));
        assert_eq!(visited, vec![a]);
    }

    #[test]
    fn for_each_pair_mutates_a_not_b() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Pos(10));
        w.insert(e, Vel(5));

        w.for_each_pair::<Pos, Vel, _>(|_e, p, v| {
            p.0 += v.0;
        });

        assert_eq!(w.get::<Pos>(e), Some(&Pos(15)));
        assert_eq!(w.get::<Vel>(e), Some(&Vel(5)));
    }

    #[test]
    fn for_each_pair_returns_scratch_capacity() {
        // После вызова скретч-буфер должен вернуться в World и
        // переиспользоваться на следующем вызове без падения.
        let mut w = World::new();
        for i in 0..100 {
            let e = w.spawn();
            w.insert(e, Pos(i));
            w.insert(e, Vel(1));
        }

        let mut count1 = 0;
        w.for_each_pair::<Pos, Vel, _>(|_, _, _| count1 += 1);
        assert_eq!(count1, 100);

        // Второй вызов — тот же буфер.
        let mut count2 = 0;
        w.for_each_pair::<Pos, Vel, _>(|_, _, _| count2 += 1);
        assert_eq!(count2, 100);
    }

    #[test]
    fn for_each_pair_empty_is_noop() {
        let mut w = World::new();
        let mut called = false;
        w.for_each_pair::<Pos, Vel, _>(|_, _, _| called = true);
        assert!(!called);
    }

    #[test]
    fn for_each_pair_missing_b_store() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Pos(1));
        // Vel ни у кого нет — B-стораджа не существует.
        let mut called = false;
        w.for_each_pair::<Pos, Vel, _>(|_, _, _| called = true);
        assert!(!called);
    }

    #[test]
    #[should_panic(expected = "distinct component types")]
    fn for_each_pair_panics_on_same_type() {
        let mut w = World::new();
        w.for_each_pair::<Pos, Pos, _>(|_, _, _| {});
    }

    #[test]
    fn sync_next_id_after_manual_insert() {
        let mut w = World::new();
        // Симулируем загрузку: id вставлены напрямую, минуя spawn.
        w.entities.push(100);
        w.entity_pos.insert(100, 0);
        w.sync_next_id();
        let new = w.spawn();
        assert!(new > 100);
    }

    #[test]
    fn events_double_buffer() {
        let mut w = World::new();
        w.send(42u32);
        w.send(43u32);

        // current видит оба.
        let cur: Vec<u32> = w.read_events_current::<u32>().copied().collect();
        assert_eq!(cur, vec![42, 43]);

        // После update — current пуст, previous содержит старые.
        w.update_events();
        let cur: Vec<u32> = w.read_events_current::<u32>().copied().collect();
        assert!(cur.is_empty());
        let all: Vec<u32> = w.read_events::<u32>().copied().collect();
        assert_eq!(all, vec![42, 43]);

        // Отправляем новое в current.
        w.send(44u32);
        let all: Vec<u32> = w.read_events::<u32>().copied().collect();
        assert_eq!(all, vec![44, 42, 43]);
    }

    #[test]
    fn insert_many_entities_then_iterate() {
        let mut w = World::new();
        for i in 0..1000 {
            let e = w.spawn();
            w.insert(e, Tag(i));
        }
        let mut sum = 0u64;
        for (_, t) in w.query::<Tag>() {
            sum += t.0 as u64;
        }
        // 0 + 1 + ... + 999 = 499500.
        assert_eq!(sum, 499_500);
    }
}