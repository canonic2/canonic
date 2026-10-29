//! Connects captured project candidates to the shared pure policy evaluator.

use super::{Effective, Snapshot, compose};
use crate::policy::{self, CustomRules, Decision, Error, Pack, Profile, Request, Scope};
use serde::Serialize;
use std::sync::Arc;

#[cfg(test)]
mod tests;

/// A read-only decision with effective candidate metadata and rule explanations.
#[derive(Debug, Serialize)]
pub struct CandidateEvaluation {
    schema_version: u8,
    mode: &'static str,
    trust: &'static str,
    /// Literal argv decision from untrusted candidate data; never an execution grant.
    pub decision: Decision,
    revision: String,
    #[serde(serialize_with = "Effective::serialize")]
    effective: Arc<Effective>,
    evaluation: policy::Evaluation,
    /// The command as matched when a declared directory option chose its folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    directory_request: Option<Request>,
}

impl CandidateEvaluation {
    /// The resolver's evaluation, without the effective policy.
    pub(crate) fn evaluation(&self) -> &policy::Evaluation {
        &self.evaluation
    }
    pub(crate) fn directory_request(&self) -> Option<&Request> {
        self.directory_request.as_ref()
    }

    pub(crate) fn unmatched_ask(&self) -> bool {
        self.decision == Decision::Ask && self.evaluation.matches.is_empty()
    }

    pub(crate) fn fallback_only_ask(&self) -> bool {
        self.evaluation.fallback_only_ask()
    }
}

/// Prepared, immutable candidate evidence for repeated read-only evaluation.
/// Owns captured artifacts; no cache hit, scope label, or revision establishes trust.
pub struct Evaluator {
    plan: Box<policy::compiled::Plan>,
    effective: Arc<Effective>,
    revision: String,
}

impl Evaluator {
    pub(crate) fn time_sensitive(&self) -> bool {
        self.plan.time_sensitive()
    }

    pub(crate) fn evaluate_resource(
        &self,
        request: &Request,
        input: &policy::normalized::Input,
        context: &policy::normalized::Context,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<CandidateEvaluation, Error> {
        let bound = self.mcp_input(input);
        let input = bound.as_ref();
        input.validate(context, &request.project)?;
        let now = crate::evidence::now();
        let observer = crate::location::Observer::new(input, &request.project, &context.cwd, None)
            .with_access_paths(self.effective.access_paths())
            .with_working_files(context.working_files.as_ref());
        let facts = policy::rules::Facts {
            input,
            context,
            executable: None,
            now: &now,
            known_cwd: true,
            known_arguments: true,
            locations: Some(&observer),
            effects: None,
        };
        let mut evaluation = self.plan.evaluate_input(request, &facts, checkpoint)?;
        self.constrain_mcp(&mut evaluation, input, request, checkpoint)?;
        Ok(self.candidate(evaluation))
    }

    fn mcp_input<'a>(
        &self,
        input: &'a policy::normalized::Input,
    ) -> std::borrow::Cow<'a, policy::normalized::Input> {
        if !matches!(input, policy::normalized::Input::Mcp { .. }) {
            return std::borrow::Cow::Borrowed(input);
        }
        let mut input = input.clone();
        if let policy::normalized::Input::Mcp {
            server,
            connection,
            arguments,
            ..
        } = &mut input
        {
            *connection = self
                .effective
                .mcp_connections()
                .get(server)
                .filter(|c| c.enabled && !c.agents.is_empty())
                .map(|c| c.facts(arguments));
        }
        std::borrow::Cow::Owned(input)
    }

    fn constrain_mcp(
        &self,
        evaluation: &mut policy::Evaluation,
        input: &policy::normalized::Input,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let policy::normalized::Input::Mcp {
            server,
            tool,
            arguments,
            ..
        } = input
        else {
            return Ok(());
        };
        let Some(mapping) = self
            .effective
            .mcp_connections()
            .get(server)
            .filter(|c| c.enabled)
            .and_then(|c| c.command_fields.get(tool))
        else {
            return Ok(());
        };
        let Ok(arguments) = mapping.arguments(arguments) else {
            evaluation.require_review();
            return Ok(());
        };
        let derived = Request {
            executable: mapping.executable.clone(),
            arguments,
            cwd: request.cwd.clone(),
            project: request.project.clone(),
        };
        // The command runs at the server. A local folder or environment cannot
        // establish its path, identity or credential requirements.
        let result = self.evaluate_context(&derived, checkpoint, false)?;
        evaluation.constrain(result.evaluation, derived);
        Ok(())
    }

    fn candidate(&self, evaluation: policy::Evaluation) -> CandidateEvaluation {
        CandidateEvaluation {
            schema_version: 1,
            mode: "preview",
            trust: "unverified",
            decision: evaluation.decision,
            revision: self.revision.clone(),
            effective: self.effective.clone(),
            evaluation,
            directory_request: None,
        }
    }

    pub(crate) fn evaluate_normalized(
        &self,
        input: &policy::normalized::Input,
        context: &policy::normalized::Context,
        project: &str,
        executable: Option<&str>,
        now: &str,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<serde_json::Value, Error> {
        use policy::normalized::Input;
        let bound = self.mcp_input(input);
        let input = bound.as_ref();
        let (name, arguments) = match input {
            Input::Argv {
                executable,
                arguments,
                ..
            } => (executable.clone(), arguments.clone()),
            _ => ("shield-unclassified-request".into(), vec![]),
        };
        let request = Request {
            executable: name,
            arguments,
            cwd: context.cwd.clone(),
            project: project.into(),
        };
        let observer = crate::location::Observer::new(input, project, &context.cwd, executable)
            .with_access_paths(self.effective.access_paths())
            .with_working_files(context.working_files.as_ref());
        let facts = policy::rules::Facts {
            input,
            context,
            executable,
            now,
            known_cwd: true,
            known_arguments: true,
            locations: Some(&observer),
            effects: None,
        };
        let mut evaluation = self.plan.evaluate_input(&request, &facts, checkpoint)?;
        self.constrain_mcp(&mut evaluation, input, &request, checkpoint)?;
        let complete = !matches!(input, Input::Unknown { .. } | Input::Shell { .. })
            && !self.plan.semantic()
            && observer.complete();
        let value = serde_json::json!({"mode":"diagnostic","decision":evaluation.decision,"disposition":"preview","normalized_request":input,"explanation":evaluation,"analysis_coverage":{"complete":complete,"model":"normalized-request-v1","unknown":if complete {None}else{Some("request or effect predicates extend beyond literal argv coverage")}},"approval_request_id":null});
        self.shell_result(input, context, project, value, checkpoint)
    }

    // Both diagnostics and native handoff resolve the complete request and its
    // inspected operations. Uncertainty cannot erase a matched whole-request deny.
    fn shell_result(
        &self,
        input: &policy::normalized::Input,
        context: &policy::normalized::Context,
        project: &str,
        mut value: serde_json::Value,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<serde_json::Value, Error> {
        let policy::normalized::Input::Shell {
            source, dialect, ..
        } = input
        else {
            return Ok(value);
        };
        let parsed = serde_json::from_value(serde_json::json!(dialect))
            .map_err(|_| Error::Contract("invalid shell dialect"))?;
        let mut analysis = crate::shell::analyze_with_working_files(
            source,
            parsed,
            &context.cwd,
            context.environment.as_ref(),
            project,
            self,
            checkpoint,
            context.working_files.as_ref(),
        )?;
        if value["decision"] != "deny"
            && (analysis["decision"] == "deny"
                || (analysis["decision"] == "ask"
                    && value["explanation"]["reason"] != "matched-exact-request")
                || value["explanation"]["matches"]
                    .as_array()
                    .is_none_or(Vec::is_empty))
        {
            value["decision"] = analysis["decision"].clone();
        }
        if !["bash", "posix"].contains(&dialect.as_str()) {
            if value["decision"] != "deny" {
                value["decision"] = serde_json::json!("ask");
            }
            analysis["complete"] = serde_json::json!(false);
            analysis["unknown"] = serde_json::json!("shell dialect analysis is unsupported");
        }
        value["explanation"]["decision"] = value["decision"].clone();
        nested_diagnostics(&mut value, &analysis);
        coverage_guidance(&mut value, &analysis);
        value["analysis_coverage"] = analysis;
        Ok(value)
    }
    /// Compose captured candidates and retain complete metadata once.
    ///
    /// # Errors
    /// Rejects invalid composition or missing retained selected artifacts.
    pub fn new(inputs: Vec<(Scope, Snapshot)>) -> Result<Self, Error> {
        let [(scope, snapshot)]: [(Scope, Snapshot); 1] = inputs
            .try_into()
            .map_err(|_| Error::Contract("exactly one project candidate required"))?;
        refuse_unset_required(&snapshot)?;
        let effective = compose(&[(scope, snapshot.lock.clone())])?;
        let revision = effective.revision()?;
        let selections: std::collections::BTreeMap<_, _> = effective
            .selections()
            .map(|(id, profile)| (id.to_owned(), profile))
            .collect();
        let packs = snapshot
            .packs
            .into_iter()
            .map(|pack| {
                let profile = *selections
                    .get(pack.id())
                    .ok_or(Error::Contract("missing selected artifact"))?;
                Ok((pack, profile))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let plan = policy::compiled::Plan::compile(
            packs,
            snapshot.custom,
            serde_json::to_value(&effective).map_err(|_| Error::Syntax)?,
        )?;
        Ok(Self {
            plan: Box::new(plan),
            effective: Arc::new(effective),
            revision,
        })
    }

    pub(crate) fn from_authenticated_compiled(bytes: Vec<u8>) -> Result<Self, Error> {
        Self::from_plan(policy::compiled::Plan::load_authenticated(bytes)?)
    }
    fn from_plan(plan: policy::compiled::Plan) -> Result<Self, Error> {
        let effective: Effective = serde_json::from_value(plan.effective().clone())
            .map_err(|_| Error::Contract("invalid compiled composition"))?;
        let revision = effective.revision()?;
        Ok(Self {
            plan: Box::new(plan),
            effective: Arc::new(effective),
            revision,
        })
    }

    pub(crate) fn compiled_bytes(&self) -> &[u8] {
        self.plan.bytes()
    }

    /// Directory options the selected packs and custom rules declare.
    pub(crate) fn directory_options(&self) -> &[policy::directories::DirectoryOption] {
        self.plan.directory_options()
    }

    /// Codex prefixes of the active rules that allow.
    pub(crate) fn codex_prefixes(&self) -> &[policy::agent_outputs::CodexPrefix] {
        self.plan.codex_prefixes()
    }

    /// Complete project metadata and selected artifacts.
    #[must_use]
    pub fn effective(&self) -> &Effective {
        &self.effective
    }

    /// Cached fingerprint of these unverified captured candidates.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Evaluate a fresh request through the same indexed matcher and authority resolver.
    /// Does not cache decisions, approve execution, or reread mutable source files.
    ///
    /// # Errors
    /// Rejects invalid requests or shared evaluator constraints.
    pub fn evaluate(&self, request: &Request) -> Result<CandidateEvaluation, Error> {
        self.evaluate_controlled(request, &|| Ok(()))
    }

    pub(crate) fn evaluate_controlled(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<CandidateEvaluation, Error> {
        self.evaluate_context(request, checkpoint, true)
    }

    pub(crate) fn evaluate_context(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        known_cwd: bool,
    ) -> Result<CandidateEvaluation, Error> {
        self.evaluate_known(request, checkpoint, known_cwd, true, None)
    }

    pub(crate) fn evaluate_context_with_working_files(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        known_cwd: bool,
        root: Option<&crate::location::WorkingFiles>,
    ) -> Result<CandidateEvaluation, Error> {
        self.evaluate_known(request, checkpoint, known_cwd, true, root)
    }

    pub(crate) fn evaluate_unknown_arguments(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        known_cwd: bool,
    ) -> Result<CandidateEvaluation, Error> {
        self.evaluate_known(request, checkpoint, known_cwd, false, None)
    }

    /// When no rule decides the command as written, other than a pack's
    /// yielding fallback, a declared directory option lets it be matched as the
    /// same command run from the folder the option chose. Explicit rules for the
    /// written form keep deciding it.
    fn evaluate_known(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        known_cwd: bool,
        known_arguments: bool,
        working_files: Option<&crate::location::WorkingFiles>,
    ) -> Result<CandidateEvaluation, Error> {
        let written = self.evaluate_written(
            request,
            checkpoint,
            known_cwd,
            known_arguments,
            working_files,
        )?;
        if known_cwd
            && written.fallback_only_ask()
            && let Some(normalized) =
                policy::directories::normalize(request, self.directory_options())
        {
            let mut result = self.evaluate_written(
                &normalized,
                checkpoint,
                known_cwd,
                known_arguments,
                working_files,
            )?;
            result.directory_request = Some(normalized);
            return Ok(result);
        }
        Ok(written)
    }

    fn evaluate_written(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        known_cwd: bool,
        known_arguments: bool,
        working_files: Option<&crate::location::WorkingFiles>,
    ) -> Result<CandidateEvaluation, Error> {
        checkpoint()?;
        let input = policy::normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: serde_json::json!({}),
        };
        let context = policy::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: working_files.cloned(),
        };
        let now = crate::evidence::now();
        let observer = crate::location::Observer::new(&input, &request.project, &request.cwd, None)
            .with_access_paths(self.effective.access_paths())
            .with_working_files(working_files);
        let facts = policy::rules::Facts {
            input: &input,
            context: &context,
            executable: None,
            now: &now,
            known_cwd,
            known_arguments,
            locations: known_cwd.then_some(&observer as &dyn policy::resources::Resources),
            effects: None,
        };
        let evaluation = self.plan.evaluate_input(request, &facts, checkpoint)?;
        checkpoint()?;
        Ok(CandidateEvaluation {
            schema_version: 1,
            mode: "preview",
            trust: "unverified",
            decision: evaluation.decision,
            revision: self.revision.clone(),
            effective: self.effective.clone(),
            evaluation,
            directory_request: None,
        })
    }
}

// Preserve operation identity, the rules and the failures contributing to the
// shell outcome. Rules match the commands inside the source, not the source
// itself, so their applied matches explain the decision.
fn nested_diagnostics(value: &mut serde_json::Value, analysis: &serde_json::Value) {
    let decision = value["decision"].clone();
    for operation in analysis["operations"].as_array().into_iter().flatten() {
        if operation["decision"] != decision {
            continue;
        }
        let reference = format!(
            "shell:{}:{}-{}",
            operation["source_unit"], operation["span"]["start"], operation["span"]["end"]
        );
        for matched in operation["evaluation"]["evaluation"]["matches"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|matched| matched["applied"] == true)
        {
            let Some(matches) = value["explanation"]["matches"].as_array_mut() else {
                break;
            };
            let same = |m: &serde_json::Value| {
                m["source"] == matched["source"]
                    && m["rule_id"] == matched["rule_id"]
                    && m["operation_ref"] == reference
            };
            if !matches.iter().any(same) {
                let mut annotated = matched.clone();
                annotated["operation_ref"] = serde_json::json!(reference);
                annotated["executable"] = operation["request"]["executable"].clone();
                matches.push(annotated);
            }
            if value["explanation"]["reason"] == "unmatched-normalized-request" {
                value["explanation"]["reason"] = serde_json::json!("matched-policy");
            }
        }
        for key in ["requirement_checks", "guidance"] {
            for item in operation["evaluation"]["evaluation"][key]
                .as_array()
                .into_iter()
                .flatten()
            {
                let mut item = item.clone();
                item["operation_ref"] = serde_json::json!(reference);
                if let Some(items) = value["explanation"][key].as_array_mut() {
                    items.push(item);
                }
            }
        }
    }
}

/// The launcher an operation runs, when it only runs its own arguments.
fn launcher(operation: &serde_json::Value) -> Option<&str> {
    operation["request"]["executable"]
        .as_str()
        .map(|name| name.rsplit('/').next().unwrap_or(name))
        .filter(|name| {
            [
                "eval", "exec", "command", "builtin", "xargs", "env", "nohup", "nice", "timeout",
                "gtimeout", "stdbuf", "time",
            ]
            .contains(name)
        })
}

/// Shield inspected a wrapper's command when that command was recorded inside
/// the wrapper's span, as for `timeout 5 git status`.
fn unwrapped(wrapper: &serde_json::Value, operations: Option<&Vec<serde_json::Value>>) -> bool {
    let span = |value: &serde_json::Value| Some((value["start"].as_u64()?, value["end"].as_u64()?));
    let Some((start, end)) = span(&wrapper["span"]) else {
        return false;
    };
    operations.into_iter().flatten().any(|inner| {
        inner["source_unit"] == wrapper["source_unit"]
            && inner["request"].is_object()
            && span(&inner["span"]).is_some_and(|(s, e)| s > start && e <= end)
    })
}

/// Rewrite advice that names the launcher the agent used when the generic list omits it.
fn run_directly(name: Option<&str>) -> String {
    match name {
        Some(name) if !["eval", "xargs"].contains(&name) => format!(
            "Run each command directly as its own simple command, without `{name}`, sh -c, eval, xargs or find -exec."
        ),
        _ => "Run each command directly as its own simple command, without sh -c, eval, xargs or find -exec.".to_owned(),
    }
}

// Name the part of an Asked shell command that asks, and why: source Shield
// could not inspect goes back to the agent to rewrite; a rule's Ask, a command
// no rule covers or a program Shield cannot inspect is the user's call. Long
// commands otherwise leave the user searching for the part that asked.
fn coverage_guidance(value: &mut serde_json::Value, analysis: &serde_json::Value) {
    // A Deny of words Shield cannot resolve, such as `cat "$p"`, is about the
    // unknown words: the literal command gets its own decision.
    let denied = value["decision"] == "deny";
    if value["decision"] != "ask" && !denied {
        return;
    }
    let authored = value["explanation"]["guidance"]
        .as_array()
        .is_some_and(|items| !items.is_empty());
    let operations = analysis["operations"].as_array();
    // Launched commands explain more than the unresolved words they carry.
    let candidates: Vec<_> = operations
        .into_iter()
        .flatten()
        .filter(|operation| operation["decision"] == value["decision"])
        .filter(|operation| !denied || operation["coverage"] == "opaque-arguments")
        .filter_map(|operation| {
            let (rank, message, alternatives) =
                coverage_reason(operation, operations, authored, denied)?;
            Some((rank, operation, message, alternatives))
        })
        .collect();
    let Some(chosen) = candidates.iter().min_by_key(|candidate| {
        (
            candidate.0,
            candidate.1["coverage"] != "background-job-kill",
        )
    }) else {
        return;
    };
    let index = |value: &serde_json::Value| value.as_u64().and_then(|v| usize::try_from(v).ok());
    let describe = |operation: &serde_json::Value, message: &str| {
        let excerpt = index(&operation["source_unit"])
            .and_then(|unit| analysis["source_units"][unit]["source"].as_str())
            .zip(index(&operation["span"]["start"]).zip(index(&operation["span"]["end"])))
            .and_then(|(source, (start, end))| source.get(start..end))
            .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|part| !part.is_empty() && part.chars().count() <= 120);
        excerpt.map_or_else(
            || format!("Part of this command {message}."),
            |part| format!("`{part}` {message}."),
        )
    };
    let (rank, operation, message, alternatives) = chosen;
    let item = coverage_item(
        *rank,
        operation,
        &describe(operation, message),
        alternatives,
    );
    // Every other literal command that asks follows the first, so a rule can
    // be offered for each of them; feedback leads with the first.
    let others: Vec<_> = candidates
        .iter()
        .filter(|(rank, other, ..)| {
            !denied
                && *rank == 2
                && !std::ptr::eq(*other, *operation)
                && other["coverage"] == "literal-argv"
        })
        .map(|(rank, other, message, alternatives)| {
            coverage_item(*rank, other, &describe(other, message), alternatives)
        })
        .collect();
    if let Some(items) = value["explanation"]["guidance"].as_array_mut() {
        items.insert(0, item);
        items.extend(others);
    }
}

/// Rank an operation's explanation and any rewrite the agent can make.
fn coverage_reason(
    operation: &serde_json::Value,
    operations: Option<&Vec<serde_json::Value>>,
    authored: bool,
    denied: bool,
) -> Option<(u8, String, Vec<String>)> {
    let rewrite = |rank: u8, message: &str, alternative: &str| {
        (rank, message.to_owned(), vec![alternative.to_owned()])
    };
    let launcher = launcher(operation);
    Some(match operation["coverage"].as_str()? {
                // A rule's own guidance already explains these.
                _ if authored && !denied && operation["coverage"] != "literal-argv" => return None,
                "indirect-execution" => (
                    0,
                    "runs further commands that Shield cannot inspect".to_owned(),
                    vec![run_directly(launcher)],
                ),
                // An inspected wrapper still changes how its command runs.
                "partial" if launcher.is_some() && unwrapped(operation, operations) => (
                    2,
                    format!(
                        "runs its command through `{}`, so the user decides",
                        launcher.unwrap_or_default()
                    ),
                    vec![],
                ),
                // Launchers that only run their own arguments can be rewritten as
                // those commands; interpreters and scripts stay ordinary Asks.
                "partial" if launcher.is_some() => (
                    0,
                    "runs further commands that Shield cannot inspect".to_owned(),
                    vec![run_directly(launcher)],
                ),
                "shell-assignment" => rewrite(
                    1,
                    "sets a shell variable that later commands may inherit",
                    "Write the value directly in each command instead of assigning a variable.",
                ),
                "loop-execution-setting" => rewrite(
                    1,
                    "changes executable lookup through the loop's PATH binding",
                    "Use a loop variable that does not change executable lookup.",
                ),
                "opaque" | "opaque-expression" | "opaque-arguments" | "possible-expansion-mutation"
                | "possible-brace-expansion" | "function-call" | "control-transfer" => rewrite(
                    1,
                    "has words or control flow that Shield cannot resolve before it runs",
                    "Use literal arguments and separate simple commands, without command substitution, computed names or shell functions.",
                ),
                "partial" => (
                    2,
                    "runs a program whose effects Shield cannot inspect, so the user decides".to_owned(),
                    vec![],
                ),
                "assignment-prefix" => (
                    2,
                    "sets environment variables that can change what the command does, so the user decides".to_owned(),
                    vec![],
                ),
                "computed-assignment" => (
                    2,
                    "sets a shell variable from a value known only when it runs, so the user decides".to_owned(),
                    vec![],
                ),
                "background-job-kill" => (
                    2,
                    "signals a PID originating from a background job in this request; runtime ownership is unverified, so the user decides".to_owned(),
                    vec![],
                ),
                "runtime-status-arguments" => (
                    2,
                    "uses an exit status known only when it runs, so the user decides".to_owned(),
                    vec![],
                ),
                "literal-argv" => rule_ask(&operation["evaluation"]["evaluation"]),
                _ => return None,
    })
}

/// Preserve the selected operation's provenance and its eligible exact request.
fn coverage_item(
    rank: u8,
    operation: &serde_json::Value,
    message: &str,
    alternatives: &[String],
) -> serde_json::Value {
    // Only "shield" guidance sends the request back to the agent to rewrite.
    let mut item = serde_json::json!({"source":if rank < 2 { "shield" } else { "decisive" },"operation_ref":format!(
        "shell:{}:{}-{}",
        operation["source_unit"], operation["span"]["start"], operation["span"]["end"]
    ),"message":message,"alternatives":alternatives,"candidate_only":true});
    // Preserve literal command evidence for the agent to investigate and use
    // when drafting a reusable rule through the documented grammar.
    if rank == 2
        && !matches!(
            operation["coverage"].as_str(),
            Some("background-job-kill" | "runtime-status-arguments")
        )
        && operation["request"]["executable"].is_string()
    {
        item["request"] = serde_json::json!({"executable":operation["request"]["executable"],"arguments":operation["request"]["arguments"]});
        let typed = &operation["policy_request"];
        // A command whose directory option chose a folder in the project is
        // offered as the plain command limited to the project, which covers
        // every way of naming that folder on any checkout. A folder named
        // with `..` may leave the project through a link, so it is not offered.
        if typed["directory_option"] == true
            && let (Some(cwd), Some(project)) = (typed["cwd"].as_str(), typed["project"].as_str())
            && std::path::Path::new(cwd).starts_with(project)
            && !std::path::Path::new(cwd)
                .components()
                .any(|part| part == std::path::Component::ParentDir)
        {
            item["request"] = serde_json::json!({"executable":typed["executable"],"arguments":typed["arguments"],"cwd":"project"});
        }
        // A download decided by its fetch is allowed by a rule for its host.
        if typed["kind"] == "web" && typed["action"] == "fetch" {
            item["fetch_url"] = typed["url"].clone();
        }
    }
    item
}

/// Why a literal command asks: its rule's guidance or reason, or no rule at all.
fn rule_ask(evaluation: &serde_json::Value) -> (u8, String, Vec<String>) {
    let matches = evaluation["matches"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if matches.is_empty() {
        return (
            2,
            "matches no rule, so the user decides".into(),
            vec!["If it should run without asking, propose a pack or rule that covers it with policy_propose.".into()],
        );
    }
    let guidance = &evaluation["guidance"][0];
    let alternatives = guidance["alternatives"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|alternative| alternative.as_str().map(str::to_owned))
        .collect();
    let applied = matches
        .iter()
        .find(|matched| matched["applied"] == true && matched["decision"] == "ask");
    let why = guidance["message"]
        .as_str()
        .or_else(|| {
            applied
                .and_then(|matched| matched["reason"].as_str())
                .filter(|reason| !reason.is_empty())
        })
        .map(|why| format!("asks for approval: {}", why.trim_end_matches('.')))
        .or_else(|| {
            applied
                .and_then(|matched| matched["rule_id"].as_str())
                .map(|rule| format!("asks for approval under rule {rule}"))
        })
        .unwrap_or_else(|| "asks for approval".into());
    (2, why, alternatives)
}

/// Evaluate retained project artifacts and custom rules.
/// No files are read and no scope labels are authenticated by this operation.
///
/// # Errors
/// Rejects invalid composition/request input, missing retained packs, or evaluator limits.
pub fn evaluate(
    inputs: &[(Scope, Snapshot)],
    request: &Request,
) -> Result<CandidateEvaluation, Error> {
    Prepared::new(inputs)?.evaluate(request)
}

/// Refuse a candidate retaining a pack whose required variable is unset,
/// naming the selection and variable.
fn refuse_unset_required(snapshot: &Snapshot) -> Result<(), Error> {
    match snapshot
        .packs
        .iter()
        .find_map(|p| p.unset_required().next().map(|v| (p.id(), v)))
    {
        Some((pack, variable)) => {
            Err(Error::binding(variable, "a required pack variable is unset").in_selection(pack))
        }
        None => Ok(()),
    }
}

/// Borrowed selected artifacts plus complete metadata for repeated pure evaluation.
pub(super) struct Prepared<'a> {
    pub(super) effective: Effective,
    pub(super) packs: Vec<(&'a Pack, Profile)>,
    pub(super) custom: Vec<&'a CustomRules>,
    pub(super) directory_options: Vec<policy::directories::DirectoryOption>,
}

impl<'a> Prepared<'a> {
    pub(super) fn new(inputs: &'a [(Scope, Snapshot)]) -> Result<Self, Error> {
        let [(scope, snapshot)] = inputs else {
            return Err(Error::Contract("exactly one project candidate required"));
        };
        refuse_unset_required(snapshot)?;
        let effective = compose(&[(*scope, snapshot.lock.clone())])?;
        let mut packs = Vec::new();
        for (id, profile) in effective.selections() {
            let pack = snapshot
                .packs
                .iter()
                .find(|pack| pack.id() == id)
                .ok_or(Error::Contract("effective selection has no retained pack"))?;
            packs.push((pack, profile));
        }
        let custom: Vec<_> = snapshot.custom.iter().collect();
        let directory_options = policy::directories::selected(&packs, &custom)?;
        Ok(Self {
            effective,
            packs,
            custom,
            directory_options,
        })
    }

    /// The decision enforcement makes, including a declared directory option.
    pub(super) fn policy(&self, request: &Request) -> Result<policy::Evaluation, Error> {
        Ok(self.decide(request)?.0)
    }

    /// Like [`Evaluator::evaluate_known`], with the normalized request when a
    /// directory option decided.
    fn decide(&self, request: &Request) -> Result<(policy::Evaluation, Option<Request>), Error> {
        let written = policy::evaluate_selected(&self.packs, &self.custom, request)?;
        if written.fallback_only_ask()
            && let Some(normalized) =
                policy::directories::normalize(request, &self.directory_options)
        {
            let evaluation = policy::evaluate_selected(&self.packs, &self.custom, &normalized)?;
            return Ok((evaluation, Some(normalized)));
        }
        Ok((written, None))
    }

    fn evaluate(self, request: &Request) -> Result<CandidateEvaluation, Error> {
        let (evaluation, directory_request) = self.decide(request)?;
        Ok(CandidateEvaluation {
            schema_version: 1,
            mode: "preview",
            trust: "unverified",
            decision: evaluation.decision,
            revision: self.effective.revision()?,
            effective: Arc::new(self.effective),
            evaluation,
            directory_request,
        })
    }
}
