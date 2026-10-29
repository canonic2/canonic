//! Exercises Shield's tools, guidance, limits and confirmation through the
//! stdio driver, with connection-local project fixtures.

use super::*;
use crate::configuration::Bundle;
use canonic_mcp::LATEST_PROTOCOL_VERSION as VERSION;
use serde_json::{Value, json};
use std::{
    io::{self, BufReader, Cursor, Read, Write},
    sync::mpsc,
    thread::{self, JoinHandle},
};
mod confirmation;

/// A fixture project. The temporary root lives as long as this value.
pub(crate) struct Project {
    _root: assert_fs::TempDir,
    pub(crate) review: Arc<crate::review::Context>,
}

impl Project {
    pub(crate) fn revision(&self) -> Value {
        crate::review::inspect(&self.review).unwrap()["policy_revision"].clone()
    }
}

/// A server for a fresh project whose enrolled agent is `agent`.
pub(crate) fn fixture(agent: Option<&str>) -> (Server, Project) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let mut context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    context.agent = agent.map(str::to_owned);
    let server = Server::for_review(context).unwrap();
    let review = server.review();
    (
        server,
        Project {
            _root: root,
            review,
        },
    )
}

pub(crate) fn initialize(capabilities: &Value, client: &str) -> [Value; 2] {
    [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,
            "capabilities":capabilities,"clientInfo":{"name":client,"version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ]
}

fn tool(id: Value, name: &str, arguments: &Value) -> Value {
    let mut request =
        json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":name,"arguments":arguments}});
    request["id"] = id;
    request
}

fn frames(messages: &[Value]) -> Vec<u8> {
    messages
        .iter()
        .flat_map(|message| {
            let mut frame = serde_json::to_vec(message).unwrap();
            frame.push(b'\n');
            frame
        })
        .collect()
}

fn lines(output: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub(crate) fn test_options() -> stdio::Options {
    options(Duration::from_secs(5), None)
}

/// Serves complete input, then returns every output frame. Only read-only
/// calls are reliable here: input ends at once, which refuses queued mutations.
fn exchange(server: Server, messages: &[Value]) -> Vec<Value> {
    let mut output = Vec::new();
    serve(
        server,
        Cursor::new(frames(messages)),
        &mut output,
        &test_options(),
    )
    .unwrap();
    lines(&output)
}

fn reply(outputs: &[Value], id: &Value) -> Value {
    outputs
        .iter()
        .find(|output| &output["id"] == id && output.get("method").is_none())
        .unwrap_or_else(|| panic!("no reply to {id}: {outputs:?}"))
        .clone()
}

fn evaluation(project: &Project, input: &Value) -> Value {
    json!({"input":input,"context":{"cwd":project.review.project_root,"environment":{}}})
}

fn argv(project: &Project) -> Value {
    evaluation(
        project,
        &json!({"kind":"argv","executable":"acme","arguments":["a b","λ","$(touch secret)"],"original_input":{"command":"acme"}}),
    )
}

/// One live connection: frames are sent and read one at a time.
pub(crate) struct Transport {
    input: Option<mpsc::SyncSender<Vec<u8>>>,
    output: mpsc::Receiver<Vec<u8>>,
    serving: Option<JoinHandle<Result<(), ServeError>>>,
}

struct Frames {
    incoming: mpsc::Receiver<Vec<u8>>,
    current: Cursor<Vec<u8>>,
}

impl Read for Frames {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.current.position() == self.current.get_ref().len() as u64 {
            self.current = Cursor::new(self.incoming.recv().unwrap_or_default());
        }
        self.current.read(output)
    }
}

struct Responses {
    outgoing: mpsc::SyncSender<Vec<u8>>,
    current: Vec<u8>,
}

impl Write for Responses {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.current.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.outgoing
            .send(std::mem::take(&mut self.current))
            .map_err(|_| io::Error::other("fixture output closed"))
    }
}

impl Transport {
    pub(crate) fn start(server: Server) -> Self {
        let (input, incoming) = mpsc::sync_channel(8);
        let (outgoing, output) = mpsc::sync_channel(8);
        let serving = thread::spawn(move || {
            serve(
                server,
                BufReader::new(Frames {
                    incoming,
                    current: Cursor::new(Vec::new()),
                }),
                Responses {
                    outgoing,
                    current: Vec::new(),
                },
                &test_options(),
            )
        });
        Self {
            input: Some(input),
            output,
            serving: Some(serving),
        }
    }

    pub(crate) fn send(&self, message: &Value) {
        self.input
            .as_ref()
            .unwrap()
            .send(frames(std::slice::from_ref(message)))
            .unwrap();
    }

    pub(crate) fn receive(&self) -> Value {
        let bytes = self.output.recv_timeout(Duration::from_secs(5)).unwrap();
        serde_json::from_slice(bytes.strip_suffix(b"\n").unwrap_or(&bytes)).unwrap()
    }

    /// Sends a request and returns the next frame.
    pub(crate) fn request(&self, message: &Value) -> Value {
        self.send(message);
        self.receive()
    }

    pub(crate) fn call(&self, id: Value, name: &str, arguments: &Value) -> Value {
        self.request(&tool(id, name, arguments))
    }

    /// Closes the input and returns the remaining frames and the result.
    pub(crate) fn close(mut self) -> (Vec<Value>, Result<(), ServeError>) {
        self.input.take();
        let result = self.serving.take().unwrap().join().unwrap();
        let rest = self
            .output
            .try_iter()
            .map(|bytes| {
                serde_json::from_slice(bytes.strip_suffix(b"\n").unwrap_or(&bytes)).unwrap()
            })
            .collect();
        (rest, result)
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.input.take();
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
    }
}

#[test]
fn catalog_publishes_every_tool_and_marks_only_policy_changes_as_mutations() {
    let catalog = catalog(None).unwrap();
    assert_eq!(
        catalog.listing()["tools"],
        crate::review::mcp::tools()["tools"]
    );
    let mutations: Vec<_> = catalog
        .tools()
        .iter()
        .filter(|tool| !tool.read_only())
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(
        mutations,
        [
            "policy_propose",
            "review_defer",
            "policy_withdraw",
            "policy_apply"
        ]
    );
}

#[test]
fn registered_host_tools_avoid_top_level_schema_unions() {
    for agent in ["claude-code", "codex"] {
        for tool in catalog(Some(agent)).unwrap().listing()["tools"]
            .as_array()
            .unwrap()
        {
            for keyword in ["oneOf", "anyOf", "allOf"] {
                assert!(
                    tool["inputSchema"].get(keyword).is_none(),
                    "{agent}: {tool}"
                );
            }
        }
    }
}

#[test]
fn documentation_tools_deliver_bundled_text_and_validate_page_arguments() {
    let (server, _project) = fixture(None);
    let mut messages = initialize(&json!({}), "Acme").to_vec();
    messages.extend([
        tool(json!(2), "shield_docs_list", &json!({"limit":1})),
        tool(
            json!(3),
            "shield_docs_search",
            &json!({"query":"operands","path":"policy-rules.md","limit":1}),
        ),
        tool(
            json!(4),
            "shield_docs_read",
            &json!({"path":"policy-rules.md","limit":1}),
        ),
        tool(
            json!(5),
            "shield_docs_read",
            &json!({"path":"policy-rules.md","start_line":0}),
        ),
        tool(
            json!(6),
            "shield_docs_search",
            &json!({"query":"rule","limit":101}),
        ),
        tool(json!(7), "shield_docs_read", &json!({"path":"missing.md"})),
        tool(
            json!(8),
            "shield_docs_search",
            &json!({"query":"rule","path":"missing.md"}),
        ),
    ]);
    let outputs = exchange(server, &messages);
    for id in 2..=4 {
        let result = reply(&outputs, &json!(id));
        assert_eq!(result["result"]["isError"], false, "{result}");
        assert_eq!(
            result["result"]["structuredContent"]["data"]["version"],
            env!("CARGO_PKG_VERSION")
        );
    }
    let listed = reply(&outputs, &json!(2));
    assert!(listed["result"]["structuredContent"]["data"]["next_offset"].is_number());
    let searched = reply(&outputs, &json!(3));
    assert_eq!(
        searched["result"]["structuredContent"]["data"]["matches"][0]["path"],
        "policy-rules.md"
    );
    let read = reply(&outputs, &json!(4));
    assert!(
        read["result"]["structuredContent"]["data"]["content"]
            .as_str()
            .unwrap()
            .starts_with("# ")
    );
    for id in [5, 6] {
        let result = reply(&outputs, &json!(id));
        assert_eq!(result["result"]["isError"], true, "{result}");
        assert_eq!(
            result["result"]["structuredContent"]["error"]["code"],
            "invalid_input"
        );
    }
    for id in [7, 8] {
        let result = reply(&outputs, &json!(id));
        assert_eq!(result["result"]["isError"], true, "{result}");
        assert_eq!(
            result["result"]["structuredContent"]["error"]["code"],
            "not_found"
        );
        assert_eq!(
            result["result"]["content"][0]["text"],
            "Unknown documentation page; call shield_docs_list."
        );
        assert_eq!(
            result["result"]["content"][0]["text"],
            result["result"]["structuredContent"]["error"]["message"]
        );
    }
}

#[test]
fn enrolled_adapters_publish_their_own_apply_schema_and_claude_keeps_its_contract() {
    for agent in [Some("claude-code"), Some("claude"), Some("acme-agent")] {
        let listing = catalog(agent).unwrap().listing().clone();
        let apply = listing["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "policy_apply")
            .unwrap();
        assert_eq!(
            apply["inputSchema"]["required"],
            json!(["proposal_id", "candidate_digest", "changes", "review"])
        );
        assert!(apply["inputSchema"].get("oneOf").is_none());
        assert_eq!(apply["inputSchema"]["additionalProperties"], false);
    }
    let listing = catalog(Some("codex")).unwrap().listing().clone();
    let apply = listing["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "policy_apply")
        .unwrap();
    assert_eq!(
        apply["inputSchema"]["required"],
        json!(["change", "reference"])
    );
    assert_eq!(apply["inputSchema"]["additionalProperties"], false);
}

#[test]
fn evaluation_is_connection_local_and_never_executes_input() {
    for _ in 0..2 {
        let (server, project) = fixture(None);
        let shell = evaluation(
            &project,
            &json!({"kind":"shell","source":"acme read; sh -c 'acme write'","dialect":"posix","dialect_version":null,"original_input":{"command":"acme"}}),
        );
        let mut messages = initialize(&json!({}), "Acme").to_vec();
        messages.extend([
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            tool(json!("λ"), "policy_evaluate", &argv(&project)),
            tool(json!(5), "policy_evaluate", &shell),
        ]);
        let outputs = exchange(server, &messages);
        assert_eq!(
            reply(&outputs, &json!(2))["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            21
        );
        let report = reply(&outputs, &json!("λ"));
        let envelope = &report["result"]["structuredContent"];
        assert_eq!(envelope["project_id"], project.review.project_id);
        assert_eq!(envelope["data"]["decision"], "ask");
        assert_eq!(report["result"]["isError"], false);
        let message = report["result"]["content"][0]["text"].as_str().unwrap();
        assert!(message.contains("preview") && message.contains("does not authorize execution"));
        assert!(!message.contains("touch secret") && message.split_whitespace().count() <= 40);
        assert_eq!(
            serde_json::from_str::<Value>(report["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap()["data"],
            envelope["data"]
        );
        let shell = &reply(&outputs, &json!(5))["result"]["structuredContent"]["data"];
        assert_eq!(shell["decision"], "ask");
        assert_eq!(
            shell["analysis_coverage"]["operations"][0]["request"]["cwd"],
            project.review.project_root
        );
    }
}

#[test]
fn guidance_uses_the_enrolled_agent_never_client_metadata() {
    for (agent, client, expected, absent) in [
        (
            Some("codex"),
            "claude-code",
            "approval policy is never",
            "MCP form elicitation",
        ),
        (
            Some("claude-code"),
            "codex",
            "MCP confirmation form in default, auto and bypassPermissions modes",
            "for Codex tool approval",
        ),
        (
            Some("acme-host"),
            "codex",
            "If this host cannot confirm",
            "MCP confirmation form in default, auto and bypassPermissions modes",
        ),
        (
            None,
            "codex",
            "If this host cannot confirm",
            "approval policy is never",
        ),
        (
            None,
            "claude-code",
            "If this host cannot confirm",
            "MCP confirmation form in default, auto and bypassPermissions modes",
        ),
    ] {
        let (server, _project) = fixture(agent);
        let outputs = exchange(server, &initialize(&json!({}), client));
        let instructions = reply(&outputs, &json!(1))["result"]["instructions"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(instructions.contains(expected), "{instructions}");
        assert!(!instructions.contains(absent), "{instructions}");
        assert_eq!(
            instructions.contains("shield policy review"),
            agent != Some("claude-code")
        );
        let catalog = catalog(agent).unwrap();
        for name in instructions
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| word.starts_with("shield_"))
        {
            assert!(catalog.get(name).is_some(), "{name}");
        }
        for name in ["policy_propose", "policy_apply", "policy_evaluate"] {
            assert!(instructions.contains(name), "{name}");
        }
    }
}

fn proposal_examples(transport: &Transport) -> Vec<Value> {
    let catalog = transport.request(&json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}));
    catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "policy_propose")
        .unwrap()["inputSchema"]["examples"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn published_rule_examples_keep_command_scope_and_report_invalid_paths() {
    let (server, project) = fixture(Some("codex"));
    let transport = Transport::start(server);
    let [start, ready] = initialize(&json!({}), "codex");
    transport.request(&start);
    transport.send(&ready);
    let revision = project.revision();
    let examples = proposal_examples(&transport);
    let mut args = examples[0].clone();
    args["change"]["rule"]["conditions"]["cwd"] = json!({"exact":"/acme/project"});
    let rejected = transport.call(json!(2), "policy_propose", &args);
    assert_eq!(rejected["result"]["isError"], true);
    let message = rejected["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap();
    assert!(
        message.contains("arguments.change.rule.conditions.cwd.exact must be object"),
        "{rejected}"
    );
    assert!(!message.contains("/acme/project"), "{rejected}");
    assert!(
        message.contains("\"cwd\":{\"exact\":{\"root\":\"project\"}}"),
        "{rejected}"
    );
    assert!(
        message.contains("within also matches subdirectories"),
        "{rejected}"
    );
    assert_eq!(project.revision(), revision);
    args = examples[0].clone();
    let proposed = transport.call(json!(3), "policy_propose", &args);
    assert_eq!(proposed["result"]["isError"], false, "{proposed}");
    let candidate = &proposed["result"]["structuredContent"]["data"]["after_candidate_id"];
    let nested = std::path::Path::new(&project.review.project_root).join("nested");
    std::fs::create_dir(&nested).unwrap();
    for (index, cwd, expected) in [
        (4, project.review.project_root.clone(), "allow"),
        (5, nested.to_str().unwrap().into(), "ask"),
    ] {
        let result = transport.call(
            json!(index),
            "policy_evaluate",
            &json!({
                "candidate_id":candidate,"input":{"kind":"argv","executable":"npm","arguments":["run","dev"],"original_input":{}},"context":{"cwd":cwd}}),
        );
        assert_eq!(
            result["result"]["structuredContent"]["data"]["decision"], expected,
            "{result}"
        );
    }
    let result = transport.call(
        json!(6),
        "policy_evaluate",
        &json!({"candidate_id":candidate,
            "input":{"kind":"argv","executable":"npm","arguments":["run","dev","--","--inspect"],"original_input":{}},
            "context":{"cwd":project.review.project_root}}),
    );
    assert_eq!(
        result["result"]["structuredContent"]["data"]["decision"], "ask",
        "{result}"
    );
    assert_eq!(project.revision(), revision);
    transport.close().1.unwrap();
}

#[test]
fn published_rule_examples_keep_fetch_scope() {
    let (server, project) = fixture(Some("codex"));
    let transport = Transport::start(server);
    let [start, ready] = initialize(&json!({}), "codex");
    transport.request(&start);
    transport.send(&ready);
    let revision = project.revision();
    let examples = proposal_examples(&transport);
    let proposed = transport.call(json!(7), "policy_propose", &examples[1]);
    assert_eq!(proposed["result"]["isError"], false, "{proposed}");
    let candidate = &proposed["result"]["structuredContent"]["data"]["after_candidate_id"];
    for (index, source, expected) in [
        (8, "curl -q http://localhost:3000/health", "allow"),
        (9, "curl -q -I http://127.0.0.1:8080/health", "allow"),
        (10, "curl -q -L http://localhost:3000/health", "ask"),
        (11, "curl -q -X POST http://localhost:3000/health", "ask"),
        (12, "curl -q https://example.com/", "ask"),
    ] {
        let result = transport.call(
            json!(index),
            "policy_evaluate",
            &json!({"candidate_id":candidate,
                "input":{"kind":"shell","source":source,"dialect":"bash","original_input":{}},
                "context":{"cwd":project.review.project_root}}),
        );
        assert_eq!(
            result["result"]["structuredContent"]["data"]["decision"], expected,
            "{result}"
        );
    }
    assert_eq!(project.revision(), revision);
    transport.close().1.unwrap();
}

#[test]
fn authority_claims_and_schema_mismatches_return_shield_invalid_input() {
    let (server, project) = fixture(None);
    let mut messages = initialize(&json!({}), "Acme").to_vec();
    let keys = ["project", "cwd", "config", "approved", "profile"];
    for (id, key) in keys.iter().enumerate() {
        let mut arguments = argv(&project);
        arguments[key] = json!("SECRET");
        messages.push(tool(json!(10 + id), "policy_evaluate", &arguments));
    }
    messages.extend([
        tool(json!(20), "policy_evaluate", &json!({})),
        tool(json!(21), "status", &json!({"config":"secret"})),
        json!({"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"status"}}),
        tool(
            json!(23),
            "rule_list",
            &json!({"rule_ref":{"scope":"custom","rule_id":"acme"},"limit":1}),
        ),
        tool(json!(24), "policy_propose", &json!({})),
        tool(
            json!(25),
            "policy_propose",
            &json!({
                "change":{"kind":"rule_remove","rule_id":"acme"},
                "changes":[{"kind":"rule_remove","rule_id":"acme"}]
            }),
        ),
    ]);
    let outputs = exchange(server, &messages);
    for id in (10..10 + keys.len()).chain([20, 21, 23, 24, 25]) {
        let rejected = reply(&outputs, &json!(id));
        assert_eq!(rejected["result"]["isError"], true, "{rejected}");
        assert_eq!(
            rejected["result"]["structuredContent"]["error"]["code"],
            "invalid_input"
        );
        // The reply names what to correct, never the submitted values.
        let text = rejected["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.starts_with("Tool arguments do not match the input schema: ")
                && text.ends_with(" Correct the arguments and retry."),
            "{text}"
        );
        assert!(!rejected.to_string().to_lowercase().contains("secret"));
    }
    assert_eq!(reply(&outputs, &json!(22))["result"]["isError"], false);
}

#[test]
fn nul_characters_in_tool_arguments_return_invalid_input_before_dispatch() {
    let (server, _project) = fixture(Some("codex"));
    let transport = Transport::start(server);
    for message in initialize(&json!({}), "Acme") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let rule = |reason: &str| {
        json!({"id":"acme-read","decision":"allow","reason":reason,
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}})
    };
    for (id, arguments) in [
        (
            2,
            json!({"change":{"kind":"rule_add","rule":rule("Acme")},"agent_reason":"Acme\0read"}),
        ),
        (
            3,
            json!({"change":{"kind":"rule_add","rule":rule("Acme\0read")}}),
        ),
        (
            4,
            json!({"change":{"kind":"rule_add","rule":rule("Acme")},"idem\0potency_key":"acme"}),
        ),
    ] {
        let rejected = transport.call(json!(id), "policy_propose", &arguments);
        assert_eq!(rejected["id"], id, "{rejected}");
        assert_eq!(rejected["result"]["isError"], true, "{rejected}");
        assert_eq!(
            rejected["result"]["structuredContent"]["error"]["code"], "invalid_input",
            "{rejected}"
        );
    }
    let listed = transport.call(json!(5), "policy_show", &json!({"status":"pending"}));
    assert_eq!(listed["result"]["isError"], false, "{listed}");
    assert_eq!(
        listed["result"]["structuredContent"]["data"]["proposals"],
        json!([]),
        "{listed}"
    );
}

#[test]
fn rule_schema_accepts_a_candidate_handle_with_an_exact_reference() {
    let (server, _project) = fixture(None);
    let mut messages = initialize(&json!({}), "Acme").to_vec();
    messages.push(tool(
        json!(7),
        "rule_list",
        &json!({
            "candidate_id":"candidate-unavailable",
            "rule_ref":{"scope":"custom","rule_id":"acme"}
        }),
    ));
    messages.push(tool(
        json!(8),
        "rule_list",
        &json!({
            "candidate_id":"candidate-unavailable",
            "rule_ref":{"scope":"custom","rule_id":"acme"},"limit":1
        }),
    ));
    let outputs = exchange(server, &messages);
    let result = reply(&outputs, &json!(7));
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        "candidate_unavailable"
    );
    let rejected = reply(&outputs, &json!(8));
    assert_eq!(
        rejected["result"]["structuredContent"]["error"]["code"],
        "invalid_input"
    );
}

#[test]
fn structured_errors_carry_the_same_recovery_as_the_text_summary() {
    let (server, _project) = fixture(None);
    let mut messages = initialize(&json!({}), "Acme").to_vec();
    let calls = [
        (
            "rule_list",
            json!({"candidate_id":"candidate-unavailable","rule_ref":{"scope":"custom","rule_id":"acme"}}),
            "candidate_unavailable",
            "status",
        ),
        (
            "pack_list",
            json!({"cursor":"acme-cursor"}),
            "cursor_unavailable",
            "without cursor",
        ),
        (
            "policy_show",
            json!({"proposal_id":"acme-missing"}),
            "not_found",
            "List current records",
        ),
    ];
    for (id, (name, arguments, _, _)) in calls.iter().enumerate() {
        messages.push(tool(json!(10 + id), name, arguments));
    }
    let outputs = exchange(server, &messages);
    for (id, (_, _, code, recovery)) in calls.iter().enumerate() {
        let result = &reply(&outputs, &json!(10 + id))["result"];
        assert_eq!(result["isError"], true, "{result}");
        let error = &result["structuredContent"]["error"];
        assert_eq!(error["code"], *code, "{result}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains(recovery), "{message}");
        assert_eq!(
            result["content"][0]["text"],
            format!("{}.", message.trim_end_matches('.'))
        );
    }
    let cancelled = json!({"error":crate::review::mcp::cancelled_error()});
    assert!(
        cancelled["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Inspect current state")
    );
    assert!(messages::summary("status", None, &cancelled).contains("Inspect current state"));
}

#[test]
fn proposals_put_the_adapter_activation_route_in_structured_data() {
    for (agent, route) in [
        (Some("claude-code"), "policy_apply for MCP confirmation"),
        (Some("codex"), "policy_apply for Codex approval"),
        (None, "shield policy review in their terminal"),
    ] {
        let (server, _project) = fixture(agent);
        let transport = Transport::start(server);
        for message in initialize(&json!({}), "acme-client") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        let proposal = transport.call(
            json!(2),
            "policy_propose",
            &json!({"change":{"kind":"rule_add","rule":{"id":"acme-read","decision":"allow",
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}}}),
        );
        let next = proposal["result"]["structuredContent"]["data"]["next_action"]
            .as_str()
            .unwrap();
        assert!(next.contains(route), "{next}");
        assert!(
            proposal["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .ends_with(next)
        );
    }
}

#[test]
fn frames_messages_and_output_failures_end_the_connection_within_shield_limits() {
    struct Counted {
        input: Cursor<Vec<u8>>,
        count: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Read for Counted {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let count = self.input.read(bytes)?;
            self.count
                .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
            Ok(count)
        }
    }
    impl io::BufRead for Counted {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            self.input.fill_buf()
        }
        fn consume(&mut self, count: usize) {
            self.count
                .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
            self.input.consume(count);
        }
    }
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("SECRET"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    for bytes in [vec![b' '; MAX_MESSAGE + 5], b"{}".to_vec()] {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let input = Counted {
            input: Cursor::new(bytes),
            count: count.clone(),
        };
        let mut output = Vec::new();
        assert!(serve(fixture(None).0, input, &mut output, &test_options()).is_err());
        assert!(count.load(std::sync::atomic::Ordering::Relaxed) <= MAX_MESSAGE + 1);
        assert_eq!(lines(&output)[0]["error"]["code"], -32700);
    }

    let ping = frames(&[json!({"jsonrpc":"2.0","id":1,"method":"ping"})]);
    assert!(serve(fixture(None).0, Cursor::new(ping), Broken, &test_options()).is_err());

    let limit = usize::try_from(MAX_MESSAGES).unwrap();
    let notices = b"{\"jsonrpc\":\"2.0\",\"method\":\"acme/unknown\"}\n".repeat(limit + 1);
    let mut output = Vec::new();
    assert!(
        serve(
            fixture(None).0,
            Cursor::new(notices),
            &mut output,
            &test_options()
        )
        .is_err()
    );
    let refused = lines(&output);
    assert_eq!(refused.len(), 1);
    assert_eq!(
        refused[0]["error"]["message"],
        "Session message limit reached"
    );
}

#[test]
fn an_expired_request_deadline_answers_without_a_tool_result() {
    let (server, _project) = fixture(None);
    let mut messages = initialize(&json!({}), "Acme").to_vec();
    messages.push(tool(json!(2), "status", &json!({})));
    let mut output = Vec::new();
    serve(
        server,
        Cursor::new(frames(&messages)),
        &mut output,
        &options(Duration::ZERO, None),
    )
    .unwrap();
    let expired = reply(&lines(&output), &json!(2));
    assert_eq!(expired["error"]["code"], -32001, "{expired}");
    assert!(
        expired["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("Request deadline exceeded")
    );
}
