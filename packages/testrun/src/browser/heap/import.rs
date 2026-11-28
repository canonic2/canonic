//! Bounded copies of caller-selected private snapshot files.
use crate::browser::session::Error;
use serde::Deserialize;
use std::{io::Read, path::Path};

struct Counts {
    names: bool,
}
impl<'de> serde::de::Visitor<'de> for Counts {
    type Value = usize;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a bounded heap field-name or unsigned-integer array")
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut values: A) -> Result<usize, A::Error> {
        let mut count = 0;
        if self.names {
            while let Some(name) = values.next_element::<String>()? {
                if count == 64 || name.len() > 256 {
                    return Err(serde::de::Error::custom("heap field bounds"));
                }
                count += 1;
            }
        } else {
            while let Some(value) = values.next_element::<u64>()? {
                if count == super::MAX_BYTES || value > 9007199254740991 {
                    return Err(serde::de::Error::custom("heap entry bounds"));
                }
                count += 1;
            }
        }
        Ok(count)
    }
}
fn entries<'de, D: serde::Deserializer<'de>>(input: D) -> Result<usize, D::Error> {
    input.deserialize_seq(Counts { names: false })
}
fn fields<'de, D: serde::Deserializer<'de>>(input: D) -> Result<usize, D::Error> {
    input.deserialize_seq(Counts { names: true })
}
#[derive(Deserialize)]
struct Meta {
    #[serde(deserialize_with = "fields")]
    node_fields: usize,
    #[serde(deserialize_with = "fields")]
    edge_fields: usize,
    #[serde(default, deserialize_with = "fields")]
    trace_function_info_fields: usize,
}
#[derive(Deserialize)]
struct Header {
    meta: Meta,
    node_count: u64,
    edge_count: u64,
    #[serde(default)]
    trace_function_count: u64,
}
#[derive(Deserialize)]
struct Snapshot {
    snapshot: Header,
    #[serde(deserialize_with = "entries")]
    nodes: usize,
    #[serde(deserialize_with = "entries")]
    edges: usize,
    #[serde(default, deserialize_with = "entries")]
    trace_function_infos: usize,
}
/// Check allocation counts without materializing the numeric arrays in Rust.
pub(crate) fn validate_snapshot(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() > super::MAX_BYTES {
        return Err("HEAP_IMPORT_BOUNDS".into());
    }
    let input: Snapshot =
        serde_json::from_slice(bytes).map_err(|_| Error::from("HEAP_EVIDENCE_FORMAT"))?;
    let header = input.snapshot;
    let counts = [
        (header.node_count, header.meta.node_fields, input.nodes),
        (header.edge_count, header.meta.edge_fields, input.edges),
        (
            header.trace_function_count,
            header.meta.trace_function_info_fields,
            input.trace_function_infos,
        ),
    ];
    if header.node_count == 0
        || header.meta.node_fields == 0
        || header.meta.edge_fields == 0
        || (header.trace_function_count > 0 && header.meta.trace_function_info_fields == 0)
        || counts
            .into_iter()
            .any(|(count, width, entries)| count.checked_mul(width as u64) != Some(entries as u64))
    {
        return Err("HEAP_EVIDENCE_FORMAT".into());
    }
    Ok(())
}

pub(crate) fn validate(path: &str, max: usize) -> Result<(), &'static str> {
    if path.len() > 4096
        || path.chars().any(char::is_control)
        || !Path::new(path).is_absolute()
        || !(1024..=super::MAX_BYTES).contains(&max)
    {
        return Err("HEAP_IMPORT_BOUNDS");
    }
    Ok(())
}
pub(crate) fn read(path: &str, max: usize) -> Result<Vec<u8>, Error> {
    validate(path, max)?;
    let path = Path::new(path);
    crate::artifacts::check_private_directory(path.parent().ok_or("HEAP_PRIVATE_FILE_REQUIRED")?)
        .map_err(|_| Error::from("HEAP_PRIVATE_FILE_REQUIRED"))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| Error::caused("HEAP_IMPORT_IO", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| Error::caused("HEAP_IMPORT_IO", error))?;
    if !metadata.is_file() {
        return Err("HEAP_PRIVATE_FILE_REQUIRED".into());
    }
    if metadata.len() > max as u64 {
        return Err("HEAP_IMPORT_BOUNDS".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: this only reads the current process's effective user ID.
        let owner = unsafe { libc::geteuid() };
        if metadata.uid() != owner || metadata.permissions().mode() & 0o077 != 0 {
            return Err("HEAP_PRIVATE_FILE_REQUIRED".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::caused("HEAP_IMPORT_IO", error))?;
    if bytes.len() > max {
        return Err("HEAP_IMPORT_BOUNDS".into());
    }
    if std::str::from_utf8(&bytes).is_err()
        || bytes
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            != Some(b'{')
    {
        return Err("HEAP_EVIDENCE_FORMAT".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declared_heap_allocations_must_match_actual_arrays() {
        let valid = serde_json::json!({"snapshot":{"meta":{"node_fields":["type"],"edge_fields":["type"]},"node_count":1,"edge_count":0},"nodes":[0],"edges":[]});
        validate_snapshot(&serde_json::to_vec(&valid).unwrap()).unwrap();
        for value in [
            serde_json::json!({"snapshot":{"meta":{"node_fields":["id"],"edge_fields":["type"]},"node_count":1,"edge_count":0},"nodes":[9007199254740992u64],"edges":[]}),
            serde_json::json!({"snapshot":{"meta":{"node_fields":["type"],"edge_fields":["type"]},"node_count":u64::MAX,"edge_count":0},"nodes":[0],"edges":[]}),
            serde_json::json!({"snapshot":{"meta":{"node_fields":["type"],"edge_fields":["type"],"trace_function_info_fields":["id"]},"node_count":1,"edge_count":0,"trace_function_count":1000000},"nodes":[0],"edges":[],"trace_function_infos":[0]}),
            serde_json::json!({"snapshot":{"meta":{"node_fields":vec!["type";65],"edge_fields":["type"]},"node_count":1,"edge_count":0},"nodes":[0],"edges":[]}),
        ] {
            assert_eq!(
                validate_snapshot(&serde_json::to_vec(&value).unwrap())
                    .unwrap_err()
                    .code,
                "HEAP_EVIDENCE_FORMAT"
            );
        }
    }
    #[test]
    fn private_heap_inputs_are_copied_bounded_and_refuse_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        crate::artifacts::directory(&private).unwrap();
        let path = private.join("Acme.heapsnapshot");
        crate::artifacts::write(&path, b"{\"Acme\":true}").unwrap();
        let bytes = read(path.to_str().unwrap(), 1024).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert_eq!(bytes, b"{\"Acme\":true}");
        assert_eq!(
            read(path.to_str().unwrap(), 1024).unwrap_err().code,
            "HEAP_EVIDENCE_FORMAT"
        );
        std::fs::write(&path, vec![b'x'; 1025]).unwrap();
        assert_eq!(
            read(path.to_str().unwrap(), 1024).unwrap_err().code,
            "HEAP_IMPORT_BOUNDS"
        );
        #[cfg(unix)]
        {
            let link = private.join("link.heapsnapshot");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(read(link.to_str().unwrap(), 1024).is_err());
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&path, b"{}").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(
                read(path.to_str().unwrap(), 1024).unwrap_err().code,
                "HEAP_PRIVATE_FILE_REQUIRED"
            );
        }
    }
}
