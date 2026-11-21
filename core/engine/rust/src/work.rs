//! Work a block does, counted in test builds only (`cargo test`, for engine-cost-budget-core): what it visits, not how
//! long it takes, so a change that adds per-block work fails on any machine. Compiled to nothing in the engine.

/// Budget entries visited by a pass over them.
pub const ENTRIES: usize = 0;
/// Note slots read (NoteRoute::slot).
pub const SLOTS: usize = 1;
/// Listed voices ranked by insertion (Heard::insert).
pub const RANKED: usize = 2;

#[cfg(test)]
thread_local!(pub static COUNTS: std::cell::Cell<[u64; 3]> = const { std::cell::Cell::new([0; 3]) });

#[inline(always)]
pub fn add(_kind: usize, _n: usize) {
    #[cfg(test)]
    COUNTS.with(|c| {
        let mut v = c.get();
        v[_kind] += _n as u64;
        c.set(v);
    });
}
