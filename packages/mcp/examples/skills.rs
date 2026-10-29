//! A self-contained offline workflow, its status tool and bundled supporting files.

use canonic_mcp::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, SkillCatalog, SkillDefinition,
    SkillFile, Tool, ToolResult,
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{io, sync::Arc};

struct Acme;
impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(identity())
    }
    fn call(&self, request: Request, _: &Context) -> Outcome {
        match request {
            Request::Tool(call) if call.name == "acme_status" => {
                Outcome::Tool(ToolResult::success(vec![Content::text("running")]))
            }
            _ => Outcome::ResourceNotFound("acme://unknown".into()),
        }
    }
}
fn identity() -> ServerInfo {
    ServerInfo::new("acme", "1.0")
        .with_instructions("Read skill://acme/status/SKILL.md for the status workflow.")
}

fn skills() -> Result<SkillCatalog, canonic_mcp::SkillError> {
    let mut legend = SkillFile::binary(
        "assets/legend.svg",
        Arc::<[u8]>::from(&include_bytes!("skills/status/assets/legend.svg")[..]),
    );
    legend.mime_type = Some("image/svg+xml".into());
    SkillCatalog::new(vec![SkillDefinition::new(
        "skill://acme/status",
        vec![
            SkillFile::text("SKILL.md", include_str!("skills/status/SKILL.md")),
            SkillFile::text(
                "references/FORMAT.md",
                include_str!("skills/status/references/FORMAT.md"),
            ),
            legend,
        ],
    )])
}

fn tools() -> Result<Catalog, canonic_mcp::CatalogError> {
    Catalog::new(vec![
        Tool::new(
            "acme_status",
            "Read Acme service state.",
            json!({"type":"object","additionalProperties":false}),
        )
        .with_annotations(canonic_mcp::Annotations {
            read_only_hint: Some(true),
            ..canonic_mcp::Annotations::default()
        }),
    ])
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = stdio::Options {
        session: canonic_mcp::SessionOptions {
            modern: Some(ModernOptions::new(identity())),
            skills: Some(Arc::new(skills()?)),
            ..canonic_mcp::SessionOptions::default()
        },
        ..stdio::Options::default()
    };
    stdio::serve(
        Arc::new(Acme),
        Arc::new(tools()?),
        io::BufReader::new(io::stdin()),
        io::stdout(),
        &options,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, path::Path};

    fn enumerate(root: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(path).expect("example directory") {
            let entry = entry.expect("example file");
            let kind = entry.file_type().expect("file type");
            assert!(
                !kind.is_symlink(),
                "source packaging does not follow symlinks"
            );
            if kind.is_dir() {
                enumerate(root, &entry.path(), files);
            } else {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .expect("owned source")
                        .to_string_lossy()
                        .replace('\\', "/"),
                    std::fs::read(entry.path()).expect("source bytes"),
                );
            }
        }
    }

    #[test]
    fn packaged_files_match_the_complete_source_tree() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/skills/status");
        let mut files = BTreeMap::new();
        enumerate(&root, &root, &mut files);
        let skills = Arc::new(skills().unwrap());
        let mut session = canonic_mcp::Session::try_new(
            Arc::new(tools().unwrap()),
            canonic_mcp::SessionOptions {
                modern: Some(ModernOptions::new(identity())),
                skills: Some(skills.clone()),
                ..canonic_mcp::SessionOptions::default()
            },
        )
        .unwrap();
        let request = canonic_mcp::rpc::request(
            json!(1),
            "skills/get",
            json!({"uri":"skill://acme/status/SKILL.md","_meta":{canonic_mcp::modern::VERSION_KEY:canonic_mcp::modern::PROTOCOL_VERSION,canonic_mcp::modern::CAPABILITIES_KEY:{}}}),
        );
        let canonic_mcp::Inbound::Reply(reply) = session.receive_value(request) else {
            panic!("skill entry")
        };
        let manifest = reply["result"]["skill"]["resources"].as_array().unwrap();
        assert_eq!(manifest.len(), files.len());
        for (path, bytes) in files {
            let uri = format!("skill://acme/status/{path}");
            let file = manifest.iter().find(|file| file["uri"] == uri).unwrap();
            assert_eq!(file["size"], bytes.len());
            assert_eq!(
                file["digest"],
                format!("sha256:{:x}", Sha256::digest(&bytes))
            );
            assert!(skills.read(&uri).is_some());
        }
    }
}
