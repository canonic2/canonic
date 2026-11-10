//! Read-only admission of the original developer-private inline receipts.
use super::{Error, RetainedConsent, directory, key, metadata, private_files};
use serde_json::Value;

pub(super) fn retained(candidate: &Value) -> Result<Option<RetainedConsent>, Error> {
    let path = directory()?.join(format!("{}.json", key(candidate)?));
    let receipt = match private_files::read(&path, super::super::LIMIT) {
        Ok(receipt) => receipt,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if receipt["schema_version"] != 1 || receipt["candidate"] != *candidate {
        return Ok(None);
    }
    let metadata = metadata(&receipt["metadata"])?;
    let checkpoints = receipt
        .get("registry_checkpoints")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|_| Error::Invalid("private legacy consent checkpoints invalid"))?
        .unwrap_or_default();
    Ok(Some(RetainedConsent {
        candidate: candidate.clone(),
        metadata,
        checkpoints,
    }))
}
