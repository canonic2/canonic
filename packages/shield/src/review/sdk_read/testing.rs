//! Real profile/context issuance in thread-local test-owned private namespaces.
use super::{Error, RegisteredReadConsumer, binary, files, mapping, mint, setup};
use std::path::{Path, PathBuf};

pub(crate) struct Fixture {
    home: assert_fs::TempDir,
    binary: PathBuf,
    root: PathBuf,
}

impl Fixture {
    pub(crate) fn new(root: &Path) -> Result<Self, Error> {
        let home =
            assert_fs::TempDir::new().map_err(|_| Error::Invalid("SDK read fixture home"))?;
        let binary = home.path().join("shield");
        std::fs::write(&binary, b"Acme owned Shield executable fixture")?;
        let fixture = Self {
            home,
            binary,
            root: root.canonicalize()?,
        };
        fixture.scoped(|| {
            let context = super::super::open(&fixture.root)?;
            setup::Prepared::new(&context, mapping::ID)?.commit(&context, &|| Ok(()))?;
            Ok(())
        })?;
        Ok(fixture)
    }

    pub(super) fn scoped<T>(
        &self,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        files::with_home(self.home.path(), || {
            binary::with_executable(&self.binary, operation)
        })
    }

    pub(crate) fn consumers(&self, count: usize) -> Result<Vec<RegisteredReadConsumer>, Error> {
        self.scoped(|| {
            let issued = (0..count)
                .map(|_| mint(&self.root, mapping::digest()))
                .collect::<Result<Vec<_>, _>>()?;
            issued
                .into_iter()
                .map(|issued| RegisteredReadConsumer::admit_secret(&self.root, issued.0))
                .collect()
        })
    }

    pub(crate) fn consumer(&self) -> Result<RegisteredReadConsumer, Error> {
        self.consumers(1)?.pop().ok_or(Error::Missing)
    }
}
