//! Connects captured project candidates to the shared pure policy evaluator.

use super::{Effective, Snapshot, compose};
use crate::policy::rules::directories::{self, DirectoryOption};
use crate::policy::{self, CustomRules, Decision, Error, Pack, Profile, Request, Scope};
use serde::Serialize;
use std::sync::Arc;

#[cfg(test)]
mod legacy_tests;
mod normalized;
#[cfg(test)]
mod tests;
pub(crate) use normalized::Evaluation as NormalizedEvaluation;

/// A read-only decision with effective candidate metadata and rule explanations.
#[derive(Debug)]
pub struct CandidateEvaluation {
    schema_version: u8,
    mode: &'static str,
    trust: &'static str,
    /// Literal argv decision from untrusted candidate data; never an execution grant.
    pub decision: Decision,
    revision: String,
    effective: Arc<Effective>,
    evaluation: policy::Evaluation,
    /// The command as matched when a declared directory option chose its folder.
    directory_request: Option<Request>,
    private_input: bool,
}

impl Serialize for CandidateEvaluation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut value = serializer.serialize_struct(
            "CandidateEvaluation",
            7 + usize::from(self.directory_request.is_some()),
        )?;
        value.serialize_field("schema_version", &self.schema_version)?;
        value.serialize_field("mode", self.mode)?;
        value.serialize_field("trust", self.trust)?;
        value.serialize_field("decision", &self.decision)?;
        value.serialize_field("revision", &self.revision)?;
        value.serialize_field("effective", &*self.effective)?;
        value.serialize_field("evaluation", &self.evaluation)?;
        if let Some(request) = &self.directory_request {
            value.serialize_field(
                "directory_request",
                &policy::resolution::diagnostics::RequestEvidence::new(request, self.private_input),
            )?;
        }
        value.end()
    }
}

impl CandidateEvaluation {
    pub(crate) fn mark_private_input(&mut self) {
        self.private_input = true;
        self.evaluation.mark_private_input();
    }
    /// The resolver's evaluation, without the effective policy.
    pub(crate) fn evaluation(&self) -> &policy::Evaluation {
        &self.evaluation
    }
    pub(crate) fn evaluation_mut(&mut self) -> &mut policy::Evaluation {
        &mut self.evaluation
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

    pub(crate) fn allows_docker_context_runtime(&self) -> bool {
        self.evaluation.allows_docker_context_runtime(|source| {
            self.effective
                .has_pack_artifact(source, &[super::composition::DOCKER_RUNTIME])
        })
    }

    pub(crate) fn stock_fetch_review(&self) -> bool {
        self.evaluation.stock_fetch_review(|source| {
            self.effective
                .has_pack_artifact(source, &super::composition::CONTAINER_BASELINES)
        })
    }

    pub(crate) fn allow_container_client_help(&mut self) {
        self.evaluation.allow_container_client_help();
        self.decision = Decision::Allow;
    }

    pub(crate) fn retain_container_fetch_review(&mut self, argv: Self) {
        self.evaluation
            .retain_container_fetch_review(argv.evaluation);
    }
}

/// Prepared, immutable candidate evidence for repeated read-only evaluation.
/// Owns captured artifacts; no cache hit, scope label, or revision establishes trust.
pub struct Evaluator {
    plan: Box<policy::resolution::compiled::Plan>,
    effective: Arc<Effective>,
    revision: String,
}

impl Evaluator {
    pub(crate) fn time_sensitive(&self) -> bool {
        self.plan.time_sensitive()
    }

    pub(crate) fn expiration_window(
        &self,
    ) -> Result<Option<policy::resolution::compiled::ExpirationWindow>, Error> {
        self.plan.expiration_window()
    }

    pub(crate) fn evaluate_resource(
        &self,
        request: &Request,
        input: &policy::resolution::normalized::Input,
        context: &policy::resolution::normalized::Context,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<CandidateEvaluation, Error> {
        self.evaluate_resource_mcp(request, input, context, None, checkpoint)
    }

    pub(crate) fn evaluate_resource_mcp(
        &self,
        request: &Request,
        input: &policy::resolution::normalized::Input,
        context: &policy::resolution::normalized::Context,
        mcp: Option<&policy::resolution::compiled::ResolvedMcp>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<CandidateEvaluation, Error> {
        let resolved = self.mcp_resolution(input, mcp)?;
        let resolved = resolved.as_deref();
        let bound = Self::mcp_input(input, resolved);
        let input = bound.as_ref();
        input.validate(context, &request.project)?;
        let now = crate::evidence::policy_now();
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
            container_read: None,
            effects: None,
        };
        let mut evaluation = self
            .plan
            .evaluate_input_mcp(request, &facts, resolved, checkpoint)?;
        self.constrain_mcp(&mut evaluation, input, request, resolved, checkpoint)?;
        let mut candidate = self.candidate(evaluation);
        if let policy::resolution::normalized::Input::Mcp { tool, .. } = input
            && crate::evidence::private_mcp_arguments(tool)
        {
            candidate.mark_private_input();
        }
        Ok(candidate)
    }

    fn mcp_input<'a>(
        input: &'a policy::resolution::normalized::Input,
        resolved: Option<&policy::resolution::compiled::ResolvedMcp>,
    ) -> std::borrow::Cow<'a, policy::resolution::normalized::Input> {
        if !matches!(input, policy::resolution::normalized::Input::Mcp { .. }) {
            return std::borrow::Cow::Borrowed(input);
        }
        let mut input = input.clone();
        if let policy::resolution::normalized::Input::Mcp {
            connection,
            arguments,
            ..
        } = &mut input
        {
            *connection = resolved.and_then(|mcp| mcp.facts(arguments));
        }
        std::borrow::Cow::Owned(input)
    }

    fn mcp_resolution<'a>(
        &self,
        input: &policy::resolution::normalized::Input,
        supplied: Option<&'a policy::resolution::compiled::ResolvedMcp>,
    ) -> Result<Option<std::borrow::Cow<'a, policy::resolution::compiled::ResolvedMcp>>, Error>
    {
        let policy::resolution::normalized::Input::Mcp { server, .. } = input else {
            return Ok(None);
        };
        if let Some(supplied) = supplied {
            if supplied.server != *server || supplied.revision != self.revision {
                return Err(Error::Contract(
                    "MCP observation belongs to another connection or policy revision",
                ));
            }
            return Ok(Some(std::borrow::Cow::Borrowed(supplied)));
        }
        Ok(Some(std::borrow::Cow::Owned(self.resolve_mcp(
            server,
            self.effective.mcp_connections(),
            "project",
        )?)))
    }

    pub(crate) fn has_mcp_integrations(&self) -> bool {
        self.plan.has_mcp_integrations()
    }

    pub(crate) fn resolve_mcp(
        &self,
        server: &str,
        connections: &crate::mcp_connections::Connections,
        source: &str,
    ) -> Result<policy::resolution::compiled::ResolvedMcp, Error> {
        self.plan
            .resolve_mcp(server, connections, source, &self.revision)
    }

    fn constrain_mcp(
        &self,
        evaluation: &mut policy::Evaluation,
        input: &policy::resolution::normalized::Input,
        request: &Request,
        resolved: Option<&policy::resolution::compiled::ResolvedMcp>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let policy::resolution::normalized::Input::Mcp {
            tool, arguments, ..
        } = input
        else {
            return Ok(());
        };
        let Some(mapping) = resolved
            .and_then(|mcp| mcp.connection.as_ref())
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
            private_input: false,
        }
    }

    pub(crate) fn evaluate_normalized(
        &self,
        input: &policy::resolution::normalized::Input,
        context: &policy::resolution::normalized::Context,
        project: &str,
        executable: Option<&str>,
        now: &str,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<serde_json::Value, Error> {
        serde_json::to_value(
            self.evaluate_normalized_report(input, context, project, executable, now, checkpoint)?,
        )
        .map_err(|_| Error::Contract("cannot encode normalized evaluation"))
    }

    pub(crate) fn evaluate_normalized_report(
        &self,
        input: &policy::resolution::normalized::Input,
        context: &policy::resolution::normalized::Context,
        project: &str,
        executable: Option<&str>,
        now: &str,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<NormalizedEvaluation, Error> {
        self.evaluate_normalized_report_mcp(
            input, context, project, executable, now, None, checkpoint,
        )
    }

    /// Evaluate with a connection the caller resolved, such as an observed host server.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors evaluate_normalized_report plus one connection"
    )]
    pub(crate) fn evaluate_normalized_report_mcp(
        &self,
        input: &policy::resolution::normalized::Input,
        context: &policy::resolution::normalized::Context,
        project: &str,
        executable: Option<&str>,
        now: &str,
        mcp: Option<&policy::resolution::compiled::ResolvedMcp>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<NormalizedEvaluation, Error> {
        use policy::resolution::normalized::Input;
        let resolved = self.mcp_resolution(input, mcp)?;
        let resolved = resolved.as_deref();
        let bound = Self::mcp_input(input, resolved);
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
        let facts = policy::rules::Facts {
            input,
            context,
            executable,
            now,
            known_cwd: true,
            known_arguments: true,
            locations: None,
            container_read: None,
            effects: None,
        };
        let (evaluation, complete) =
            self.evaluate_diagnostic_input(&request, facts, resolved, checkpoint)?;
        if let Input::Argv { original_input, .. } = input
            && let Some(normalized) =
                directory_fallback(&request, &evaluation, self.directory_options())
        {
            let matched_input = Input::Argv {
                executable: normalized.executable.clone(),
                arguments: normalized.arguments.clone(),
                original_input: original_input.clone(),
            };
            let matched_context = policy::resolution::normalized::Context {
                cwd: normalized.cwd.clone(),
                ..context.clone()
            };
            let (evaluation, complete) = self.evaluate_diagnostic_input(
                &normalized,
                policy::rules::Facts {
                    input: &matched_input,
                    context: &matched_context,
                    ..facts
                },
                None,
                checkpoint,
            )?;
            return Ok(NormalizedEvaluation::new(input, evaluation, complete)
                .with_directory_request(normalized));
        }
        let value = NormalizedEvaluation::new(input, evaluation, complete);
        self.shell_result(input, context, project, value, checkpoint)
    }

    fn evaluate_diagnostic_input(
        &self,
        request: &Request,
        facts: policy::rules::Facts<'_>,
        resolved: Option<&policy::resolution::compiled::ResolvedMcp>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<(policy::Evaluation, bool), Error> {
        use policy::resolution::normalized::Input;
        let observer = crate::location::Observer::new(
            facts.input,
            &request.project,
            &facts.context.cwd,
            facts.executable,
        )
        .with_access_paths(self.effective.access_paths())
        .with_working_files(facts.context.working_files.as_ref());
        let facts = policy::rules::Facts {
            locations: Some(&observer),
            ..facts
        };
        let mut evaluation = self
            .plan
            .evaluate_input_mcp(request, &facts, resolved, checkpoint)?;
        self.constrain_mcp(&mut evaluation, facts.input, request, resolved, checkpoint)?;
        let complete = !matches!(facts.input, Input::Unknown { .. } | Input::Shell { .. })
            && !self.plan.semantic()
            && observer.complete();
        Ok((evaluation, complete))
    }

    // Both diagnostics and native handoff resolve the complete request and its
    // inspected operations. Uncertainty cannot erase a matched whole-request deny.
    fn shell_result(
        &self,
        input: &policy::resolution::normalized::Input,
        context: &policy::resolution::normalized::Context,
        project: &str,
        mut value: NormalizedEvaluation,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<NormalizedEvaluation, Error> {
        let policy::resolution::normalized::Input::Shell {
            source,
            dialect,
            dialect_version,
            ..
        } = input
        else {
            return Ok(value);
        };
        let parsed = match dialect.as_str() {
            "bash" => crate::shell::Dialect::Bash,
            "posix" => crate::shell::Dialect::Posix,
            "powershell" => crate::shell::Dialect::Powershell,
            "cmd" => crate::shell::Dialect::Cmd,
            "unsupported" => crate::shell::Dialect::Unsupported,
            _ => return Err(Error::Contract("invalid shell dialect")),
        };
        let analysis = crate::shell::analyze_versioned_report_with_working_files(
            source,
            parsed,
            dialect_version.as_deref(),
            &context.cwd,
            context.environment.as_ref(),
            project,
            self,
            checkpoint,
            context.working_files.as_ref(),
        )?;
        value.shell(analysis);
        Ok(value)
    }
    /// Compose captured candidates and retain complete metadata once.
    ///
    /// # Errors
    /// Rejects invalid composition or missing retained selected artifacts.
    pub fn new(inputs: Vec<(Scope, Snapshot)>) -> Result<Self, Error> {
        Self::prepare(inputs, None)
    }

    /// Recover a retained source using only a known encoding that independently
    /// reproduces its recorded revision; no hash or policy identity is overridden.
    pub(crate) fn new_for_revision(
        inputs: Vec<(Scope, Snapshot)>,
        expected: &str,
    ) -> Result<Self, Error> {
        Self::prepare(inputs, Some(expected))
    }

    fn prepare(inputs: Vec<(Scope, Snapshot)>, expected: Option<&str>) -> Result<Self, Error> {
        let _trace = crate::diagnostics::trace::span("policy.prepare");
        crate::diagnostics::trace::count("policy.preparations", 1);
        let [(scope, snapshot)]: [(Scope, Snapshot); 1] = inputs
            .try_into()
            .map_err(|_| Error::Contract("exactly one project candidate required"))?;
        refuse_unset_required(&snapshot)?;
        let effective = compose(&[(scope, snapshot.lock.clone())])?;
        let effective = match expected {
            Some(expected) => effective.for_revision(expected)?,
            None => effective,
        };
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
        let plan = policy::resolution::compiled::Plan::compile(
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

    /// The revision these captured candidates would have, without compiling them.
    pub(crate) fn revision_of(inputs: &[(Scope, Snapshot)]) -> Result<String, Error> {
        let [(scope, snapshot)] = inputs else {
            return Err(Error::Contract("exactly one project candidate required"));
        };
        compose(&[(*scope, snapshot.lock.clone())])?.revision()
    }

    pub(crate) fn from_authenticated_compiled(bytes: Vec<u8>) -> Result<Self, Error> {
        Self::from_plan(policy::resolution::compiled::Plan::load_authenticated(
            bytes,
        )?)
    }
    fn from_plan(plan: policy::resolution::compiled::Plan) -> Result<Self, Error> {
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
    pub(crate) fn directory_options(&self) -> &[DirectoryOption] {
        self.plan.directory_options()
    }

    /// Codex prefixes of the active rules that allow.
    pub(crate) fn codex_prefixes(&self) -> &[policy::rules::agent_outputs::CodexPrefix] {
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
        // This API evaluates one argv invocation. Shell components use the
        // working-files API with the analyzer's explicit solitary marker.
        let scope = crate::location::WorkingFiles {
            sole_operation: true,
            ..Default::default()
        };
        self.evaluate_known(request, checkpoint, known_cwd, true, Some(&scope))
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

    pub(crate) fn evaluate_container_argv(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<CandidateEvaluation, Error> {
        let read = crate::location::container_read(&request.executable, &request.arguments);
        self.evaluate_written(request, checkpoint, false, true, None, read)
    }

    pub(crate) fn evaluate_unknown_arguments(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        known_cwd: bool,
    ) -> Result<CandidateEvaluation, Error> {
        self.evaluate_known(request, checkpoint, known_cwd, false, None)
    }

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
            false,
        )?;
        if known_cwd
            && let Some(normalized) =
                directory_fallback(request, written.evaluation(), self.directory_options())
        {
            let mut result = self.evaluate_written(
                &normalized,
                checkpoint,
                known_cwd,
                known_arguments,
                working_files,
                false,
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
        container_read: bool,
    ) -> Result<CandidateEvaluation, Error> {
        checkpoint()?;
        let input = policy::resolution::normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: serde_json::json!({}),
        };
        let context = policy::resolution::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: working_files.cloned(),
        };
        let now = crate::evidence::policy_now();
        let observer = crate::location::Observer::new(&input, &request.project, &request.cwd, None)
            .with_access_paths(self.effective.access_paths())
            .with_working_files(working_files);
        let approved = |source: &str| {
            self.effective
                .has_pack_artifact(source, &super::composition::CONTAINER_BASELINES)
        };
        let facts = policy::rules::Facts {
            input: &input,
            context: &context,
            executable: None,
            now: &now,
            known_cwd,
            known_arguments,
            locations: known_cwd.then_some(&observer as &dyn policy::rules::resources::Resources),
            container_read: container_read.then_some(&approved as &dyn Fn(&str) -> bool),
            effects: None,
        };
        let evaluation = self.plan.evaluate_input(request, &facts, checkpoint)?;
        checkpoint()?;
        Ok(self.candidate(evaluation))
    }
}

/// A declared directory option can retry an unmatched or yielding Ask.
/// An explicit decision for the submitted argv still takes precedence.
fn directory_fallback(
    request: &Request,
    written: &policy::Evaluation,
    options: &[DirectoryOption],
) -> Option<Request> {
    if written.fallback_only_ask() {
        directories::normalize(request, options)
    } else {
        None
    }
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
    pub(super) directory_options: Vec<DirectoryOption>,
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
        let directory_options = directories::selected(&packs, &custom)?;
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
        if let Some(normalized) = directory_fallback(request, &written, &self.directory_options) {
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
            private_input: false,
        })
    }
}
