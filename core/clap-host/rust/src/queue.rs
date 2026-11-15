// SPDX-License-Identifier: AGPL-3.0-or-later
//! Internal.h's Queue: a bounded Vyukov queue. Producers claim independent slots; the one consumer never spins on an
//! unfinished producer. No allocation after it is made, no locks.
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering::*};

struct Slot<T> {
    sequence: AtomicUsize,
    value: UnsafeCell<T>,
}

#[repr(align(64))]
struct Aligned<T>(T);

pub struct Queue<T> {
    slots: Box<[Slot<T>]>,
    write: Aligned<AtomicUsize>,
    read: Aligned<UnsafeCell<usize>>,
}
// SAFETY: a slot's value is written by the producer that claimed it and read by the one consumer after its sequence
// says so (release/acquire).
unsafe impl<T: Send> Sync for Queue<T> {}

impl<T: Copy + Default> Queue<T> {
    pub fn new(capacity: usize) -> Self {
        let slots = (0..capacity).map(|i| Slot { sequence: AtomicUsize::new(i), value: UnsafeCell::new(T::default()) }).collect();
        Queue { slots, write: Aligned(AtomicUsize::new(0)), read: Aligned(UnsafeCell::new(0)) }
    }
    pub fn push(&self, value: T) -> bool {
        let capacity = self.slots.len();
        let mut position = self.write.0.load(Relaxed);
        loop {
            let slot = &self.slots[position % capacity];
            let difference = slot.sequence.load(Acquire).wrapping_sub(position) as isize;
            if difference == 0 {
                if self.write.0.compare_exchange_weak(position, position + 1, Relaxed, Relaxed).is_ok() {
                    // SAFETY: this producer claimed the slot; the consumer reads it only after the release below.
                    unsafe { *slot.value.get() = value };
                    slot.sequence.store(position + 1, Release);
                    return true;
                }
            } else if difference < 0 {
                return false;
            } else {
                position = self.write.0.load(Relaxed);
            }
        }
    }
    /// The one consumer's.
    pub fn pop(&self) -> Option<T> {
        // SAFETY: only the one consumer touches read.
        let read = unsafe { &mut *self.read.0.get() };
        let slot = &self.slots[*read % self.slots.len()];
        if slot.sequence.load(Acquire) != *read + 1 {
            return None;
        }
        // SAFETY: published by its producer's release, seen by the acquire above.
        let value = unsafe { *slot.value.get() };
        slot.sequence.store(*read + self.slots.len(), Release);
        *read += 1;
        Some(value)
    }
}
