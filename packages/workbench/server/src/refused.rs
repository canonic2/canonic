//! An error that knows the HTTP status it deserves.
//!
//! Services return it inside `anyhow::Error`; the routes answer with its
//! status and message, and with their own fallback status for other errors.

use std::fmt;

/// A request refused with a status and a message for the person asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refused {
    pub(crate) status: u16,
    pub(crate) message: String,
}

impl Refused {
    pub(crate) fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

/// The status an error deserves: its own when refused, else `fallback`.
pub(crate) fn status_of(error: &anyhow::Error, fallback: u16) -> u16 {
    error
        .downcast_ref::<Refused>()
        .map_or(fallback, |refused| refused.status)
}

/// A refused request as an error, for `?` and `Err(...)`.
pub(crate) fn refused(status: u16, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Refused::new(status, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_keeps_its_status_and_others_take_the_fallback() {
        assert_eq!(status_of(&refused(404, "not here"), 500), 404);
        assert_eq!(status_of(&anyhow::anyhow!("broken"), 500), 500);
        assert_eq!(refused(403, "no").to_string(), "no");
    }
}
