//! Owns pack `dependencies` declarations and their conversion to registry resolution.
//!
//! Resolution itself belongs to `canonic_registry::resolution`; profiles are its
//! ordered level, so dependencies inherit or map a parent's profile.

use super::{Error, Pack, Profile};
use canonic_registry::resolution::{self, Dependency, Instance, Level, Release, Root};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

/// Artifact dependency metadata, bound by the artifact's exact digest.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// Fully qualified publisher/name.
    pub pack: String,
    /// Semantic version requirement.
    pub requirement: String,
    /// Optional enrolled source identity; omission inherits the parent source.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub registry: Option<String>,
    /// Optional complete monotonic parent-profile mapping.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub profiles: Option<Mapping>,
}

/// Required dependency profile for each of the three parent profiles.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    /// Requirement from a loose parent.
    pub loose: Profile,
    /// Requirement from a recommended parent.
    pub recommended: Profile,
    /// Requirement from a strict parent.
    pub strict: Profile,
}

impl Level for Profile {
    fn all() -> &'static [Self] {
        &[Self::Loose, Self::Recommended, Self::Strict]
    }
}

impl Declaration {
    /// The resolver edge this declaration describes.
    pub(crate) fn parsed(&self) -> Result<Dependency<Profile>, resolution::Error> {
        Dependency::parse(
            &self.pack,
            &self.requirement,
            self.registry.as_deref(),
            self.profiles
                .as_ref()
                .map(|p| vec![p.loose, p.recommended, p.strict]),
        )
    }
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

pub(super) fn declarations<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<Vec<Declaration>>, D::Error> {
    struct Sequence;
    impl<'de> serde::de::Visitor<'de> for Sequence {
        type Value = Option<Vec<Declaration>>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a dependency sequence")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut input: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = input.next_element()? {
                values.push(value);
            }
            Ok(Some(values))
        }
    }
    input.deserialize_any(Sequence)
}

/// Requires a manual candidate's packs to form their own complete closure.
pub(crate) fn validate_packs(packs: &[(&Pack, Profile)]) -> Result<(), Error> {
    if !packs.iter().any(|(p, _)| !p.dependencies().is_empty()) {
        return Ok(());
    }
    let mut releases = Vec::new();
    let mut roots = Vec::new();
    let mut enrolled = BTreeSet::new();
    for (pack, profile) in packs {
        let parts: Vec<_> = pack.id().split('/').collect();
        let (registry, name) = match parts.as_slice() {
            [publisher, name] => ("candidate".to_owned(), format!("{publisher}/{name}")),
            [registry, publisher, name] => ((*registry).to_owned(), format!("{publisher}/{name}")),
            _ => return Err(Error::Contract("invalid dependency pack identity")),
        };
        enrolled.insert(registry.clone());
        let instance = Instance {
            registry,
            pack: name
                .parse()
                .map_err(|_| Error::Contract("incomplete or conflicting dependency closure"))?,
        };
        let version: semver::Version = pack
            .version()
            .parse()
            .map_err(|_| Error::Contract("invalid dependency version"))?;
        roots.push(Root {
            instance: instance.clone(),
            requirement: format!("={version}")
                .parse()
                .map_err(|_| Error::Contract("invalid dependency version"))?,
            pin: Some(version.clone()),
            level: *profile,
        });
        releases.push(Release {
            instance,
            version,
            dependencies: pack
                .dependencies()
                .iter()
                .map(Declaration::parsed)
                .collect::<Result<_, _>>()
                .map_err(|_| Error::Contract("invalid dependencies"))?,
        });
    }
    resolution::resolve(&roots, &releases, &enrolled)
        .map_err(|_| Error::Contract("incomplete or conflicting dependency closure"))?;
    Ok(())
}
