//! Owns strict normalized host input, bounded text-read binding and gate transport.
use super::{AgentAccess, Error, handoff, read_text::Binding};
use crate::review::observations::{self, Host};
use crate::{policy, runtime::Project};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, io::Read, path::Path, process::ExitCode};

const MAX_INPUT: usize = 65_536;

/// Trusted adapter capability for this invocation, never selected by normalized input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeApproval {
    Prompt,
    Fallback,
    ExistingPrompt,
}

#[derive(Clone, Copy)]
pub(crate) struct NativeContext<'a> {
    pub integration: &'a str,
    pub approval: NativeApproval,
    pub context_complete: bool,
    pub working_files: Option<&'a crate::location::WorkingFiles>,
}
mod native_gate;

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    protocol_version: u8,
    request_id: String,
    expected_project_id: Option<String>,
    cwd: String,
    operation: Operation,
    host: Option<Host>,
    approval_request_id: Option<String>,
}

/// Native envelopes are built internally from vendor fields the adapter already
/// bounded (shell source to 64 KiB); JSON escaping may enlarge that source, so
/// their envelope bound matches the normalized input bound instead of stdin's.
const MAX_NATIVE_INPUT: usize = 262_144;

impl Input {
    fn parse(bytes: &[u8], limit: usize) -> Option<Self> {
        if bytes.len() > limit {
            return None;
        }
        let input: Self = serde_json::from_slice(bytes).ok()?;
        (input.protocol_version == 1
            && !input.request_id.is_empty()
            && input.request_id.len() <= 128
            && !input.request_id.contains('\0')
            && input.cwd.len() <= 4096
            && !input.cwd.contains('\0'))
        .then_some(input)
        .filter(|i| {
            i.host.as_ref().is_none_or(Host::valid)
                && i.operation.valid_window()
                && i.approval_request_id
                    .as_ref()
                    .is_none_or(|s| !s.is_empty() && s.len() <= 128 && !s.contains('\0'))
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Operation {
    Mcp {
        alias: String,
        tool: String,
        arguments: Value,
    },
    Register,
    ReadText {
        path: String,
        offset: Option<u64>,
        limit: Option<u64>,
    },
    Unsupported {
        name: String,
    },
    /// A host file tool's typed effect: a write to one path, or a read of a file
    /// or of the existing entries below a directory.
    File {
        action: FileAction,
        path: String,
        recursive: bool,
        /// How a recursive read selects the entries below `path`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        search: Option<Search>,
    },
    Shell {
        source: String,
        dialect: crate::shell::Dialect,
    },
    /// A host web tool: a search for a query, or a GET of one URL.
    Web {
        action: WebAction,
        query: Option<String>,
        url: Option<String>,
    },
    Completion {
        delivery_id: String,
        request_id: Option<String>,
        status: String,
        operation: Box<Operation>,
    },
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FileAction {
    Read,
    Write,
    Remove,
}
/// A recursive read's selection: a names-only listing or a content search, and
/// the globs and file types that choose its entries. The native gate checks it
/// as the ripgrep walk it performs.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Search {
    names_only: bool,
    globs: Vec<String>,
    types: Vec<String>,
}
impl Search {
    fn valid(&self) -> bool {
        self.globs.len() <= 16
            && self.types.len() <= 16
            && self
                .globs
                .iter()
                .chain(&self.types)
                .all(|s| !s.is_empty() && s.len() <= 4096 && !s.contains('\0'))
    }
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WebAction {
    Search,
    Fetch,
}
impl Operation {
    fn valid_window(&self) -> bool {
        match self {
            Self::Mcp {
                alias,
                tool,
                arguments,
            } => {
                crate::mcp_connections::valid_name(alias)
                    && !tool.is_empty()
                    && tool.len() <= 128
                    && arguments.is_object()
            }
            Self::ReadText { offset, limit, .. } => [offset, limit]
                .into_iter()
                .all(|n| n.is_none_or(|n| n > 0 && n <= 1_000_000)),
            Self::File {
                path,
                recursive,
                search,
                ..
            } => {
                Path::new(path).is_absolute()
                    && path.len() <= 4096
                    && !path.contains('\0')
                    && search.as_ref().is_none_or(|s| *recursive && s.valid())
            }
            Self::Web { action, query, url } => {
                let bounded = |s: &String| !s.is_empty() && s.len() <= 8192 && !s.contains('\0');
                match action {
                    WebAction::Search => query.as_ref().is_some_and(bounded) && url.is_none(),
                    WebAction::Fetch => url.as_ref().is_some_and(bounded) && query.is_none(),
                }
            }
            _ => true,
        }
    }
}

pub(crate) fn run(root: &Path) -> ExitCode {
    let mut bytes = Vec::new();
    let result = std::io::stdin()
        .lock()
        .take((MAX_INPUT + 1) as u64)
        .read_to_end(&mut bytes);
    let (value, code) = if result.is_ok() {
        evaluate(root, &bytes)
    } else {
        (failure(None, "input-read"), 2)
    };
    println!("{value}");
    ExitCode::from(code)
}

pub(crate) fn evaluate(root: &Path, bytes: &[u8]) -> (Value, u8) {
    evaluate_with_original(root, bytes, None, None, None, &|_| {})
}

/// One project-bound snapshot shared by host classification and evaluation.
pub(crate) struct Prepared {
    project: Project,
    snapshot: crate::runtime::Snapshot,
}

impl Prepared {
    pub(crate) fn load(
        root: &Path,
        stage: &dyn Fn(&'static str),
    ) -> Result<Self, crate::runtime::Error> {
        stage("snapshot_load_start");
        let project = Project::open(root)?;
        let snapshot = project.snapshot()?;
        stage("snapshot_load_ready");
        Ok(Self { project, snapshot })
    }

    pub(crate) fn snapshot(&self) -> &crate::runtime::Snapshot {
        &self.snapshot
    }
}

/// Native hook input was bounded and parsed by the vendor boundary. Retain its
/// arguments without duplicating shell source in the bounded normalized envelope.
pub(crate) fn evaluate_native(
    root: &Path,
    bytes: &[u8],
    original: &Value,
    native: NativeContext<'_>,
    prepared: Option<Prepared>,
    stage: &dyn Fn(&'static str),
) -> (Value, u8) {
    evaluate_with_original(root, bytes, Some(original), Some(native), prepared, stage)
}

fn evaluate_with_original(
    root: &Path,
    bytes: &[u8],
    original: Option<&Value>,
    native: Option<NativeContext<'_>>,
    prepared: Option<Prepared>,
    stage: &dyn Fn(&'static str),
) -> (Value, u8) {
    let limit = if native.is_some() {
        MAX_NATIVE_INPUT
    } else {
        MAX_INPUT
    };
    let input = Input::parse(bytes, limit);
    stage("policy_evaluation_start");
    let (mut value, mut code) = match &input {
        None => (failure(None, "invalid-input"), 2),
        Some(parsed) => completion_response(parsed)
            .unwrap_or_else(|| evaluate_inner(root, parsed, original, native, prepared, stage)),
    };
    stage("policy_evaluation_ready");
    if let (Some(agent), Some(input)) = (native, input.as_ref())
        && !matches!(input.operation, Operation::Completion { .. })
        && code == 0
        && let Err(reason) = native_gate::resolve(root, agent, &mut value)
    {
        value = failure(Some(&input.request_id), reason);
        code = 2;
    }
    stage("native_handoff_ready");
    if let Some(fields) = value.as_object_mut() {
        fields.remove("_shell_analysis");
        fields.remove("_codex_covered");
        fields.remove("_file_targets");
        fields.remove("_file_access");
        fields.remove("_file_search");
    }
    let observed = value
        .as_object_mut()
        .and_then(|v| v.remove("_observation"))
        .unwrap_or(Value::Null);
    let explanation = observed["evaluation"]
        .get("explanation")
        .unwrap_or(&observed["evaluation"]["evaluation"]);
    value["diagnostics"] = crate::agent_message::diagnostics(explanation);
    crate::evidence::redact(&mut value["diagnostics"]);
    // Finalize analyzer rewrite refusals before publication, so history records
    // the same blocked handoff the native feedback delivers to the host.
    if native.is_some()
        && value["disposition"] != "allowed"
        && matches!(
            value["code"].as_str(),
            Some("approval-required" | "policy-deny")
        )
        && (value["code"] == "policy-deny"
            || native.is_some_and(|context| context.approval != NativeApproval::ExistingPrompt))
        && crate::agent_message::rewrite_detail(&value["diagnostics"]).is_some()
    {
        value["decision"] = json!("deny");
        value["code"] = json!("request-rewrite");
        value["disposition"] = json!("blocked");
    }
    if value.get("approval").is_none() {
        value["approval"] = json!({"request_id":null,"status":"none","approval_id":null});
    }
    stage("history_start");
    value["history"] = match input {
        Some(input) if input.host.is_some() && !matches!(input.operation, Operation::Register) => {
            let result = if matches!(input.operation, Operation::Completion { .. }) && code != 0 {
                Err(crate::review::Error::Invalid("invalid completion"))
            } else {
                record(root, &input, &value, &observed, original)
            };
            result.unwrap_or_else(|_| json!({"status":"failed","request_id":null,"event_id":null}))
        }
        _ => json!({"status":"not-requested","request_id":null,"event_id":null}),
    };
    stage("history_ready");
    (value, code)
}

fn record(
    root: &Path,
    input: &Input,
    value: &Value,
    observed: &Value,
    original: Option<&Value>,
) -> Result<Value, crate::review::Error> {
    let id = input
        .expected_project_id
        .as_deref()
        .ok_or(crate::review::Error::Invalid(
            "observation registration missing",
        ))?;
    let host = input.host.as_ref().expect("caller checked host");
    let context = observations::context(root, id, &input.cwd, host)?;
    if let Operation::Completion {
        delivery_id,
        request_id,
        status,
        operation,
    } = &input.operation
    {
        return observations::completion(
            &context,
            &crate::review::EventInput {
                delivery_id: delivery_id.clone(),
                request_id: request_id.clone(),
                status: status.clone(),
            },
            &json!(operation),
        );
    }
    let policy = observed["evaluation"]["decision"].clone();
    let host_decision = match value["disposition"].as_str() {
        Some("allowed") => json!("allow"),
        Some("blocked") => json!("deny"),
        Some("approval_required") => json!("ask"),
        _ => value["decision"].clone(),
    };
    observations::decision(
        &context,
        json!({"operation":input.operation,"original_input":original,"request":observed["request"],
        "evaluation":observed["evaluation"],"policy_revision":value["identity"]["policy_revision"],
        "activation_id":value["identity"]["activation_id"],"candidate_digest":value["identity"]["candidate_digest"],
        "decision":policy,"host_decision":host_decision,"disposition":value["disposition"],"ask_fallback":value["ask_fallback"],"integration":value["integration"],"host_context_complete":value["host_context_complete"],"code":value["code"],"approval":value["approval"],"execution":"unobserved"}),
    )
}

fn completion_response(input: &Input) -> Option<(Value, u8)> {
    let Operation::Completion {
        status, operation, ..
    } = &input.operation
    else {
        return None;
    };
    let valid_read = match operation.as_ref() {
        Operation::ReadText {
            path,
            offset,
            limit,
        } => {
            Path::new(path).is_absolute()
                && path.len() <= 4096
                && !path.contains('\0')
                && [offset, limit]
                    .into_iter()
                    .all(|n| n.is_none_or(|n| n > 0 && n <= 1_000_000))
        }
        Operation::Shell { source, .. } => {
            !source.is_empty() && source.len() <= MAX_INPUT && !source.contains('\0')
        }
        file @ Operation::File { .. } => file.valid_window(),
        web @ Operation::Web { .. } => web.valid_window(),
        mcp @ Operation::Mcp { .. } => mcp.valid_window(),
        _ => false,
    };
    let valid_event = input.host.as_ref().is_some_and(|h| match status.as_str() {
        "completed" | "unknown" => h.event == "PostToolUse",
        "failed" | "cancelled" => h.event == "PostToolUseFailure",
        _ => false,
    });
    Some(if valid_read && valid_event {
        (
            json!({"protocol_version":1,"request_id":input.request_id,"decision":"deny",
            "code":"completion-observed","identity":null,"operation":null}),
            0,
        )
    } else {
        (failure(Some(&input.request_id), "invalid-input"), 2)
    })
}

fn evaluate_inner(
    root: &Path,
    input: &Input,
    original: Option<&Value>,
    native: Option<NativeContext<'_>>,
    prepared: Option<Prepared>,
    stage: &dyn Fn(&'static str),
) -> (Value, u8) {
    let working_files = native.and_then(|n| n.working_files);
    let id = Some(input.request_id.as_str());
    let Prepared {
        project,
        mut snapshot,
    } = match prepared.map_or_else(|| Prepared::load(root, stage), Ok) {
        Ok(prepared) => prepared,
        Err(error) => return (failure(id, runtime_failure(&error)), 2),
    };
    snapshot.observe_working_files(working_files);
    let identity = snapshot.identity();
    if !matches!(input.operation, Operation::Register)
        && input.expected_project_id.as_deref() != Some(identity.project_id())
    {
        return (failure(id, "project-changed"), 2);
    }
    let metadata = json!(identity);
    let response = |code: &str, decision: &str, operation: Option<&Operation>| {
        json!({"protocol_version":1,"request_id":input.request_id,"decision":decision,
            "code":code,"identity":metadata,"operation":operation})
    };
    let root = identity.project_root();
    if !Path::new(&input.cwd).is_absolute() {
        return (failure(id, "invalid-context"), 2);
    }
    let Ok(cwd) = fs::canonicalize(&input.cwd) else {
        return (failure(id, "invalid-context"), 2);
    };
    if !cwd.is_dir() || !cwd.starts_with(root) || cwd.to_str().is_none() {
        return (failure(id, "invalid-context"), 2);
    }
    let operation = match &input.operation {
        Operation::Register => return (response("registration", "deny", None), 0),
        Operation::Unsupported { .. } => {
            return (response("unsupported-operation", "deny", None), 0);
        }
        // Shell, file and web decisions rely on the native gate's protected-path
        // and operator-access checks; without native context they fail closed.
        Operation::Shell { .. }
        | Operation::File { .. }
        | Operation::Web { .. }
        | Operation::Mcp { .. }
            if native.is_none() =>
        {
            return (response("unsupported-operation", "deny", None), 0);
        }
        Operation::Completion { .. } => unreachable!(),
        Operation::Mcp { .. } => {
            return evaluate_bound_mcp(
                input,
                &project,
                &snapshot,
                &cwd,
                native.expect("native MCP context checked"),
                original,
            );
        }
        Operation::Shell { source, dialect } => {
            return evaluate_shell(input, &project, &snapshot, &cwd, source, *dialect, original);
        }
        Operation::File {
            action,
            path,
            recursive,
            ..
        } => {
            return evaluate_file(
                input, &project, &snapshot, &cwd, *action, path, *recursive, original,
            );
        }
        Operation::Web { action, query, url } => {
            return evaluate_web(
                input,
                &project,
                &snapshot,
                &cwd,
                *action,
                query.as_deref().or(url.as_deref()).unwrap_or_default(),
                original,
            );
        }
        Operation::ReadText {
            path,
            offset,
            limit,
        } => Operation::ReadText {
            path: path.clone(),
            offset: *offset,
            limit: *limit,
        },
    };
    evaluate_read(input, &project, &snapshot, &cwd, &operation, original)
}

fn evaluate_shell(
    input: &Input,
    project: &Project,
    snapshot: &crate::runtime::Snapshot,
    cwd: &Path,
    source: &str,
    dialect: crate::shell::Dialect,
    original: Option<&Value>,
) -> (Value, u8) {
    let Ok(analysis) = snapshot.analyze_shell(
        source,
        dialect,
        cwd.to_str().expect("validated cwd"),
        original,
    ) else {
        return (failure(Some(&input.request_id), "invalid-input"), 2);
    };
    // Decide the submitted command; revalidate the active generation before
    // handing the result to the host. The evidence carries the shell source as
    // the same normalized request the analysis resolved, so its bound is the
    // shell source bound rather than the per-argument argv bound.
    let request = policy::Request {
        executable: "shield-unclassified-request".into(),
        arguments: vec![],
        cwd: cwd.to_str().expect("validated cwd").into(),
        project: snapshot
            .identity()
            .project_root()
            .to_str()
            .expect("UTF-8 root")
            .into(),
    };
    let resource = crate::review::runtime::consumer::ResourceRequest {
        input: policy::normalized::Input::Shell {
            source: source.into(),
            dialect: json!(dialect).as_str().expect("dialect string").into(),
            dialect_version: None,
            original_input: original.cloned().unwrap_or_else(|| json!({})),
        },
        context: policy::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let Ok(evidence) = snapshot.evaluate_resource(request, resource) else {
        return (failure(Some(&input.request_id), "invalid-input"), 2);
    };
    let handed = project.with_current(&evidence, |_, _| {
        if snapshot.time_sensitive() {
            snapshot.analyze_shell(
                source,
                dialect,
                cwd.to_str().expect("validated cwd"),
                original,
            )
        } else {
            Ok(analysis)
        }
    });
    let analysis = match handed {
        Ok(Ok(analysis)) => analysis,
        Err(crate::runtime::Error::Locked) => {
            return (failure(Some(&input.request_id), "runtime-busy"), 2);
        }
        _ => return (failure(Some(&input.request_id), "handoff-failed"), 2),
    };
    let (decision, code) = match analysis["decision"].as_str() {
        Some("allow") => ("allow", "policy-allow"),
        Some("deny") => ("deny", "policy-deny"),
        Some("ask") => ("ask", "approval-required"),
        _ => return (failure(Some(&input.request_id), "invalid-decision"), 2),
    };
    // Codex runs a command its Shield prefix rule covers without asking. A
    // prefix rules file that differs from this policy may cover any command.
    let codex_covered = decision == "ask"
        && (snapshot.codex_covers(&analysis["analysis_coverage"]["operations"])
            || crate::review::integrations::prefix_rules_drifted(
                snapshot.identity().project_root(),
                snapshot.codex_prefixes(),
            ));
    (
        json!({"protocol_version":1,"request_id":input.request_id,"decision":decision,"code":code,
        "identity":snapshot.identity(),"operation":null,"_shell_analysis":analysis["analysis_coverage"],
        "_codex_covered":codex_covered,
        "_observation":{"request":evidence.request(),"evaluation":{"decision":analysis["decision"],"coverage":analysis["analysis_coverage"]["coverage"],"explanation":analysis["explanation"],"policy_revision":snapshot.identity().policy_revision()}}}),
        0,
    )
}

// A host file tool is decided as the typed file request it performs; location
// rules check the path against approved roots and exclusions.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the shell and read entries"
)]
fn evaluate_file(
    input: &Input,
    project: &Project,
    snapshot: &crate::runtime::Snapshot,
    cwd: &Path,
    action: FileAction,
    path: &str,
    recursive: bool,
    original: Option<&Value>,
) -> (Value, u8) {
    let identity = snapshot.identity();
    let id = Some(input.request_id.as_str());
    let (name, sources, destination) = match action {
        FileAction::Read => ("read", vec![path.to_owned()], None),
        FileAction::Write => ("write", vec![], Some(path.to_owned())),
        FileAction::Remove => ("remove", vec![path.to_owned()], None),
    };
    let request = policy::Request {
        executable: format!("shield:file-{name}"),
        arguments: vec![path.to_owned()],
        cwd: cwd.to_str().expect("validated cwd").into(),
        project: identity
            .project_root()
            .to_str()
            .expect("UTF-8 registered root")
            .into(),
    };
    let resource = crate::review::runtime::consumer::ResourceRequest {
        input: policy::normalized::Input::File {
            action: name.into(),
            sources,
            destination,
            recursive,
            original_input: original.cloned().unwrap_or_else(|| json!(input.operation)),
        },
        context: policy::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let Ok(evidence) = snapshot.evaluate_resource(request, resource) else {
        return (failure(id, "invalid-input"), 2);
    };
    let observation = json!({"request":evidence.request(),"evaluation":evidence.evaluation()});
    let (decision, code) = match handoff(project, AgentAccess::Ordinary, evidence, |_, _| ()) {
        Ok(()) => ("allow", "policy-allow"),
        Err(Error::Denied) => ("deny", "policy-deny"),
        Err(Error::ApprovalRequired) => ("ask", "approval-required"),
        Err(error) => return (failure(id, handoff_failure(&error)), 2),
    };
    let mut value = json!({"protocol_version":1,"request_id":input.request_id,"decision":decision,
        "code":code,"identity":identity,"operation":null,"_file_targets":[path],"_file_access":name});
    // A recursive read walks the tree below its path; without a stated
    // selection it is checked as a content search of every entry.
    if recursive {
        value["_file_search"] = match &input.operation {
            Operation::File {
                search: Some(search),
                ..
            } => json!(search),
            _ => json!({"names_only":false,"globs":[],"types":[]}),
        };
    }
    value["_observation"] = observation;
    (value, 0)
}

// A host web tool is decided as the typed web request it performs; web rules
// match its action and, for a fetch, the URL.
fn evaluate_web(
    input: &Input,
    project: &Project,
    snapshot: &crate::runtime::Snapshot,
    cwd: &Path,
    action: WebAction,
    target: &str,
    original: Option<&Value>,
) -> (Value, u8) {
    let identity = snapshot.identity();
    let id = Some(input.request_id.as_str());
    let (name, query, url, method) = match action {
        WebAction::Search => ("search", Some(target.to_owned()), None, None),
        WebAction::Fetch => (
            "fetch",
            None,
            Some(target.to_owned()),
            Some("GET".to_owned()),
        ),
    };
    let request = policy::Request {
        executable: format!("shield:web-{name}"),
        arguments: vec![target.to_owned()],
        cwd: cwd.to_str().expect("validated cwd").into(),
        project: identity
            .project_root()
            .to_str()
            .expect("UTF-8 registered root")
            .into(),
    };
    let resource = crate::review::runtime::consumer::ResourceRequest {
        input: policy::normalized::Input::Web {
            action: name.into(),
            query,
            url,
            // A fetch names its client so a rule can tell host tools from shell downloads.
            conditions: if name == "fetch" {
                json!({"client":"agent"})
            } else {
                json!({})
            },
            method,
            original_input: original.cloned().unwrap_or_else(|| json!(input.operation)),
        },
        context: policy::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let Ok(evidence) = snapshot.evaluate_resource(request, resource) else {
        return (failure(id, "invalid-input"), 2);
    };
    let observation = json!({"request":evidence.request(),"evaluation":evidence.evaluation()});
    let (decision, code) = match handoff(project, AgentAccess::Ordinary, evidence, |_, _| ()) {
        Ok(()) => ("allow", "policy-allow"),
        Err(Error::Denied) => ("deny", "policy-deny"),
        Err(Error::ApprovalRequired) => ("ask", "approval-required"),
        Err(error) => return (failure(id, handoff_failure(&error)), 2),
    };
    let mut value = json!({"protocol_version":1,"request_id":input.request_id,"decision":decision,
        "code":code,"identity":identity,"operation":null});
    value["_observation"] = observation;
    (value, 0)
}

fn evaluate_bound_mcp(
    input: &Input,
    project: &Project,
    snapshot: &crate::runtime::Snapshot,
    cwd: &Path,
    native: NativeContext<'_>,
    original: Option<&Value>,
) -> (Value, u8) {
    let Operation::Mcp {
        alias,
        tool,
        arguments,
    } = &input.operation
    else {
        unreachable!()
    };
    let root = snapshot.identity().project_root();
    let id = Some(input.request_id.as_str());
    let agent = native.integration;
    let Some((server, connection)) = snapshot.mcp_connection(agent, alias) else {
        return (failure(id, "unsupported-operation"), 0);
    };
    let projected =
        crate::review::integrations::mcp_projection(root, agent, server, connection).ok();
    if projected.as_ref().is_none_or(|transport| {
        crate::review::integrations::verify_mcp_connection_at(root, cwd, agent, alias, transport)
            .is_err()
    }) {
        return (failure(id, "mcp-binding-changed"), 2);
    }
    evaluate_mcp_policy(
        input,
        project,
        snapshot,
        cwd,
        (server, tool, arguments),
        original,
    )
}

fn evaluate_mcp_policy(
    input: &Input,
    project: &Project,
    snapshot: &crate::runtime::Snapshot,
    cwd: &Path,
    (server, tool, arguments): (&str, &str, &Value),
    original: Option<&Value>,
) -> (Value, u8) {
    let request = policy::Request {
        executable: "shield:mcp".into(),
        arguments: vec![],
        cwd: cwd.to_str().expect("validated cwd").into(),
        project: snapshot
            .identity()
            .project_root()
            .to_str()
            .expect("registered UTF-8 root")
            .into(),
    };
    let resource = crate::review::runtime::consumer::ResourceRequest {
        input: policy::normalized::Input::Mcp {
            server: server.into(),
            tool: tool.into(),
            arguments: arguments.clone(),
            connection: None,
            original_input: original.cloned().unwrap_or_else(|| json!(input.operation)),
        },
        context: policy::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let Ok(evidence) = snapshot.evaluate_resource(request, resource) else {
        return (failure(Some(&input.request_id), "invalid-input"), 2);
    };
    let observation = json!({"request":evidence.request(),"evaluation":evidence.evaluation()});
    let (decision, code) = match handoff(project, AgentAccess::Ordinary, evidence, |_, _| ()) {
        Ok(()) => ("allow", "policy-allow"),
        Err(Error::Denied) => ("deny", "policy-deny"),
        Err(Error::ApprovalRequired) => ("ask", "approval-required"),
        Err(error) => return (failure(Some(&input.request_id), handoff_failure(&error)), 2),
    };
    (
        json!({"protocol_version":1,"request_id":input.request_id,"decision":decision,"code":code,
        "identity":snapshot.identity(),"operation":null,"_observation":observation}),
        0,
    )
}

fn evaluate_read(
    input: &Input,
    project: &Project,
    snapshot: &crate::runtime::Snapshot,
    cwd: &Path,
    operation: &Operation,
    original: Option<&Value>,
) -> (Value, u8) {
    let identity = snapshot.identity();
    let root = identity.project_root();
    let id = Some(input.request_id.as_str());
    let response = |code: &str, decision: &str, operation: Option<&Operation>| {
        json!({"protocol_version":1,"request_id":input.request_id,"decision":decision,
            "code":code,"identity":identity,"operation":operation})
    };
    let Operation::ReadText {
        path,
        offset,
        limit,
    } = &operation
    else {
        unreachable!()
    };
    let Ok(binding) = Binding::open_scoped(root, cwd, path, snapshot.access_paths()) else {
        return (response("unverified-read", "deny", None), 0);
    };
    let canonical = binding
        .path
        .to_str()
        .expect("validated UTF-8 path")
        .to_owned();
    let operation = Operation::ReadText {
        path: canonical.clone(),
        offset: *offset,
        limit: *limit,
    };
    let mut arguments = vec![canonical];
    if let Some(n) = offset {
        arguments.extend(["--offset".into(), n.to_string()]);
    }
    if let Some(n) = limit {
        arguments.extend(["--limit".into(), n.to_string()]);
    }
    let request = policy::Request {
        executable: "shield:read-text".into(),
        arguments,
        cwd: cwd.to_str().expect("UTF-8 cwd checked by evaluator").into(),
        project: root.to_str().expect("UTF-8 registered root").into(),
    };
    let resource = read_resource(&binding.path, &request.cwd, original, &input.operation);
    let Ok(evidence) = snapshot.evaluate_resource(request, resource) else {
        return (failure(id, "invalid-input"), 2);
    };
    let observation = json!({"request":evidence.request(),"evaluation":evidence.evaluation()});
    let (mut result, code) = if let Some(host) = &input.host
        && let Some(consumer) = &host.consumer_id
    {
        let outcome = observations::context(
            identity.project_root(),
            identity.project_id(),
            &input.cwd,
            host,
        )
        .and_then(|context| {
            crate::review::read_approval::gate(
                &context,
                consumer,
                &evidence,
                &binding.digest,
                input.approval_request_id.as_deref(),
                || binding.verify().is_ok(),
            )
        });
        match outcome {
            Ok(gate) => {
                let allowed = gate["allowed"] == true;
                let mut value = response(
                    gate["code"].as_str().unwrap_or("handoff-failed"),
                    if allowed { "allow" } else { "deny" },
                    if allowed { Some(&operation) } else { None },
                );
                value["approval"] = gate["approval"].clone();
                (value, 0)
            }
            Err(crate::review::Error::Locked) => (failure(id, "runtime-busy"), 2),
            Err(_) => (failure(id, "handoff-failed"), 2),
        }
    } else {
        match handoff(project, AgentAccess::Ordinary, evidence, |_, _| {
            binding.verify()
        }) {
            Ok(Ok(())) => (response("allowed", "allow", Some(&operation)), 0),
            Ok(Err(())) => (response("read-changed", "deny", None), 0),
            Err(Error::Denied) => (response("policy-deny", "deny", None), 0),
            Err(Error::ApprovalRequired) => (response("approval-required", "deny", None), 0),
            Err(error) => (failure(id, handoff_failure(&error)), 2),
        }
    };
    result["_observation"] = observation;
    (result, code)
}

fn read_resource(
    path: &Path,
    cwd: &str,
    original: Option<&Value>,
    operation: &Operation,
) -> crate::review::runtime::consumer::ResourceRequest {
    crate::review::runtime::consumer::ResourceRequest {
        input: policy::normalized::Input::File {
            action: "read".into(),
            sources: vec![path.to_str().expect("UTF-8 path").into()],
            destination: None,
            recursive: false,
            original_input: original.cloned().unwrap_or_else(|| json!(operation)),
        },
        context: policy::normalized::Context {
            cwd: cwd.into(),
            environment: None,
            working_files: None,
        },
    }
}

/// A runtime failure's receipt code. A lock held by another writer is brief,
/// so its code tells the consumer that retrying can succeed.
fn runtime_failure(error: &crate::runtime::Error) -> &'static str {
    match error {
        crate::runtime::Error::Locked => "runtime-busy",
        crate::runtime::Error::CacheUnavailable => "compiled-cache-unavailable",
        _ => "runtime-unavailable",
    }
}

/// A failed handoff's receipt code; a briefly held lock can be retried.
fn handoff_failure(error: &Error) -> &'static str {
    match error {
        Error::Runtime(crate::runtime::Error::Locked) => "runtime-busy",
        _ => "handoff-failed",
    }
}

fn failure(id: Option<&str>, code: &str) -> Value {
    json!({"protocol_version":1,"request_id":id,"decision":"deny","code":code,"identity":null,"operation":null,
        "history":{"status":"not-requested","request_id":null,"event_id":null}})
}
