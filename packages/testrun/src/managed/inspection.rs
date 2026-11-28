//! Inspection and media requests use the connected device actor's resources.
use super::Caller;
use crate::error::Result;
use serde_json::{Value, json};
use std::path::Path;

impl Caller {
    pub(super) async fn inspect(&self, directory: &Path, name: &str, args: Value) -> Result<Value> {
        let connection = self.selected(directory)?;
        Self::send(
            &connection,
            json!({"kind":"inspection","tool":name,"arguments":args}),
        )
        .await
    }
}
