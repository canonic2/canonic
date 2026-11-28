//! Caller copies of immutable native trace evidence. Publication never replaces files.
use crate::browser::{artifacts::Artifact, session::Error};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
mod paths;
pub(crate) use paths::{absolute as client_absolute, validate};

pub(crate) fn resolve(path: &str) -> Result<String, Error> {
    paths::destination(path)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "TRACE_EXPORT_PATH_INVALID".into())
}
pub(crate) fn prepare(path: &str) -> Result<String, Error> {
    let path = resolve(path)?;
    directory(Path::new(&path))?;
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Err("TRACE_EXPORT_EXISTS".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(_) => Err("TRACE_EXPORT_IO".into()),
    }
}
fn directory(path: &Path) -> Result<PathBuf, Error> {
    let mut parent = path.parent().ok_or("TRACE_EXPORT_PATH_INVALID")?;
    loop {
        match std::fs::symlink_metadata(parent) {
            Ok(_) => {
                crate::artifacts::check_output_directory(parent)
                    .map_err(|_| Error::from("TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED"))?;
                return Ok(parent.to_path_buf());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                parent = parent.parent().ok_or("TRACE_EXPORT_PATH_INVALID")?
            }
            Err(_) => return Err("TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED".into()),
        }
    }
}
fn create_parents(path: &Path) -> Result<(), Error> {
    let mut current = directory(path)?;
    let parent = path.parent().ok_or("TRACE_EXPORT_PATH_INVALID")?;
    let missing = parent
        .strip_prefix(&current)
        .map_err(|_| Error::from("TRACE_EXPORT_PATH_INVALID"))?
        .to_path_buf();
    for name in missing.components() {
        current.push(name.as_os_str());
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.recursive(false).create(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("TRACE_EXPORT_IO".into()),
        }
        crate::artifacts::check_private_directory(&current)
            .map_err(|_| Error::from("TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED"))?;
    }
    Ok(())
}
pub(crate) fn write(path: &str, artifact: &Artifact) -> Result<Value, Error> {
    let path = prepare(path)?;
    let bytes = artifact.read()?;
    let gzip = path.ends_with(".gz");
    let bytes = if gzip {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&bytes)
            .map_err(|_| Error::from("TRACE_EXPORT_IO"))?;
        encoder
            .finish()
            .map_err(|_| Error::from("TRACE_EXPORT_IO"))?
    } else {
        bytes
    };
    create_parents(Path::new(&path))?;
    crate::artifacts::write(Path::new(&path), &bytes)
        .map_err(|_| Error::from("TRACE_EXPORT_IO"))?;
    Ok(
        json!({"path":path,"format":if gzip {"chrome-trace-event-json-gzip"} else {"chrome-trace-event-json"},"mimeType":if gzip {"application/gzip"} else {"application/json"},"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"evidenceId":artifact.id}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::artifacts::Format;
    use std::io::Read;
    #[test]
    fn native_trace_exports_preserve_bytes_and_refuse_replacement_or_changed_evidence() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("exports");
        crate::artifacts::directory(&directory).unwrap();
        let original = br#"{"traceEvents":[{"name":"Acme"}]}"#;
        let artifact = Format {
            prefix: "trace",
            extension: "json",
            mime: "application/json",
            name: "chrome-trace-event-json",
            max_bytes: 1024,
        }
        .write(root.path(), original, json!({}))
        .unwrap();
        for name in ["Acme.json", "Acme.json.gz"] {
            let path = directory.join(name);
            let path = path.to_str().unwrap();
            let receipt = write(path, &artifact).unwrap();
            let stored = std::fs::read(path).unwrap();
            assert_eq!(receipt["sha256"], format!("{:x}", Sha256::digest(&stored)));
            let decoded = if name.ends_with(".gz") {
                let mut decoded = Vec::new();
                flate2::read::GzDecoder::new(stored.as_slice())
                    .read_to_end(&mut decoded)
                    .unwrap();
                decoded
            } else {
                stored.clone()
            };
            assert_eq!(decoded, original);
            assert_eq!(
                write(path, &artifact).unwrap_err().code,
                "TRACE_EXPORT_EXISTS"
            );
            assert_eq!(std::fs::read(path).unwrap(), stored);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        std::fs::write(
            artifact.summary["attachment"]["path"].as_str().unwrap(),
            b"changed",
        )
        .unwrap();
        let path = directory.join("changed.json");
        assert_eq!(
            write(path.to_str().unwrap(), &artifact).unwrap_err().code,
            "EVIDENCE_CHANGED"
        );
        assert!(!path.exists());
        let pending = directory.join("later/changed.record");
        assert_eq!(
            write(pending.to_str().unwrap(), &artifact)
                .unwrap_err()
                .code,
            "EVIDENCE_CHANGED"
        );
        assert!(!directory.join("later").exists());
    }
    #[test]
    fn native_trace_export_paths_refuse_unsafe_shapes_writable_parents_and_sdk_options() {
        for path in [
            "http://example.com/Acme.json",
            "file:///tmp/Acme.json",
            "/tmp/Acme.json\0",
            "",
        ] {
            assert_eq!(validate(path), Err("TRACE_EXPORT_PATH_INVALID"));
        }
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("exports");
        crate::artifacts::directory(&directory).unwrap();
        let path = directory.join("Acme.json");
        prepare(path.to_str().unwrap()).unwrap();
        let options: super::super::Options =
            serde_json::from_value(json!({"filePath":path})).unwrap();
        assert!(options.validate(true).is_ok());
        assert_eq!(
            options.validate(false),
            Err("TRACE_NATIVE_OPTIONS_UNSUPPORTED")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            let link = directory.join("link.json");
            symlink(&path, &link).unwrap();
            assert_eq!(
                prepare(link.to_str().unwrap()).unwrap_err().code,
                "TRACE_EXPORT_EXISTS"
            );
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o777)).unwrap();
            assert_eq!(
                prepare(path.to_str().unwrap()).unwrap_err().code,
                "TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED"
            );
        }
    }

    #[test]
    fn native_trace_export_filename_replaces_the_last_extension_like_the_pinned_helper() {
        let root = tempfile::tempdir().unwrap();
        let original = br#"{"traceEvents":[{"name":"Acme filename"}]}"#;
        let artifact = Format {
            prefix: "trace",
            extension: "json",
            mime: "application/json",
            name: "chrome-trace-event-json",
            max_bytes: 1024,
        }
        .write(root.path(), original, json!({}))
        .unwrap();
        for (requested, expected, gzip) in [
            ("Acme.txt", "Acme.json", false),
            ("Acme.backup.GZ", "Acme.backup.json", false),
            ("Acme.", "Acme.json", false),
            (".Acme", ".Acme.json", false),
            (".gz", ".gz.gz", true),
            ("Acme-\u{e9}.trace.gz", "Acme-\u{e9}.trace.gz", true),
        ] {
            let case = root.path().join(uuid::Uuid::new_v4().to_string());
            crate::artifacts::directory(&case).unwrap();
            let receipt = write(case.join(requested).to_str().unwrap(), &artifact).unwrap();
            assert_eq!(
                receipt["path"],
                std::fs::canonicalize(&case)
                    .unwrap()
                    .join(expected)
                    .to_str()
                    .unwrap()
            );
            let bytes = std::fs::read(case.join(expected)).unwrap();
            let decoded = if gzip {
                let mut out = Vec::new();
                flate2::read::GzDecoder::new(bytes.as_slice())
                    .read_to_end(&mut out)
                    .unwrap();
                out
            } else {
                bytes
            };
            assert_eq!(decoded, original);
        }
    }

    #[cfg(unix)]
    #[test]
    fn native_trace_export_shared_parents_preserve_private_bytes_and_deferred_children() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let original = br#"{"traceEvents":[{"name":"Acme shared parent"}]}"#;
        let artifact = Format {
            prefix: "trace",
            extension: "json",
            mime: "application/json",
            name: "chrome-trace-event-json",
            max_bytes: 1024,
        }
        .write(root.path(), original, json!({}))
        .unwrap();
        for mode in [0o700, 0o750, 0o755, 0o1700, 0o1770, 0o1777] {
            let parent = root.path().join(format!("mode-{mode:o}"));
            crate::artifacts::directory(&parent).unwrap();
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(mode)).unwrap();
            for name in ["direct.json", "missing/deep/nested.json"] {
                let requested = parent.join(name);
                prepare(requested.to_str().unwrap()).unwrap();
                if name.starts_with("missing/") {
                    assert!(!parent.join("missing").exists());
                }
                let receipt = write(requested.to_str().unwrap(), &artifact).unwrap();
                let path = Path::new(receipt["path"].as_str().unwrap());
                assert_eq!(std::fs::read(path).unwrap(), original);
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                assert_eq!(
                    write(requested.to_str().unwrap(), &artifact)
                        .unwrap_err()
                        .code,
                    "TRACE_EXPORT_EXISTS"
                );
            }
            assert_eq!(
                std::fs::metadata(&parent).unwrap().permissions().mode() & 0o7777,
                mode
            );
            for child in ["missing", "missing/deep"] {
                assert_eq!(
                    std::fs::metadata(parent.join(child))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
            assert_eq!(
                std::fs::read_dir(&parent).unwrap().count(),
                2,
                "No publication temporary may remain"
            );
        }
        for mode in [0o770, 0o772, 0o777] {
            let parent = root.path().join(format!("mode-{mode:o}"));
            crate::artifacts::directory(&parent).unwrap();
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(mode)).unwrap();
            let requested = parent.join("missing/Acme.json");
            assert_eq!(
                prepare(requested.to_str().unwrap()).unwrap_err().code,
                "TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED"
            );
            assert!(std::fs::read_dir(parent).unwrap().next().is_none());
        }
    }

    #[test]
    fn native_trace_export_filename_prepares_missing_private_parents_without_creating_them() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("exports");
        crate::artifacts::directory(&private).unwrap();
        let requested = private.join("missing/deep/Acme.json");
        prepare(requested.to_str().unwrap()).unwrap();
        assert!(
            !private.join("missing").exists(),
            "Preparation must not publish directories before capture completes"
        );
        let artifact = Format {
            prefix: "trace",
            extension: "json",
            mime: "application/json",
            name: "chrome-trace-event-json",
            max_bytes: 1024,
        }
        .write(root.path(), b"{\"traceEvents\":[]}", json!({}))
        .unwrap();
        let receipt = write(requested.to_str().unwrap(), &artifact).unwrap();
        assert_eq!(
            receipt["path"],
            std::fs::canonicalize(&private)
                .unwrap()
                .join("missing/deep/Acme.json")
                .to_str()
                .unwrap()
        );
        assert!(private.join("missing/deep/Acme.json").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in ["missing", "missing/deep"] {
                assert_eq!(
                    std::fs::metadata(private.join(path))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
        }
    }
}
