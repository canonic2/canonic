//! Non-authorizing normalized evaluations and immutable before/after comparisons.
use super::{
    Checkpoint, Context, Error, fields, required, text,
    views::{Captured, Views},
};
use crate::{
    configuration,
    policy::{
        self,
        resolution::normalized::{Context as RequestContext, Input},
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// Only this module can construct an already-redacted diagnostic result.
pub(super) struct Projected {
    value: Value,
    redacted: bool,
}
impl Projected {
    fn encode_sources<T: Serialize + ?Sized>(
        value: &T,
        checkpoint: Checkpoint<'_>,
        sources: &[&str],
    ) -> Result<Self, Error> {
        let interrupted = std::cell::RefCell::new(None);
        let result = crate::evidence::projection::encode_controlled_with_sources(
            value,
            4 * 1024 * 1024,
            &|| {
                checkpoint().map_err(|error| {
                    *interrupted.borrow_mut() = Some(error);
                    <serde_json::Error as serde::ser::Error>::custom(
                        "diagnostic encoding interrupted",
                    )
                })
            },
            sources,
        );
        if let Some(error) = interrupted.into_inner() {
            return Err(error.into());
        }
        let (value, redacted) = result.map_err(|_| Error::Policy(policy::Error::Limit))?;
        Ok(Self { value, redacted })
    }
    pub(super) fn into_parts(self) -> (Value, bool) {
        (self.value, self.redacted)
    }
}

pub(super) fn evaluate(
    context: &Context,
    views: &mut Views,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Projected, Error> {
    #[derive(Serialize)]
    struct Report<'a> {
        #[serde(flatten)]
        evaluation: configuration::NormalizedEvaluation,
        #[serde(skip_serializing_if = "Option::is_none")]
        mcp_match: Option<Value>,
        candidate_id: &'a str,
        policy_revision: &'a str,
    }
    fields(args, &["input", "context", "candidate_id"])?;
    let (input, request_context) = request(context, args)?;
    let id = match text(args, "candidate_id")? {
        Some(id) => id.into(),
        None => views.active(context)?,
    };
    let captured = views.prepare(context, &id)?;
    let (evaluation, mcp_match) =
        evaluate_captured(context, &captured, &input, &request_context, checkpoint)?;
    checkpoint()?;
    let report = Report {
        evaluation,
        mcp_match,
        candidate_id: &id,
        policy_revision: captured.evaluator()?.revision(),
    };
    let projected =
        Projected::encode_sources(&report, checkpoint, &report.evaluation.sensitive_sources())?;
    checkpoint()?;
    Ok(projected)
}
pub(super) fn compare(
    context: &Context,
    views: &mut Views,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Projected, Error> {
    fields(
        args,
        &[
            "before_candidate_id",
            "after_candidate_id",
            "domain",
            "input",
            "context",
        ],
    )?;
    let before_id = required(args, "before_candidate_id")?;
    let after_id = required(args, "after_candidate_id")?;
    let before = views.get(before_id)?;
    let after = views.get(after_id)?;
    checkpoint()?;
    if required(args, "domain")? != "sample"
        && (args.get("input").is_some() || args.get("context").is_some())
    {
        return Err(Error::Invalid("only sample accepts input/context"));
    }
    let mut value = match required(args, "domain")? {
        "sample" => sample_compare(context, views, args, before_id, after_id, checkpoint)?,
        "literal_argv" => {
            context
                .progress
                .stage(crate::review::progress::Stage::ComparePolicy);
            json!(configuration::compare_policy(
                &snapshots(&before)?,
                &snapshots(&after)?
            )?)
        }
        "supported_semantic" => {
            json!({"domain":"supported_semantic","complete":false,"relation":"unknown","coverage":"no complete semantic comparison model is implemented"})
        }
        _ => return Err(Error::Invalid("invalid comparison domain")),
    };
    if crate::review::model::redact_comparison(&mut value) {
        value["redacted"] = json!(true);
    }
    checkpoint()?;
    value["before_candidate_id"] = json!(before_id);
    value["after_candidate_id"] = json!(after_id);
    value["domain"] = args["domain"].clone();
    value["engine_version"] = json!(env!("CARGO_PKG_VERSION"));
    value["model"] = json!("normalized-request-v1");
    let redacted = if args["domain"] == "sample" {
        value["redacted"] == true
    } else {
        crate::review::model::redact(&mut value) || value["redacted"] == true
    };
    Ok(Projected { value, redacted })
}
fn sample_compare(
    context: &Context,
    views: &mut Views,
    args: &Value,
    before_id: &str,
    after_id: &str,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    #[derive(Serialize)]
    struct Witness<'a> {
        input: &'a Input,
        relation: &'static str,
        before: configuration::NormalizedEvaluation,
        after: configuration::NormalizedEvaluation,
    }
    #[derive(Serialize)]
    struct Sample<'a> {
        domain: &'static str,
        complete: bool,
        relation: &'static str,
        coverage: &'static str,
        context: &'a RequestContext,
        witnesses: [Witness<'a>; 1],
    }

    let (input, ctx) = request(context, args)?;
    let before = views.prepare(context, before_id)?;
    let after = views.prepare(context, after_id)?;
    context
        .progress
        .stage(crate::review::progress::Stage::ComparePolicy);
    let (a, _) = evaluate_captured(context, &before, &input, &ctx, checkpoint)?;
    let (b, _) = evaluate_captured(context, &after, &input, &ctx, checkpoint)?;
    let relation = match b.decision.cmp(&a.decision) {
        std::cmp::Ordering::Equal => "equivalent",
        std::cmp::Ordering::Greater => "restrictive",
        std::cmp::Ordering::Less => "expansive",
    };
    let sample = Sample {
        domain: "sample",
        complete: false,
        relation: "unknown",
        coverage: "one request witness; no whole-policy proof",
        context: &ctx,
        witnesses: [Witness {
            input: &input,
            relation,
            before: a,
            after: b,
        }],
    };
    let mut sources = sample.witnesses[0].before.sensitive_sources();
    sources.extend(sample.witnesses[0].after.sensitive_sources());
    sources.sort_unstable();
    sources.dedup();
    let projected = Projected::encode_sources(&sample, checkpoint, &sources)?;
    let (mut value, redacted) = projected.into_parts();
    if redacted {
        value["redacted"] = json!(true);
    }
    Ok(value)
}

fn snapshots(captured: &Captured) -> Result<Vec<(policy::Scope, configuration::Snapshot)>, Error> {
    crate::review::snapshots(&BTreeMap::from([(
        "project".into(),
        captured.candidate.clone(),
    )]))
}
fn request(context: &Context, args: &Value) -> Result<(Input, RequestContext), Error> {
    let input: Input = serde_json::from_value(args["input"].clone())
        .map_err(|_| Error::Invalid("invalid normalized input"))?;
    let mut ctx: RequestContext = serde_json::from_value(args["context"].clone())
        .map_err(|_| Error::Invalid("invalid request context"))?;
    input.validate(&ctx, &context.project_root)?;
    let cwd = Path::new(&ctx.cwd).canonicalize()?;
    if !cwd.is_dir() || !cwd.starts_with(&context.project_root) {
        return Err(Error::Invalid(
            "cwd must be a directory inside registered project",
        ));
    }
    ctx.cwd = cwd.to_string_lossy().into_owned();
    Ok((input, ctx))
}
fn evaluate_captured(
    context: &Context,
    captured: &Captured,
    input: &Input,
    ctx: &RequestContext,
    checkpoint: Checkpoint<'_>,
) -> Result<(configuration::NormalizedEvaluation, Option<Value>), Error> {
    let evaluator = captured.evaluator()?;
    let executable = resolve(input, ctx);
    let host = host_mcp(context, evaluator, input, ctx)?;
    let mut value = evaluator.evaluate_normalized_report_mcp(
        host.as_ref().map_or(input, |(bound, _, _)| bound),
        ctx,
        &context.project_root,
        executable.as_deref(),
        &crate::review::model::now(),
        host.as_ref().map(|(_, resolved, _)| resolved),
        checkpoint,
    )?;
    value.annotate_rule_references(&selectors(&captured.metadata));
    let matched = host.map(|(_, resolved, alias)| {
        json!({"alias":alias,"source":resolved.source,"matches":resolved.matches,
            "effective_connection_identity":"unverified"})
    });
    Ok((value, matched))
}

/// Match an MCP server the calling agent configured to installed pack
/// integrations, as the agent's hook does. Project-managed connection IDs and
/// unmatched servers keep the plain evaluation.
fn host_mcp(
    context: &Context,
    evaluator: &configuration::Evaluator,
    input: &Input,
    ctx: &RequestContext,
) -> Result<Option<(Input, policy::resolution::compiled::ResolvedMcp, String)>, Error> {
    let Input::Mcp { server, .. } = input else {
        return Ok(None);
    };
    let Some(agent) = context.agent.as_deref() else {
        return Ok(None);
    };
    let managed = evaluator.effective().mcp_connections();
    if !evaluator.has_mcp_integrations() || managed.contains_key(server) {
        return Ok(None);
    }
    let Some(host) = crate::agents::mcp_host_server(
        agent,
        Path::new(&context.project_root),
        Path::new(&ctx.cwd),
        server,
        managed,
    )?
    else {
        return Ok(None);
    };
    let resolved = evaluator.resolve_mcp(&host.server(), &host.connections, host.source)?;
    if resolved.matches.is_empty() {
        return Ok(None);
    }
    let mut bound = input.clone();
    if let Input::Mcp { server, .. } = &mut bound {
        *server = host.server();
    }
    Ok(Some((bound, resolved, host.alias)))
}
fn resolve(input: &Input, ctx: &RequestContext) -> Option<String> {
    let executable = match input {
        Input::Argv { executable, .. } => executable.as_str(),
        _ => return None,
    };
    let resolved = if Path::new(executable).is_absolute() {
        Path::new(executable).canonicalize().ok()
    } else if executable.contains('/') {
        Path::new(&ctx.cwd).join(executable).canonicalize().ok()
    } else {
        ctx.environment
            .as_ref()
            .and_then(|e| e.get("PATH"))
            .and_then(|path| {
                std::env::split_paths(path).find_map(|p| {
                    let directory = if p.is_absolute() {
                        p
                    } else {
                        Path::new(&ctx.cwd).join(p)
                    };
                    launch_path(&directory.join(executable))
                })
            })
    };
    resolved
        .and_then(|p| launch_path(&p))
        .map(|p| p.to_string_lossy().into_owned())
}
fn launch_path(path: &Path) -> Option<std::path::PathBuf> {
    let path = path.canonicalize().ok()?;
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path)
}
fn selectors(metadata: &Value) -> BTreeMap<String, String> {
    let mut selectors = BTreeMap::new();
    let mut ambiguous = std::collections::BTreeSet::new();
    for installed in metadata["installed"].as_array().into_iter().flatten() {
        if let (Some(id), Some(selector)) =
            (installed["id"].as_str(), installed["selector"].as_str())
        {
            if selectors
                .get(id)
                .is_some_and(|previous| previous != selector)
            {
                ambiguous.insert(id.to_owned());
            } else {
                selectors.insert(id.into(), selector.into());
            }
            if let Some(identity) = installed["source_identity"].as_str() {
                selectors.insert(format!("{identity}/{id}"), selector.into());
            }
        }
    }
    for id in ambiguous {
        selectors.remove(&id);
    }
    selectors
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn reference_fixture() -> (
        configuration::Evaluator,
        Value,
        BTreeMap<&'static str, Value>,
    ) {
        let first = "pack:123e4567-e89b-42d3-a456-426614174000";
        let second = "pack:123e4567-e89b-42d3-a456-426614174001";
        let metadata = json!({"installed":[
            {"id":"acme/plain","source_identity":"plain-origin","selector":first},
            {"id":"acme/known","source_identity":"known-origin","selector":second}
        ]});
        let mut selections = Vec::new();
        let mut packs = Vec::new();
        for (id, origin) in [
            ("acme/plain", None),
            ("acme/known", Some("known-origin")),
            ("acme/missing", None),
        ] {
            let mut selection = json!({"id":id,"version":"1.0.0","profile":"recommended","path":format!("{}.yml", id.replace('/', "-"))});
            if let Some(origin) = origin {
                selection["registry_identity"] = json!(origin);
            }
            selections.push(selection);
            packs.push(json!({"schema_version":1,"kind":"pack","id":id,"version":"1.0.0",
                "description":"Acme diagnostic reference fixture","license":"MIT","maintainers":["Acme"],
                "engines":{"shield":env!("CARGO_PKG_VERSION")},
                "capabilities":["argument-rules-v1","requirements-v1","structured-rules-v1"],
                "rules":[{"id":"guard","decision":"allow","conditions":{"domain":"argv","executable":"acme"},
                    "requirements":{"arguments":{"kind":"exact","values":["safe"]}},
                    "on_unmet":{"decision":"deny","message":"Use the safe operation.","alternatives":["acme safe"]}}]
            }).to_string());
        }
        let source = configuration::Candidate::from_yaml(
            &json!({"schema_version":1,"packs":selections}).to_string(),
        )
        .expect("valid reference fixture")
        .capture(&packs, None)
        .expect("valid reference fixture");
        let evaluator = configuration::Evaluator::new(vec![(policy::Scope::Project, source)])
            .expect("valid reference fixture");
        let expected = BTreeMap::from([
            (
                "acme/plain",
                json!({"scope":"pack","pack_selector":first,"rule_id":"guard"}),
            ),
            (
                "known-origin/acme/known",
                json!({"scope":"pack","pack_selector":second,"rule_id":"guard"}),
            ),
            ("acme/missing", Value::Null),
        ]);
        (evaluator, metadata, expected)
    }

    #[test]
    fn typed_evaluations_project_all_owned_references_without_annotating_submitted_json() {
        let (evaluator, metadata, expected) = reference_fixture();
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().to_str().unwrap();
        let context = RequestContext {
            cwd: cwd.into(),
            environment: None,
            working_files: None,
        };
        let original = json!({"source":"acme/plain","rule_id":"submitted-data",
            "rule_ref":{"scope":"pack","pack_selector":"acme/known","rule_id":"submitted-reference"},
            "authorization":null,"nested":[{"source":"acme/known","rule_id":"also-submitted"}]});
        for input in [
            Input::Argv {
                executable: "acme".into(),
                arguments: vec!["--token".into(), "private-value".into()],
                original_input: original.clone(),
            },
            Input::Shell {
                source: "acme --token private-value".into(),
                dialect: "posix".into(),
                dialect_version: None,
                original_input: original.clone(),
            },
        ] {
            let shell = matches!(&input, Input::Shell { .. });
            let mut report = evaluator
                .evaluate_normalized_report(
                    &input,
                    &context,
                    cwd,
                    None,
                    "2030-01-01T00:00:00Z",
                    &|| Ok(()),
                )
                .unwrap();
            report.annotate_rule_references(&selectors(&metadata));
            // The legacy projection is an independent parity oracle here;
            // production encoding never materializes this unredacted tree.
            let mut legacy = serde_json::to_value(&report).unwrap();
            let legacy_redacted = crate::evidence::redact(&mut legacy);
            if shell {
                // Typed privacy annotations repair the legacy AST-word leak.
                // Whole source withholding is deliberate; policy scalars and
                // unrelated submitted objects are not inferred to be secrets.
                for pointer in [
                    "/normalized_request/source",
                    "/analysis_coverage/source",
                    "/analysis_coverage/source_units/0/source",
                    "/analysis_coverage/structure/words/2/raw",
                    "/analysis_coverage/structure/words/2/literal",
                    "/analysis_coverage/structure/words/2/fragments/0/raw",
                ] {
                    *legacy.pointer_mut(pointer).unwrap() = json!("[REDACTED]");
                }
            }
            let (value, redacted) =
                Projected::encode_sources(&report, &|| Ok(()), &report.sensitive_sources())
                    .unwrap()
                    .into_parts();
            assert_eq!(value, legacy);
            assert_eq!(redacted, legacy_redacted);
            assert!(redacted);
            assert!(
                !value.to_string().contains("private-value"),
                "secret evidence paths: {:?}",
                secret_paths(&value, "private-value")
            );
            assert_eq!(value["decision"], "deny");
            assert_eq!(value["normalized_request"]["original_input"], original);
            assert!(value["approval_request_id"].is_null());
            check_owned_references(&value["explanation"], &expected);
            if shell {
                assert_eq!(value["normalized_request"]["source"], "[REDACTED]");
                let operation = &value["analysis_coverage"]["operations"][0];
                assert_eq!(
                    operation["request"]["arguments"],
                    json!(["--token", "[REDACTED]"])
                );
                assert!(operation["policy_request"].is_null());
                assert!(operation.get("stdin").is_none());
                check_owned_references(&operation["evaluation"]["evaluation"], &expected);
                assert_eq!(value["analysis_coverage"]["request"], operation["request"]);
                assert_eq!(
                    value["analysis_coverage"]["evaluation"],
                    operation["evaluation"]
                );
            } else {
                assert_eq!(
                    value["normalized_request"]["arguments"],
                    json!(["--token", "[REDACTED]"])
                );
            }
        }
    }

    fn secret_paths(value: &Value, secret: &str) -> Vec<String> {
        fn visit(value: &Value, path: &str, secret: &str, found: &mut Vec<String>) {
            match value {
                Value::String(text) if text.contains(secret) => found.push(path.into()),
                Value::Object(fields) => {
                    for (key, value) in fields {
                        visit(
                            value,
                            &format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                            secret,
                            found,
                        );
                    }
                }
                Value::Array(items) => {
                    for (index, value) in items.iter().enumerate() {
                        visit(value, &format!("{path}/{index}"), secret, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        visit(value, "", secret, &mut found);
        found
    }

    #[test]
    fn typed_ask_projection_keeps_policy_metadata_and_masks_private_source_descendants() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().to_str().unwrap();
        let custom = crate::policy::rules::document(&[
            json!({"id":"guard","decision":"ask","conditions":{"domain":"argv","executable":"echo"},
                "requirements":{"arguments":{"kind":"exact","values":["safe"]}},
                "on_unmet":{"decision":"ask","message":"Use the safe operation.","alternatives":["echo safe"]}}),
            json!({"id":"outer","decision":"ask","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}})
        ]).unwrap().to_string();
        let source =
            configuration::Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n")
                .unwrap()
                .capture(&[], Some(&custom))
                .unwrap();
        let evaluator =
            configuration::Evaluator::new(vec![(policy::Scope::Project, source)]).unwrap();
        let context = RequestContext {
            cwd: cwd.into(),
            environment: None,
            working_files: None,
        };
        for (source, dialect) in [
            ("echo '--token' 'private-value'", "posix"),
            (
                r#"acme --token "$(echo private-value)"; echo visible"#,
                "bash",
            ),
            (r#"acme --token "$(git -C private-value status)""#, "bash"),
            ("Write-Output '--token' 'private-value'", "powershell"),
        ] {
            let original =
                json!({"command":source,"data":{"source":"acme/source","rule_id":"submitted"}});
            let input = Input::Shell {
                source: source.into(),
                dialect: dialect.into(),
                dialect_version: None,
                original_input: original.clone(),
            };
            let mut report = evaluator
                .evaluate_normalized_report(
                    &input,
                    &context,
                    cwd,
                    None,
                    "2030-01-01T00:00:00Z",
                    &|| Ok(()),
                )
                .unwrap();
            report.annotate_rule_references(&BTreeMap::new());
            let raw = serde_json::to_value(&report).unwrap();
            assert_eq!(raw["normalized_request"]["source"], source);
            assert_eq!(raw["normalized_request"]["original_input"], original);
            assert!(raw.to_string().contains("private-value"));
            let (value, changed) =
                Projected::encode_sources(&report, &|| Ok(()), &report.sensitive_sources())
                    .unwrap()
                    .into_parts();
            assert!(changed);
            assert_ask_projection(&value, &original, source, dialect);
        }
    }

    fn assert_ask_projection(value: &Value, original: &Value, source: &str, dialect: &str) {
        let leaks = secret_paths(value, "private-value");
        assert!(leaks.is_empty(), "secret evidence paths: {leaks:?}");
        assert_eq!(value["decision"], "ask");
        assert!(value["approval_request_id"].is_null());
        assert_eq!(
            value["normalized_request"]["original_input"]["command"],
            "[REDACTED]"
        );
        assert_eq!(
            value["normalized_request"]["original_input"]["data"],
            original["data"]
        );
        if dialect != "powershell" && !source.contains("git -C") {
            assert!(
                value["explanation"]["matches"]
                    .as_array()
                    .expect("complete projected evidence")
                    .iter()
                    .any(|matched| matched["rule_id"] == "guard"
                        && matched["rule_ref"]["scope"] == "custom")
            );
            assert!(
                value["explanation"]["guidance"]
                    .as_array()
                    .expect("complete projected evidence")
                    .iter()
                    .any(|guidance| guidance["message"] == "Use the safe operation.")
            );
        }
        if source.ends_with("echo visible") {
            let operations = value["analysis_coverage"]["operations"]
                .as_array()
                .expect("complete projected evidence");
            assert!(
                operations
                    .iter()
                    .any(|operation| operation["source_unit"] == 0
                        && operation["request"]["arguments"] == json!(["visible"]))
            );
            assert!(
                operations
                    .iter()
                    .filter(|operation| operation["source_unit"] == 1)
                    .all(|operation| operation["decision"] == "ask")
            );
            assert!(
                operations
                    .iter()
                    .any(|operation| operation["source_unit"] == 1
                        && operation["request"]["arguments"] == "[REDACTED]")
            );
        }
    }

    fn check_owned_references(evaluation: &Value, expected: &BTreeMap<&str, Value>) {
        let matches = evaluation["matches"]
            .as_array()
            .expect("complete projected evidence");
        assert_eq!(matches.len(), expected.len());
        let mut checks = 0;
        for matched in matches {
            let source = matched["source"]
                .as_str()
                .expect("complete projected evidence");
            assert_eq!(matched["rule_ref"], expected[source], "{source}");
            let local = matched["requirement_checks"]
                .as_array()
                .expect("complete projected evidence");
            assert!(!local.is_empty());
            for check in local {
                assert_eq!(check["rule_ref"], expected[source]);
                checks += 1;
            }
        }
        let top = evaluation["requirement_checks"]
            .as_array()
            .expect("complete projected evidence");
        assert_eq!(top.len(), checks);
        for reference in expected.values() {
            assert!(
                top.iter()
                    .any(|check| check.get("rule_ref") == Some(reference))
            );
            assert!(
                evaluation["guidance"]
                    .as_array()
                    .expect("complete projected evidence")
                    .iter()
                    .any(|item| item.get("rule_ref") == Some(reference))
            );
        }
    }

    #[test]
    fn executable_observation_uses_request_cwd_for_relative_path_entries() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("bin")).unwrap();
        std::os::unix::fs::symlink("/bin/echo", root.path().join("bin/acme")).unwrap();
        let input: Input = serde_json::from_value(
            json!({"kind":"argv","executable":"acme","arguments":[],"original_input":{}}),
        )
        .unwrap();
        let ctx = RequestContext {
            working_files: None,
            cwd: root.path().to_string_lossy().into_owned(),
            environment: Some(BTreeMap::from([("PATH".into(), "bin".into())])),
        };
        assert_eq!(
            resolve(&input, &ctx),
            Some(
                Path::new("/bin/echo")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }
}
