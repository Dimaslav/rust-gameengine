//! Event bus с двойной буферизацией.
//!
//! В начале кадра события из `current` переезжают в `previous`.
//! Читатель видит события этого и предыдущего кадра — так у систем есть
//! минимум один полный кадр, чтобы прочитать сообщение, даже если оно
//! отправлено системой, работающей после них.

use std::any::{Any, TypeId};
use std::collections::HashMap;

#[derive(Default)]
pub struct Events {
    current: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    previous: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Events {
    pub fn new() -> Self {
        Self::default()
    }

    /// Отправить событие. Доступно читателям в этом и следующем кадре.
    pub fn send<E: 'static + Send + Sync>(&mut self, event: E) {
        let queue = self
            .current
            .entry(TypeId::of::<E>())
            .or_insert_with(|| Box::new(Vec::<E>::new()));
        queue.downcast_mut::<Vec<E>>().unwrap().push(event);
    }

    /// Прочитать события — текущие + предыдущего кадра.
    pub fn read<E: 'static + Send + Sync>(&self) -> impl Iterator<Item = &E> {
        let current = self
            .current
            .get(&TypeId::of::<E>())
            .and_then(|q| q.downcast_ref::<Vec<E>>())
            .map(|v| v.iter())
            .into_iter()
            .flatten();

        let previous = self
            .previous
            .get(&TypeId::of::<E>())
            .and_then(|q| q.downcast_ref::<Vec<E>>())
            .map(|v| v.iter())
            .into_iter()
            .flatten();

        current.chain(previous)
    }

    /// Прочитать только события этого кадра (без предыдущего).
    pub fn read_current<E: 'static + Send + Sync>(&self) -> impl Iterator<Item = &E> {
        self.current
            .get(&TypeId::of::<E>())
            .and_then(|q| q.downcast_ref::<Vec<E>>())
            .map(|v| v.iter())
            .into_iter()
            .flatten()
    }

    /// Вызывается движком в начале кадра: `current` → `previous`.
    pub fn update(&mut self) {
        self.previous = std::mem::take(&mut self.current);
    }
}