//! Packaging, manifest integrity and published-tool references of the bundled skills.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

fn source_tree(root: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(path).expect("skill source directory") {
        let entry = entry.expect("skill source entry");
        let kind = entry.file_type().expect("file type");
        assert!(!kind.is_symlink(), "skill sources contain no symlinks");
        if kind.is_dir() {
            source_tree(root, &entry.path(), files);
        } else {
            let relative = entry.path().strip_prefix(root).unwrap().to_owned();
            files.insert(
                relative.to_string_lossy().replace('\\', "/"),
                std::fs::read(entry.path()).expect("skill source bytes"),
            );
        }
    }
}

fn entry(uri: &str) -> serde_json::Value {
    let mut session = canonic_mcp_kit::Session::try_new(
        crate::protocol::tools(),
        canonic_mcp_kit::SessionOptions {
            modern: Some(canonic_mcp_kit::ModernOptions::new(
                canonic_mcp_kit::ServerInfo::new("canonic-testrun", "fixture"),
            )),
            skills: Some(std::sync::Arc::new(catalog().unwrap())),
            ..canonic_mcp_kit::SessionOptions::default()
        },
    )
    .unwrap();
    let request = canonic_mcp_kit::rpc::request(
        serde_json::json!(1),
        "skills/get",
        serde_json::json!({"uri":uri,"_meta":{
            canonic_mcp_kit::modern::VERSION_KEY:canonic_mcp_kit::modern::PROTOCOL_VERSION,
            canonic_mcp_kit::modern::CAPABILITIES_KEY:{}}}),
    );
    let canonic_mcp_kit::Inbound::Reply(reply) = session.receive_value(request) else {
        panic!("skills/get answers from the core")
    };
    reply["result"]["skill"].clone()
}

#[test]
fn embedded_manifests_match_the_complete_source_tree_byte_for_byte() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let mut sources = BTreeMap::new();
    source_tree(&root, &root, &mut sources);
    let embedded: BTreeMap<_, _> = SKILL_FILES
        .iter()
        .map(|(skill, path, bytes)| (format!("{skill}/{path}"), bytes.to_vec()))
        .collect();
    assert_eq!(embedded, sources);
    let catalog = catalog().unwrap();
    for (path, bytes) in &sources {
        let skill = path.split('/').next().unwrap();
        let uri = format!("{NAMESPACE}/{path}");
        let manifest = entry(&format!("{NAMESPACE}/{skill}/SKILL.md"));
        let file = manifest["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["uri"] == uri)
            .unwrap_or_else(|| panic!("{uri} is in its manifest"));
        assert_eq!(file["size"], bytes.len());
        assert_eq!(
            file["digest"],
            format!("sha256:{:x}", Sha256::digest(bytes))
        );
        assert!(catalog.read(&uri).is_some());
    }
}

#[test]
fn ui_verification_is_listed_with_its_trigger_in_frontmatter() {
    let skill = entry("skill://testrun/ui-verification/SKILL.md");
    assert_eq!(skill["frontmatter"]["name"], "ui-verification");
    let description = skill["frontmatter"]["description"].as_str().unwrap();
    assert!(description.contains("uncertain"), "{description}");
}

#[test]
fn skill_text_names_only_published_tools_and_bundled_guides() {
    let tools = crate::protocol::tools();
    let browser = crate::browser::contract::schemas();
    let guides = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
    for (skill, path, bytes) in SKILL_FILES {
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        for token in text.split('`').skip(1).step_by(2) {
            if token.ends_with(".md") {
                assert!(guides.join(token).is_file(), "{skill}/{path}: {token}");
            } else if token.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                // Pagination identifiers are fields or an error code, not tool names.
                let pagination_term =
                    ["cursor", "limit", "query", "invalid_cursor"].contains(&token);
                assert!(
                    tools.get(token).is_some()
                        || names_operation(&browser, token)
                        || pagination_term,
                    "{skill}/{path}: {token}"
                );
            }
        }
    }
}

fn names_operation(schema: &serde_json::Value, name: &str) -> bool {
    if schema["properties"]["operation"]["const"] == name {
        return true;
    }
    match schema {
        serde_json::Value::Object(fields) => {
            fields.values().any(|value| names_operation(value, name))
        }
        serde_json::Value::Array(values) => values.iter().any(|value| names_operation(value, name)),
        _ => false,
    }
}
