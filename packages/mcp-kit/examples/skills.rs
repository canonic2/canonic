//! A self-contained offline workflow, its status tool and bundled supporting
//! files, plus a generated queue workflow the handler serves on request.

use canonic_mcp_kit::{
    Catalog, Client, Content, DynamicSkill, ModernOptions, ReadDirectoryResult, ReadResourceResult,
    Resource, ResourceContents, RpcError, ServerInfo, SkillCatalog, SkillDefinition, SkillFile,
    Tool, ToolResult,
    resource::DIRECTORY_MIME_TYPE,
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{io, sync::Arc};

/// The frontmatter every generated queue document carries.
const QUEUE: &str =
    "---\nname: queue\ndescription: Review the items waiting in the Acme queue.\n---\n";

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
            Request::Method { method, params } => {
                let uri = params["uri"].as_str().unwrap_or_default();
                let answer = if method == "resources/directory/read" {
                    queue_directory(uri).map(Outcome::Directory)
                } else {
                    queue_file(uri).map(Outcome::Resource)
                };
                answer.unwrap_or_else(|| Outcome::ResourceNotFound(uri.into()))
            }
            _ => Outcome::ResourceNotFound("acme://unknown".into()),
        }
    }
}
fn identity() -> ServerInfo {
    ServerInfo::new("acme", "1.0").with_instructions(
        "Read skill://acme/status/SKILL.md for the status workflow and skill://acme/queue/SKILL.md for the queue.",
    )
}

/// Generates a queue file from current state; the document keeps [`QUEUE`]'s frontmatter.
fn queue_file(uri: &str) -> Option<ReadResourceResult> {
    let waiting = 3;
    let (mime, text) = match uri {
        "skill://acme/queue/SKILL.md" => (
            "text/markdown",
            format!(
                "{QUEUE}\n# Review the Acme queue\n\n{waiting} items are waiting. Read [today's items](items/today.md).\n"
            ),
        ),
        "skill://acme/queue/items/today.md" => (
            "text/markdown",
            "- acme-101\n- acme-102\n- acme-103\n".into(),
        ),
        _ => return None,
    };
    let contents = ResourceContents::text(uri, Some(mime.into()), text).ok()?;
    ReadResourceResult::new(vec![contents]).ok()
}

/// Lists one generated directory's direct children.
fn queue_directory(uri: &str) -> Option<ReadDirectoryResult> {
    let child = |uri: &str, name: &str, mime: &str| {
        let mut resource = Resource::new(uri, name);
        resource.mime_type = Some(mime.into());
        resource
    };
    let children = match uri {
        "skill://acme/queue" => vec![
            child("skill://acme/queue/SKILL.md", "SKILL.md", "text/markdown"),
            child("skill://acme/queue/items", "items", DIRECTORY_MIME_TYPE),
        ],
        "skill://acme/queue/items" => vec![child(
            "skill://acme/queue/items/today.md",
            "today.md",
            "text/markdown",
        )],
        _ => return None,
    };
    ReadDirectoryResult::new(children, None).ok()
}

fn skills() -> Result<SkillCatalog, canonic_mcp_kit::SkillError> {
    let mut legend = SkillFile::binary(
        "assets/legend.svg",
        Arc::<[u8]>::from(&include_bytes!("skills/status/assets/legend.svg")[..]),
    );
    legend.mime_type = Some("image/svg+xml".into());
    SkillCatalog::with_dynamic(
        vec![SkillDefinition::new(
            "skill://acme/status",
            vec![
                SkillFile::text("SKILL.md", include_str!("skills/status/SKILL.md")),
                SkillFile::text(
                    "references/FORMAT.md",
                    include_str!("skills/status/references/FORMAT.md"),
                ),
                legend,
            ],
        )],
        &[DynamicSkill::new("skill://acme/queue", QUEUE)],
    )?
    .with_directory_reads()
}

fn tools() -> Result<Catalog, canonic_mcp_kit::CatalogError> {
    Catalog::new(vec![
        Tool::new(
            "acme_status",
            "Read Acme service state.",
            json!({"type":"object","additionalProperties":false}),
        )
        .with_annotations(canonic_mcp_kit::Annotations {
            read_only_hint: Some(true),
            ..canonic_mcp_kit::Annotations::default()
        }),
    ])
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = stdio::Options {
        session: canonic_mcp_kit::SessionOptions {
            modern: Some(ModernOptions::new(identity())),
            skills: Some(Arc::new(skills()?)),
            ..canonic_mcp_kit::SessionOptions::default()
        },
        ..stdio::Options::default()
    };
    #[expect(
        clippy::bufreader_stdin,
        reason = "stdio::serve moves the reader to another thread and StdinLock is not Send"
    )]
    let input = io::BufReader::new(io::stdin());
    stdio::serve(
        Arc::new(Acme),
        Arc::new(tools()?),
        input,
        io::stdout(),
        &options,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_mcp_kit::{Inbound, Session, SessionOptions, modern};
    use serde_json::Value;
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

    fn session(skills: &Arc<SkillCatalog>) -> Session {
        Session::try_new(
            Arc::new(tools().unwrap()),
            SessionOptions {
                modern: Some(ModernOptions::new(identity())),
                skills: Some(skills.clone()),
                ..SessionOptions::default()
            },
        )
        .unwrap()
    }

    fn request(id: u64, method: &str, mut params: Value) -> Value {
        params["_meta"] =
            json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}});
        canonic_mcp_kit::rpc::request(json!(id), method, params)
    }

    #[test]
    fn packaged_files_match_the_complete_source_tree() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/skills/status");
        let mut files = BTreeMap::new();
        enumerate(&root, &root, &mut files);
        let skills = Arc::new(skills().unwrap());
        let mut session = session(&skills);
        let get = request(
            1,
            "skills/get",
            json!({"uri":"skill://acme/status/SKILL.md"}),
        );
        let Inbound::Reply(reply) = session.receive_value(get) else {
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

    /// An independent runtime answers generated reads itself and checks them
    /// with the catalog before encoding, as the shared driver does.
    #[test]
    fn an_independent_runtime_serves_the_generated_queue() {
        let skills = Arc::new(skills().unwrap());
        let mut session = session(&skills);
        let Inbound::Reply(page) = session.receive_value(request(
            1,
            "resources/directory/read",
            json!({"uri":"skill://acme/status"}),
        )) else {
            panic!("static directories are answered by the core")
        };
        assert_eq!(page["result"]["resources"][0]["name"], "SKILL.md");
        let Inbound::Method { id, method, params } = session.receive_value(request(
            2,
            "resources/directory/read",
            json!({"uri":"skill://acme/queue/items"}),
        )) else {
            panic!("generated directories reach the runtime")
        };
        assert_eq!(method, "resources/directory/read");
        let uri = params["uri"].as_str().unwrap();
        let listing = queue_directory(uri).unwrap();
        skills.check_directory(uri, &listing).unwrap();
        let context = session.request_context(&id).unwrap();
        let bytes = context
            .encode_success(&id, &listing, 1 << 20, &|| Ok(()))
            .unwrap();
        assert!(session.finish(&id));
        let reply: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reply["result"]["resultType"], "complete");
        assert_eq!(reply["result"]["resources"][0]["name"], "today.md");
        let Inbound::Method { id, params, .. } = session.receive_value(request(
            3,
            "resources/read",
            json!({"uri":"skill://acme/queue/SKILL.md"}),
        )) else {
            panic!("generated documents reach the runtime")
        };
        let uri = params["uri"].as_str().unwrap();
        assert!(skills.dynamic_document(uri));
        skills.check_read(uri, &queue_file(uri).unwrap()).unwrap();
        assert!(session.finish(&id));
    }
}
