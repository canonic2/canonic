//! Exact schema-3 dependency declarations from the historical resolver.
//! Keeping this wire shape separate prevents future source fields becoming old
//! policy merely because the current dependency type learns to deserialize them.
use crate::policy::{Mapping, Profile};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Declaration {
    pub(super) pack: String,
    requirement: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) registry: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    profiles: Option<Profiles>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profiles {
    loose: Profile,
    recommended: Profile,
    strict: Profile,
}

impl Declaration {
    pub(super) fn current(&self) -> crate::policy::Declaration {
        crate::policy::Declaration {
            pack: self.pack.clone(),
            requirement: self.requirement.clone(),
            registry: self.registry.clone(),
            profiles: self.profiles.as_ref().map(|p| Mapping {
                loose: p.loose,
                recommended: p.recommended,
                strict: p.strict,
            }),
        }
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
    Vec::deserialize(input).map(Some)
}
