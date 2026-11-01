//! Prepared, offline JSON Schema 2020-12 validation with explicit resource budgets.

mod bounds;
mod diagnostic;
mod plan;
#[cfg(test)]
mod tests;

use crate::control::Interrupted;
use serde_json::Value;
use std::sync::Arc;

/// Supported default and explicit schema dialect.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
/// Maximum JSON/schema/reference traversal depth.
pub const MAX_DEPTH: usize = 64;
/// Maximum encoded bytes in one schema document.
pub const MAX_SCHEMA_BYTES: usize = 256 * 1024;
/// Maximum raw JSON nodes in a schema document.
pub const MAX_SCHEMA_NODES: usize = 32768;
/// Derived node ceiling; the encoded byte bound is at least as restrictive.
pub const MAX_VALUE_NODES: usize = MAX_VALUE_BYTES;
/// Maximum encoded bytes in a value being validated.
pub const MAX_VALUE_BYTES: usize = 4 * 1024 * 1024;
/// Conservative instance-aware validation work ceiling.
pub const MAX_WORK: usize = 8 * 1024 * 1024;

/// Classification of a validation refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentErrorKind {
    /// The instance does not satisfy the schema.
    Mismatch,
    /// Size, depth or conservative work bounds were exceeded.
    Limit,
    /// The caller's cancellation/deadline checkpoint failed.
    Interrupted,
    /// An ad-hoc schema could not be prepared.
    InvalidSchema,
}

/// A value that cannot be accepted under its schema and validation budget.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ArgumentError {
    /// Where the mismatch is, such as `arguments.point.x`.
    pub path: String,
    /// A sentence naming the path and required recovery.
    pub message: String,
    /// Whether this is a mismatch, budget refusal, interruption or invalid schema.
    pub kind: ArgumentErrorKind,
}

impl ArgumentError {
    pub(crate) fn new(path: &str, message: String) -> Self {
        Self {
            path: path.into(),
            message,
            kind: ArgumentErrorKind::Mismatch,
        }
    }

    pub(crate) fn refusal(path: &str, kind: ArgumentErrorKind) -> Self {
        let reason = match kind {
            ArgumentErrorKind::Limit => "exceeds the schema validation budget",
            ArgumentErrorKind::Interrupted => "validation was interrupted",
            ArgumentErrorKind::InvalidSchema => "has an invalid or unsupported schema",
            ArgumentErrorKind::Mismatch => "does not match its schema",
        };
        Self {
            path: path.into(),
            message: format!("{path} {reason}"),
            kind,
        }
    }
}

/// A schema that cannot be prepared under the supported dialect and limits.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("schema at {pointer} {problem}")]
pub struct SchemaError {
    /// JSON pointer of the offending schema.
    pub pointer: String,
    /// The invalid declaration, unresolved reference or resource limit.
    pub problem: String,
}

impl SchemaError {
    fn new(problem: impl Into<String>) -> Self {
        Self {
            pointer: "/".into(),
            problem: problem.into(),
        }
    }
}

/// Immutable validator prepared once; clones share the compiled representation.
#[derive(Debug, Clone)]
pub struct PreparedSchema(Arc<Prepared>);

#[derive(Debug)]
struct Prepared {
    source: Value,
    validator: jsonschema::Validator,
    plan: plan::Plan,
}

impl PartialEq for PreparedSchema {
    fn eq(&self, other: &Self) -> bool {
        self.0.source == other.0.source
    }
}

impl PreparedSchema {
    /// Validates the dialect and schema, resolves bundled references and prepares
    /// assertions once. No network or filesystem retrieval is available.
    ///
    /// # Errors
    /// Refuses malformed schemas, unsupported dialects, unavailable references,
    /// non-descending cycles, excessive reference expansion and size/depth limits.
    pub fn new(schema: &Value) -> Result<Self, SchemaError> {
        let (registry, plan) = bounds::prepare(schema)?;
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .with_base_uri(bounds::BASE)
            .with_registry(&registry)
            .offline()
            .without_content_encoding_support("base64")
            .without_content_media_type_support("application/json")
            .with_pattern_options(
                jsonschema::PatternOptions::regex()
                    .size_limit(32 * 1024)
                    .dfa_size_limit(32 * 1024),
            )
            .should_validate_formats(true)
            .should_ignore_unknown_formats(true)
            // Preserve the established UUID spelling check.
            .with_format("uuid", diagnostic::is_uuid)
            .build(schema)
            .map_err(|error| SchemaError {
                pointer: error.instance_path().to_string(),
                problem: error.to_string(),
            })?;
        Ok(Self(Arc::new(Prepared {
            source: schema.clone(),
            validator,
            plan,
        })))
    }

    /// Validates a value, naming paths from `arguments`.
    ///
    /// # Errors
    /// Returns a mismatch or a deterministic resource-budget refusal.
    pub fn validate(&self, value: &Value) -> Result<(), ArgumentError> {
        self.validate_at(value, "arguments")
    }

    /// Validates a value, naming paths from `path`.
    ///
    /// # Errors
    /// Returns a mismatch or a deterministic resource-budget refusal.
    pub fn validate_at(&self, value: &Value, path: &str) -> Result<(), ArgumentError> {
        self.validate_checked(value, path, &|| Ok(()))
    }

    /// Checks the caller's cancellation/deadline before, during preflight and after
    /// bounded synchronous validation. No background thread or retry is started.
    ///
    /// # Errors
    /// Returns a mismatch, budget refusal or interruption without accepting a partial evaluation.
    pub fn validate_checked(
        &self,
        value: &Value,
        path: &str,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<(), ArgumentError> {
        let interrupted = || ArgumentError::refusal(path, ArgumentErrorKind::Interrupted);
        check().map_err(|_| interrupted())?;
        bounds::value(value, check).map_err(|kind| ArgumentError::refusal(path, kind))?;
        self.0
            .plan
            .check(value, check)
            .map_err(|kind| ArgumentError::refusal(path, kind))?;
        let result = self
            .0
            .validator
            .validate(value)
            .map_err(|error| diagnostic::argument(&self.0.source, value, &error, path));
        check().map_err(|_| interrupted())?;
        result
    }
}

/// Checks and validates an ad-hoc schema. Catalogs use [`PreparedSchema`] to avoid
/// repeating preparation on each request.
///
/// # Errors
/// Returns a mismatch, budget refusal or invalid-schema error.
pub fn validate(schema: &Value, value: &Value) -> Result<(), ArgumentError> {
    validate_at(schema, value, "arguments")
}

/// Checks and validates an ad-hoc schema with a caller-selected diagnostic root.
///
/// # Errors
/// Returns a mismatch, budget refusal or invalid-schema error.
pub fn validate_at(schema: &Value, value: &Value, path: &str) -> Result<(), ArgumentError> {
    PreparedSchema::new(schema)
        .map_err(|_| ArgumentError::refusal(path, ArgumentErrorKind::InvalidSchema))?
        .validate_at(value, path)
}

/// Checks whether a schema can be prepared under this package's dialect and budgets.
///
/// # Errors
/// Returns the schema declaration, reference or resource-limit error.
pub fn check(schema: &Value) -> Result<(), SchemaError> {
    PreparedSchema::new(schema).map(|_| ())
}
