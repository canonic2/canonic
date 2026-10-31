//! Validated user questions and answers. Workflow authority and storage belong to callers.

mod form;
mod formats;
#[cfg(test)]
mod tests;

use crate::{RpcError, modern, rpc};
pub use form::Form;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Maximum encoded question map.
pub const MAX_REQUEST_BYTES: usize = 65536;
/// Maximum encoded answer map.
pub const MAX_RESPONSE_BYTES: usize = 16384;
/// Maximum simultaneous questions.
pub const MAX_REQUESTS: usize = 8;

pub(crate) fn invalid(message: &str) -> RpcError {
    modern::invalid(&format!("Invalid elicitation: {message}"))
}

pub(crate) fn key(value: &str) -> Result<(), RpcError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(invalid("key"));
    }
    Ok(())
}

/// An immutable form or external interaction, validated before publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    params: Value,
    form: Option<Form>,
    legacy_id: Option<String>,
}

impl Request {
    /// Constructs a form question, reusing its validated schema.
    ///
    /// # Errors
    /// Refuses blank/oversized messages or combined definition overflow.
    pub fn form(message: &str, form: Form) -> Result<Self, RpcError> {
        Self::build(
            json!({"mode":"form","message":message,"requestedSchema":form.as_value()}),
            Some(form),
            None,
        )
    }

    /// Constructs an HTTPS interaction. `legacy_id` is required only for legacy URL issuance.
    /// This performs no network access and grants no destination authority.
    ///
    /// # Errors
    /// Refuses unsafe URLs, invalid IDs, messages or combined definition overflow.
    pub fn url(message: &str, url: &str, legacy_id: Option<&str>) -> Result<Self, RpcError> {
        formats::url(url)?;
        if let Some(id) = legacy_id {
            key(id)?;
        }
        Self::build(
            json!({"mode":"url","message":message,"url":url}),
            None,
            legacy_id.map(str::to_owned),
        )
    }

    fn build(
        params: Value,
        form: Option<Form>,
        legacy_id: Option<String>,
    ) -> Result<Self, RpcError> {
        let message = params["message"]
            .as_str()
            .ok_or_else(|| invalid("message"))?;
        if message.trim().is_empty() || message.len() > 16384 {
            return Err(invalid("message"));
        }
        modern::bounded(&params, MAX_REQUEST_BYTES, 16)?;
        Ok(Self {
            params,
            form,
            legacy_id,
        })
    }

    /// Restores a modern request through the same constructors, without authenticating storage.
    ///
    /// # Errors
    /// Refuses unknown fields, unsupported methods, invalid schemas and bounds.
    pub fn restore(value: &Value) -> Result<Self, RpcError> {
        modern::bounded(value, MAX_REQUEST_BYTES, 32)?;
        if !rpc::only(value, &["method", "params"]) || value["method"] != "elicitation/create" {
            return Err(invalid("request"));
        }
        let params = &value["params"];
        let message = params["message"]
            .as_str()
            .ok_or_else(|| invalid("message"))?;
        let request = match params["mode"].as_str() {
            Some("form") => Self::form(message, Form::new(params["requestedSchema"].clone())?)?,
            Some("url") => Self::url(
                message,
                params["url"].as_str().ok_or_else(|| invalid("URL"))?,
                None,
            )?,
            _ => return Err(invalid("mode")),
        };
        if request.as_value() != *value {
            return Err(invalid("stored request fields"));
        }
        Ok(request)
    }

    /// The modern wire request, without JSON-RPC ID or envelope.
    #[must_use]
    pub fn as_value(&self) -> Value {
        json!({"method":"elicitation/create","params":self.params})
    }

    /// The required client mode.
    #[must_use]
    pub fn mode(&self) -> &'static str {
        if self.form.is_some() { "form" } else { "url" }
    }

    /// Checks the current request/connection's declared mode.
    ///
    /// # Errors
    /// Returns a missing-capability refusal; no earlier declaration is consulted.
    pub fn require_capability(&self, capabilities: &Value) -> Result<(), RpcError> {
        let mode = self.mode();
        let supported = capabilities["elicitation"].as_object().is_some_and(|map| {
            (mode == "form" && map.is_empty()) || map.get(mode).is_some_and(Value::is_object)
        });
        if supported {
            return Ok(());
        }
        Err(
            RpcError::new(-32021, "Client lacks required elicitation mode")
                .with_data(json!({"requiredCapabilities":{"elicitation":{mode:{}}}})),
        )
    }

    pub(crate) fn legacy_params(
        &self,
        version: &str,
        capabilities: &Value,
    ) -> Result<Value, RpcError> {
        self.require_capability(capabilities)?;
        let mut params = self.params.clone();
        if version == "2025-06-18" {
            let form = self
                .form
                .as_ref()
                .ok_or_else(|| invalid("URL mode unavailable in this revision"))?;
            params["requestedSchema"] = form.legacy()?;
            params
                .as_object_mut()
                .ok_or_else(|| invalid("request"))?
                .remove("mode");
        } else {
            if version != "2025-11-25" {
                return Err(invalid("legacy revision"));
            }
            if self.form.is_none() {
                params["elicitationId"] = json!(
                    self.legacy_id
                        .as_ref()
                        .ok_or_else(|| invalid("legacy URL ID required"))?
                );
            }
        }
        Ok(params)
    }

    /// Validates an answer against this exact question. URL consent is not completion.
    ///
    /// # Errors
    /// Refuses invalid actions, metadata, content and schema mismatches without accepted values.
    pub fn answer(&self, value: &Value) -> Result<Response, RpcError> {
        modern::bounded(value, MAX_RESPONSE_BYTES, 32)?;
        if !rpc::only(value, &["action", "content", "_meta"])
            || value.get("_meta").is_some_and(|meta| !meta.is_object())
        {
            return Err(invalid("answer envelope"));
        }
        let action = match value["action"].as_str() {
            Some("accept") => Action::Accept,
            Some("decline") => Action::Decline,
            Some("cancel") => Action::Cancel,
            _ => return Err(invalid("action")),
        };
        let content = value.get("content");
        if content.is_some_and(|content| !content.is_object()) {
            return Err(invalid("content object"));
        }
        let accepted = if action == Action::Accept {
            match &self.form {
                Some(form) => Some(form.validate(content)?),
                None if content.is_none() => None,
                None => return Err(invalid("URL answer content")),
            }
        } else {
            None
        };
        Ok(Response {
            action,
            content: accepted,
            meta: value.get("_meta").cloned(),
        })
    }
}

/// The host user's reported action, distinct from transport cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Submitted values, or consent to an external interaction.
    Accept,
    /// Explicit refusal.
    Decline,
    /// Dismissal.
    Cancel,
}

/// An answer whose content was checked against its issued definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Reported action; it does not establish operation authority.
    pub action: Action,
    /// Validated accepted form data; absent on refusal, dismissal or URL consent.
    pub content: Option<Value>,
    /// Bounded answer metadata, separate from form fields.
    pub meta: Option<Value>,
}

/// Result of a connection-bound rich wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A validated user response.
    Response(Response),
    /// The host could not be asked, or returned an RPC error.
    Unavailable,
    /// The host sent a malformed or schema-invalid response.
    Invalid,
}

/// Validated questions for one ordinary round or retained task snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requests(BTreeMap<String, Request>);

impl Requests {
    /// Builds a bounded map; duplicate keys are refused before insertion.
    ///
    /// # Errors
    /// Refuses invalid/duplicate keys, empty maps and aggregate count/size overflow.
    pub fn new(entries: impl IntoIterator<Item = (String, Request)>) -> Result<Self, RpcError> {
        let mut requests = BTreeMap::new();
        for (name, request) in entries {
            key(&name)?;
            if requests.len() >= MAX_REQUESTS || requests.insert(name, request).is_some() {
                return Err(invalid("question count or duplicate key"));
            }
        }
        if requests.is_empty() {
            return Err(invalid("empty question map"));
        }
        let requests = Self(requests);
        modern::bounded(&requests.as_value(), MAX_REQUEST_BYTES, 32)?;
        Ok(requests)
    }

    /// Restores modern questions, refusing any data that would be lost.
    ///
    /// # Errors
    /// Refuses malformed or excessive maps and invalid definitions.
    pub fn restore(value: &Value) -> Result<Self, RpcError> {
        modern::bounded(value, MAX_REQUEST_BYTES, 32)?;
        let map = value.as_object().ok_or_else(|| invalid("question map"))?;
        Self::new(
            map.iter()
                .map(|(key, value)| Ok((key.clone(), Request::restore(value)?)))
                .collect::<Result<Vec<_>, RpcError>>()?,
        )
    }

    /// The modern wire map, preserving stable keys and values.
    #[must_use]
    pub fn as_value(&self) -> Value {
        Value::Object(
            self.0
                .iter()
                .map(|(key, request)| (key.clone(), request.as_value()))
                .collect(),
        )
    }

    /// Validates every required mode using only the supplied capabilities.
    ///
    /// # Errors
    /// Refuses the first unsupported question mode.
    pub fn require_capabilities(&self, capabilities: &Value) -> Result<(), RpcError> {
        for request in self.0.values() {
            request.require_capability(capabilities)?;
        }
        Ok(())
    }

    /// Checks all known answers before returning any; missing answers remain absent.
    /// Unknown keys are ignored, including their opaque payloads.
    ///
    /// # Errors
    /// Refuses structural/byte bounds or any malformed known answer; performs no consumption.
    pub fn answers(&self, responses: &Value) -> Result<BTreeMap<String, Response>, RpcError> {
        validate_responses(responses)?;
        self.0
            .iter()
            .filter_map(|(key, request)| {
                responses
                    .get(key)
                    .map(|value| request.answer(value).map(|answer| (key.clone(), answer)))
            })
            .collect()
    }
}

pub(crate) fn validate_responses(value: &Value) -> Result<(), RpcError> {
    modern::bounded(value, MAX_RESPONSE_BYTES, 32)?;
    for name in value
        .as_object()
        .ok_or_else(|| invalid("response map"))?
        .keys()
    {
        key(name)?;
    }
    Ok(())
}
