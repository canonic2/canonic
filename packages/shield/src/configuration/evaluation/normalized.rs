//! Typed normalized and nested-shell diagnostic evidence.
use crate::{
    policy::{
        self, Decision,
        resolution::{
            diagnostics::{GuidanceRecord, GuidanceRequest},
            normalized::Input,
        },
    },
    shell::{Coverage, Operation, PolicyRequest, Report},
};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Evaluation {
    mode: &'static str,
    pub(crate) decision: Decision,
    disposition: &'static str,
    normalized_request: Input,
    pub(crate) explanation: policy::Evaluation,
    #[serde(serialize_with = "CoverageReport::serialize")]
    pub(crate) analysis_coverage: CoverageReport,
    approval_request_id: Option<&'static str>,
}

pub(crate) enum CoverageReport {
    Literal { complete: bool },
    Shell(Box<Report>),
}
impl CoverageReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Literal {
            complete: bool,
            model: &'static str,
            unknown: Option<&'static str>,
        }
        match self {
            Self::Literal { complete } => Literal {
                complete: *complete,
                model: "normalized-request-v1",
                unknown: (!complete)
                    .then_some("request or effect predicates extend beyond literal argv coverage"),
            }
            .serialize(serializer),
            Self::Shell(report) => report.serialize(serializer),
        }
    }
}

impl Evaluation {
    pub(crate) fn sensitive_sources(&self) -> Vec<&str> {
        match &self.analysis_coverage {
            CoverageReport::Shell(report) => report.sensitive_sources(),
            CoverageReport::Literal { .. } => Vec::new(),
        }
    }
    pub(crate) fn annotate_rule_references(
        &mut self,
        selectors: &std::collections::BTreeMap<String, String>,
    ) {
        self.explanation.annotate_rule_references(selectors);
        if let CoverageReport::Shell(report) = &mut self.analysis_coverage {
            report.visit_evaluations_mut(|candidate| {
                candidate
                    .evaluation_mut()
                    .annotate_rule_references(selectors);
            });
        }
    }
    pub(super) fn new(input: &Input, explanation: policy::Evaluation, complete: bool) -> Self {
        Self {
            mode: "diagnostic",
            decision: explanation.decision,
            disposition: "preview",
            normalized_request: input.clone(),
            explanation,
            analysis_coverage: CoverageReport::Literal { complete },
            approval_request_id: None,
        }
    }
    pub(super) fn shell(&mut self, mut analysis: Report) {
        if self.decision != Decision::Deny
            && (analysis.decision == Decision::Deny
                || (analysis.decision == Decision::Ask
                    && self.explanation.reason != "matched-exact-request")
                || self.explanation.matches.is_empty())
        {
            self.decision = analysis.decision;
        }
        if !analysis.supported() {
            self.decision = self.decision.max(Decision::Ask);
            analysis.mark_unsupported();
        }
        self.explanation.decision = self.decision;
        self.nested(&analysis);
        self.guidance(&analysis);
        self.analysis_coverage = CoverageReport::Shell(Box::new(analysis));
    }
    fn nested(&mut self, analysis: &Report) {
        for operation in analysis
            .operations
            .iter()
            .filter(|operation| operation.decision == self.decision)
        {
            let Some(evaluation) = operation
                .evaluation()
                .map(super::CandidateEvaluation::evaluation)
            else {
                continue;
            };
            let reference = reference(operation);
            for matched in evaluation
                .matches
                .iter()
                .filter(|matched| matched.applied || matched.yielded_reason.is_some())
            {
                if !self.explanation.matches.iter().any(|existing| {
                    existing.source == matched.source
                        && existing.rule_id == matched.rule_id
                        && existing.operation_ref.as_deref() == Some(&reference)
                }) {
                    let mut annotated = matched.clone();
                    annotated.operation_ref = Some(reference.clone());
                    annotated.executable = operation
                        .request()
                        .map(|request| request.executable.clone())
                        .into();
                    self.explanation.matches.push(annotated);
                }
                if matched.applied && self.explanation.reason == "unmatched-normalized-request" {
                    self.explanation.reason = "matched-policy";
                }
            }
            self.explanation.requirement_checks.extend(
                evaluation
                    .requirement_checks
                    .iter()
                    .cloned()
                    .map(|mut item| {
                        item.operation_ref.clone_from(&reference);
                        item
                    }),
            );
            self.explanation
                .guidance
                .extend(evaluation.guidance.iter().cloned().map(|mut item| {
                    item.operation_ref.clone_from(&reference);
                    item
                }));
        }
    }
    fn guidance(&mut self, analysis: &Report) {
        let denied = self.decision == Decision::Deny;
        if self.decision != Decision::Ask && !denied {
            return;
        }
        let authored = !self.explanation.guidance.is_empty();
        let candidates: Vec<_> = analysis
            .operations
            .iter()
            .filter(|operation| operation.decision == self.decision)
            .filter(|operation| !denied || operation.coverage == Coverage::OpaqueArguments)
            .filter_map(|operation| {
                reason(operation, &analysis.operations, authored, denied)
                    .map(|(rank, message, alternatives)| (rank, operation, message, alternatives))
            })
            .collect();
        let Some(chosen) = candidates.iter().min_by_key(|(rank, operation, ..)| {
            (*rank, operation.coverage != Coverage::BackgroundJobKill)
        }) else {
            return;
        };
        let describe = |operation: &Operation, message: &str| {
            let excerpt = analysis
                .source_units
                .get(operation.source_unit)
                .zip(operation.span())
                .and_then(|(unit, (start, end))| unit.source.get(start..end))
                .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|part| !part.is_empty() && part.chars().count() <= 120);
            excerpt.map_or_else(
                || format!("Part of this command {message}."),
                |part| format!("`{part}` {message}."),
            )
        };
        let (rank, operation, message, alternatives) = chosen;
        let sources = analysis.sensitive_sources();
        let private = |operation: &Operation| {
            operation.private_input()
                || analysis
                    .source_units
                    .get(operation.source_unit)
                    .is_some_and(|unit| sources.contains(&unit.source.as_str()))
        };
        let first = item(
            *rank,
            operation,
            describe(operation, message),
            alternatives,
            private(operation),
        );
        let others: Vec<_> = candidates
            .iter()
            .filter(|(rank, other, ..)| {
                !denied
                    && *rank == 2
                    && !std::ptr::eq(*other, *operation)
                    && other.coverage == Coverage::LiteralArgv
            })
            .map(|(rank, other, message, alternatives)| {
                item(
                    *rank,
                    other,
                    describe(other, message),
                    alternatives,
                    private(other),
                )
            })
            .collect();
        self.explanation.guidance.insert(0, first);
        self.explanation.guidance.extend(others);
    }
}

fn reference(operation: &Operation) -> String {
    match operation.span() {
        Some((start, end)) => format!("shell:{}:{start}-{end}", operation.source_unit),
        None => format!("shell:{}:null-null", operation.source_unit),
    }
}
fn launcher(operation: &Operation) -> Option<&str> {
    operation
        .request()
        .map(|request| {
            request
                .executable
                .rsplit('/')
                .next()
                .unwrap_or(&request.executable)
        })
        .filter(|name| crate::shell::LAUNCHERS.contains(name))
}
fn unwrapped(wrapper: &Operation, operations: &[Operation]) -> bool {
    let Some((start, end)) = wrapper.span() else {
        return false;
    };
    operations.iter().any(|inner| {
        inner.source_unit == wrapper.source_unit
            && inner.request().is_some()
            && inner.span().is_some_and(|(s, e)| s > start && e <= end)
    })
}
fn run_directly(name: Option<&str>) -> String {
    match name {
        Some(name) if !["eval","xargs"].contains(&name) => format!("Run each command directly as its own simple command, without `{name}`, sh -c, eval, xargs or find -exec."),
        _ => "Run each command directly as its own simple command, without sh -c, eval, xargs or find -exec.".into(),
    }
}
fn reason(
    operation: &Operation,
    operations: &[Operation],
    authored: bool,
    denied: bool,
) -> Option<(u8, String, Vec<String>)> {
    use Coverage as C;
    let rewrite =
        |message: &str, alternative: &str| (1, message.to_owned(), vec![alternative.to_owned()]);
    let launcher = launcher(operation);
    Some(match operation.coverage {
        _ if authored && !denied && operation.coverage != C::LiteralArgv => return None,
        C::IndirectExecution => (0,"runs further commands that Shield cannot inspect".into(),vec![run_directly(launcher)]),
        C::Partial if launcher.is_some() && unwrapped(operation,operations) => (2,format!("runs its command through `{}`, so the user decides",launcher.unwrap_or_default()),vec![]),
        C::Partial if launcher.is_some() => (0,"runs further commands that Shield cannot inspect".into(),vec![run_directly(launcher)]),
        C::ShellAssignment => rewrite("sets a shell variable that later commands may inherit","Write the value directly in each command instead of assigning a variable."),
        C::LoopExecutionSetting => rewrite("changes executable lookup through the loop's PATH binding","Use a loop variable that does not change executable lookup."),
        C::Opaque | C::OpaqueExpression | C::OpaqueArguments | C::PossibleExpansionMutation | C::PossibleBraceExpansion | C::FunctionCall | C::ControlTransfer => rewrite("has words or control flow that Shield cannot resolve before it runs","Use literal arguments and separate simple commands, without command substitution, computed names or shell functions."),
        C::Partial => (2,"runs a program whose effects Shield cannot inspect, so the user decides".into(),vec![]),
        C::AssignmentPrefix => (2,"sets environment variables that can change what the command does, so the user decides".into(),vec![]),
        C::ComputedAssignment => (2,"sets a shell variable from a value known only when it runs, so the user decides".into(),vec![]),
        C::BackgroundJobKill => (2,"signals a PID originating from a background job in this request; runtime ownership is unverified, so the user decides".into(),vec![]),
        C::RuntimeStatusArguments => (2,"uses an exit status known only when it runs, so the user decides".into(),vec![]),
        C::LiteralArgv => rule_ask(operation.evaluation()?.evaluation()),
        _ => return None,
    })
}
fn item(
    rank: u8,
    operation: &Operation,
    message: String,
    alternatives: &[String],
    private_input: bool,
) -> GuidanceRecord {
    let mut item = GuidanceRecord {
        alternatives: alternatives.into(),
        branch: None,
        candidate_only: true,
        message: Some(message),
        operation_ref: reference(operation),
        rule_ref: None,
        source: Some(if rank < 2 { "shield" } else { "decisive" }),
        request: None,
        fetch_url: policy::resolution::diagnostics::TextField::Absent,
        private_input,
    };
    if rank == 2
        && !matches!(
            operation.coverage,
            Coverage::BackgroundJobKill | Coverage::RuntimeStatusArguments
        )
        && let Some(request) = operation.request()
    {
        item.request = Some(GuidanceRequest {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            cwd: None,
        });
        match operation.policy_request() {
            Some(PolicyRequest::DirectoryOption {
                request,
                directory_option: true,
                ..
            }) if std::path::Path::new(&request.cwd).starts_with(&request.project)
                && !std::path::Path::new(&request.cwd)
                    .components()
                    .any(|part| part == std::path::Component::ParentDir) =>
            {
                item.request = Some(GuidanceRequest {
                    executable: request.executable.clone(),
                    arguments: request.arguments.clone(),
                    cwd: Some("project"),
                });
            }
            Some(PolicyRequest::Normalized(Input::Web { action, url, .. }))
                if action == "fetch" =>
            {
                item.fetch_url = url.clone().into();
            }
            _ => {}
        }
    }
    item
}
fn rule_ask(evaluation: &policy::Evaluation) -> (u8, String, Vec<String>) {
    if evaluation.matches.is_empty() {
        return (2,"matches no rule, so the user decides".into(),vec!["If it should run without asking, propose a pack or rule that covers it with policy_propose.".into()]);
    }
    let guidance = evaluation.guidance.first();
    let alternatives = guidance.map_or_default(|item| item.alternatives.clone());
    let applied = evaluation
        .matches
        .iter()
        .find(|matched| matched.applied && matched.decision == Decision::Ask);
    let why = guidance
        .and_then(|item| item.message.as_deref())
        .or_else(|| {
            applied
                .map(|matched| matched.reason.as_str())
                .filter(|reason| !reason.is_empty())
        })
        .map(|why| format!("asks for approval: {}", why.trim_end_matches('.')))
        .or_else(|| {
            applied.map(|matched| format!("asks for approval under rule {}", matched.rule_id))
        })
        .unwrap_or_else(|| "asks for approval".into());
    (2, why, alternatives)
}
