//! Thread-local work counts for pure-core scaling regressions; absent from library builds.
use std::cell::Cell;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) preparations: usize,
    pub(crate) page_definitions: usize,
    pub(crate) encodings: usize,
    pub(crate) measurements: usize,
    pub(crate) encoded_bytes: usize,
    pub(crate) measured_bytes: usize,
}

thread_local! {
    static COUNTS: Cell<Counts> = Cell::new(Counts::default());
}

pub(crate) fn record(update: impl FnOnce(&mut Counts)) {
    COUNTS.with(|cell| {
        let mut counts = cell.get();
        update(&mut counts);
        cell.set(counts);
    });
}

pub(crate) fn reset() {
    COUNTS.with(|cell| cell.set(Counts::default()));
}

pub(crate) fn counts() -> Counts {
    COUNTS.with(Cell::get)
}
