//! Prepared legacy visibility and stateless catalog/revision-bound tool cursors.
use super::{Catalog, ResultSchema, Tool};
use crate::rpc::{self, RpcError};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Tools per wire page; consumers must treat page size as server-selected.
pub const PAGE_SIZE: usize = 64;
/// Maximum accepted opaque tool cursor bytes.
pub const MAX_CURSOR_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pages {
    legacy: Vec<usize>,
    digest: String,
}

impl Pages {
    pub(super) fn new(tools: &[Tool], outputs: &[ResultSchema]) -> Self {
        let mut digest = Sha256::new();
        // The digest writer streams metadata directly; no second catalog tree or buffer.
        serde_json::to_writer(&mut digest, tools)
            .expect("Tool fields are JSON-safe and the SHA-256 writer is infallible");
        Self {
            legacy: outputs
                .iter()
                .enumerate()
                .filter_map(|(i, output)| output.legacy.then_some(i))
                .collect(),
            digest: format!("{:x}", digest.finalize()),
        }
    }

    pub(super) fn page(
        &self,
        catalog: &Catalog,
        params: &Value,
        version: &str,
    ) -> Result<Value, RpcError> {
        if !rpc::only(params, &["cursor", "_meta"]) {
            return Err(invalid());
        }
        let legacy = if crate::SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
            true
        } else if version == crate::modern::PROTOCOL_VERSION {
            false
        } else {
            return Err(invalid());
        };
        let count = if legacy {
            self.legacy.len()
        } else {
            catalog.tools.len()
        };
        let prefix = format!("tools:{}:{version}:", self.digest);
        let start = match params.get("cursor") {
            None => 0,
            Some(Value::String(cursor)) if cursor.len() <= MAX_CURSOR_BYTES => {
                let offset = cursor.strip_prefix(&prefix).ok_or_else(invalid)?;
                let index = offset.parse::<usize>().map_err(|_| invalid())?;
                if index == 0
                    || index >= count
                    || !index.is_multiple_of(PAGE_SIZE)
                    || offset != index.to_string()
                {
                    return Err(invalid());
                }
                index
            }
            _ => return Err(invalid()),
        };
        let end = start.saturating_add(PAGE_SIZE).min(count);
        let tools: Vec<&Tool> = (start..end)
            .map(|index| &catalog.tools[if legacy { self.legacy[index] } else { index }])
            .collect();
        let mut result = json!({"tools":tools});
        if end < count {
            result["nextCursor"] = json!(format!("{prefix}{end}"));
        }
        Ok(result)
    }
}

fn invalid() -> RpcError {
    RpcError::new(
        rpc::INVALID_PARAMS,
        "Invalid tool listing parameters or cursor",
    )
}

#[cfg(test)]
mod tests;
