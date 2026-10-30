//! Bounded reads from local and HTTPS sources, catalog and artifact acquisition,
//! and trust-on-first-use discovery for new registrations.
//!
//! Local reads are confined to the canonical source directory. HTTPS reads use
//! ordinary TLS validation with public roots, a fixed origin, no proxy, no
//! redirects, no content encoding and bounded size and time.

mod hosted;

use crate::{
    catalog::{self, Catalog, CatalogError, Entry},
    digest,
    files::{self, Bounded},
    names,
    source::{self, Source, SourceError},
};
use std::{
    io,
    path::Path,
    time::{Duration, SystemTime},
};

/// Maximum catalog document size, owned by the catalog format.
pub use crate::catalog::MAX_CATALOG_BYTES;
/// Longest overall deadline per HTTPS request; longer deadlines are capped.
pub const MAX_DEADLINE: Duration = Duration::from_hours(24);
/// Maximum size of `catalog-key.pub` and `catalog.sig`.
pub const MAX_TRUST_BYTES: u64 = 4096;

/// Acquisition failures.
#[derive(Debug, thiserror::Error)]
pub enum AcquisitionError {
    /// The registration location is neither a directory nor a supported URL.
    #[error("registry location must be a directory or a safe https:// URL")]
    Location,
    /// The artifact path is unsafe.
    #[error("invalid registry artifact path")]
    Path,
    /// The local source's directory is gone from its enrolled location, or
    /// its canonical path no longer matches it.
    #[error("local registry identity changed")]
    LocalIdentityChanged,
    /// A local artifact resolves outside its source.
    #[error("registry artifact escapes its source")]
    Escapes,
    /// The HTTPS request failed, including TLS validation and timeouts.
    #[error("registry HTTPS acquisition failed")]
    Transport,
    /// The response was not an unencoded HTTP 200.
    #[error("registry requires an unencoded HTTP 200 response")]
    Response,
    /// The response or file exceeds its size bound.
    #[error("registry response exceeds limits")]
    TooLarge,
    /// The published key is not 32 bytes of hexadecimal.
    #[error("registry signing key is invalid")]
    Key,
    /// A signed catalog does not declare its registry ID.
    #[error("signed registry identity missing")]
    RegistryId,
    /// Artifact bytes do not match their catalog digest.
    #[error("registry artifact digest mismatch")]
    Digest,
    /// The catalog was refused.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// The derived source is invalid.
    #[error(transparent)]
    Source(#[from] SourceError),
    /// Local filesystem failure.
    #[error("registry read failed")]
    Io(#[from] io::Error),
}

/// A newly discovered source and its first catalog, for operator review.
///
/// Nothing is enrolled until the caller confirms and stores the source.
#[derive(Clone, Debug)]
pub struct Discovery {
    /// The source to enroll, with its trust-on-first-use key.
    pub source: Source,
    /// Its authenticated catalog.
    pub catalog: Catalog,
}

/// Reads registry sources. Each HTTPS request runs within its own absolute
/// deadline, fixed when the request starts.
#[derive(Clone, Debug)]
pub struct Fetcher {
    client: hosted::Client,
}

impl Fetcher {
    /// A fetcher using public TLS roots and an overall per-request deadline,
    /// capped at [`MAX_DEADLINE`].
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self {
            client: hosted::Client::new(timeout, None),
        }
    }

    /// A fetcher trusting only `roots`, for the test support module.
    #[cfg(feature = "test-support")]
    pub(crate) fn with_test_roots(timeout: Duration, roots: ureq::tls::RootCerts) -> Self {
        Self {
            client: hosted::Client::new(timeout, Some(roots)),
        }
    }

    /// Reads one file from a source, at most `max_bytes` long.
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] for unsafe paths, moved or escaping local
    /// files, failed or unexpected HTTPS responses and oversized content.
    pub fn read(
        &self,
        source: &Source,
        path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, AcquisitionError> {
        if !names::is_artifact_path(path) {
            return Err(AcquisitionError::Path);
        }
        if source.is_signed() {
            return self.client.read(source.location(), path, max_bytes);
        }
        let root = Path::new(source.location());
        // A directory that is gone from its enrolled location, or replaced by
        // a file, has moved as surely as one that became a link.
        let canonical = root.canonicalize().map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => {
                AcquisitionError::LocalIdentityChanged
            }
            _ => AcquisitionError::Io(error),
        })?;
        if canonical != root || !canonical.is_dir() {
            return Err(AcquisitionError::LocalIdentityChanged);
        }
        let file = root.join(path).canonicalize()?;
        if !file.starts_with(root) {
            return Err(AcquisitionError::Escapes);
        }
        match files::read_bounded(&file, max_bytes)? {
            Bounded::Bytes(bytes) => Ok(bytes),
            Bounded::NotFile => Err(AcquisitionError::Path),
            Bounded::TooLarge => Err(AcquisitionError::TooLarge),
        }
    }

    /// Reads and authenticates a source's catalog at time `now`.
    ///
    /// The caller still admits it through [`crate::checkpoint::Checkpoints`].
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] for read or authentication failures.
    pub fn catalog(&self, source: &Source, now: SystemTime) -> Result<Catalog, AcquisitionError> {
        let bytes = self.read(source, "catalog.json", MAX_CATALOG_BYTES)?;
        let signature = if source.is_signed() {
            Some(self.read(source, "catalog.sig", MAX_TRUST_BYTES)?)
        } else {
            None
        };
        Ok(Catalog::verify(source, &bytes, signature.as_deref(), now)?)
    }

    /// Reads one release's artifact and verifies it against its catalog digest.
    ///
    /// # Errors
    /// Returns [`AcquisitionError::Digest`] for other bytes, and other
    /// [`AcquisitionError`] values as [`Fetcher::read`].
    pub fn artifact(
        &self,
        source: &Source,
        entry: &Entry,
        max_bytes: u64,
    ) -> Result<Vec<u8>, AcquisitionError> {
        let bytes = self.read(source, &entry.path, max_bytes)?;
        if digest::sha256_hex(&bytes) != entry.sha256 {
            return Err(AcquisitionError::Digest);
        }
        Ok(bytes)
    }

    /// Discovers a new source from an operator-supplied location.
    ///
    /// An `https://` location gets a trailing `/` if missing. Its published
    /// `catalog-key.pub` is accepted on first use only if it verifies the
    /// catalog, which must also declare its `registry_id`. Any other location
    /// is a local directory, canonicalized here.
    ///
    /// # Errors
    /// Returns [`AcquisitionError`] when the location, key or catalog is refused.
    pub fn discover(&self, location: &str, now: SystemTime) -> Result<Discovery, AcquisitionError> {
        if location.starts_with("https://") {
            self.discover_signed(location, now)
        } else {
            let source = discover_local(location)?;
            let catalog = self.catalog(&source, now)?;
            Ok(Discovery { source, catalog })
        }
    }

    /// Accepts the origin's published key only if it signs the catalog it
    /// serves, takes the registry ID from those authenticated bytes, and
    /// authenticates the same bytes in full, so the reviewed catalog is the
    /// one whose signature was checked.
    fn discover_signed(
        &self,
        location: &str,
        now: SystemTime,
    ) -> Result<Discovery, AcquisitionError> {
        let location = if location.ends_with('/') {
            location.to_owned()
        } else {
            format!("{location}/")
        };
        let location =
            source::canonical_https_location(&location).ok_or(AcquisitionError::Location)?;
        let key = self
            .client
            .read(&location, "catalog-key.pub", MAX_TRUST_BYTES)?;
        let key = std::str::from_utf8(&key)
            .ok()
            .and_then(|key| digest::normalize_hex(key.trim(), 32))
            .ok_or(AcquisitionError::Key)?;
        let bytes = self
            .client
            .read(&location, "catalog.json", MAX_CATALOG_BYTES)?;
        let signature = self
            .client
            .read(&location, "catalog.sig", MAX_TRUST_BYTES)?;
        catalog::verify_signature(&key, &bytes, &signature)?;
        let declared: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| CatalogError::Malformed)?;
        let registry_id = declared["registry_id"]
            .as_str()
            .ok_or(AcquisitionError::RegistryId)?;
        let source = Source::signed(&location, &key, registry_id)?;
        let catalog = Catalog::verify(&source, &bytes, Some(&signature), now)?;
        Ok(Discovery { source, catalog })
    }
}

/// A local source for the canonical form of an existing directory.
fn discover_local(location: &str) -> Result<Source, AcquisitionError> {
    let root = Path::new(location)
        .canonicalize()
        .map_err(|_| AcquisitionError::Location)?;
    if !root.is_dir() {
        return Err(AcquisitionError::Location);
    }
    Ok(Source::local(&root)?)
}

#[cfg(test)]
mod tests;
