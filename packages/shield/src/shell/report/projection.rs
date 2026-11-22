//! Borrowed owner projections for operations inside private source geometry.
use super::{Data, Input, Invocation, Operation, PolicyRequest, Serialize, effects};
use crate::policy::resolution::diagnostics::{Private, RequestEvidence};
use serde::ser::SerializeMap;

impl Serialize for Operation {
    #[expect(
        clippy::too_many_lines,
        reason = "one exhaustive borrowed operation codec keeps field presence and privacy decisions together"
    )]
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_map(None)?;
        if let Some(span) = &self.span {
            fields.serialize_entry("span", span)?;
        }
        fields.serialize_entry("coverage", &self.coverage)?;
        fields.serialize_entry("decision", &self.decision)?;
        fields.serialize_entry("source_unit", &self.source_unit)?;
        fields.serialize_entry("cwd", &Private::new(&self.cwd, self.private_input))?;
        fields.serialize_entry("dialect", &self.dialect)?;
        let private = self.private_input;
        match &self.data {
            Data::Invocation {
                request,
                evaluation,
                detail,
            } => {
                fields.serialize_entry("request", &RequestEvidence::new(request, private))?;
                fields.serialize_entry("evaluation", evaluation)?;
                match detail.as_ref() {
                    Invocation::Literal {
                        effects,
                        evaluation_model,
                        context,
                        policy_request,
                        transition,
                        stdin,
                    } => {
                        fields.serialize_entry(
                            "effects",
                            &effects.as_ref().map(|hints| Effects { hints, private }),
                        )?;
                        fields.serialize_entry("evaluation_model", evaluation_model)?;
                        fields.serialize_entry("context", context)?;
                        fields.serialize_entry(
                            "policy_request",
                            &policy_request
                                .as_ref()
                                .map(|request| Policy { request, private }),
                        )?;
                        if let Some(transition) = transition {
                            fields.serialize_entry(
                                "cwd_success",
                                &Private::new(&transition.cwd_success, private),
                            )?;
                            fields.serialize_entry(
                                "cwd_failure",
                                &Private::new(&transition.cwd_failure, private),
                            )?;
                            fields.serialize_entry(
                                "logical_cwd_success",
                                &transition
                                    .logical_cwd_success
                                    .as_deref()
                                    .map(|value| Private::new(value, private)),
                            )?;
                        }
                        if let Some(stdin) = stdin {
                            fields.serialize_entry("stdin", stdin)?;
                        }
                    }
                    Invocation::Bound {
                        evaluation_model,
                        context,
                    } => {
                        fields.serialize_entry("evaluation_model", evaluation_model)?;
                        fields.serialize_entry("context", context)?;
                    }
                    Invocation::Kill {
                        targets,
                        runtime_ownership,
                        evaluation_model,
                        context,
                    } => {
                        fields.serialize_entry("targets", targets)?;
                        fields.serialize_entry("runtime_ownership", runtime_ownership)?;
                        fields.serialize_entry("evaluation_model", evaluation_model)?;
                        fields.serialize_entry("context", context)?;
                    }
                    Invocation::Foreign {
                        effects,
                        evaluation_model,
                        context,
                        policy_request,
                    } => {
                        fields.serialize_entry(
                            "effects",
                            &effects.as_deref().map(|hints| Effects { hints, private }),
                        )?;
                        fields.serialize_entry("evaluation_model", evaluation_model)?;
                        fields.serialize_entry("context", context)?;
                        if let Some(request) = policy_request {
                            fields
                                .serialize_entry("policy_request", &Policy { request, private })?;
                        }
                    }
                }
            }
            Data::File {
                normalized_request,
                evaluation,
                context,
            } => {
                fields.serialize_entry(
                    "normalized_request",
                    &InputEvidence {
                        input: normalized_request,
                        private,
                    },
                )?;
                fields.serialize_entry("evaluation", evaluation)?;
                fields.serialize_entry("context", context)?;
            }
            Data::Redirection {
                redirection,
                context,
            } => {
                fields.serialize_entry("redirection", redirection)?;
                if let Some(context) = context {
                    fields.serialize_entry("context", context)?;
                }
            }
            Data::Assignments { assignments } => {
                fields.serialize_entry("assignments", assignments)?;
            }
            Data::Named { name } => fields.serialize_entry("name", &Private::new(name, private))?,
            Data::Operator { operator } => fields.serialize_entry("operator", operator)?,
            Data::Descriptor { descriptor_flow } => {
                if private {
                    fields.serialize_entry(
                        "descriptor_flow",
                        &descriptor_flow.private_input_view(),
                    )?;
                } else {
                    fields.serialize_entry("descriptor_flow", descriptor_flow)?;
                }
            }
            Data::Indirect { nested } => {
                #[derive(Serialize)]
                struct View<'a> {
                    kind: &'static str,
                    source: Option<Private<&'a str>>,
                    argv: Private<&'a [String]>,
                    argument_index: usize,
                    namespace: &'static str,
                }
                fields.serialize_entry(
                    "nested",
                    &View {
                        kind: nested.kind,
                        source: nested
                            .source
                            .as_deref()
                            .map(|value| Private::new(value, private)),
                        argv: Private::new(nested.argv.as_slice(), private),
                        argument_index: nested.argument_index,
                        namespace: nested.namespace,
                    },
                )?;
            }
            Data::Background { origin } => fields.serialize_entry("origin", origin)?,
            Data::Variable { variable } => {
                fields.serialize_entry("variable", &Private::new(variable, private))?;
            }
            Data::Expression { syntax, words } => {
                fields.serialize_entry("syntax", syntax)?;
                fields.serialize_entry("words", words)?;
            }
            Data::Syntax { syntax } => fields.serialize_entry("syntax", syntax)?,
            Data::Empty {} => {}
        }
        fields.end()
    }
}

struct Effects<'a> {
    hints: &'a effects::Hints,
    private: bool,
}
impl Serialize for Effects<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.private {
            self.hints.private_input_view().serialize(serializer)
        } else {
            self.hints.serialize(serializer)
        }
    }
}
struct Policy<'a> {
    request: &'a PolicyRequest,
    private: bool,
}
impl Serialize for Policy<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.request {
            PolicyRequest::Normalized(input) => InputEvidence {
                input,
                private: self.private,
            }
            .serialize(serializer),
            PolicyRequest::DirectoryOption {
                kind,
                request,
                directory_option,
            } => {
                #[derive(Serialize)]
                struct View<'a> {
                    kind: &'a str,
                    #[serde(flatten)]
                    request: RequestEvidence<'a>,
                    directory_option: bool,
                }
                View {
                    kind,
                    request: RequestEvidence::new(request, self.private),
                    directory_option: *directory_option,
                }
                .serialize(serializer)
            }
        }
    }
}
struct InputEvidence<'a> {
    input: &'a Input,
    private: bool,
}
impl Serialize for InputEvidence<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !self.private {
            return self.input.serialize(serializer);
        }
        let mut fields = serializer.serialize_map(None)?;
        match self.input {
            Input::Argv {
                executable,
                arguments,
                original_input,
            } => {
                fields.serialize_entry("kind", "argv")?;
                fields.serialize_entry("executable", &Private::new(executable, true))?;
                fields.serialize_entry("arguments", &Private::new(arguments, true))?;
                fields.serialize_entry("original_input", &Private::new(original_input, true))?;
            }
            Input::Shell {
                source,
                dialect,
                dialect_version,
                original_input,
            } => {
                fields.serialize_entry("kind", "shell")?;
                fields.serialize_entry("source", &Private::new(source, true))?;
                fields.serialize_entry("dialect", dialect)?;
                fields.serialize_entry("dialect_version", dialect_version)?;
                fields.serialize_entry("original_input", &Private::new(original_input, true))?;
            }
            Input::File {
                action,
                sources,
                destination,
                recursive,
                original_input,
            } => {
                fields.serialize_entry("kind", "file")?;
                fields.serialize_entry("action", action)?;
                fields.serialize_entry("sources", &Private::new(sources, true))?;
                fields.serialize_entry(
                    "destination",
                    &destination
                        .as_deref()
                        .map(|value| Private::new(value, true)),
                )?;
                fields.serialize_entry("recursive", recursive)?;
                fields.serialize_entry("original_input", &Private::new(original_input, true))?;
            }
            Input::Web {
                action,
                query,
                url,
                method,
                conditions,
                original_input,
            } => {
                fields.serialize_entry("kind", "web")?;
                fields.serialize_entry("action", action)?;
                fields.serialize_entry(
                    "query",
                    &query.as_deref().map(|value| Private::new(value, true)),
                )?;
                fields.serialize_entry(
                    "url",
                    &url.as_deref().map(|value| Private::new(value, true)),
                )?;
                fields.serialize_entry(
                    "method",
                    &method.as_deref().map(|value| Private::new(value, true)),
                )?;
                fields.serialize_entry("conditions", &Private::new(conditions, true))?;
                fields.serialize_entry("original_input", &Private::new(original_input, true))?;
            }
            Input::Mcp {
                server,
                tool,
                arguments,
                connection,
                original_input,
            } => {
                fields.serialize_entry("kind", "mcp")?;
                fields.serialize_entry("server", &Private::new(server, true))?;
                fields.serialize_entry("tool", &Private::new(tool, true))?;
                fields.serialize_entry("arguments", &Private::new(arguments, true))?;
                if connection.is_some() {
                    fields.serialize_entry("connection", connection)?;
                }
                fields.serialize_entry("original_input", &Private::new(original_input, true))?;
            }
            Input::Unknown {
                host_tool,
                original_input,
            } => {
                fields.serialize_entry("kind", "unknown")?;
                fields.serialize_entry("host_tool", &Private::new(host_tool, true))?;
                fields.serialize_entry("original_input", &Private::new(original_input, true))?;
            }
        }
        fields.end()
    }
}
