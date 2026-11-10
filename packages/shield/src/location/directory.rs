//! Bounded current directory observations; never executes a shell builtin.
use crate::policy::Error;
use std::{cell::Cell, path::Path};

pub(crate) struct Directories {
    observations: Cell<usize>,
}

impl Directories {
    pub(crate) fn new() -> Self {
        Self {
            observations: Cell::new(0),
        }
    }

    pub(crate) fn observe(
        &self,
        path: &Path,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Option<String>, Error> {
        checkpoint()?;
        if !path.is_absolute() || path.as_os_str().len() > 4096 {
            return Ok(None);
        }
        let count = self.observations.get() + 1;
        if count > 128 {
            return Err(Error::Limit);
        }
        self.observations.set(count);
        let observed = path
            .canonicalize()
            .ok()
            .filter(|path| path.is_dir())
            .and_then(|path| path.to_str().map(str::to_owned));
        checkpoint()?;
        Ok(observed)
    }
}
