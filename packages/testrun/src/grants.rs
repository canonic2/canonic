//! Caller-owned, expiring single-use capabilities; registries never share authority.
#[cfg(any(unix, windows, test))]
use crate::error::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
pub(crate) type Grants = Arc<Mutex<HashMap<String, Instant>>>;
#[cfg(any(unix, windows, test))]
pub(crate) fn issue_grant(grants: &Grants) -> Result<String> {
    use std::time::Duration;
    let mut grants = grants
        .lock()
        .map_err(|_| Error::new("INTERNAL", "Grant registry unavailable"))?;
    grants.retain(|_, expires| *expires > Instant::now());
    if grants.len() >= 32 {
        return Err(Error::new("RESOURCE_LIMIT", "Too many pending host grants"));
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    grants.insert(token.clone(), Instant::now() + Duration::from_secs(30));
    Ok(token)
}
pub(crate) fn consume_grant(grants: &Grants, token: &str) -> bool {
    grants
        .lock()
        .ok()
        .and_then(|mut grants| grants.remove(token))
        .is_some_and(|expires| expires > Instant::now())
}
