//! Host-settings decode boundary inside the existing private settings transaction.

use super::{Result, Value, invalid};

/// The shipped settings shape is an unversioned JSON object. Keep all fields,
/// including unrelated settings, rather than inventing a marker or importing
/// old signing files. Future versioned readers/converters enter here; Store
/// owns validation, settings.lock and atomic publication.
pub(super) fn settings(bytes: &[u8]) -> Result<Value> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| invalid("Signing settings contain invalid JSON"))?;
    if !value.is_object() {
        return Err(invalid("Signing settings must be a JSON object"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use crate::{artifacts, host_setup::storage::Store};
    use serde_json::json;
    use std::fs;

    #[test]
    fn upgrade_dispatch_preserves_unversioned_preferences_and_unrelated_fields() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("Testrun");
        artifacts::directory(&directory).unwrap();
        let path = directory.join("settings.json");
        let store = Store::at(path.clone());
        let bytes = br#"{ "ios": {"runnerTeamId":"ACME000001"}, "other": {"keep":true} }"#;
        artifacts::write(&path, bytes).unwrap();
        assert_eq!(store.saved().unwrap().as_deref(), Some("ACME000001"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let teams = [("ACME000002".into(), "Acme".into())].into();
        store.resolve(&teams, Some("ACME000002")).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["other"], json!({"keep":true}));
        assert_eq!(saved["ios"]["runnerTeamId"], "ACME000002");
        fs::write(&path, b"[]").unwrap();
        assert!(store.saved().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"[]");
    }
}
