//! Bind only selected certificate files; application objects remain opaque.
use super::{HostFile, bind_file_path};
use crate::error::Result;
use serde_json::Value;

pub(super) fn bind(
    options: Option<&mut Value>,
    base: &mut Option<std::path::PathBuf>,
    cwd: &mut impl FnMut() -> std::io::Result<std::path::PathBuf>,
) -> Result<()> {
    let Some(items) = options
        .and_then(|options| options.get_mut("clientCertificates"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    if items.len() > 16 {
        return Ok(());
    }
    for item in items {
        for (inline, path) in [("cert", "certPath"), ("key", "keyPath"), ("pfx", "pfxPath")] {
            if item.get(inline).is_some_and(|value| !value.is_null()) {
                continue;
            }
            let field = item
                .get_mut(path)
                .filter(|field| field.as_str() != Some(""));
            bind_file_path(field, base, cwd, HostFile::Certificate)?;
        }
    }
    Ok(())
}
