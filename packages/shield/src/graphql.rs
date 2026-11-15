//! Request-owned, demand-driven GraphQL evidence. No I/O or permission decisions.
mod carrier;
mod document;
mod model;
mod schema;
#[cfg(test)]
mod tests;

use crate::policy::{Error, rules::patterns::Provider};
use serde::Serialize;
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub(crate) use model::{selectors, selectors_mut, validate, validate_model};
pub(crate) use schema::definitions as schema;
pub(crate) type Shared = Rc<RefCell<Analysis>>;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    stage: &'static str,
    reason: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    path: Vec<String>,
}
impl Failure {
    pub(crate) fn new(stage: &'static str, reason: &'static str) -> Self {
        Self {
            code: "predicate_mismatch",
            stage,
            reason,
            path: vec![],
        }
    }
}

#[derive(Default)]
pub(crate) struct Analysis {
    document: Option<Result<document::Document, Failure>>,
    results: BTreeMap<String, Option<Failure>>,
    visits: usize,
}
impl Analysis {
    pub(crate) fn check(
        &mut self,
        predicate: &Value,
        arguments: &[String],
        environment: Option<&BTreeMap<String, String>>,
        patterns: &dyn Provider,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Option<Failure>, Error> {
        checkpoint()?;
        if self.document.is_none() {
            record("graphql.decode");
            self.document = Some(match carrier::decode(arguments, environment) {
                Ok(carrier) => document::parse(&carrier, &mut self.visits, checkpoint)?,
                Err(failure) => Err(failure),
            });
        }
        let document = match self.document.as_ref().ok_or(Error::Syntax)? {
            Ok(document) => document,
            Err(failure) => return Ok(Some(failure.clone())),
        };
        let selection = predicate.get("selection");
        let loaded = selection
            .and_then(|m| m["prepared_graphql"].as_str())
            .map(|key| patterns.model(key))
            .transpose()?;
        let model = loaded.as_deref().or(selection);
        let mutation = predicate["operation"] == "mutation";
        let root_fields = predicate.get("root_fields").and_then(Value::as_array);
        let model_key = if let Some(key) = selection.and_then(|m| m["prepared_graphql"].as_str()) {
            key.to_owned()
        } else if let Some(model) = model {
            crate::policy::fingerprint(&serde_json::to_vec(model).map_err(|_| Error::Syntax)?)
        } else {
            String::new()
        };
        // Results are reused per operation type, mutation class and model.
        let key = match root_fields {
            Some(fields) if mutation => format!(
                "mutation:{}:{model_key}",
                serde_json::to_string(fields).map_err(|_| Error::Syntax)?
            ),
            _ => model_key,
        };
        if let Some(result) = self.results.get(&key) {
            return Ok(result.clone());
        }
        let expected = document::Expectation {
            kind: if mutation {
                document::Kind::Mutation
            } else {
                document::Kind::Query
            },
            model,
            root_fields: root_fields.map(Vec::as_slice),
        };
        let result = document.check(&expected, patterns, &mut self.visits, checkpoint)?;
        self.results.insert(key, result.clone());
        Ok(result)
    }
}

pub(crate) fn record(name: &'static str) {
    crate::diagnostics::trace::count(name, 1);
    #[cfg(test)]
    COUNTS.with(|counts| {
        let mut values = counts.get();
        if let Some(index) = [
            "graphql.decode",
            "graphql.parse",
            "graphql.selection",
            "graphql.model_loaded",
        ]
        .iter()
        .position(|n| *n == name)
        {
            values[index] += 1;
        }
        counts.set(values);
    });
}
#[cfg(test)]
thread_local! { static COUNTS:std::cell::Cell<[usize;4]>=const { std::cell::Cell::new([0;4]) }; }
#[cfg(test)]
pub(crate) fn take_counts() -> [usize; 4] {
    COUNTS.with(|counts| counts.replace([0; 4]))
}

/// Move admitted immutable models out of eagerly loaded compiled metadata.
pub(crate) fn detach(
    value: &mut Value,
    image: &mut impl FnMut(&Value) -> Result<String, Error>,
) -> Result<(), Error> {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                if key == "graphql" {
                    if let Some(model) = child.get_mut("selection") {
                        let key = image(model)?;
                        *model = serde_json::json!({"prepared_graphql":key});
                    }
                } else if !matches!(key.as_str(), "exact" | "input" | "context") {
                    detach(child, image)?;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                detach(item, image)?;
            }
        }
        _ => {}
    }
    Ok(())
}
/// Validate the small descriptor through ordinary source admission, without
/// hydrating model images during authenticated plan loading.
pub(crate) fn descriptor(value: &mut Value) -> Result<(), Error> {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                if key == "graphql" {
                    if let Some(model) = child.get("selection")
                        && let Some(key) = model["prepared_graphql"].as_str()
                    {
                        if model.as_object().is_none_or(|o| o.len() != 1)
                            || !key.starts_with("graphql:")
                        {
                            return Err(Error::Contract("invalid prepared GraphQL descriptor"));
                        }
                        let mutation = child["operation"] == "mutation";
                        let fields = child.as_object_mut().ok_or(Error::Syntax)?;
                        if mutation {
                            // Admission requires a mutation's model. The prepared
                            // image is validated separately, so this validation-only
                            // copy carries a minimal stand-in rooted at Mutation.
                            fields.insert(
                                "selection".into(),
                                serde_json::json!({"root":"mutation","states":{"mutation":{
                                    "types":["Mutation"],"fields":{"__typename":{"leaf":true}}}}}),
                            );
                        } else {
                            fields.remove("selection");
                        }
                    }
                } else if !matches!(key.as_str(), "exact" | "input" | "context") {
                    descriptor(child)?;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                descriptor(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn visit(visits: &mut usize, checkpoint: &dyn Fn() -> Result<(), Error>) -> Result<(), Error> {
    checkpoint()?;
    *visits += 1;
    if *visits > 32_768 {
        return Err(Error::Limit);
    }
    Ok(())
}
