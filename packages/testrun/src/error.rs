//! Stable boundary errors. Platform diagnostics stay out of agent-facing results.
use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize, Clone)]
#[error("{code}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            details: None,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_ARGUMENT", message)
    }
    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Identify a refused mobile action without changing its cause or error code.
    pub(crate) fn at_action(mut self, index: usize, kind: Option<&str>) -> Self {
        self.message = format!(
            "actions[{index}] ({}) refused: {}",
            kind.unwrap_or("unknown"),
            self.message
        );
        let mut details = self.details.take().unwrap_or_else(|| serde_json::json!({}));
        if !details.is_object() {
            details = serde_json::json!({"cause": details});
        }
        details["actionIndex"] = serde_json::json!(index);
        details["actionType"] = serde_json::json!(kind);
        self.details = Some(details);
        self
    }
}

pub type Result<T> = std::result::Result<T, Error>;
