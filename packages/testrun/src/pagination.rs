//! Testrun scopes and camelCase pages over MCP Kit's application contract.
use crate::error::{Error, Result};
use canonic_mcp_kit::page::{Builder, Limits, Page, Position, Request, Scope, digest};
use serde::Serialize;
use serde_json::{Value, json};
pub(crate) mod human;
#[cfg(test)]
mod tests;
pub(crate) mod ui;

pub(crate) const BYTES: Limits = Limits {
    default: 16_384,
    max: 65_536,
    ..Limits::STANDARD
};

/// Preserve bytewise identity order through 64 bytes, then break long-prefix
/// ties by the full identity digest. The key always fits the shared codec.
pub(crate) fn identity_key(identity: &str) -> Vec<u8> {
    let mut key = vec![0; 64];
    let prefix = identity.len().min(64);
    key[..prefix].copy_from_slice(&identity.as_bytes()[..prefix]);
    key.extend(digest(identity.as_bytes()));
    key
}

impl From<canonic_mcp_kit::page::Error> for Error {
    fn from(error: canonic_mcp_kit::page::Error) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

pub(crate) fn scope(name: &str, query: &impl Serialize) -> Result<Scope> {
    Ok(Scope::new(
        name,
        digest(
            &serde_json::to_vec(query)
                .map_err(|_| Error::new("INTERNAL", "Cannot encode page query"))?,
        ),
    ))
}
pub(crate) fn filters(args: &Value) -> Value {
    fn sorted(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                fields.values_mut().for_each(sorted);
                fields.sort_keys();
            }
            Value::Array(values) => values.iter_mut().for_each(sorted),
            _ => {}
        }
    }
    let mut query = args.clone();
    if let Some(fields) = query.as_object_mut() {
        fields.remove("cursor");
        fields.remove("limit");
    }
    sorted(&mut query);
    query
}

pub(crate) fn request(args: &Value, scope: &Scope) -> Result<(Request, Option<Vec<u8>>)> {
    let request = Request::from_mcp(args, Limits::STANDARD)?;
    let after = request
        .cursor
        .as_deref()
        .map(|cursor| match scope.decode(cursor)? {
            Position::Keyset(key) => Ok(key),
            Position::Offset(_) => Err(canonic_mcp_kit::page::Error::InvalidCursor),
        })
        .transpose()?;
    Ok((request, after))
}

/// The producer supplies ascending unique keys and borrowed rows; only admitted
/// candidates are projected. A conservative cursor may lead to an empty end page.
pub(crate) fn collect<T>(
    name: &str,
    query: &impl Serialize,
    args: &Value,
    rows: impl IntoIterator<Item = (Vec<u8>, T)>,
    mut view: impl FnMut(T) -> Result<Value>,
    read_tool: &str,
) -> Result<Page> {
    let scope = scope(name, query)?;
    let (request, after) = request(args, &scope)?;
    let mut builder = Builder::new(
        &scope,
        request.limit,
        Limits::STANDARD,
        crate::protocol::MAX_REPLY,
        read_tool,
    )?;
    let mut rows = rows
        .into_iter()
        .filter(|(key, _)| after.as_ref().is_none_or(|after| key > after));
    let mut ended = false;
    while !builder.is_full() {
        let Some((key, row)) = rows.next() else {
            ended = true;
            break;
        };
        if !builder.push(view(row)?, &Position::Keyset(key))? {
            break;
        }
    }
    Ok(builder.finish(!ended && rows.size_hint().1 != Some(0)))
}

pub(crate) fn properties(limits: Limits) -> Value {
    let mut properties = limits.properties();
    properties["limit"]["description"] = json!(if limits.max == BYTES.max {
        "Maximum bytes in this page; invalid limits are refused and the serialized-item budget may end it earlier."
    } else {
        "Maximum items in this page; invalid limits are refused and the serialized-item budget may end it earlier."
    });
    properties
}

pub(crate) fn declare(tool: &mut Value, limits: Limits) {
    fn schema(input: &mut Value, limits: Limits) {
        if let Some(branches) = input.get_mut("anyOf").and_then(Value::as_array_mut) {
            for branch in branches {
                schema(branch, limits);
            }
        }
        if input.get("properties").is_none() {
            input["properties"] = json!({});
        }
        let properties = input["properties"]
            .as_object_mut()
            .expect("tool properties");
        properties.remove("offset");
        properties.remove("maxBytes");
        properties.extend(
            self::properties(limits)
                .as_object()
                .expect("page properties")
                .clone(),
        );
    }
    schema(&mut tool["inputSchema"], limits);
    tool["outputSchema"] = json!({"type":"object","properties":canonic_mcp_kit::page::output_properties(canonic_mcp_kit::page::Casing::Camel)});
}

/// Directory enumeration has no seek API. Keep at most one metadata window;
/// callers decode only records admitted to their page, never the full history.
pub(crate) fn directory_keys(path: &std::path::Path, after: &str) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Error::new("STATE_IO", "Cannot enumerate retained records")),
    };
    let mut keys = std::collections::BTreeSet::new();
    for entry in entries {
        let entry =
            entry.map_err(|_| Error::new("STATE_IO", "Cannot enumerate retained record"))?;
        let Ok(key) = entry.file_name().into_string() else {
            continue;
        };
        if key.as_str() <= after {
            continue;
        }
        keys.insert(key);
        if keys.len() > 100 {
            keys.pop_last();
        }
    }
    Ok(keys.into_iter().collect())
}

/// Append-only byte positions are scoped to a producer's owned file incarnation.
/// `available` bounds committed data; callers retain their receipt/access checks.
pub(crate) fn stream(
    name: &str,
    query: &impl Serialize,
    args: &Value,
    path: &std::path::Path,
    available: Option<u64>,
    running: bool,
    base64: bool,
) -> Result<Value> {
    use std::io::{Read, Seek};
    let request = Request::from_mcp_units(args, BYTES)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = match options.open(path) {
        Ok(file) => Some(file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(Error::new("STATE_IO", "Cannot open owned output")),
    };
    let metadata = file
        .as_ref()
        .map(std::fs::File::metadata)
        .transpose()
        .map_err(|_| Error::new("STATE_IO", "Cannot inspect owned output"))?;
    if metadata
        .as_ref()
        .is_some_and(|metadata| !metadata.is_file())
    {
        return Err(Error::new(
            "STATE_IO",
            "Owned output must be a regular file",
        ));
    }
    let birth = metadata
        .as_ref()
        .and_then(|metadata| metadata.created().ok());
    #[cfg(unix)]
    let native = metadata.as_ref().map(|metadata| {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    });
    #[cfg(not(unix))]
    let native = birth;
    let incarnation = if metadata.is_some() {
        digest(format!("{birth:?}:{native:?}").as_bytes())[..16].to_vec()
    } else {
        vec![0; 16]
    };
    let scope = scope(name, query)?;
    let start = match request
        .cursor
        .as_deref()
        .map(|cursor| scope.decode(cursor))
        .transpose()?
    {
        Some(Position::Keyset(key))
            if key.len() == 24 && (key[8..] == [0; 16] || key[8..] == incarnation) =>
        {
            u64::from_be_bytes(
                key[..8]
                    .try_into()
                    .map_err(|_| canonic_mcp_kit::page::Error::InvalidCursor)?,
            )
        }
        None => 0,
        _ => return Err(canonic_mcp_kit::page::Error::InvalidCursor.into()),
    };
    let size = metadata.as_ref().map_or(0, std::fs::Metadata::len);
    let size = available.map_or(size, |available| available.min(size));
    if start > size {
        return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
    }
    let mut bytes = Vec::new();
    if let Some(file) = file.as_mut() {
        file.seek(std::io::SeekFrom::Start(start))
            .map_err(|_| Error::new("STATE_IO", "Cannot seek owned output"))?;
        file.take(request.limit.get().min(size - start))
            .read_to_end(&mut bytes)
            .map_err(|_| Error::new("STATE_IO", "Cannot read owned output"))?;
    }
    while encoded_bytes(&bytes, base64).to_string().len() + 2 > BYTES.max_bytes {
        bytes.truncate(bytes.len() / 2);
    }
    let end = start + bytes.len() as u64;
    let mut builder = Builder::new(
        &scope,
        Limits::STANDARD.limit(Some(1))?,
        Limits::STANDARD,
        crate::protocol::MAX_REPLY,
        name,
    )?;
    let mut position = end.to_be_bytes().to_vec();
    position.extend(incarnation);
    builder.push(encoded_bytes(&bytes, base64), &Position::Keyset(position))?;
    let mut page = builder.finish(running || end < size);
    let mut result = page.items.remove(0);
    result["nextCursor"] = json!(page.next_cursor);
    result["running"] = json!(running);
    result["unit"] = json!("bytes");
    Ok(result)
}

fn encoded_bytes(bytes: &[u8], base64: bool) -> Value {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.encode(bytes);
    if base64 {
        json!({"dataBase64":raw})
    } else {
        json!({"text":String::from_utf8_lossy(bytes),"dataBase64":raw})
    }
}
