//! Pack references typed by operators and stable selectors for installed packs.
//!
//! A catalog reference is `name`, `publisher/name`, `alias/name` or
//! `alias/publisher/name`, with an optional exact `@version`. An installed
//! selector is `pack:` followed by a UUID-formatted value that never changes
//! for the lifetime of that installed instance.

use crate::{
    digest,
    names::{self, PackId},
};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

/// What an operator's pack argument refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    /// An already installed instance.
    Installed(InstalledSelector),
    /// A pack to look up in a catalog.
    Catalog(Reference),
}

/// A catalog reference, resolved against one registry's catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// Explicit registry alias; `None` means the operator's default registry.
    pub alias: Option<String>,
    /// `name` or `publisher/name` as typed.
    pub pack: String,
    /// Exact version, which also pins the selection.
    pub version: Option<Version>,
}

/// A stable `pack:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` selector.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstalledSelector(String);

/// Malformed selectors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectorError {
    /// The input exceeds the text bound.
    #[error("pack reference is too long")]
    Limit,
    /// The `@` suffix is not an exact semantic version.
    #[error("exact pack version required")]
    Version,
    /// The reference does not have one to three valid segments.
    #[error("invalid pack reference")]
    Reference,
    /// The installed selector is not `pack:` plus a lowercase UUID form.
    #[error("invalid installed pack selector")]
    Installed,
}

/// Parses an operator reference; `is_alias` reports registered aliases.
///
/// A two-segment reference whose first segment is a registered alias selects
/// that registry; otherwise the first segment is a publisher in the default
/// registry.
///
/// ```
/// use canonic_registry::selector::{Selector, parse};
/// let Selector::Catalog(reference) = parse("team/acme/git@1.2.0", |a| a == "team").unwrap() else {
///     unreachable!()
/// };
/// assert_eq!(reference.alias.as_deref(), Some("team"));
/// assert_eq!(reference.pack, "acme/git");
/// assert!(reference.is_pinned());
/// ```
///
/// # Errors
/// Returns [`SelectorError`] for oversized, unversionable or malformed input.
pub fn parse(input: &str, is_alias: impl Fn(&str) -> bool) -> Result<Selector, SelectorError> {
    if input.len() > names::MAX_TEXT {
        return Err(SelectorError::Limit);
    }
    if input.starts_with("pack:") {
        return input.parse().map(Selector::Installed);
    }
    let (name, version) = match input.rsplit_once('@') {
        Some((name, version)) => (
            name,
            Some(Version::parse(version).map_err(|_| SelectorError::Version)?),
        ),
        None => (input, None),
    };
    let parts: Vec<_> = name.split('/').collect();
    if !parts.iter().all(|p| names::is_identifier(p)) {
        return Err(SelectorError::Reference);
    }
    let (alias, pack) = match parts.as_slice() {
        [name] => (None, (*name).to_owned()),
        [alias, name] if is_alias(alias) => (Some((*alias).to_owned()), (*name).to_owned()),
        [publisher, name] => (None, format!("{publisher}/{name}")),
        [alias, publisher, name] => (Some((*alias).to_owned()), format!("{publisher}/{name}")),
        _ => return Err(SelectorError::Reference),
    };
    Ok(Selector::Catalog(Reference {
        alias,
        pack,
        version,
    }))
}

/// What an installed instance exposes to reference matching.
#[derive(Clone, Copy, Debug)]
pub struct Installed<'a> {
    /// The instance's `pack:` selector.
    pub selector: &'a str,
    /// The alias the caller associates with its source, if any.
    pub alias: Option<&'a str>,
    /// Fully qualified publisher/name.
    pub id: &'a PackId,
}

/// Why no single installed instance matched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstalledError {
    /// Nothing installed matches.
    #[error("pack is not installed")]
    Missing,
    /// Several instances match; the operator must use a `pack:` selector.
    #[error("installed reference is ambiguous; use pack:<uuid>")]
    Ambiguous,
}

/// Finds the one installed instance an operator reference names.
///
/// Any `@version` suffix is ignored. When the reference's first segment is an
/// alias of some installed instance, only that alias's instances match, by
/// `publisher/name` or bare name. Otherwise an instance matches by its
/// `pack:` selector, `publisher/name`, bare name, `alias/publisher/name` or
/// `alias/name`.
///
/// # Errors
/// Returns [`InstalledError`] unless exactly one instance matches.
pub fn find_installed<'a>(
    installed: impl IntoIterator<Item = Installed<'a>>,
    reference: &str,
) -> Result<usize, InstalledError> {
    let installed: Vec<_> = installed.into_iter().collect();
    let name = reference.split('@').next().unwrap_or_default();
    let scoped = name
        .split_once('/')
        .filter(|(alias, _)| installed.iter().any(|i| i.alias == Some(*alias)));
    let found: Vec<_> = installed
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let id = item.id.to_string();
            if let Some((alias, rest)) = scoped {
                return item.alias == Some(alias) && (id == rest || item.id.name() == rest);
            }
            item.selector == name
                || id == name
                || item.id.name() == name
                || item.alias.is_some_and(|alias| {
                    format!("{alias}/{id}") == name || format!("{alias}/{}", item.id.name()) == name
                })
        })
        .map(|(index, _)| index)
        .collect();
    match found.as_slice() {
        [index] => Ok(*index),
        [] => Err(InstalledError::Missing),
        _ => Err(InstalledError::Ambiguous),
    }
}

impl Reference {
    /// Whether an exact version pins the selection.
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        self.version.is_some()
    }

    /// The fully qualified identity when the reference names a publisher.
    #[must_use]
    pub fn pack_id(&self) -> Option<PackId> {
        self.pack.parse().ok()
    }
}

impl InstalledSelector {
    /// Derives a selector from caller-chosen unique seed bytes.
    ///
    /// Callers include enough context (for example project, source, pack and
    /// time) that distinct installations never share a seed.
    #[must_use]
    pub fn from_seed(seed: &[u8]) -> Self {
        let hex = digest::sha256_hex(seed);
        Self(format!(
            "pack:{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        ))
    }

    /// The full `pack:` selector text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for InstalledSelector {
    type Err = SelectorError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let uuid = value
            .strip_prefix("pack:")
            .ok_or(SelectorError::Installed)?;
        let groups: Vec<_> = uuid.split('-').collect();
        let valid = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
            && groups.iter().all(|g| {
                g.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            });
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(SelectorError::Installed)
        }
    }
}

impl fmt::Display for InstalledSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for InstalledSelector {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for InstalledSelector {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(input: &str) -> Reference {
        match parse(input, |a| a == "team").unwrap() {
            Selector::Catalog(reference) => reference,
            Selector::Installed(_) => panic!("catalog reference expected"),
        }
    }

    #[test]
    fn segments_select_alias_or_publisher() {
        let bare = catalog("git");
        assert_eq!((bare.alias, bare.pack.as_str()), (None, "git"));
        let publisher = catalog("acme/git");
        assert_eq!(publisher.alias, None);
        assert_eq!(publisher.pack_id(), Some("acme/git".parse().unwrap()));
        let aliased = catalog("team/git");
        assert_eq!(
            (aliased.alias.as_deref(), aliased.pack.as_str()),
            (Some("team"), "git")
        );
        assert_eq!(aliased.pack_id(), None);
        let full = catalog("other/acme/git@1.0.0-beta.1");
        assert_eq!(full.alias.as_deref(), Some("other"));
        assert_eq!(full.version, Some("1.0.0-beta.1".parse().unwrap()));
        assert!(!catalog("acme/git").is_pinned());
    }

    #[test]
    fn malformed_references_refuse() {
        for (input, error) in [
            ("acme/git@^1", SelectorError::Version),
            ("acme/git@", SelectorError::Version),
            ("a/b/c/d", SelectorError::Reference),
            ("Acme/git", SelectorError::Reference),
            ("acme//git", SelectorError::Reference),
            ("", SelectorError::Reference),
            ("pack:not-a-uuid", SelectorError::Installed),
        ] {
            assert_eq!(parse(input, |_| false), Err(error), "{input}");
        }
        assert_eq!(
            parse(&"a".repeat(4097), |_| false),
            Err(SelectorError::Limit)
        );
    }

    #[test]
    fn installed_references_match_selector_identity_name_and_alias() {
        let ids: Vec<PackId> = ["acme/git", "other/git", "acme/docker"]
            .iter()
            .map(|id| id.parse().unwrap())
            .collect();
        let selectors: Vec<_> = (0..3)
            .map(|i| InstalledSelector::from_seed(&[i]).to_string())
            .collect();
        let aliases = [Some("team"), Some("main"), None];
        let installed = || {
            (0..3).map(|i| Installed {
                selector: &selectors[i],
                alias: aliases[i],
                id: &ids[i],
            })
        };
        for (reference, expected) in [
            (selectors[1].as_str(), Ok(1)),
            ("acme/git", Ok(0)),
            ("acme/git@1.0.0", Ok(0)),
            ("docker", Ok(2)),
            ("team/git", Ok(0)),
            ("team/acme/git", Ok(0)),
            ("main/git", Ok(1)),
            ("main/other/git", Ok(1)),
            ("git", Err(InstalledError::Ambiguous)),
            ("team/docker", Err(InstalledError::Missing)),
            ("missing", Err(InstalledError::Missing)),
            ("/acme/docker", Err(InstalledError::Missing)),
        ] {
            assert_eq!(
                find_installed(installed(), reference),
                expected,
                "{reference}"
            );
        }
    }

    #[test]
    fn installed_selectors_are_stable_uuid_forms() {
        let selector = InstalledSelector::from_seed(b"acme-project:source:acme/git:2026");
        assert_eq!(
            selector,
            InstalledSelector::from_seed(b"acme-project:source:acme/git:2026")
        );
        assert_ne!(selector, InstalledSelector::from_seed(b"other"));
        assert_eq!(
            parse(selector.as_str(), |_| false),
            Ok(Selector::Installed(selector.clone()))
        );
        let json = serde_json::to_string(&selector).unwrap();
        assert_eq!(
            serde_json::from_str::<InstalledSelector>(&json).unwrap(),
            selector
        );
        assert!(
            "pack:ABCDEF12-0000-0000-0000-000000000000"
                .parse::<InstalledSelector>()
                .is_err()
        );
        assert!(
            "pack:00000000-0000-0000-0000-00000000000"
                .parse::<InstalledSelector>()
                .is_err()
        );
    }
}
