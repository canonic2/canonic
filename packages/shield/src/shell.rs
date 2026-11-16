//! Owns bounded shell structure, literal operation previews, and conservative aggregation.

mod arithmetic;
mod bindings;
mod brace;
mod budget;
mod cmd;
mod control;
mod cwd;
mod expansion;
mod flow;
mod native;
mod powershell;
mod provenance;
mod redirection;
mod report;
mod state;
mod structure;
mod words;
mod wrappers;

use crate::{
    configuration::Evaluator,
    policy::{Decision, Error, Request},
};
#[cfg(test)]
use report::ReportCoverage;
pub(crate) use report::{Coverage, Operation, PolicyRequest, Report, SourceUnit};
use report::{Data, Facts as ReportFacts, Invocation, Parent as SourceParent, Transition};
use serde_json::{Value, json};
use state::{FunctionBinding, State, state_effect, taint};
use std::{collections::BTreeMap, rc::Rc};
use structure::Node;
use words::{assignment, literal_words};

#[cfg(test)]
mod tests;

/// Explicit source dialect; unsupported dialects remain unclassified.
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    /// POSIX quoting for the bounded literal subset.
    Posix,
    /// Bash quoting for the bounded literal subset; history syntax is opaque.
    Bash,
    /// Common-version literal native PowerShell invocations and sequences.
    Powershell,
    /// Bounded literal native cmd invocations and command chains.
    Cmd,
    /// A selected shell whose actual dialect has not been established.
    Unsupported,
}

/// Preview structured literal shell operations, conservatively aggregating every branch.
/// No source, expansion, executable, or subprocess is executed.
///
/// # Errors
/// Rejects resource exhaustion, NUL, malformed supported quoting, and invalid contexts.
pub fn analyze(
    source: &str,
    dialect: Dialect,
    cwd: &str,
    project: &str,
    evaluator: &Evaluator,
) -> Result<Value, Error> {
    analyze_controlled(source, dialect, cwd, None, project, evaluator, &|| Ok(()))
}

pub(crate) fn analyze_controlled(
    source: &str,
    dialect: Dialect,
    cwd: &str,
    environment: Option<&BTreeMap<String, String>>,
    project: &str,
    evaluator: &Evaluator,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Value, Error> {
    analyze_with_working_files(
        source,
        dialect,
        cwd,
        environment,
        project,
        evaluator,
        checkpoint,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "adds adapter-bound artifact observations to the existing shell entry"
)]
pub(crate) fn analyze_with_working_files(
    source: &str,
    dialect: Dialect,
    cwd: &str,
    environment: Option<&BTreeMap<String, String>>,
    project: &str,
    evaluator: &Evaluator,
    checkpoint: &dyn Fn() -> Result<(), Error>,
    working_files: Option<&crate::location::WorkingFiles>,
) -> Result<Value, Error> {
    analyze_versioned_with_working_files(
        source,
        dialect,
        None,
        cwd,
        environment,
        project,
        evaluator,
        checkpoint,
        working_files,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "preserves reported dialect version alongside request-owned shell observations"
)]
pub(crate) fn analyze_versioned_with_working_files(
    source: &str,
    dialect: Dialect,
    dialect_version: Option<&str>,
    cwd: &str,
    environment: Option<&BTreeMap<String, String>>,
    project: &str,
    evaluator: &Evaluator,
    checkpoint: &dyn Fn() -> Result<(), Error>,
    working_files: Option<&crate::location::WorkingFiles>,
) -> Result<Value, Error> {
    analyze_versioned_report_with_working_files(
        source,
        dialect,
        dialect_version,
        cwd,
        environment,
        project,
        evaluator,
        checkpoint,
        working_files,
    )?
    .into_value()
}

#[expect(
    clippy::too_many_arguments,
    reason = "preserves the existing explicit shell analysis context"
)]
pub(crate) fn analyze_versioned_report_with_working_files(
    source: &str,
    dialect: Dialect,
    dialect_version: Option<&str>,
    cwd: &str,
    environment: Option<&BTreeMap<String, String>>,
    project: &str,
    evaluator: &Evaluator,
    checkpoint: &dyn Fn() -> Result<(), Error>,
    working_files: Option<&crate::location::WorkingFiles>,
) -> Result<Report, Error> {
    checkpoint()?;
    validate_version(dialect_version)?;
    if source.len() > 65_536 {
        return Err(Error::Limit);
    }
    if source.contains('\0') {
        return Err(Error::Contract("NUL in shell source"));
    }
    if source.trim().is_empty() {
        return Err(Error::Contract("empty shell source"));
    }
    let context = Request {
        executable: "shield-context-validation".into(),
        arguments: vec![],
        cwd: cwd.into(),
        project: project.into(),
    };
    context.validate()?;
    let opaque = || Node::Opaque {
        span: structure::Span {
            start: 0,
            end: source.len(),
        },
    };
    let tree = match dialect {
        Dialect::Posix | Dialect::Bash => structure::parse(source, dialect, checkpoint)?,
        Dialect::Powershell => powershell::parse(source, checkpoint)?.unwrap_or_else(opaque),
        Dialect::Cmd => cmd::parse(source, checkpoint)?.unwrap_or_else(opaque),
        Dialect::Unsupported => opaque(),
    };
    let native = matches!(dialect, Dialect::Powershell | Dialect::Cmd)
        && !matches!(tree, Node::Opaque { .. });
    // Missing inputs are inert only when nothing else in the request can
    // create them first. This observation adds no adapter file permissions.
    let sole = sole_filter(&tree).then(|| crate::location::WorkingFiles {
        sole_operation: true,
        ..working_files.cloned().unwrap_or_default()
    });
    let working_files = sole.as_ref().or(working_files);
    let mut analysis = Analysis {
        evaluator,
        working_files,
        checkpoint,
        context,
        directories: crate::location::directory::Directories::new(),
        clients: ClientEnvironment {
            node_preload: environment
                .and_then(|environment| environment.get("NODE_OPTIONS"))
                .is_some_and(|options| node_preload_options(options)),
            http_configuration: environment
                .is_some_and(crate::http_client::environment_changes_request),
            inherited: environment.map(|environment| environment.keys().cloned().collect()),
            // Ordinary agent shells use cwd-relative lookup. Hidden shell-local
            // settings are outside this preview contract; reported CDPATH and
            // source-level environment changes still retain uncertainty.
            cdpath_empty: environment
                .is_none_or(|environment| environment.get("CDPATH").is_none_or(String::is_empty)),
        },
        operations: Vec::new(),
        evidence_bytes: 0,
        complete: true,
        dialect,
        nested_bytes: source.len(),
        source_unit: 0,
        source_units: vec![SourceUnit {
            source: source.into(),
            dialect,
            dialect_version: dialect_version.map(str::to_owned),
            parent: None,
            structure: None,
        }],
        frames: Vec::new(),
        suppressed: false,
        exit_bytes: 0,
    };
    // Shells set IFS at startup rather than importing it from the environment.
    let mut state = State::default();
    state.cwd_paths.push(cwd::Directory {
        physical: cwd.into(),
        previous: None,
        logical: environment
            .and_then(|environment| environment.get("PWD"))
            .filter(|reported| reported.as_str() == cwd)
            .cloned(),
        derived: false,
    });
    state
        .variables
        .insert("IFS".into(), expansion::DEFAULT_IFS.into());
    let decision = analysis.visit(&tree, &mut state, 0)?;
    checkpoint()?;
    let result = Report::new(
        decision,
        dialect,
        dialect_version,
        source,
        analysis.operations,
        analysis.source_units,
        evaluator.revision(),
        tree,
        analysis.complete,
        native,
        checkpoint,
    )?;
    budget::measure(&result, budget::MAX_EVIDENCE_BYTES, checkpoint)?;
    Ok(result)
}

pub(crate) fn validate_version(version: Option<&str>) -> Result<(), Error> {
    if version.is_some_and(|version| {
        version.is_empty() || version.len() > 64 || version.chars().any(char::is_control)
    }) {
        return Err(Error::Contract(
            "shell dialect version must be bounded reported text",
        ));
    }
    Ok(())
}

/// Only coverage produced by the complete dedicated native parser is admitted.
pub(crate) fn supported_dialect(analysis: &Value) -> bool {
    match analysis["dialect"].as_str() {
        Some("bash" | "posix") => true,
        Some("powershell" | "cmd") => {
            analysis["native_coverage"] == "literal-native-v1"
                && matches!(
                    analysis["coverage"].as_str(),
                    Some("single-literal-invocation" | "structured-literal-operations")
                )
        }
        _ => false,
    }
}

pub(crate) fn opaque_native(analysis: &Value) -> bool {
    matches!(
        analysis["dialect"].as_str(),
        Some("powershell" | "cmd" | "unsupported")
    ) && !supported_dialect(analysis)
}

struct Analysis<'a> {
    evaluator: &'a Evaluator,
    working_files: Option<&'a crate::location::WorkingFiles>,
    checkpoint: &'a dyn Fn() -> Result<(), Error>,
    context: Request,
    directories: crate::location::directory::Directories,
    clients: ClientEnvironment,
    operations: Vec<Operation>,
    evidence_bytes: usize,
    complete: bool,
    dialect: Dialect,
    nested_bytes: usize,
    source_unit: usize,
    source_units: Vec<SourceUnit>,
    frames: Vec<control::Frame>,
    suppressed: bool,
    exit_bytes: usize,
}

/// Reported environment effects that prevent classifying client commands as pure reads.
struct ClientEnvironment {
    node_preload: bool,
    http_configuration: bool,
    /// Names of the reported environment, when the host reported one.
    inherited: Option<std::collections::BTreeSet<String>>,
    cdpath_empty: bool,
}

fn redirection_actions(operator: &str) -> &'static [&'static str] {
    match operator {
        "<" => &["read"],
        "<>" => &["read", "write"],
        ">" | ">|" | ">>" | "&>" | "&>>" => &["write"],
        _ => &[],
    }
}

/// Syntax-only redirections open no ordinary path; their expansions and any
/// interpreter bodies still pass through the surrounding traversal.
fn inert_redirection(redirect: &structure::Redirection) -> Option<Coverage> {
    if matches!(redirect.operator.as_str(), "<<" | "<<-" | "<<<")
        && redirect
            .descriptor
            .as_ref()
            .is_none_or(|descriptor| descriptor.parse::<u32>().is_ok())
        && (redirect.operator == "<<<" || redirect.heredoc.is_some())
    {
        Some(Coverage::InlineInputRedirection)
    } else if !redirection_actions(&redirect.operator).is_empty()
        && redirect.target.literal.as_deref() == Some("/dev/null")
    {
        Some(Coverage::NullDeviceRedirection)
    } else if ["<&", ">&"].contains(&redirect.operator.as_str())
        && redirect.target.literal.as_deref().is_some_and(|value| {
            let number = value.strip_suffix('-').unwrap_or(value);
            value == "-" || (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
        })
    {
        Some(Coverage::DescriptorDuplication)
    } else {
        None
    }
}

impl Analysis<'_> {
    fn simple_command(
        &mut self,
        span: structure::Span,
        words: &[structure::Word],
        redirections: &[structure::Redirection],
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        // A redirection of descriptor 0 replaces or closes the piped input.
        if redirections.iter().any(|redirect| {
            redirect
                .descriptor
                .as_deref()
                .map_or(redirect.operator.starts_with('<'), |fd| fd == "0")
        }) {
            state.stdin_pipe = false;
        }
        let redirects = self.redirections(redirections, words, state, depth)?;
        if words.is_empty() {
            state.stdin_pipe = false;
            return Ok(redirects.max(Decision::Ask));
        }
        let prefix = words
            .iter()
            .take_while(|word| assignment(&word.raw))
            .count();
        if prefix > 0 {
            self.complete = false;
            let expansions = self.expansions(&words[..prefix], state, depth)?;
            // A variable the environment already exports reaches every later
            // child, and zsh ties names such as `path` to PATH. Only a literal
            // value, unlike `$!` or `$(…)`, can be written into the commands.
            let coverage = if prefix < words.len() {
                Coverage::AssignmentPrefix
            } else if words.iter().all(|word| word.literal.is_some()) {
                Coverage::ShellAssignment
            } else {
                Coverage::ComputedAssignment
            };
            self.record(
                Operation::new(
                    coverage,
                    Decision::Ask,
                    Data::Assignments {
                        assignments: words[..prefix].to_vec(),
                    },
                )
                .at(span),
            )?;
            if prefix == words.len() {
                state.environment_unknown = true;
                bindings::assignments(words, state, self.dialect, false)?;
                return Ok(expansions.max(redirects).max(Decision::Ask));
            }
            let mut launch = state.clone();
            launch.environment_unknown = true;
            let operands =
                positional_words(&words[prefix..], state, self.dialect, self.checkpoint)?;
            // Operand expansion uses the parent values. Temporary assignment
            // bindings are then visible to a called function's body.
            bindings::assignments(&words[..prefix], &mut launch, self.dialect, true)?;
            let decision = self.command(span, &operands, &mut launch, depth, false, Some(state))?;
            if self.dialect == Dialect::Bash
                && operands
                    .first()
                    .and_then(|word| word.literal.as_ref())
                    .is_some_and(|name| state.functions.contains_key(name))
            {
                bindings::restore_prefix(&words[..prefix], state, &mut launch);
            }
            if words[prefix]
                .literal
                .as_deref()
                .is_none_or(|name| opaque_executable(name) || state.functions.contains_key(name))
            {
                state.join(&launch);
            }
            return Ok(decision.max(expansions).max(redirects).max(Decision::Ask));
        }
        Ok(self
            .command(span, words, state, depth, false, None)?
            .max(redirects))
    }

    fn define_function(
        &mut self,
        name: &str,
        body: &Node,
        state: &mut State,
    ) -> Result<Decision, Error> {
        self.complete = false;
        state.functions.insert(
            name.into(),
            vec![FunctionBinding {
                body: Rc::new(body.clone()),
                source_unit: self.source_unit,
                dialect: self.dialect,
            }],
        );
        state.defined_functions.insert(name.into());
        self.record(Operation::new(
            Coverage::FunctionDefinition,
            Decision::Ask,
            Data::Named { name: name.into() },
        ))?;
        Ok(Decision::Ask)
    }

    fn redirections(
        &mut self,
        redirections: &[structure::Redirection],
        words: &[structure::Word],
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        let flow = redirection::analyze(redirections);
        // Descriptor evidence moves into the report; traversal needs only the
        // final stdin target to find interpreter bodies supplied by redirects.
        let stdin = flow.stdin.clone();
        let mut decision = if flow.syntax_complete {
            Decision::Allow
        } else {
            Decision::Ask
        };
        if !redirections.is_empty() {
            self.record(Operation::new(
                Coverage::DescriptorFlow,
                decision,
                Data::Descriptor {
                    descriptor_flow: flow,
                },
            ))?;
        }
        for (index, redirection) in redirections.iter().enumerate() {
            let opened = self.file_redirection(redirection, state)?;
            if opened != Decision::Allow {
                self.complete = false;
            }
            decision = decision.max(opened);
            if let Some(body) = &redirection.heredoc {
                if body.expand {
                    for mut substitution in structure::heredoc_substitutions(
                        &body.source,
                        body.span.start,
                        self.checkpoint,
                    )? {
                        substitution.span = structure::heredoc_span(body, substitution.span);
                        decision = decision.max(self.substitution(&substitution, state, depth)?);
                    }
                }
                if let Some(dialect) = stdin_shell(words, state)
                    && stdin == (redirection::Target::Heredoc { index })
                    && !body.source.trim().is_empty()
                {
                    let previous = self.dialect;
                    self.dialect = dialect;
                    let mut launch = state.clone();
                    launch.environment_unknown = true;
                    launch.functions.clear();
                    launch.defined_functions.clear();
                    launch.locals.clear();
                    bindings::stdin_environment(words, &mut launch, previous)?;
                    launch.positionals = Some(Vec::new());
                    launch.arg0 = None;
                    let result = self.embedded(
                        &body.source,
                        body.span,
                        "interpreter-heredoc",
                        &launch,
                        depth,
                    );
                    self.dialect = previous;
                    decision = decision.max(result?);
                }
            } else {
                decision = decision.max(self.expansions(
                    std::slice::from_ref(&redirection.target),
                    state,
                    depth,
                )?);
                if stdin == (redirection::Target::HereString { index })
                    && let Some(dialect) = stdin_shell(words, state)
                    && let Some(source) = &redirection.target.literal
                    && !source.trim().is_empty()
                {
                    let previous = self.dialect;
                    self.dialect = dialect;
                    let mut launch = state.clone();
                    launch.environment_unknown = true;
                    launch.functions.clear();
                    launch.defined_functions.clear();
                    launch.locals.clear();
                    bindings::stdin_environment(words, &mut launch, previous)?;
                    launch.positionals = Some(Vec::new());
                    launch.arg0 = None;
                    let result = self.embedded(
                        source,
                        redirection.target.span,
                        "interpreter-here-string",
                        &launch,
                        depth,
                    );
                    self.dialect = previous;
                    decision = decision.max(result?);
                }
            }
        }
        Ok(decision)
    }

    fn file_redirection(
        &mut self,
        redirect: &structure::Redirection,
        state: &mut State,
    ) -> Result<Decision, Error> {
        use crate::policy::resolution::normalized::{Context, Input};
        if let Some(coverage) = inert_redirection(redirect) {
            self.record(
                Operation::new(
                    coverage,
                    Decision::Allow,
                    Data::Redirection {
                        redirection: redirect.clone(),
                        context: None,
                    },
                )
                .at(redirect.span),
            )?;
            return Ok(Decision::Allow);
        }
        let actions = redirection_actions(&redirect.operator);
        let Some(path) = redirect
            .target
            .literal
            .as_deref()
            .filter(|_| !state.cwd_unknown && !actions.is_empty())
        else {
            self.record(
                Operation::new(
                    Coverage::Redirection,
                    Decision::Ask,
                    Data::Redirection {
                        redirection: redirect.clone(),
                        context: Some(ReportFacts::cwd(!state.cwd_unknown)),
                    },
                )
                .at(redirect.span),
            )?;
            return Ok(Decision::Ask);
        };
        let context = Context {
            cwd: self.context.cwd.clone(),
            environment: None,
            working_files: self.working_files.cloned(),
        };
        let mut decision = Decision::Allow;
        for action in actions {
            let input = Input::File {
                action: (*action).into(),
                sources: if *action == "read" {
                    vec![path.into()]
                } else {
                    vec![]
                },
                destination: (*action == "write").then(|| path.into()),
                recursive: false,
                original_input: json!({"operator":redirect.operator,"target":path}),
            };
            let evaluation = self.evaluator.evaluate_resource(
                &self.context,
                &input,
                &context,
                self.checkpoint,
            )?;
            decision = decision.max(evaluation.decision);
            self.record(
                Operation::new(
                    Coverage::FileRedirection,
                    evaluation.decision,
                    Data::File {
                        normalized_request: input,
                        evaluation: Box::new(evaluation),
                        context: ReportFacts::cwd(true),
                    },
                )
                .at(redirect.span),
            )?;
        }
        if actions.contains(&"write") {
            // Opening a regular output can change later observations, but it
            // does not rename the shell's already observed directory.
            state.filesystem_unknown = true;
        }
        Ok(decision)
    }

    fn embedded(
        &mut self,
        source: &str,
        span: structure::Span,
        kind: &str,
        state: &State,
        depth: usize,
    ) -> Result<Decision, Error> {
        self.frames
            .push(control::Frame::new(control::Kind::Process));
        let result = self.embedded_state(source, span, kind, &mut state.clone(), depth);
        self.frames.pop();
        result
    }

    fn embedded_state(
        &mut self,
        source: &str,
        span: structure::Span,
        kind: &str,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        self.nested_bytes += source.len();
        if self.nested_bytes > 65_536 {
            return Err(Error::Limit);
        }
        let nested = structure::parse(source, self.dialect, self.checkpoint)?;
        let old_unit = self.source_unit;
        self.source_unit = self.source_units.len();
        let unit = self.source_unit;
        self.source_units.push(SourceUnit {
            source: source.into(),
            dialect: self.dialect,
            dialect_version: None,
            parent: Some(SourceParent {
                source_unit: old_unit,
                span,
                kind: kind.into(),
            }),
            structure: None,
        });
        let result = self.visit(&nested, state, depth + 1);
        self.source_units[unit].structure = Some(nested);
        self.source_unit = old_unit;
        result
    }

    fn record(&mut self, mut operation: Operation) -> Result<(), Error> {
        if self.operations.len() >= 256 {
            return Err(Error::Limit);
        }
        operation.source_unit = self.source_unit;
        operation.cwd.clone_from(match &operation.data {
            Data::Invocation { request, .. } => &request.cwd,
            _ => &self.context.cwd,
        });
        operation.dialect = self.dialect;
        self.evidence_bytes += budget::measure(
            &operation,
            budget::MAX_EVIDENCE_BYTES.saturating_sub(self.evidence_bytes),
            self.checkpoint,
        )?;
        self.operations.push(operation);
        Ok(())
    }

    fn command(
        &mut self,
        span: structure::Span,
        words: &[structure::Word],
        state: &mut State,
        depth: usize,
        bypass_functions: bool,
        operand_state: Option<&State>,
    ) -> Result<Decision, Error> {
        (self.checkpoint)()?;
        if depth > 32 {
            return Err(Error::Limit);
        }
        // Only this command reads the pipe; wrapped, nested and function
        // launches analyzed from here are treated as reading their own input.
        let piped = std::mem::take(&mut state.stdin_pipe);
        let resolved = positional_words(words, state, self.dialect, self.checkpoint)?;
        let words = resolved.as_slice();
        if words.is_empty() {
            self.complete = false;
            self.record(Operation::new(Coverage::Opaque, Decision::Ask, Data::Empty {}).at(span))?;
            return Ok(Decision::Ask);
        }
        let wrapper = wrapper_offset(words).filter(|_| {
            (words[0].literal.as_deref() != Some("builtin") || self.dialect == Dialect::Bash)
                && (bypass_functions
                    || !words[0]
                        .literal
                        .as_ref()
                        .is_some_and(|name| state.functions.contains_key(name)))
        });
        let piped = piped && wrapper.is_none();
        let mut expansions = self.expansions(
            &words[..wrapper.map_or(words.len(), |wrapper| wrapper.offset)],
            state,
            depth,
        )?;
        if let Some(offset) = wrapper {
            expansions =
                expansions.max(self.wrapper(span, words, state, offset, depth, operand_state)?);
        }
        if let Some(name) = words[0].literal.as_deref()
            && !bypass_functions
            && !(self.dialect == Dialect::Posix && control::special_builtin(name))
            && let Some(bindings) = state.functions.get(name).cloned()
        {
            self.complete = false;
            self.record(
                Operation::new(
                    Coverage::FunctionCall,
                    Decision::Ask,
                    Data::Named { name: name.into() },
                )
                .at(span),
            )?;
            return Ok(self
                .function_calls(words, bindings, state, depth, operand_state)?
                .max(expansions));
        }
        let literal: Option<Vec<_>> = words.iter().map(|word| word.literal.clone()).collect();
        let Some(words) = literal else {
            self.complete = false;
            // An unresolved executable can change shell state. Opaque operands
            // of a known literal name do not themselves change parent cwd/env;
            // command/process substitutions run in isolated shell environments.
            if let Some(name) = words[0].literal.as_deref() {
                // Prefix assignments affect the launch environment, not the
                // already-expanded operands (including symbolic PID origins).
                if let Some(targets) =
                    provenance::kill_targets(words, operand_state.unwrap_or(state))
                {
                    return Ok(expansions.max(self.background_kill(span, words, state, &targets)?));
                }
                let decision = self.unknown_arguments(span, words, state)?;
                control::transfer(
                    &[name.into(), "unknown-control-operand".into()],
                    &mut self.frames,
                    state,
                    self.suppressed,
                    &mut self.exit_bytes,
                )?;
                state_effect(name, None, state);
                state.command_outcomes();
                return Ok(expansions.max(decision));
            }
            state.cwd_unknown = true;
            state.filesystem_unknown = true;
            state.command_outcomes();
            state.environment_unknown = true;
            state.brace_expansion = None;
            state.variables.clear();
            state.defined_functions.clear();
            self.record(Operation::new(Coverage::Opaque, Decision::Ask, Data::Empty {}).at(span))?;
            return Ok(expansions.max(Decision::Ask));
        };
        // A wrapper launches the real command itself, so the pipe is not
        // attributed to the wrapper's request.
        Ok(self
            .literal_command(span, &words, state, depth, piped)?
            .max(expansions))
    }

    fn unknown_arguments(
        &mut self,
        span: structure::Span,
        words: &[structure::Word],
        state: &State,
    ) -> Result<Decision, Error> {
        self.complete = false;
        let request = Request {
            executable: words
                .first()
                .and_then(|word| word.literal.clone())
                .ok_or(Error::Contract("known executable required"))?,
            arguments: words[1..]
                .iter()
                .map_while(|word| word.literal.clone())
                .collect(),
            ..self.context.clone()
        };
        let evaluation = self.evaluator.evaluate_unknown_arguments(
            &request,
            self.checkpoint,
            !state.cwd_unknown,
        )?;
        budget::measure(
            &evaluation,
            budget::MAX_EVIDENCE_BYTES.saturating_sub(self.evidence_bytes),
            self.checkpoint,
        )?;
        let decision = evaluation.decision.max(Decision::Ask);
        let mut status_only = false;
        for word in words.iter().filter(|word| word.literal.is_none()) {
            status_only = expansion::status_only(word, self.dialect)?;
            if !status_only {
                break;
            }
        }
        self.record(
            Operation::new(
                if status_only {
                    Coverage::RuntimeStatusArguments
                } else {
                    Coverage::OpaqueArguments
                },
                decision,
                Data::Invocation {
                    request,
                    evaluation: Box::new(evaluation),
                    detail: Box::new(Invocation::Bound {
                        evaluation_model: "known-executable-restriction-bound",
                        context: ReportFacts::cwd(!state.cwd_unknown)
                            .environment(!state.environment_unknown)
                            .arguments(false),
                    }),
                },
            )
            .at(span),
        )?;
        Ok(decision)
    }

    fn background_kill(
        &mut self,
        span: structure::Span,
        words: &[structure::Word],
        state: &State,
        targets: &[provenance::Job],
    ) -> Result<Decision, Error> {
        self.complete = false;
        let request = Request {
            executable: words[0].literal.clone().expect("known kill executable"),
            arguments: words[1..]
                .iter()
                .map_while(|word| word.literal.clone())
                .collect(),
            ..self.context.clone()
        };
        let evaluation = self.evaluator.evaluate_unknown_arguments(
            &request,
            self.checkpoint,
            !state.cwd_unknown,
        )?;
        budget::measure(
            &evaluation,
            budget::MAX_EVIDENCE_BYTES.saturating_sub(self.evidence_bytes),
            self.checkpoint,
        )?;
        let decision = evaluation.decision.max(Decision::Ask);
        self.record(
            Operation::new(
                Coverage::BackgroundJobKill,
                decision,
                Data::Invocation {
                    request,
                    evaluation: Box::new(evaluation),
                    detail: Box::new(Invocation::Kill {
                        targets: targets.to_vec(),
                        runtime_ownership: "unverified",
                        evaluation_model: "known-executable-restriction-bound",
                        context: ReportFacts::cwd(!state.cwd_unknown)
                            .environment(!state.environment_unknown)
                            .arguments(false),
                    }),
                },
            )
            .at(span),
        )?;
        Ok(decision)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one command transition records evidence before nested code and applies cwd/status outcomes in that order"
    )]
    fn literal_command(
        &mut self,
        span: structure::Span,
        words: &[String],
        state: &mut State,
        depth: usize,
        piped: bool,
    ) -> Result<Decision, Error> {
        let request = Request {
            executable: words[0].clone(),
            arguments: words[1..].to_vec(),
            ..self.context.clone()
        };
        let loop_transfer = control::resolved_loop_transfer(words, &self.frames);
        let same_directory = !state.cwd_unknown
            && same_directory(words, &request.cwd)
            && (!state.filesystem_unknown
                || words
                    .last()
                    .is_some_and(|target| matches!(target.as_str(), "." | "./")));
        let directory_change = words[0] == "cd" && !same_directory;
        let target_directory = if directory_change {
            cwd::resolve(
                words,
                state,
                self.clients.cdpath_empty,
                &self.directories,
                self.checkpoint,
            )?
        } else {
            None
        };
        let resolved_directory = target_directory.is_some();
        let syntax_check = !self.clients.node_preload
            && crate::location::node_check_file(
                &words[0],
                &words[1..].iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .is_some();
        let opaque = (opaque_executable(&words[0])
            && !version_query(words)
            && !builtin_query(words)
            && !syntax_check
            && !same_directory
            && !resolved_directory
            && !loop_transfer)
            || find_launches(words)
            || bindings::printf_mutates(words);
        let effects = crate::effects::classify(&request);
        let indirect = self.tool_nested(effects.as_ref(), span, state, depth)?;
        // Cwd determines contextual rule eligibility. Submitted constructs and
        // unresolved operands have their own coverage checks.
        let unknown_context = state.cwd_unknown;
        let (evaluation, policy_request) = self.contextual_evaluation(&request, state)?;
        budget::measure(
            &evaluation,
            budget::MAX_EVIDENCE_BYTES.saturating_sub(self.evidence_bytes),
            self.checkpoint,
        )?;
        let mut decision = evaluation.decision;
        // A resolved loop transfer has no external invocation of its own.
        // Preserve authored Ask/Deny rules, but not the unmatched-command fallback.
        if loop_transfer && evaluation.unmatched_ask() {
            decision = Decision::Allow;
        }
        if (same_directory || resolved_directory) && evaluation.fallback_only_ask() {
            decision = Decision::Allow;
        }
        if opaque || unknown_context {
            self.complete = false;
            decision = decision.max(Decision::Ask);
        }
        let transition = target_directory.as_ref().map(|target| Transition {
            cwd_success: target.physical.clone(),
            cwd_failure: request.cwd.clone(),
            logical_cwd_success: target.logical.clone(),
        });
        self.record(
            Operation::new(
                if same_directory {
                    Coverage::UnchangedWorkingDirectory
                } else if resolved_directory {
                    Coverage::ObservedWorkingDirectoryChange
                } else if loop_transfer && decision == Decision::Allow {
                    Coverage::ResolvedControlTransfer
                } else if opaque || unknown_context {
                    Coverage::Partial
                } else {
                    Coverage::LiteralArgv
                },
                decision,
                Data::Invocation {
                    request,
                    evaluation: Box::new(evaluation),
                    detail: Box::new(Invocation::Literal {
                        effects: effects.map(Box::new),
                        evaluation_model: if state.cwd_unknown {
                            "cwd-independent-restriction-bound"
                        } else {
                            "literal-argv"
                        },
                        context: ReportFacts::cwd(!state.cwd_unknown)
                            .environment(!state.environment_unknown),
                        policy_request: policy_request.map(Box::new),
                        transition,
                        stdin: piped.then_some("pipe"),
                    }),
                },
            )
            .at(span),
        )?;
        decision = decision.max(self.shell_script(words, span, state, depth, "shell-c-script")?);
        let eval = self.eval_script(words, span, state, depth)?;
        if let Some(result) = eval {
            decision = decision.max(result);
        }
        if control::transfer(
            words,
            &mut self.frames,
            state,
            self.suppressed,
            &mut self.exit_bytes,
        )? && !loop_transfer
        {
            self.complete = false;
            self.record(
                Operation::new(
                    Coverage::ControlTransfer,
                    Decision::Ask,
                    Data::Named {
                        name: words[0].clone(),
                    },
                )
                .at(span),
            )?;
            decision = decision.max(Decision::Ask);
        }
        if opaque
            && eval.is_none()
            && !directory_change
            && !(self.dialect == Dialect::Bash && bindings::local_declaration(words, state))
        {
            state_effect(&words[0], Some(&words[1..]), state);
        }
        if same_directory {
            // Even a no-op cd updates shell directory variables. Do not retain
            // old bindings for later expansions of PWD or OLDPWD.
            state.variables.remove("PWD");
            state.variables.remove("OLDPWD");
            cwd::unchanged(state);
        }
        if directory_change {
            cwd::change(state, target_directory);
            state.variables.remove("PWD");
            state.variables.remove("OLDPWD");
        } else if filesystem_may_change(words, syntax_check) {
            state.filesystem_mutation();
            state.command_outcomes();
        }
        Ok(decision.max(indirect))
    }

    fn contextual_evaluation(
        &self,
        request: &Request,
        state: &State,
    ) -> Result<
        (
            crate::configuration::CandidateEvaluation,
            Option<PolicyRequest>,
        ),
        Error,
    > {
        let evaluation = self.evaluator.evaluate_context_with_working_files(
            request,
            self.checkpoint,
            !state.cwd_unknown,
            self.working_files,
        )?;
        // The evaluator matched the command as run from the folder its declared
        // directory option chose; report that request beside the written one.
        if let Some(normalized) = evaluation.directory_request() {
            let typed = PolicyRequest::DirectoryOption {
                kind: "argv",
                request: normalized.clone(),
                directory_option: true,
            };
            return Ok((evaluation, Some(typed)));
        }
        if !state.cwd_unknown
            && !self.clients.node_preload
            && evaluation.fallback_only_ask()
            && let Some(file) = crate::location::node_check_file(
                &request.executable,
                &request
                    .arguments
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        {
            use crate::policy::resolution::normalized::{Context, Input};
            let input = Input::File {
                action: "read".into(),
                sources: vec![file.into()],
                destination: None,
                recursive: false,
                original_input: json!(request),
            };
            let context = Context {
                cwd: request.cwd.clone(),
                environment: None,
                working_files: self.working_files.cloned(),
            };
            let read =
                self.evaluator
                    .evaluate_resource(request, &input, &context, self.checkpoint)?;
            return Ok((read, Some(PolicyRequest::Normalized(input))));
        }
        if !state.cwd_unknown
            && !state.environment_unknown
            && !self.clients.http_configuration
            && evaluation.fallback_only_ask()
            && let Some(fetch) = crate::http_client::fetch(
                &request.executable,
                &request
                    .arguments
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
            && !self.http_client_config(&fetch)
        {
            return self.http_fetch_evaluation(request, &fetch);
        }
        if !state.cwd_unknown && evaluation.fallback_only_ask() {
            let arguments: Vec<_> = request.arguments.iter().map(String::as_str).collect();
            if crate::location::text_read_only(&request.executable, &arguments)
                && let Some(files) = crate::location::text_files(&request.executable, &arguments)
                && !files.is_empty()
            {
                use crate::policy::resolution::normalized::{Context, Input};
                let input = Input::File {
                    action: "read".into(),
                    sources: files.into_iter().map(|(_, path)| path).collect(),
                    destination: None,
                    recursive: false,
                    original_input: json!(request),
                };
                let context = Context {
                    cwd: request.cwd.clone(),
                    environment: None,
                    working_files: self.working_files.cloned(),
                };
                let read =
                    self.evaluator
                        .evaluate_resource(request, &input, &context, self.checkpoint)?;
                return Ok((read, Some(PolicyRequest::Normalized(input))));
            }
        }
        Ok((evaluation, None))
    }

    /// Whether the client would load a configuration or credentials file.
    /// Native adapters observe the home folder; previews have no such observation.
    fn http_client_config(&self, fetch: &crate::http_client::Fetch<'_>) -> bool {
        let Some(scope) = self.working_files else {
            return false;
        };
        if scope.http_environment {
            return true;
        }
        // Only curl skips its configuration with -q, and curl reads .netrc only with -n.
        if fetch.skips_config && !fetch.reads_netrc {
            return false;
        }
        scope.home.as_deref().is_none_or(|home| {
            crate::location::http_client_config(home, fetch.client, fetch.reads_netrc)
        })
    }

    /// Decide a recognized download by its typed effects: the web fetch and,
    /// when it names one, the output write. The more restrictive decides, the
    /// fetch on a tie, and its typed request is retained with the operation.
    fn http_fetch_evaluation(
        &self,
        request: &Request,
        fetch: &crate::http_client::Fetch<'_>,
    ) -> Result<
        (
            crate::configuration::CandidateEvaluation,
            Option<PolicyRequest>,
        ),
        Error,
    > {
        use crate::policy::resolution::normalized::{Context, Input};
        let context = Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: self.working_files.cloned(),
        };
        let web = Input::Web {
            action: "fetch".into(),
            query: None,
            url: Some(fetch.url.into()),
            method: Some(fetch.method.into()),
            conditions: json!({"client":"shell","redirects":fetch.redirects}),
            original_input: json!(request),
        };
        let fetched = self
            .evaluator
            .evaluate_resource(request, &web, &context, self.checkpoint)?;
        let Some(output) = fetch.output else {
            return Ok((fetched, Some(PolicyRequest::Normalized(web))));
        };
        let file = Input::File {
            action: "write".into(),
            sources: vec![],
            destination: Some(output.into()),
            recursive: false,
            original_input: json!(request),
        };
        let written =
            self.evaluator
                .evaluate_resource(request, &file, &context, self.checkpoint)?;
        Ok(if written.decision > fetched.decision {
            (written, Some(PolicyRequest::Normalized(file)))
        } else {
            (fetched, Some(PolicyRequest::Normalized(web)))
        })
    }

    fn eval_script(
        &mut self,
        words: &[String],
        span: structure::Span,
        state: &mut State,
        depth: usize,
    ) -> Result<Option<Decision>, Error> {
        if words[0] != "eval" {
            return Ok(None);
        }
        let mut operands = &words[1..];
        if self.dialect == Dialect::Bash && operands.first().is_some_and(|word| word == "--") {
            operands = &operands[1..];
        } else if operands.first().is_some_and(|word| word.starts_with('-')) {
            return Ok(None);
        }
        let bytes =
            operands.iter().map(String::len).sum::<usize>() + operands.len().saturating_sub(1);
        if bytes > 65_536usize.saturating_sub(self.nested_bytes) {
            return Err(Error::Limit);
        }
        let source = operands.join(" ");
        self.complete = false;
        Ok(Some(
            self.embedded_state(&source, span, "eval-script", state, depth)?
                .max(Decision::Ask),
        ))
    }

    fn function_calls(
        &mut self,
        words: &[structure::Word],
        bindings: Vec<FunctionBinding>,
        state: &mut State,
        depth: usize,
        operand_state: Option<&State>,
    ) -> Result<Decision, Error> {
        let mut decision = Decision::Ask;
        let input = state.clone();
        let definite = words[0]
            .literal
            .as_ref()
            .is_some_and(|name| input.defined_functions.contains(name));
        let mut output: Option<State> = None;
        for binding in bindings {
            let old_unit = self.source_unit;
            let old_dialect = self.dialect;
            self.source_unit = binding.source_unit;
            self.dialect = binding.dialect;
            let mut branch = input.clone();
            branch.environment_unknown = true;
            branch.locals.push(BTreeMap::new());
            self.frames
                .push(control::Frame::new(control::Kind::Function));
            branch.positionals = if words[1..].iter().any(|word| {
                word.literal.is_none() && matches!(word.raw.as_str(), "\"$@\"" | "\"${@}\"")
            }) {
                None
            } else {
                Some(words[1..].iter().map(|word| word.literal.clone()).collect())
            };
            let result = self.visit(&binding.body, &mut branch, depth + 1);
            let frame = self
                .frames
                .pop()
                .ok_or(Error::Contract("function frame missing"))?;
            frame.function(&mut branch);
            self.source_unit = old_unit;
            self.dialect = old_dialect;
            decision = decision.max(result?);
            branch.positionals.clone_from(&input.positionals);
            branch.arg0.clone_from(&input.arg0);
            if let Some(output) = &mut output {
                output.join(&branch);
            } else {
                output = Some(branch);
            }
        }
        if let Some(output) = output {
            if definite {
                *state = output;
            } else {
                state.join(&output);
            }
        }
        if !definite {
            let mut external = input;
            let span = structure::Span {
                start: words[0].span.start,
                end: words.last().map_or(words[0].span.end, |word| word.span.end),
            };
            if let Some(wrapper) = wrapper_offset(words) {
                decision = decision.max(self.wrapper(
                    span,
                    words,
                    &mut external,
                    wrapper,
                    depth,
                    operand_state,
                )?);
            }
            let literal: Option<Vec<_>> = words.iter().map(|word| word.literal.clone()).collect();
            decision = decision.max(if let Some(literal) = literal {
                self.literal_command(span, &literal, &mut external, depth + 1, false)?
            } else {
                self.unknown_arguments(span, words, &external)?
            });
            state.join(&external);
        }
        Ok(decision)
    }

    fn shell_script(
        &mut self,
        words: &[String],
        span: structure::Span,
        state: &State,
        depth: usize,
        kind: &str,
    ) -> Result<Decision, Error> {
        let Some((at, dialect)) = wrappers::shell_command(words) else {
            return Ok(Decision::Allow);
        };
        let source = &words[at];
        let old_dialect = self.dialect;
        self.dialect = dialect;
        let mut launch = state.clone();
        launch.brace_expansion = None;
        launch.functions.clear();
        launch.defined_functions.clear();
        launch.locals.clear();
        launch
            .variables
            .retain(|name, _| launch.exported.contains(name) && !bindings::dynamic_variable(name));
        launch.environment_unknown = true;
        launch.arg0 = words.get(at + 1).cloned();
        launch.positionals = Some(
            words
                .get(at + 2..)
                .unwrap_or_default()
                .iter()
                .cloned()
                .map(Some)
                .collect(),
        );
        let result = self.embedded(source, span, kind, &launch, depth);
        self.dialect = old_dialect;
        result
    }

    fn tool_nested(
        &mut self,
        effects: Option<&crate::effects::Hints>,
        span: structure::Span,
        state: &State,
        depth: usize,
    ) -> Result<Decision, Error> {
        (self.checkpoint)()?;
        if depth > 32 {
            return Err(Error::Limit);
        }
        let mut decision = Decision::Allow;
        for nested in effects.into_iter().flat_map(|effects| &effects.nested) {
            self.complete = false;
            let mut launch = state.clone();
            launch.brace_expansion = None;
            launch.cwd_unknown = true;
            launch.environment_unknown = true;
            launch.functions.clear();
            launch.defined_functions.clear();
            launch.locals.clear();
            launch.variables.clear();
            launch.positionals = None;
            launch.arg0 = None;
            self.record(
                Operation::new(
                    Coverage::IndirectExecution,
                    Decision::Ask,
                    Data::Indirect {
                        nested: nested.into(),
                    },
                )
                .at(span),
            )?;
            decision = decision.max(Decision::Ask);
            if let Some(source) = &nested.source {
                if !source.trim().is_empty() {
                    if nested.kind == "git-shell-alias" {
                        launch.positionals = Some(nested.argv.iter().cloned().map(Some).collect());
                    }
                    // Git's supported alias/foreach forms provide shell source,
                    // independent of the outer submitted shell dialect.
                    let previous = self.dialect;
                    self.dialect = Dialect::Posix;
                    let result = self.embedded(source, span, nested.kind, &launch, depth + 1);
                    self.dialect = previous;
                    decision = decision.max(result?);
                }
            } else if let Some(executable) = nested.argv.first() {
                let request = Request {
                    executable: executable.clone(),
                    arguments: nested.argv[1..].to_vec(),
                    ..self.context.clone()
                };
                let evaluation =
                    self.evaluator
                        .evaluate_context(&request, self.checkpoint, false)?;
                budget::measure(
                    &evaluation,
                    budget::MAX_EVIDENCE_BYTES.saturating_sub(self.evidence_bytes),
                    self.checkpoint,
                )?;
                let effects = crate::effects::classify(&request).map(Rc::new);
                decision = decision.max(evaluation.decision);
                self.record(
                    Operation::new(
                        Coverage::Partial,
                        evaluation.decision.max(Decision::Ask),
                        Data::Invocation {
                            request,
                            evaluation: Box::new(evaluation),
                            detail: Box::new(Invocation::Foreign {
                                effects: effects.clone(),
                                evaluation_model: "cwd-independent-restriction-bound",
                                context: ReportFacts::cwd(false)
                                    .environment(false)
                                    .namespace(nested.namespace),
                            }),
                        },
                    )
                    .at(span),
                )?;
                decision =
                    decision.max(self.tool_nested(effects.as_deref(), span, &launch, depth + 1)?);
                decision = decision.max(self.shell_script(
                    &nested.argv,
                    span,
                    &launch,
                    depth + 1,
                    "container-shell-script",
                )?);
            }
        }
        Ok(decision)
    }

    fn wrapper(
        &mut self,
        span: structure::Span,
        words: &[structure::Word],
        state: &mut State,
        wrapper: Wrapper,
        depth: usize,
        operand_state: Option<&State>,
    ) -> Result<Decision, Error> {
        self.complete = false;
        let mut nested_state = state.clone();
        nested_state.environment_unknown = true;
        // command bypasses shell function lookup; other wrappers start a fresh
        // executable lookup rather than invoking parent-shell functions.
        let parent_builtin = matches!(words[0].literal.as_deref(), Some("command" | "builtin"));
        if !parent_builtin {
            nested_state.functions.clear();
            nested_state.defined_functions.clear();
        }
        bindings::wrapper_environment(&words[..wrapper.offset], &mut nested_state);
        if wrapper.cwd_unknown {
            nested_state.cwd_unknown = true;
            nested_state.command_outcomes();
        }
        let offset = wrapper.offset;
        let start = words[offset].span.start;
        if !parent_builtin {
            self.frames
                .push(control::Frame::new(control::Kind::Process));
        }
        let result = self.command(
            structure::Span {
                start,
                end: span.end,
            },
            &words[offset..],
            &mut nested_state,
            depth + 1,
            true,
            operand_state,
        );
        if !parent_builtin {
            self.frames.pop();
        }
        let decision = result?;
        let unchanged_directory = parent_builtin
            && !state.cwd_unknown
            && words
                .iter()
                .map(|word| word.literal.clone())
                .collect::<Option<Vec<_>>>()
                .is_some_and(|literal| same_directory(&literal, &self.context.cwd));
        if parent_builtin {
            *state = nested_state;
        }
        Ok(if unchanged_directory {
            decision
        } else {
            decision.max(Decision::Ask)
        })
    }

    fn expansions(
        &mut self,
        words: &[structure::Word],
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        let mut decision = Decision::Allow;
        if words.iter().any(|word| !word.substitutions.is_empty()) {
            state.filesystem_mutation();
        }
        if self.dialect == Dialect::Bash
            && state.brace_expansion.is_none()
            && let Some(word) = words.iter().find(|word| {
                // An empty `{}` stays literal whether or not braceexpand is on.
                word.resolved
                    && word.literal.is_some()
                    && word.raw.replace("{}", "").contains('{')
                    && word.fragments.len() == 1
                    && word.fragments[0].kind == "unquoted"
            })
        {
            self.complete = false;
            self.record(
                Operation::new(
                    Coverage::PossibleBraceExpansion,
                    Decision::Ask,
                    Data::Syntax {
                        syntax: "braceexpand-unobserved",
                    },
                )
                .at(word.span),
            )?;
            decision = Decision::Ask;
        }
        if words.iter().any(|word| expansion::may_mutate(word, state)) {
            state.variables.clear();
            state.environment_unknown = true;
            self.complete = false;
            self.record(Operation::new(
                Coverage::PossibleExpansionMutation,
                Decision::Ask,
                Data::Empty {},
            ))?;
            decision = Decision::Ask;
        }
        for substitution in words.iter().flat_map(|word| &word.substitutions) {
            decision = decision.max(self.substitution(substitution, state, depth)?);
        }
        Ok(decision)
    }

    fn substitution(
        &mut self,
        substitution: &structure::Substitution,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        self.complete = false;
        if substitution.kind == "current-shell-substitution" && self.dialect != Dialect::Bash {
            self.record(
                Operation::new(Coverage::Opaque, Decision::Ask, Data::Empty {})
                    .at(substitution.span),
            )?;
            return Ok(Decision::Ask);
        }
        self.nested_bytes += substitution.source.len();
        if self.nested_bytes > 65_536 {
            return Err(Error::Limit);
        }
        let nested = structure::parse(&substitution.source, self.dialect, self.checkpoint)?;
        let old_unit = self.source_unit;
        self.source_unit = self.source_units.len();
        let unit = self.source_unit;
        self.source_units.push(SourceUnit {
            source: substitution.source.clone(),
            dialect: self.dialect,
            dialect_version: None,
            parent: Some(SourceParent {
                source_unit: old_unit,
                span: substitution.span,
                kind: substitution.kind.into(),
            }),
            structure: None,
        });
        let mut branch = state.clone();
        let result = if substitution.kind == "current-shell-substitution" {
            self.visit(&nested, &mut branch, depth + 1)
        } else {
            self.isolated(&nested, &mut branch, depth + 1)
        };
        self.source_units[unit].structure = Some(nested);
        self.source_unit = old_unit;
        if substitution.kind == "current-shell-substitution" {
            // Possible Bash 5.3 current-environment execution, never a claim
            // about the actual shell version or an evaluated expansion result.
            state.join(&branch);
            state.environment_unknown = true;
        }
        Ok(result?.max(Decision::Ask))
    }
}

fn stdin_shell(mut words: &[structure::Word], state: &State) -> Option<Dialect> {
    let mut external = false;
    for _ in 0..32 {
        let prefix = words
            .iter()
            .take_while(|word| assignment(&word.raw))
            .count();
        words = &words[prefix..];
        let name = words.first()?.literal.as_deref()?;
        if matches!(name, "sh" | "/bin/sh" | "bash" | "/bin/bash") {
            if (!external && state.functions.contains_key(name))
                || !(words.len() == 1
                    || (words.len() == 2 && words[1].literal.as_deref() == Some("-s")))
            {
                return None;
            }
            return Some(if matches!(name, "bash" | "/bin/bash") {
                Dialect::Bash
            } else {
                Dialect::Posix
            });
        }
        let wrapper = wrapper_offset(words)?;
        words = &words[wrapper.offset..];
        external = true;
    }
    None
}

// Only explicit, bounded shapes are inspected. The wrapper retains ask and
// the nested launch environment is uncertain. Unknown option arity is never
// guessed. In particular env -S and sudo shell options are not unwrapped.
#[derive(Clone, Copy)]
struct Wrapper {
    offset: usize,
    cwd_unknown: bool,
}

fn wrapper_offset(words: &[structure::Word]) -> Option<Wrapper> {
    wrappers::offset(words)
}

fn positional_words(
    words: &[structure::Word],
    state: &State,
    dialect: Dialect,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Vec<structure::Word>, Error> {
    let mut uncertain;
    let state = if words.iter().any(|word| expansion::may_mutate(word, state)) {
        uncertain = state.clone();
        uncertain.variables.clear();
        &uncertain
    } else {
        state
    };
    let mut result = Vec::new();
    for word in words {
        checkpoint()?;
        if word.literal.is_some() || word.resolved {
            result.push(word.clone());
            continue;
        }
        if dialect == Dialect::Bash
            && state.brace_expansion != Some(false)
            && word.fragments.len() == 1
            && word.fragments[0].kind == "unquoted"
            && word.raw.contains('{')
            && let Some(expanded) = brace::expand(&word.raw, checkpoint)?
        {
            for value in expanded {
                let mut resolved = word.clone();
                resolved.literal = literal_words(&value, dialect)?.and_then(|mut values| {
                    if values.len() == 1 {
                        values.pop()
                    } else {
                        None
                    }
                });
                resolved.resolved = true;
                result.push(resolved);
                if result.len() > 257 {
                    return Err(Error::Limit);
                }
            }
            continue;
        }
        let values = expansion::argument_words(word, state, dialect, checkpoint)?
            .unwrap_or_else(|| vec![None]);
        for literal in values {
            let mut resolved = word.clone();
            resolved.literal = literal;
            resolved.resolved = true;
            result.push(resolved);
        }
        if result.len() > 257 {
            return Err(Error::Limit);
        }
    }
    Ok(result)
}

/// Whether the whole request is one literal text-filter invocation without
/// redirections or assignments, so it runs no other operation.
fn sole_filter(tree: &Node) -> bool {
    let Node::Command {
        words,
        redirections,
        ..
    } = tree
    else {
        return false;
    };
    redirections.is_empty()
        && words.iter().all(|word| word.literal.is_some())
        && words
            .first()
            .and_then(|word| word.literal.as_deref())
            .is_some_and(|name| {
                matches!(
                    name.rsplit('/').next(),
                    Some("sed" | "awk" | "jq" | "sort" | "uniq" | "cut" | "diff" | "cmp")
                )
            })
}

// An interpreter asked only for its version prints it and runs no code.
fn version_query(words: &[String]) -> bool {
    let name = words[0].rsplit('/').next().unwrap_or(&words[0]);
    let interpreter = bindings::versioned_interpreter(name)
        || [
            "python", "python3", "node", "nodejs", "deno", "bun", "ruby", "perl", "lua", "bash",
            "zsh", "ksh", "fish", "tclsh",
        ]
        .contains(&name);
    let flag = words.get(1).map(String::as_str);
    interpreter
        && words.len() == 2
        && (flag == Some("--version")
            || (flag == Some("-v") && ["node", "nodejs", "deno", "bun"].contains(&name))
            || (flag == Some("-V") && name.starts_with("python")))
}

/// Builtin queries that report state without running or changing anything:
/// `command -v`/`-V` describe how names resolve, and `ulimit` given only option
/// flags prints the current limits. Policy decides them like other commands.
fn builtin_query(words: &[String]) -> bool {
    let rest = &words[1..];
    match words[0].as_str() {
        "command" => {
            let options: Vec<&str> = rest
                .iter()
                .map(String::as_str)
                .take_while(|word| word.starts_with('-') && *word != "--")
                .collect();
            options.iter().any(|option| option.contains(['v', 'V']))
                && options.iter().all(|option| {
                    option.len() > 1 && option[1..].bytes().all(|b| b"pvV".contains(&b))
                })
        }
        "ulimit" => rest.iter().all(|word| {
            word.len() > 1
                && word.starts_with('-')
                && word[1..].bytes().all(|b| b.is_ascii_alphabetic())
        }),
        _ => false,
    }
}

// find launches other programs only through its command actions.
fn find_launches(words: &[String]) -> bool {
    words[0].rsplit('/').next() == Some("find")
        && words[1..]
            .iter()
            .any(|word| ["-exec", "-execdir", "-ok", "-okdir"].contains(&word.as_str()))
}

// NODE_OPTIONS uses double-quoted words. Removing quotes and escapes may
// overclassify malformed options, but cannot hide a known preload spelling.
fn node_preload_options(options: &str) -> bool {
    options
        .replace(['"', '\\'], "")
        .replace('_', "-")
        .split_ascii_whitespace()
        .any(|option| {
            option.starts_with("-r")
                || ["--require", "--import", "--loader", "--experimental-loader"]
                    .iter()
                    .any(|flag| option == *flag || option.starts_with(&format!("{flag}=")))
        })
}

fn same_directory(words: &[String], cwd: &str) -> bool {
    let words = if words.first().is_some_and(|word| word == "command") {
        &words[1..]
    } else {
        words
    };
    if words.first().is_none_or(|word| word != "cd") {
        return false;
    }
    let arguments = &words[1..];
    let arguments = arguments.strip_prefix(&["--".into()]).unwrap_or(arguments);
    matches!(arguments, [target] if matches!(target.as_str(), "." | "./") || target == cwd)
}

fn filesystem_may_change(words: &[String], syntax_check: bool) -> bool {
    if matches!(
        words[0].as_str(),
        ":" | "true" | "false" | "pwd" | "echo" | "printf" | "cd"
    ) || version_query(words)
        || builtin_query(words)
        || syntax_check
        || parent_directory_builtin(words)
    {
        return false;
    }
    !crate::location::text_read_only(
        &words[0],
        &words[1..].iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

fn parent_directory_builtin(words: &[String]) -> bool {
    let command = words.first().is_some_and(|word| word == "command");
    if !command && words.first().is_none_or(|word| word != "builtin") {
        return false;
    }
    let mut arguments = &words[1..];
    while command && arguments.first().is_some_and(|word| word == "-p") {
        arguments = &arguments[1..];
    }
    if arguments.first().is_some_and(|word| word == "--") {
        arguments = &arguments[1..];
    }
    arguments.first().is_some_and(|word| word == "cd")
}

fn opaque_executable(executable: &str) -> bool {
    let name = executable.rsplit('/').next().unwrap_or(executable);
    executable.is_empty()
        || executable.contains('=')
        || bindings::unknown_builtin(name)
        || bindings::versioned_interpreter(name)
        || matches!(
            name,
            "if" | "then"
                | "else"
                | "elif"
                | "fi"
                | "for"
                | "while"
                | "until"
                | "do"
                | "done"
                | "case"
                | "esac"
                | "function"
                | "select"
                | "time"
                | "eval"
                | "source"
                | "."
                | "exec"
                | "env"
                | "sudo"
                | "command"
                | "builtin"
                | "cd"
                | "pushd"
                | "popd"
                | "trap"
                | "alias"
                | "unalias"
                | "set"
                | "shift"
                | "return"
                | "break"
                | "continue"
                | "exit"
                | "export"
                | "unset"
                | "readonly"
                | "declare"
                | "local"
                | "read"
                | "sh"
                | "bash"
                | "zsh"
                | "fish"
                | "python"
                | "python3"
                | "node"
                | "perl"
                | "ruby"
                | "dash"
                | "ash"
                | "ksh"
                | "csh"
                | "tcsh"
                | "lua"
                | "tclsh"
                | "nodejs"
                | "deno"
                | "bun"
                | "nohup"
                | "nice"
                | "timeout"
                | "stdbuf"
                | "xargs"
                | "ssh"
                | "su"
                | "doas"
                | "chroot"
                | "busybox"
                | "powershell"
                | "pwsh"
                | "cmd"
        )
}
