//! Historical predicates may be carried forward, never authored through an edit.
use crate::{policy, review::Error};
use serde_json::Value;

pub(in crate::review) fn validate(before: &Value, after: &Value) -> Result<(), Error> {
    for field in ["rules", "local_rules"] {
        if before[field] == after[field] {
            continue;
        }
        let Some(source) = after[field].as_str() else {
            continue;
        };
        let next = policy::normalize_retained_custom(source)?;
        let annotated = next["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|rule| {
                rule.get("historical_matcher").is_some()
                    || rule.get("historical_normalized").is_some()
            })
            .collect::<Vec<_>>();
        if annotated.is_empty() {
            continue;
        }
        let base = before[field]
            .as_str()
            .map(policy::normalize_retained_custom)
            .transpose()?;
        for rule in annotated {
            let unchanged = base.as_ref().is_some_and(|base| {
                base["rules"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|old| old["id"] == rule["id"] && old == rule)
            });
            if !unchanged {
                return Err(Error::Invalid(
                    "historical annotations may only preserve an unchanged rule with the same ID in the same layer; rewrite edited rules using current conditions without historical annotations",
                ));
            }
        }
    }
    Ok(())
}
