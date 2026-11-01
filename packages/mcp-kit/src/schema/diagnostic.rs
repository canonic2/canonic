//! Stable path-oriented diagnostics over prepared validator errors.

use super::ArgumentError;
use jsonschema::{
    ValidationError,
    error::{TypeKind, ValidationErrorKind},
};
use serde_json::Value;

pub(super) fn argument(
    schema: &Value,
    instance: &Value,
    error: &ValidationError<'_>,
    root: &str,
) -> ArgumentError {
    let (mut path, cursor) = location(instance, &error.instance_path().to_string(), root);
    let mismatch = || ArgumentError::new(&path, format!("{path} does not match its schema"));
    let message = match error.kind() {
        ValidationErrorKind::Required { property } => format!(
            "{path}.{} is required",
            property.as_str().unwrap_or("field")
        ),
        ValidationErrorKind::AdditionalProperties { unexpected } => {
            if let Some(field) = unexpected.first() {
                path.push('.');
                path.push_str(field);
            }
            format!("{path} is not a supported field")
        }
        ValidationErrorKind::Type { kind } => match kind {
            TypeKind::Single(kind) => format!("{path} must be {kind}"),
            TypeKind::Multiple(kinds) => format!(
                "{path} must be {}",
                kinds
                    .iter()
                    .map(|kind| kind.to_string())
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        },
        ValidationErrorKind::Format { format } => format!("{path} must match format {format}"),
        ValidationErrorKind::Enum { options } => format!("{path} must match one of {options}"),
        ValidationErrorKind::Constant { expected_value } => {
            format!("{path} must match one of {expected_value}")
        }
        ValidationErrorKind::MinItems { limit } => format!(
            "{path} must contain at least {limit} items; received {}",
            cursor.as_array().map_or(0, Vec::len)
        ),
        ValidationErrorKind::MaxItems { limit } => format!(
            "{path} must contain at most {limit} items; received {}",
            cursor.as_array().map_or(0, Vec::len)
        ),
        ValidationErrorKind::MinProperties { limit } => {
            format!("{path} must contain at least {limit} fields")
        }
        ValidationErrorKind::MaxProperties { limit } => {
            format!("{path} must contain at most {limit} fields")
        }
        ValidationErrorKind::Minimum { .. } | ValidationErrorKind::Maximum { .. } => {
            let parent = error
                .schema_path()
                .to_string()
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_owned())
                .unwrap_or_default();
            let schema = schema.pointer(&parent).unwrap_or(schema);
            if let (Some(min), Some(max)) = (schema.get("minimum"), schema.get("maximum")) {
                format!("{path} must be between {min} and {max}; received {cursor}")
            } else {
                return mismatch();
            }
        }
        ValidationErrorKind::AnyOf { context } | ValidationErrorKind::OneOfNotValid { context } => {
            if let Some(error) = union_argument(schema, instance, context, root) {
                return error;
            }
            return mismatch();
        }
        _ => return mismatch(),
    };
    ArgumentError::new(&path, message)
}

fn union_argument(
    schema: &Value,
    instance: &Value,
    context: &[Vec<ValidationError<'_>>],
    root: &str,
) -> Option<ArgumentError> {
    let depth = |error: &ArgumentError| {
        error
            .path
            .bytes()
            .filter(|byte| matches!(byte, b'.' | b'['))
            .count()
    };
    let failures: Vec<_> = context
        .iter()
        .flatten()
        .map(|error| {
            (
                argument(schema, instance, error, root),
                matches!(
                    error.kind(),
                    ValidationErrorKind::AdditionalProperties { .. }
                ),
            )
        })
        .collect();
    let deepest = failures.iter().map(|(error, _)| depth(error)).max()?;
    let mut candidates: Vec<_> = failures
        .iter()
        .filter(|(error, _)| depth(error) == deepest)
        .collect();
    // A different object variant can forbid the same field that the intended
    // variant constrains. Preserve its actionable constraint rather than the
    // alternative's unknown-field complaint at that exact path.
    candidates.retain(|(error, extra)| {
        !extra
            || !failures
                .iter()
                .any(|(other, extra)| !extra && other.path == error.path && depth(other) == deepest)
    });
    let first = &candidates.first()?.0;
    candidates
        .iter()
        .all(|(error, _)| error == first)
        .then(|| first.clone())
}

fn location<'a>(instance: &'a Value, pointer: &str, root: &str) -> (String, &'a Value) {
    let mut path = root.to_owned();
    let mut cursor = instance;
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        if cursor.is_array() {
            path.push('[');
            path.push_str(&segment);
            path.push(']');
            cursor = segment
                .parse::<usize>()
                .ok()
                .and_then(|index| cursor.get(index))
                .unwrap_or(&Value::Null);
        } else {
            path.push('.');
            path.push_str(&segment);
            cursor = cursor.get(&segment).unwrap_or(&Value::Null);
        }
    }
    (path, cursor)
}

pub(super) fn is_uuid(text: &str) -> bool {
    text.split('-').zip([8, 4, 4, 4, 12]).all(|(group, size)| {
        group.len() == size && group.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) && text.len() == 36
}
