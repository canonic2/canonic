//! Selects literal executable candidates without changing matcher or resolver semantics.

use std::{collections::BTreeMap, ops::Range, slice};

pub(super) struct Index(BTreeMap<String, Vec<usize>>);

impl Index {
    pub(super) fn new<'a>(executables: impl Iterator<Item = Option<&'a str>>) -> Self {
        let mut entries: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (position, executable) in executables.enumerate() {
            entries
                .entry(executable.unwrap_or_default().into())
                .or_default()
                .push(position);
        }
        Self(entries)
    }

    pub(super) fn candidates(
        &self,
        executable: &str,
        length: usize,
        indexed: bool,
    ) -> Candidates<'_> {
        if indexed {
            Candidates::Indexed(
                self.0
                    .get(executable)
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .chain(self.0.get("").map_or(&[][..], Vec::as_slice).iter()),
            )
        } else {
            Candidates::All(0..length)
        }
    }
}

pub(super) enum Candidates<'a> {
    Indexed(std::iter::Chain<slice::Iter<'a, usize>, slice::Iter<'a, usize>>),
    All(Range<usize>),
}

impl Iterator for Candidates<'_> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        match self {
            Self::Indexed(values) => values.next().copied(),
            Self::All(values) => values.next(),
        }
    }
}
