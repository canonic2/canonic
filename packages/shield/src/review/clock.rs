//! Epoch clock sampling for review authority and bounded retention views.
use super::Error;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn now_ms() -> Result<u64, Error> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("clock before epoch"))?
            .as_millis(),
    )
    .map_err(|_| Error::Invalid("clock out of range"))
}
// Retention and inspection preserve their conservative saturation behavior.
pub(super) fn best_effort_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}
