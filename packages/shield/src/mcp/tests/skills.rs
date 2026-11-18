//! Static publication and modern per-call context exercised through shared stdio.
use super::*;

pub(super) fn modern(id: i64, method: &str, mut params: Value, capabilities: Value) -> Value {
    let mut metadata = json!({
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
    });
    metadata["io.modelcontextprotocol/clientCapabilities"] = capabilities;
    params["_meta"] = metadata;
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

use crate::mcp::skills::SOURCES;

#[test]
fn static_discovery_list_and_read_need_no_project_or_handshake() {
    let mut messages = vec![
        modern(1, "server/discover", json!({}), json!({})),
        modern(2, "skills/list", json!({}), json!({})),
        modern(3, "prompts/list", json!({}), json!({})),
        modern(
            4,
            "resources/read",
            json!({"uri":"skill://shield/missing/SKILL.md"}),
            json!({}),
        ),
        modern(5, "tools/list", json!({}), json!({})),
    ];
    for (index, (uri, _)) in (0..).zip(SOURCES) {
        let file = format!("{uri}/SKILL.md");
        messages.push(modern(
            10 + index,
            "skills/get",
            json!({"uri":file}),
            json!({}),
        ));
        messages.push(modern(
            20 + index,
            "resources/read",
            json!({"uri":file}),
            json!({}),
        ));
    }
    let mut output = Vec::new();
    serve_static(Cursor::new(frames(&messages)), &mut output, &test_options()).unwrap();
    let outputs = lines(&output);
    let discovery = reply(&outputs, &json!(1));
    assert!(
        discovery["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/skills"]
            .is_object()
    );
    let listed = reply(&outputs, &json!(2))["result"]["skills"].clone();
    assert_eq!(listed.as_array().unwrap().len(), SOURCES.len());
    for (index, (uri, source)) in (0..).zip(SOURCES) {
        let file = format!("{uri}/SKILL.md");
        let entry = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["resources"][0]["uri"] == file.as_str())
            .unwrap_or_else(|| panic!("{file} not listed: {listed}"));
        assert_eq!(
            entry,
            &reply(&outputs, &json!(10 + index))["result"]["skill"]
        );
        assert_eq!(
            reply(&outputs, &json!(20 + index))["result"]["contents"][0]["text"],
            source
        );
        assert_eq!(entry["resources"].as_array().unwrap().len(), 1);
        assert_eq!(entry["resources"][0]["size"], source.len());
        assert_eq!(
            entry["resources"][0]["digest"],
            format!("sha256:{}", crate::policy::fingerprint(source.as_bytes()))
        );
    }
    assert_eq!(
        reply(&outputs, &json!(3))["result"]["prompts"][0]["name"],
        "install_pack"
    );
    let reads = (0..)
        .zip(SOURCES)
        .flat_map(|(index, _)| [10 + index, 20 + index]);
    for id in [1, 2, 3].into_iter().chain(reads) {
        let result = reply(&outputs, &json!(id))["result"].clone();
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["cacheScope"], "private");
        assert_eq!(result["ttlMs"], 0);
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "shield"
        );
    }
    assert_eq!(
        reply(&outputs, &json!(4))["error"]["code"],
        rpc::INVALID_PARAMS
    );
    assert_eq!(reply(&outputs, &json!(5))["result"]["tools"], json!([]));
}

/// Result fields, result codes and rule fields the skills name besides tools
/// and their arguments.
const NAMED_FIELDS: [&str; 5] = [
    "apply_arguments",
    "after_candidate_id",
    "stale_revision",
    "expires_at",
    "pack_updates",
];

#[test]
fn bundled_skills_name_only_published_tools_and_arguments() {
    let tools = catalog(None).unwrap();
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
    for (uri, source) in SOURCES {
        let prose = source.split("```").step_by(2).collect::<String>();
        let mut tool = None;
        for reference in prose.split('`').skip(1).step_by(2) {
            if reference.starts_with('{') {
                // An inline example passes only arguments of the tool named before it.
                let example: Value = serde_json::from_str(reference)
                    .unwrap_or_else(|error| panic!("{uri}: {reference}: {error}"));
                if let Some(schema) = tool {
                    let schema: &Value = schema;
                    for key in example.as_object().unwrap().keys() {
                        assert!(
                            schema["properties"].get(key).is_some(),
                            "{uri}: {reference}: unknown argument {key}"
                        );
                    }
                }
            } else if std::path::Path::new(reference).extension() == Some("md".as_ref()) {
                assert!(
                    docs.join(reference).is_file(),
                    "{uri}: unknown bundled guide: {reference}"
                );
            } else if reference.contains('_') && !NAMED_FIELDS.contains(&reference) {
                assert!(
                    tools.tools().iter().any(|tool| tool.name == reference
                        || tool.input_schema["properties"].get(reference).is_some()),
                    "{uri}: unknown tool or argument reference: {reference}"
                );
            }
            if !reference.starts_with('{') {
                tool = tools
                    .tools()
                    .iter()
                    .find(|tool| tool.name == reference)
                    .map(|tool| &tool.input_schema);
            }
        }
    }
}

#[test]
fn every_skill_folder_is_served_and_every_linked_skill_exists() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let mut folders: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .map(|name| format!("skill://shield/{name}"))
        .collect();
    folders.sort();
    let served: Vec<&str> = SOURCES.iter().map(|(uri, _)| *uri).collect();
    assert_eq!(folders, served, "register each skills/ folder in SOURCES");
    // Agent-facing text links skills by URI; each must be served.
    for text in [
        include_str!("../messages.rs"),
        include_str!("../../agents/feedback.rs"),
    ] {
        for link in text.split("skill://").skip(1) {
            let uri = format!("skill://{}", link.split("/SKILL.md").next().unwrap());
            assert!(served.contains(&uri.as_str()), "unserved skill link {uri}");
        }
    }
}

#[test]
fn custom_rule_examples_are_valid_rules_that_decide_as_described() {
    use crate::configuration::{Candidate, Evaluator};
    let source = include_str!("../../../skills/custom-rules/SKILL.md");
    let rules: Vec<Value> = source
        .split("```json")
        .skip(1)
        .map(|block| {
            let example: Value = serde_json::from_str(block.split("```").next().unwrap()).unwrap();
            assert_eq!(example.as_object().unwrap().len(), 1, "{example}");
            example["rule"].clone()
        })
        .collect();
    assert_eq!(rules.len(), 6);
    let document = crate::policy::rules::document(&rules).unwrap();
    let candidate = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n")
        .unwrap()
        .capture(&[], Some(&document.to_string()))
        .unwrap();
    let evaluator = Evaluator::new(vec![(crate::policy::Scope::Project, candidate)]).unwrap();
    let root = assert_fs::TempDir::new().unwrap();
    let project = root.path().to_str().unwrap();
    let nested = root.path().join("packages");
    std::fs::create_dir(&nested).unwrap();
    let nested = nested.to_str().unwrap();
    for (command, cwd, expected) in [
        ("make -C packages/api test", project, "allow"),
        ("make -C packages/api test", nested, "ask"),
        ("make -C packages/api deploy", project, "ask"),
        ("acme-check verify --format json", nested, "allow"),
        ("acme-check verify --write", project, "ask"),
        ("acme-db --read-only --user analyst", project, "allow"),
        ("acme-db --user analyst", project, "deny"),
        ("acme-deploy --env staging", project, "deny"),
        ("curl -q https://docs.example.com/guide", project, "allow"),
        ("curl -q https://example.com/", project, "ask"),
    ] {
        let result = crate::shell::analyze(
            command,
            crate::shell::Dialect::Bash,
            cwd,
            project,
            &evaluator,
        )
        .unwrap();
        assert_eq!(result["decision"], expected, "{command}: {result}");
    }
}

#[test]
fn bundled_reads_remain_responsive_while_the_evaluator_is_busy() {
    use canonic_mcp_kit::stdio::Handler;
    struct Busy {
        server: Server,
        started: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Handler for Busy {
        fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
            self.server.initialize(client)
        }
        fn admit(&self, request: &Request) -> stdio::Admission {
            self.server.admit(request)
        }
        fn call(&self, request: Request, context: &stdio::Context) -> Outcome {
            self.started.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            self.server.call(request, context)
        }
    }
    let (server, project) = fixture(None);
    let before = project.revision();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (input_tx, input_rx) = mpsc::sync_channel(8);
    let (output_tx, output_rx) = mpsc::sync_channel(8);
    let worker = thread::spawn(move || {
        stdio::serve(
            Arc::new(Busy {
                server,
                started: started_tx,
                release: Mutex::new(release_rx),
            }),
            Arc::new(catalog(None).unwrap()),
            BufReader::new(Frames {
                incoming: input_rx,
                current: Cursor::new(Vec::new()),
            }),
            Responses {
                outgoing: output_tx,
                current: Vec::new(),
            },
            &bundled_options(&test_options()).unwrap(),
        )
        .unwrap();
    });
    input_tx
        .send(frames(&[modern(
            1,
            "tools/call",
            json!({"name":"status","arguments":{}}),
            json!({}),
        )]))
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    input_tx
        .send(frames(&[modern(2, "skills/list", json!({}), json!({}))]))
        .unwrap();
    let output = output_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(lines(&output)[0]["id"], 2);
    release_tx.send(()).unwrap();
    assert_eq!(
        lines(&output_rx.recv_timeout(Duration::from_secs(5)).unwrap())[0]["id"],
        1
    );
    drop(input_tx);
    worker.join().unwrap();
    assert_eq!(project.revision(), before);
}

#[test]
fn static_reads_respect_the_output_frame_limit() {
    let mut options = test_options();
    let mut listed = Vec::new();
    serve_static(
        Cursor::new(frames(&[modern(1, "skills/list", json!({}), json!({}))])),
        &mut listed,
        &test_options(),
    )
    .unwrap();
    let listed = lines(&listed);
    let resource = &listed[0]["result"]["skills"][0]["resources"][0];
    let uri = resource["uri"]
        .as_str()
        .expect("a bundled skill to read")
        .to_owned();
    assert!(resource["size"].as_u64().unwrap() > 1024, "{resource}");
    options.max_output_bytes = 1024;
    let mut output = Vec::new();
    serve_static(
        Cursor::new(frames(&[modern(
            1,
            "resources/read",
            json!({"uri":uri}),
            json!({}),
        )])),
        &mut output,
        &options,
    )
    .unwrap();
    let replies = lines(&output);
    assert!(replies[0]["error"].is_object(), "{replies:?}");
    assert!(output.len() <= 1025);
}

#[test]
fn modern_and_legacy_inspection_share_no_request_capabilities() {
    let (server, project) = fixture(Some("claude"));
    let before = project.revision();
    let transport = Transport::start(server);
    let request = modern(
        10,
        "tools/call",
        json!({"name":"policy_show","arguments":{"status":"pending"}}),
        json!({}),
    );
    let response = transport.request(&request);
    assert_eq!(response["result"]["resultType"], "complete");
    assert_eq!(
        response["result"]["structuredContent"]["data"]["proposals"],
        json!([])
    );
    for message in initialize(&json!({"elicitation":{"form":{}}}), "codex") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let legacy = transport.call(json!(11), "history_list", &json!({}));
    assert!(legacy["result"].get("resultType").is_none());
    let modern = transport.request(&modern(12, "skills/list", json!({}), json!({})));
    assert_eq!(modern["result"]["resultType"], "complete");
    assert_eq!(project.revision(), before);
}
