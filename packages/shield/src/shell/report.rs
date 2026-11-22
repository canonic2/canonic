//! Typed raw shell evidence. JSON is only a compatibility/display boundary.
use super::{Dialect, provenance, redirection, structure};
use crate::{
    configuration::CandidateEvaluation,
    effects,
    policy::{Decision, Request, resolution::normalized::Input},
};
use serde::{Serialize, ser::SerializeStruct};
mod projection;

pub(crate) struct Report {
    pub(crate) decision: Decision,
    pub(crate) dialect: Dialect,
    pub(crate) dialect_version: Option<String>,
    pub(crate) source: String,
    pub(crate) operations: Vec<Operation>,
    pub(crate) source_units: Vec<SourceUnit>,
    pub(crate) policy_revision: String,
    pub(crate) coverage: ReportCoverage,
    pub(super) structure: structure::Node,
    pub(super) native: bool,
    unsupported: bool,
    sensitive_units: Vec<bool>,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ReportCoverage {
    SingleLiteralInvocation,
    StructuredLiteralOperations,
    Partial,
}
impl Report {
    #[expect(
        clippy::too_many_arguments,
        reason = "collects the existing bounded analysis products without a JSON intermediate"
    )]
    pub(super) fn new(
        decision: Decision,
        dialect: Dialect,
        dialect_version: Option<&str>,
        source: &str,
        mut operations: Vec<Operation>,
        mut source_units: Vec<SourceUnit>,
        policy_revision: &str,
        mut structure: structure::Node,
        complete: bool,
        native: bool,
        checkpoint: &dyn Fn() -> Result<(), crate::policy::Error>,
    ) -> Result<Self, crate::policy::Error> {
        let sensitive_units = mark_private_evidence(
            &mut structure,
            &mut source_units,
            &mut operations,
            checkpoint,
        )?;
        let coverage = if !complete {
            ReportCoverage::Partial
        } else if matches!(structure, structure::Node::Command { .. }) {
            ReportCoverage::SingleLiteralInvocation
        } else {
            ReportCoverage::StructuredLiteralOperations
        };
        Ok(Self {
            decision,
            dialect,
            dialect_version: dialect_version.map(str::to_owned),
            source: source.into(),
            operations,
            source_units,
            policy_revision: policy_revision.into(),
            coverage,
            structure,
            native,
            unsupported: false,
            sensitive_units,
        })
    }
    pub(crate) fn supported(&self) -> bool {
        match self.dialect {
            Dialect::Bash | Dialect::Posix => true,
            Dialect::Powershell | Dialect::Cmd => {
                self.native && self.coverage != ReportCoverage::Partial
            }
            Dialect::Unsupported => false,
        }
    }
    pub(crate) fn mark_unsupported(&mut self) {
        self.unsupported = true;
    }
    #[cfg(test)]
    pub(crate) fn complete(&self) -> bool {
        !self.unsupported && self.coverage != ReportCoverage::Partial
    }
    /// Only complete source-unit strings with owner-proven credential-bearing
    /// words become echo masks. Individual policy scalar values are not masks.
    pub(crate) fn sensitive_sources(&self) -> Vec<&str> {
        self.source_units
            .iter()
            .zip(&self.sensitive_units)
            .filter_map(|(unit, private)| private.then_some(unit.source.as_str()))
            .collect()
    }
    pub(crate) fn visit_evaluations_mut(
        &mut self,
        mut visitor: impl FnMut(&mut CandidateEvaluation),
    ) {
        for operation in &mut self.operations {
            match &mut operation.data {
                Data::Invocation { evaluation, .. } | Data::File { evaluation, .. } => {
                    visitor(evaluation);
                }
                _ => {}
            }
        }
    }
    pub(crate) fn into_value(self) -> Result<serde_json::Value, crate::policy::Error> {
        serde_json::to_value(self)
            .map_err(|_| crate::policy::Error::Contract("cannot encode shell evidence"))
    }
}

fn mark_private_evidence(
    root: &mut structure::Node,
    units: &mut [SourceUnit],
    operations: &mut [Operation],
    checkpoint: &dyn Fn() -> Result<(), crate::policy::Error>,
) -> Result<Vec<bool>, crate::policy::Error> {
    use structure::evidence;
    evidence::mark(root);
    let mut private = vec![false; units.len()];
    if let Some(root_private) = private.first_mut() {
        *root_private = evidence::opaque(root) || !evidence::spans(root).is_empty();
    }
    for (index, unit) in units.iter_mut().enumerate().skip(1) {
        checkpoint()?;
        if let Some(node) = &mut unit.structure {
            evidence::mark(node);
            private[index] = evidence::opaque(node) || !evidence::spans(node).is_empty();
        }
    }
    // Child source units are appended after their parents. A private child
    // masks its enclosing source span, including escaped/quoted source echoes.
    for index in (1..units.len()).rev() {
        if !private[index] {
            continue;
        }
        checkpoint()?;
        let parent = units[index]
            .parent
            .as_ref()
            .ok_or(crate::policy::Error::Contract(
                "shell source parent missing",
            ))?;
        let (parent_unit, span) = (parent.source_unit, parent.span);
        if parent_unit >= index {
            return Err(crate::policy::Error::Contract(
                "shell source parent invalid",
            ));
        }
        if parent_unit == 0 {
            evidence::mark_span(root, span);
        } else if let Some(node) = &mut units[parent_unit].structure {
            evidence::mark_span(node, span);
        }
        private[parent_unit] = true;
    }
    let private_input = private_descendants(root, units, checkpoint)?;
    for (source, descendant) in private.iter_mut().zip(&private_input) {
        *source |= *descendant;
    }
    let mut spans = Vec::with_capacity(units.len());
    spans.push(evidence::spans(root));
    for unit in units.iter_mut().skip(1) {
        spans.push(
            unit.structure
                .as_mut()
                .map_or_else(Vec::new, evidence::spans),
        );
    }
    for operation in operations {
        checkpoint()?;
        let Some(ranges) = spans.get(operation.source_unit) else {
            return Err(crate::policy::Error::Contract(
                "shell operation source invalid",
            ));
        };
        operation.private_input = private_input[operation.source_unit];
        if operation.private_input {
            match &mut operation.data {
                Data::Invocation { evaluation, .. } | Data::File { evaluation, .. } => {
                    evaluation.mark_private_input();
                }
                _ => {}
            }
        }
        match &mut operation.data {
            Data::Assignments { assignments }
            | Data::Expression {
                words: assignments, ..
            } => {
                for word in assignments {
                    evidence::mark_ranges(word, ranges);
                }
            }
            Data::Redirection { redirection, .. } => {
                evidence::mark_ranges(&mut redirection.target, ranges);
                if let Some(body) = &mut redirection.heredoc {
                    body.private_evidence |= evidence::intersects(body.span, ranges);
                }
            }
            _ => {}
        }
    }
    Ok(private)
}

/// Parent-first source geometry, independent of values or process output.
fn private_descendants(
    root: &mut structure::Node,
    units: &mut [SourceUnit],
    checkpoint: &dyn Fn() -> Result<(), crate::policy::Error>,
) -> Result<Vec<bool>, crate::policy::Error> {
    use structure::evidence;
    let mut private = vec![false; units.len()];
    for index in 1..units.len() {
        checkpoint()?;
        let parent = units[index]
            .parent
            .as_ref()
            .ok_or(crate::policy::Error::Contract(
                "shell source parent missing",
            ))?;
        let (parent_unit, span) = (parent.source_unit, parent.span);
        if parent_unit >= index {
            return Err(crate::policy::Error::Contract(
                "shell source parent invalid",
            ));
        }
        let ranges = if parent_unit == 0 {
            evidence::spans(root)
        } else {
            units[parent_unit]
                .structure
                .as_mut()
                .map_or_else(Vec::new, evidence::spans)
        };
        if private[parent_unit] || evidence::intersects(span, &ranges) {
            private[index] = true;
            let end = units[index].source.len();
            if let Some(node) = &mut units[index].structure {
                evidence::mark_span(node, structure::Span { start: 0, end });
            }
        }
    }
    Ok(private)
}
impl Serialize for Report {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 15
            + usize::from(self.native && self.coverage != ReportCoverage::Partial)
            + usize::from(self.dialect_version.is_some())
            + if self.unsupported { 2 } else { 0 };
        let mut report = serializer.serialize_struct("ShellReport", fields)?;
        report.serialize_field("schema_version", &2u8)?;
        report.serialize_field("mode", "preview")?;
        report.serialize_field("model", "structured-literal-shell-v2")?;
        report.serialize_field("dialect", &self.dialect)?;
        report.serialize_field("source", &self.source)?;
        report.serialize_field(
            "span",
            &structure::Span {
                start: 0,
                end: self.source.len(),
            },
        )?;
        report.serialize_field("coverage", &self.coverage)?;
        report.serialize_field("decision", &self.decision)?;
        let single = (self.coverage == ReportCoverage::SingleLiteralInvocation)
            .then(|| self.operations.first())
            .flatten();
        report.serialize_field(
            "request",
            &single.and_then(|operation| {
                operation.request().map(|request| {
                    crate::policy::resolution::diagnostics::RequestEvidence::new(
                        request,
                        operation.private_input(),
                    )
                })
            }),
        )?;
        report.serialize_field("evaluation", &single.and_then(Operation::evaluation))?;
        report.serialize_field("structure", &self.structure)?;
        report.serialize_field("operations", &self.operations)?;
        report.serialize_field("source_units", &self.source_units)?;
        report.serialize_field("policy_revision", &self.policy_revision)?;
        report.serialize_field("permission_domain", "literal-argv")?;
        if self.native && self.coverage != ReportCoverage::Partial {
            report.serialize_field("native_coverage", "literal-native-v1")?;
        }
        if let Some(version) = &self.dialect_version {
            report.serialize_field("dialect_version", version)?;
        }
        if self.unsupported {
            report.serialize_field("complete", &false)?;
            report.serialize_field("unknown", "shell dialect analysis is unsupported")?;
        }
        report.end()
    }
}

#[derive(Serialize)]
pub(crate) struct SourceUnit {
    pub(crate) source: String,
    pub(crate) dialect: Dialect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dialect_version: Option<String>,
    pub(super) parent: Option<Parent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) structure: Option<structure::Node>,
}
#[derive(Serialize)]
pub(super) struct Parent {
    pub(super) source_unit: usize,
    pub(super) span: structure::Span,
    pub(super) kind: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Coverage {
    AssignmentPrefix,
    ShellAssignment,
    ComputedAssignment,
    FunctionDefinition,
    DescriptorFlow,
    InlineInputRedirection,
    NullDeviceRedirection,
    DescriptorDuplication,
    Redirection,
    FileRedirection,
    Opaque,
    FunctionCall,
    RuntimeStatusArguments,
    OpaqueArguments,
    BackgroundJobKill,
    UnchangedWorkingDirectory,
    ObservedWorkingDirectoryChange,
    ResolvedControlTransfer,
    Partial,
    LiteralArgv,
    ControlTransfer,
    IndirectExecution,
    PossibleBraceExpansion,
    PossibleExpansionMutation,
    Coprocess,
    BackgroundJob,
    LoopExecutionSetting,
    OpaqueExpression,
}

pub(crate) struct Operation {
    span: Option<structure::Span>,
    pub(crate) coverage: Coverage,
    pub(crate) decision: Decision,
    pub(crate) source_unit: usize,
    pub(crate) cwd: String,
    pub(crate) dialect: Dialect,
    pub(super) data: Data,
    private_input: bool,
}
impl Operation {
    pub(super) fn new(coverage: Coverage, decision: Decision, data: Data) -> Self {
        Self {
            span: None,
            coverage,
            decision,
            source_unit: 0,
            cwd: String::new(),
            dialect: Dialect::Unsupported,
            data,
            private_input: false,
        }
    }
    pub(super) fn at(mut self, span: structure::Span) -> Self {
        self.span = Some(span);
        self
    }
    pub(crate) fn span(&self) -> Option<(usize, usize)> {
        self.span.map(|span| (span.start, span.end))
    }
    pub(crate) fn private_input(&self) -> bool {
        self.private_input
    }
    pub(crate) fn request(&self) -> Option<&Request> {
        match &self.data {
            Data::Invocation { request, .. } => Some(request),
            _ => None,
        }
    }
    pub(crate) fn evaluation(&self) -> Option<&CandidateEvaluation> {
        match &self.data {
            Data::Invocation { evaluation, .. } | Data::File { evaluation, .. } => Some(evaluation),
            _ => None,
        }
    }
    pub(crate) fn policy_request(&self) -> Option<&PolicyRequest> {
        match &self.data {
            Data::Invocation { detail, .. } => match detail.as_ref() {
                Invocation::Literal { policy_request, .. }
                | Invocation::Foreign { policy_request, .. } => policy_request.as_deref(),
                _ => None,
            },
            _ => None,
        }
    }
}

pub(super) enum Data {
    Invocation {
        request: Request,
        evaluation: Box<CandidateEvaluation>,
        detail: Box<Invocation>,
    },
    File {
        normalized_request: Input,
        evaluation: Box<CandidateEvaluation>,
        context: Facts,
    },
    Redirection {
        redirection: structure::Redirection,
        context: Option<Facts>,
    },
    Assignments {
        assignments: Vec<structure::Word>,
    },
    Named {
        name: String,
    },
    Operator {
        operator: String,
    },
    Descriptor {
        descriptor_flow: redirection::Flow,
    },
    Indirect {
        nested: Nested,
    },
    Background {
        origin: provenance::Job,
    },
    Variable {
        variable: String,
    },
    Expression {
        syntax: String,
        words: Vec<structure::Word>,
    },
    Syntax {
        syntax: &'static str,
    },
    Empty {},
}
pub(super) enum Invocation {
    Literal {
        effects: Option<Box<effects::Hints>>,
        evaluation_model: &'static str,
        context: Facts,
        policy_request: Option<Box<PolicyRequest>>,
        transition: Option<Transition>,
        stdin: Option<&'static str>,
    },
    Bound {
        evaluation_model: &'static str,
        context: Facts,
    },
    Kill {
        targets: Vec<provenance::Job>,
        runtime_ownership: &'static str,
        evaluation_model: &'static str,
        context: Facts,
    },
    Foreign {
        effects: Option<std::rc::Rc<effects::Hints>>,
        evaluation_model: &'static str,
        context: Facts,
        policy_request: Option<Box<PolicyRequest>>,
    },
}
#[derive(Serialize)]
pub(super) struct Transition {
    pub(super) cwd_success: String,
    pub(super) cwd_failure: String,
    pub(super) logical_cwd_success: Option<String>,
}
#[derive(Serialize)]
pub(super) struct Facts {
    pub(super) cwd_known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) environment_known: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) arguments_known: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) namespace: Option<&'static str>,
}
impl Facts {
    pub(super) fn cwd(known: bool) -> Self {
        Self {
            cwd_known: known,
            environment_known: None,
            arguments_known: None,
            namespace: None,
        }
    }
    pub(super) fn environment(mut self, known: bool) -> Self {
        self.environment_known = Some(known);
        self
    }
    pub(super) fn arguments(mut self, known: bool) -> Self {
        self.arguments_known = Some(known);
        self
    }
    pub(super) fn namespace(mut self, namespace: &'static str) -> Self {
        self.namespace = Some(namespace);
        self
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum PolicyRequest {
    Normalized(Input),
    DirectoryOption {
        kind: &'static str,
        #[serde(flatten)]
        request: Request,
        directory_option: bool,
    },
}
#[derive(Serialize)]
pub(super) struct Nested {
    kind: &'static str,
    source: Option<String>,
    argv: Vec<String>,
    argument_index: usize,
    namespace: &'static str,
}
impl From<&effects::NestedExecution> for Nested {
    fn from(nested: &effects::NestedExecution) -> Self {
        Self {
            kind: nested.kind,
            source: nested.source.clone(),
            argv: nested.argv.clone(),
            argument_index: nested.argument_index,
            namespace: nested.namespace,
        }
    }
}
