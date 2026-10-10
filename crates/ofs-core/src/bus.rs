//! Named, typed signals shared by all models. Values live in per-type arenas; a `Signal<T>` is an index.
use std::collections::BTreeMap;
use std::marker::PhantomData;

use glam::{DQuat, DVec3};

use crate::rng::Fnv1a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Scalar,
    Vec3,
    Quat,
}

#[derive(Debug)]
pub struct Signal<T> {
    index: usize,
    _t: PhantomData<T>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}

pub trait BusValue: Copy + Default + 'static {
    fn kind() -> SignalKind;
    fn arena(bus: &Bus) -> &Vec<Self>;
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self>;
}

#[derive(Debug, Default)]
pub struct Bus {
    scalars: Vec<f64>,
    vec3s: Vec<DVec3>,
    quats: Vec<DQuat>,
    names: BTreeMap<String, (SignalKind, usize)>,
}

impl BusValue for f64 {
    fn kind() -> SignalKind {
        SignalKind::Scalar
    }
    fn arena(bus: &Bus) -> &Vec<Self> {
        &bus.scalars
    }
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self> {
        &mut bus.scalars
    }
}

impl BusValue for DVec3 {
    fn kind() -> SignalKind {
        SignalKind::Vec3
    }
    fn arena(bus: &Bus) -> &Vec<Self> {
        &bus.vec3s
    }
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self> {
        &mut bus.vec3s
    }
}

impl BusValue for DQuat {
    fn kind() -> SignalKind {
        SignalKind::Quat
    }
    fn arena(bus: &Bus) -> &Vec<Self> {
        &bus.quats
    }
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self> {
        &mut bus.quats
    }
}

impl Bus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `name`, or returns the existing signal if it was already registered with the same type.
    pub fn signal<T: BusValue>(&mut self, name: &str) -> Signal<T> {
        if let Some(&(kind, index)) = self.names.get(name) {
            assert_eq!(kind, T::kind(), "signal '{name}' already registered as {kind:?}");
            return Signal { index, _t: PhantomData };
        }
        let arena = T::arena_mut(self);
        let index = arena.len();
        arena.push(T::default());
        self.names.insert(name.to_string(), (T::kind(), index));
        Signal { index, _t: PhantomData }
    }

    pub fn lookup<T: BusValue>(&self, name: &str) -> Option<Signal<T>> {
        match self.names.get(name) {
            Some(&(kind, index)) if kind == T::kind() => Some(Signal { index, _t: PhantomData }),
            _ => None,
        }
    }

    pub fn get<T: BusValue>(&self, s: Signal<T>) -> T {
        T::arena(self)[s.index]
    }

    pub fn set<T: BusValue>(&mut self, s: Signal<T>, value: T) {
        T::arena_mut(self)[s.index] = value;
    }

    /// Hash of every signal's name and exact bit pattern, in name order.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv1a::new();
        for (name, &(kind, i)) in &self.names {
            h.write(name.as_bytes());
            let values: Vec<f64> = match kind {
                SignalKind::Scalar => vec![self.scalars[i]],
                SignalKind::Vec3 => self.vec3s[i].to_array().to_vec(),
                SignalKind::Quat => self.quats[i].to_array().to_vec(),
            };
            for v in values {
                h.write(&v.to_bits().to_le_bytes());
            }
        }
        h.finish()
    }

    /// Name of the first signal (in name order) holding NaN or infinity.
    pub fn first_non_finite(&self) -> Option<&str> {
        // Checked every tick: scan the contiguous arenas, and search by name only when something is wrong.
        let all_finite = self.scalars.iter().all(|v| v.is_finite())
            && self.vec3s.iter().all(|v| v.is_finite())
            && self.quats.iter().all(|q| q.is_finite());
        if all_finite {
            return None;
        }
        self.names
            .iter()
            .find(|&(_, &(kind, i))| match kind {
                SignalKind::Scalar => !self.scalars[i].is_finite(),
                SignalKind::Vec3 => !self.vec3s[i].is_finite(),
                SignalKind::Quat => !self.quats[i].is_finite(),
            })
            .map(|(name, _)| name.as_str())
    }
}
