//! Registry aliases, defaults and scope precedence.
//!
//! Aliases select sources for discovery only. A product supplies one reserved
//! built-in alias with a pinned source; project registrations shadow global ones,
//! and neither can shadow the built-in alias. The domain values here are pure;
//! [`GlobalFile`] persists the global scope.

mod global_file;

pub use global_file::{GlobalFile, MAX_FILE_BYTES};

use crate::{names, source::Source};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::PathBuf,
};

/// Maximum registrations in one scope.
pub const MAX_REGISTRATIONS: usize = 64;

/// Registrations of one scope: a project's or the user's global ones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registrations {
    /// Sources by alias.
    #[serde(default)]
    pub sources: BTreeMap<String, Source>,
    /// Default alias; `None` inherits the next scope's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Where a registration came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The product's pinned registry.
    Builtin,
    /// The user's global registrations.
    Global,
    /// The current project's registrations.
    Project,
}

/// Registration and persistence failures.
#[derive(Debug, thiserror::Error)]
pub enum RegistrationError {
    /// Aliases are 1–64 lowercase letters, digits or hyphens.
    #[error("invalid registry alias")]
    InvalidAlias,
    /// The built-in alias cannot be added, removed or shadowed.
    #[error("{0} is reserved")]
    Reserved(String),
    /// A registered source's identity is not derived from what it is, so it
    /// could impersonate another source, such as the built-in one.
    #[error("registry {0} has an identity that does not match its source")]
    Identity(String),
    /// The alias is already registered in this scope.
    #[error("registry alias already registered")]
    Duplicate,
    /// The alias is not registered.
    #[error("registry is not registered: {0}")]
    Missing(String),
    /// The scope already holds [`MAX_REGISTRATIONS`] registrations.
    #[error("registry registration limit exceeded")]
    Limit,
    /// A global default must name a global registration or the built-in alias.
    #[error("registry default is not registered")]
    InvalidDefault,
    /// The registrations file is not valid.
    #[error("invalid registry registrations file")]
    Malformed,
    /// The new registrations would serialize beyond [`MAX_FILE_BYTES`], so
    /// the written file could not be read back.
    #[error("registry registrations would exceed the file size limit")]
    TooLarge,
    /// The file path has no parent, or ends in `.lock` or `.new` in any letter
    /// case, which its own lock or temporary file would use.
    #[error("registry registrations file path is unusable")]
    Path,
    /// The registrations file or its directory is a symbolic link.
    #[error("registry registrations cannot be a link")]
    Link,
    /// The file changed after the caller read it.
    #[error("registry registrations changed; review again")]
    Changed,
    /// Another writer holds the lock, or a crashed writer left it behind.
    #[error("registry registrations are locked by another writer: {0}")]
    Locked(PathBuf),
    /// The new registrations were written, but syncing their directory
    /// failed, so they may revert to the previous ones after a crash.
    #[error("registry registrations were written but may not survive a crash")]
    NotDurable(#[source] io::Error),
    /// The new registrations were written, but the lock could not be removed;
    /// later writes are refused until it is. Reported in preference to
    /// [`RegistrationError::NotDurable`] when both occur.
    #[error("registry registrations were written but the lock remains: {0}")]
    LockRemains(PathBuf, #[source] io::Error),
    /// Nothing was written, because of `refusal`, and the lock could not be
    /// removed either; later writes are refused until it is.
    #[error("registry registrations were not written ({refusal}) and the lock remains: {}", lock.display())]
    RefusedLockRemains {
        /// Why nothing was written.
        refusal: Box<RegistrationError>,
        /// The lock file to remove.
        lock: PathBuf,
        /// Why the lock could not be removed.
        #[source]
        source: io::Error,
    },
    /// Filesystem failure.
    #[error("registry registrations I/O failed")]
    Io(#[from] io::Error),
}

impl Registrations {
    /// Validates aliases, bounds and, for global scope, the default.
    ///
    /// # Errors
    /// Returns [`RegistrationError`] for the first invalid field.
    pub fn validate(&self, builtin: &str, scope: Scope) -> Result<(), RegistrationError> {
        if self.sources.len() > MAX_REGISTRATIONS {
            return Err(RegistrationError::Limit);
        }
        for (alias, source) in &self.sources {
            alias_allowed(alias, builtin)?;
            if !source.has_derived_identity() {
                return Err(RegistrationError::Identity(alias.clone()));
            }
        }
        if let Some(default) = &self.default {
            if !names::is_identifier(default) {
                return Err(RegistrationError::InvalidAlias);
            }
            if scope == Scope::Global && default != builtin && !self.sources.contains_key(default) {
                return Err(RegistrationError::InvalidDefault);
            }
        }
        Ok(())
    }

    /// Registers `source` under a new alias.
    ///
    /// # Errors
    /// Returns [`RegistrationError`] for invalid, reserved or duplicate aliases,
    /// or when the scope is full.
    pub fn add(
        &mut self,
        alias: &str,
        source: Source,
        builtin: &str,
    ) -> Result<(), RegistrationError> {
        alias_allowed(alias, builtin)?;
        if !source.has_derived_identity() {
            return Err(RegistrationError::Identity(alias.to_owned()));
        }
        if self.sources.contains_key(alias) {
            return Err(RegistrationError::Duplicate);
        }
        if self.sources.len() >= MAX_REGISTRATIONS {
            return Err(RegistrationError::Limit);
        }
        self.sources.insert(alias.to_owned(), source);
        Ok(())
    }

    /// Removes an alias, clearing the default when it named that alias.
    /// Installed packs from the source are unaffected.
    ///
    /// # Errors
    /// Returns [`RegistrationError`] for invalid, reserved or missing aliases.
    pub fn remove(&mut self, alias: &str, builtin: &str) -> Result<Source, RegistrationError> {
        alias_allowed(alias, builtin)?;
        let source = self
            .sources
            .remove(alias)
            .ok_or_else(|| RegistrationError::Missing(alias.to_owned()))?;
        if self.default.as_deref() == Some(alias) {
            self.default = None;
        }
        Ok(source)
    }

    /// Makes `alias` this scope's default. It must be registered in
    /// `available`: for a global default, merge without a project scope.
    ///
    /// # Errors
    /// Returns [`RegistrationError::InvalidAlias`] for a malformed alias and
    /// [`RegistrationError::Missing`] when `available` does not register it.
    pub fn set_default(
        &mut self,
        alias: &str,
        available: &Effective,
    ) -> Result<(), RegistrationError> {
        if !names::is_identifier(alias) {
            return Err(RegistrationError::InvalidAlias);
        }
        if available.get(alias).is_none() {
            return Err(RegistrationError::Missing(alias.to_owned()));
        }
        self.default = Some(alias.to_owned());
        Ok(())
    }
}

fn alias_allowed(alias: &str, builtin: &str) -> Result<(), RegistrationError> {
    if !names::is_identifier(alias) {
        return Err(RegistrationError::InvalidAlias);
    }
    if alias == builtin {
        return Err(RegistrationError::Reserved(builtin.to_owned()));
    }
    Ok(())
}

/// The registrations in effect: built-in, then global, then project.
#[derive(Clone, Debug)]
pub struct Effective {
    sources: BTreeMap<String, (Source, Scope)>,
    default: String,
}

impl Effective {
    /// Merges scopes. Project aliases shadow global ones; the built-in alias
    /// is always present and never shadowed. The default is the project's,
    /// else the global one, else the built-in alias.
    ///
    /// Both scopes are validated first, however they were loaded, so no
    /// registration in effect can claim the built-in alias or another
    /// source's identity.
    ///
    /// # Errors
    /// Returns the first [`RegistrationError`] from
    /// [`Registrations::validate`] for either scope.
    pub fn merge(
        builtin: (&str, &Source),
        global: &Registrations,
        project: Option<&Registrations>,
    ) -> Result<Self, RegistrationError> {
        global.validate(builtin.0, Scope::Global)?;
        if let Some(project) = project {
            project.validate(builtin.0, Scope::Project)?;
        }
        let mut sources = BTreeMap::new();
        for (scope, registrations) in [(Scope::Global, Some(global)), (Scope::Project, project)] {
            for (alias, source) in registrations.iter().flat_map(|r| &r.sources) {
                sources.insert(alias.clone(), (source.clone(), scope));
            }
        }
        sources.insert(builtin.0.to_owned(), (builtin.1.clone(), Scope::Builtin));
        let default = project
            .and_then(|p| p.default.clone())
            .or_else(|| global.default.clone())
            .unwrap_or_else(|| builtin.0.to_owned());
        Ok(Self { sources, default })
    }

    /// The source registered under `alias`.
    #[must_use]
    pub fn get(&self, alias: &str) -> Option<&Source> {
        self.sources.get(alias).map(|(source, _)| source)
    }

    /// The scope that supplied `alias`.
    #[must_use]
    pub fn scope(&self, alias: &str) -> Option<Scope> {
        self.sources.get(alias).map(|(_, scope)| *scope)
    }

    /// The default alias, which may name a registration that no longer exists.
    #[must_use]
    pub fn default_alias(&self) -> &str {
        &self.default
    }

    /// The source for an explicit alias, or the default when `None`.
    ///
    /// # Errors
    /// Returns [`RegistrationError::Missing`] when the alias is not registered.
    pub fn select(&self, alias: Option<&str>) -> Result<&Source, RegistrationError> {
        let alias = alias.unwrap_or(&self.default);
        self.get(alias)
            .ok_or_else(|| RegistrationError::Missing(alias.to_owned()))
    }

    /// Every registration with its alias and scope, in alias order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Source, Scope)> {
        self.sources
            .iter()
            .map(|(alias, (source, scope))| (alias.as_str(), source, *scope))
    }

    /// The first registration, in alias order, whose source has `identity`.
    ///
    /// Several aliases may name the same source; alias order makes the
    /// choice deterministic. The scope tells callers whether it is built in.
    #[must_use]
    pub fn by_identity(&self, identity: &str) -> Option<(&str, &Source, Scope)> {
        self.iter()
            .find(|(_, source, _)| source.identity() == identity)
    }

    /// Identities of every registered source, for dependency resolution.
    #[must_use]
    pub fn identities(&self) -> BTreeSet<String> {
        self.sources
            .values()
            .map(|(source, _)| source.identity().to_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests;
