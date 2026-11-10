//! Selected raw bytes are bound to the canonical lock, never inferred from a
//! manifest's copies of historical whole-bundle digests.
use super::{Context, Error, RuleRef};
use crate::review::storage::{
    Store,
    objects::Pins,
    sources::{self, Namespace},
};
use serde_json::{Value, json};
use std::{borrow::Cow, path::Path};
#[cfg(test)]
mod tests;

struct Source<'a> {
    reference: &'a Value,
    inline: Option<Cow<'a, Value>>,
    namespace: Namespace,
    _pin: Pins,
}
impl<'a> Source<'a> {
    fn open(context: &Context, reference: &'a Value) -> Result<Self, Error> {
        let store = Store::inspect(Path::new(&context.project_root))?;
        let namespace = Namespace::project(&store);
        let pin = sources::pin_inspection(&store, reference)?;
        let inline = if reference["kind"] == "retained-source-v2" {
            None
        } else if reference["kind"] == "retained-source-v1" {
            // V1 has one indivisible object. Read it once for this view.
            Some(Cow::Owned(namespace.captured(reference)?))
        } else {
            Some(Cow::Borrowed(reference))
        };
        Ok(Self {
            reference,
            inline,
            namespace,
            _pin: pin,
        })
    }
    fn field(&self, field: &str, index: Option<usize>) -> Result<Value, Error> {
        match &self.inline {
            Some(value) => Ok(match index {
                Some(index) => value[field].get(index).ok_or(Error::Missing)?.clone(),
                None => value[field].clone(),
            }),
            None => self.namespace.field(self.reference, field, index),
        }
    }
    fn header(&self) -> Result<Value, Error> {
        let mut value = json!({});
        for field in ["schema_version", "kind", "trust", "lock"] {
            value[field] = self.field(field, None)?;
        }
        Ok(value)
    }
}

pub(in crate::review) fn policy(
    context: &Context,
    reference: &Value,
    revision: &str,
) -> Result<Value, Error> {
    let source = Source::open(context, reference)?;
    let mut candidate = source.header()?;
    crate::configuration::recorded_effective(&candidate, revision)?;
    candidate["rules"] = source.field("rules", None)?;
    crate::configuration::verify_recorded_fields(&candidate, revision)?;
    Ok(candidate)
}

/// Canonical lock identity only. No body, matcher or whole-bundle claim.
pub(in crate::review) fn revision(
    context: &Context,
    reference: &Value,
    expected: Option<&str>,
) -> Result<String, Error> {
    let candidate = Source::open(context, reference)?.header()?;
    match expected {
        Some(expected) => {
            crate::configuration::recorded_effective(&candidate, expected)?;
            Ok(expected.into())
        }
        None => crate::configuration::recorded_revision(&candidate).map_err(Into::into),
    }
}

pub(in crate::review) fn rules(
    context: &Context,
    reference: &Value,
    metadata: &Value,
    revision: &str,
    selector: Option<&Value>,
) -> Result<Value, Error> {
    let selected: Option<RuleRef> = selector
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| Error::Invalid("invalid rule reference"))
        })
        .transpose()?;
    let source = Source::open(context, reference)?;
    let mut candidate = source.header()?;
    // Validate the canonical lock/revision before using any selected body.
    crate::configuration::recorded_effective(&candidate, revision)?;
    if !matches!(selected, Some(RuleRef::Pack { .. })) {
        candidate["rules"] = source.field("rules", None)?;
        candidate["local_rules"] = source.field("local_rules", None)?;
    }
    if !matches!(selected, Some(RuleRef::Custom { .. })) {
        candidate["config"] = source.field("config", None)?;
        candidate["local_config"] = source.field("local_config", None)?;
        let count = metadata["installed"].as_array().map_or(0, Vec::len);
        if count > 64 {
            return Err(Error::Invalid("pack selection limit"));
        }
        let mut packs = vec![Value::Null; count];
        for (index, pack) in metadata["installed"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if selected.as_ref().is_some_and(|selected| !matches!(selected, RuleRef::Pack {pack_selector,..} if pack_selector == &pack["selector"])) { continue; }
            packs[index] = source.field("packs", Some(index))?;
        }
        candidate["packs"] = json!(packs);
    }
    crate::configuration::verify_recorded_fields(&candidate, revision)?;
    if candidate.get("packs").is_some() {
        let config = crate::configuration::layers::merged(&candidate)?;
        for (index, source) in candidate["packs"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(source) = source.as_str() else {
                continue;
            };
            let selected = &config["packs"][index];
            let row = &metadata["installed"][index];
            if row["id"] != selected["id"]
                || row["version"] != selected["version"]
                || row["profile"].as_str()
                    != Some(selected["profile"].as_str().unwrap_or("recommended"))
                || row["digest"] != crate::policy::fingerprint(source.as_bytes())
            {
                return Err(crate::policy::Error::ArtifactMismatch.into());
            }
        }
    }
    Ok(candidate)
}
