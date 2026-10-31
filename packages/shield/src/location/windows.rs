//! Native file identities from handles, including links inspected without following them.
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::fs::OpenOptionsExt,
    path::Path,
};

/// Observe the entry itself; parent components retain ordinary native resolution.
pub(crate) fn entry(path: &Path) -> io::Result<(u64, u64)> {
    // Opening a directory requires BACKUP_SEMANTICS; OPEN_REPARSE_POINT keeps
    // the final link's identity rather than silently inspecting its target.
    const FLAGS: u32 = 0x0200_0000 | 0x0020_0000;
    let file = OpenOptions::new()
        .read(true)
        .access_mode(0x0080) // FILE_READ_ATTRIBUTES, without reading file contents.
        .custom_flags(FLAGS)
        .open(path)?;
    identity(&file)
}

fn identity(file: &File) -> io::Result<(u64, u64)> {
    let info = winapi_util::file::information(file)?;
    Ok((info.volume_serial_number(), info.file_index()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_shell_mounts_and_drive_relative_paths_are_not_native_operands() {
        for path in ["/tmp/acme", "/c/acme", r"\acme", "C:acme"] {
            assert!(!super::super::native_operand(Path::new(path)), "{path}");
        }
        for path in [
            "acme.txt",
            "src/acme.txt",
            "C:/acme",
            r"\\server\share\acme",
        ] {
            assert!(super::super::native_operand(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn native_junction_reads_cannot_escape_project_location_rules() {
        use crate::{
            configuration::{Candidate, Evaluator},
            policy::{
                Scope,
                normalized::{Context, Input},
            },
        };
        use serde_json::json;
        let project = assert_fs::TempDir::new().unwrap();
        let outside = assert_fs::TempDir::new().unwrap();
        std::fs::write(project.path().join("inside.txt"), "Acme inside").unwrap();
        std::fs::write(outside.path().join("outside.txt"), "Acme outside").unwrap();
        crate::test_support::junction(&project.path().join("escape"), outside.path()).unwrap();
        let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","location-v1","requirements-v1"],
            "rules":[{"id":"read","decision":"allow","conditions":{"domain":"file","action":"read"},"requirements":{"sources":{"all":{"within":{"root":"project"}}}},"on_unmet":{"decision":"deny"}}]}).to_string();
        let candidate = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n")
            .unwrap()
            .capture(&[], Some(&rules))
            .unwrap();
        let evaluator = Evaluator::new(vec![(Scope::Project, candidate)]).unwrap();
        let root = project.path().canonicalize().unwrap();
        let root = root.to_str().unwrap();
        for (path, expected) in [("inside.txt", "allow"), ("escape/outside.txt", "deny")] {
            let input = Input::File {
                action: "read".into(),
                sources: vec![Path::new(root).join(path).to_str().unwrap().into()],
                destination: None,
                recursive: false,
                original_input: json!({}),
            };
            let context = Context {
                cwd: root.into(),
                environment: None,
                working_files: None,
            };
            let result = evaluator
                .evaluate_normalized(
                    &input,
                    &context,
                    root,
                    None,
                    "2026-01-01T00:00:00Z",
                    &|| Ok(()),
                )
                .unwrap();
            assert_eq!(result["decision"], expected, "{path}: {result}");
        }
    }

    #[test]
    fn native_identity_distinguishes_replacements_and_recognizes_hard_links() {
        let root = assert_fs::TempDir::new().unwrap();
        let original = root.path().join("original");
        let alias = root.path().join("alias");
        std::fs::write(&original, "Acme").unwrap();
        std::fs::hard_link(&original, &alias).unwrap();
        let before = entry(&original).unwrap();
        assert_eq!(before, entry(&alias).unwrap());
        std::fs::remove_file(&alias).unwrap();
        std::fs::write(&alias, "Acme replacement").unwrap();
        assert_ne!(before, entry(&alias).unwrap());
        assert_ne!(entry(root.path()).unwrap(), before);
        assert_eq!(entry(&original).unwrap(), before);
        assert_eq!(
            entry(&root.path().join("missing")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
