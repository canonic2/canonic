//! Diagnostic commands share the MCP schemas and the existing manager client.
use crate::error::{Error, Result};
use clap::Subcommand;
use serde_json::{Value, json};
#[derive(Subcommand)]
pub(super) enum DiagnosticCommand {
    /// Inspect effects, scope, costs and dependencies without starting anything.
    Capabilities,
    /// Plan a bounded reproduction; JSON options never dispatch actions.
    Plan {
        preset: String,
        #[arg(long)]
        platform: String,
        #[arg(long)]
        options: Option<String>,
    },
    /// Verify an explicitly selected bundled dependency; no download or installation.
    Prepare {
        component: String,
        #[arg(long)]
        version: Option<String>,
    },
    /// Read a scoped numeric projection with free text omitted (filters are JSON).
    Project {
        investigation_id: String,
        analysis_id: String,
        #[arg(long)]
        filters: Option<String>,
        #[command(flatten)]
        page: super::arguments::PageArgs,
    },
    /// Export a projection, investigation JSON/Markdown report or private native artifact; filters select format, report and scope.
    Export {
        investigation_id: String,
        analysis_id: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        filters: Option<String>,
        #[command(flatten)]
        page: super::arguments::PageArgs,
    },
    /// Open an investigation; use the same submission ID to recover a lost reply.
    Open {
        #[arg(long)]
        submission_id: String,
        #[arg(long)]
        question: String,
        #[arg(long)]
        binding: Option<String>,
    },
    /// Collect a declared source or retain a host action marker (JSON source).
    Start {
        investigation_id: String,
        #[arg(long)]
        submission_id: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        limits: Option<String>,
    },
    /// Inspect retained state without starting the manager.
    Status { investigation_id: String },
    /// Read private evidence bytes as base64.
    Read {
        investigation_id: String,
        collector_id: String,
        #[command(flatten)]
        page: super::arguments::PageArgs,
    },
    /// Stop one or all collectors, preserving output.
    Stop {
        investigation_id: String,
        #[arg(long)]
        collector_id: Option<String>,
    },
    /// Close the investigation and stop its collectors.
    Close { investigation_id: String },
    /// Remove a closed, settled investigation's owned evidence.
    Remove { investigation_id: String },
    /// Build a reusable index from finalized evidence in an isolated parser.
    Analyze {
        investigation_id: String,
        collector_id: String,
        #[arg(long)]
        submission_id: String,
        #[arg(long)]
        metadata: Option<String>,
    },
    /// Inspect retained analysis state.
    AnalysisStatus {
        investigation_id: String,
        analysis_id: String,
    },
    /// Cancel analysis while preserving evidence.
    AnalysisCancel {
        investigation_id: String,
        analysis_id: String,
    },
    /// Query private rows or select investigation/report views with JSON filters; inspect diagnostics capabilities for routing.
    Query {
        investigation_id: String,
        analysis_id: String,
        #[arg(long)]
        filters: Option<String>,
        #[command(flatten)]
        page: super::arguments::PageArgs,
    },
    /// Compare compatible observations, explaining mismatches.
    Compare {
        investigation_id: String,
        analysis_id: String,
        other_investigation_id: String,
        other_analysis_id: String,
    },
}

fn object(text: &str) -> Result<Value> {
    serde_json::from_str(text).map_err(|_| Error::invalid("Expected a diagnostic JSON object"))
}
pub(super) async fn run(command: DiagnosticCommand, machine: bool) -> Result<()> {
    let (name, args) = command_arguments(command)?;
    let definition = crate::diagnostics::definition(name).unwrap();
    crate::schema::validate(&definition["inputSchema"], &args)?;
    let result = crate::managed::Client::new().call(name, args).await;
    super::presentation::print_result(result?, machine, "Diagnostics")
}

fn command_arguments(command: DiagnosticCommand) -> Result<(&'static str, Value)> {
    Ok(match command {
        DiagnosticCommand::Capabilities => ("diagnostics_capabilities", json!({})),
        DiagnosticCommand::Plan {
            preset,
            platform,
            options,
        } => {
            let mut args = options
                .as_deref()
                .map(object)
                .transpose()?
                .unwrap_or_else(|| json!({}));
            if !args.is_object() || args.get("preset").is_some() || args.get("platform").is_some() {
                return Err(Error::invalid(
                    "Plan options cannot override preset/platform",
                ));
            }
            args["preset"] = json!(preset);
            args["platform"] = json!(platform);
            ("diagnostics_plan", args)
        }
        DiagnosticCommand::Prepare { component, version } => {
            let mut args = json!({"component":component});
            if let Some(version) = version {
                args["version"] = json!(version);
            }
            ("diagnostics_prepare", args)
        }
        command @ (DiagnosticCommand::Project { .. }
        | DiagnosticCommand::Export { .. }
        | DiagnosticCommand::Analyze { .. }
        | DiagnosticCommand::AnalysisStatus { .. }
        | DiagnosticCommand::AnalysisCancel { .. }
        | DiagnosticCommand::Query { .. }
        | DiagnosticCommand::Compare { .. }) => analysis_arguments(command)?,
        command => collector_arguments(command)?,
    })
}

fn analysis_arguments(command: DiagnosticCommand) -> Result<(&'static str, Value)> {
    Ok(match command {
        DiagnosticCommand::Project {
            investigation_id,
            analysis_id,
            filters,
            page,
        } => {
            let mut args = projection_arguments(investigation_id, analysis_id, filters)?;
            page.add_to(&mut args);
            ("diagnostics_project", args)
        }
        DiagnosticCommand::Export {
            investigation_id,
            analysis_id,
            path,
            filters,
            page,
        } => {
            let mut args = projection_arguments(investigation_id, analysis_id, filters)?;
            page.add_to(&mut args);
            args["path"] = json!(path);
            ("diagnostics_export", args)
        }
        DiagnosticCommand::Analyze {
            investigation_id,
            collector_id,
            submission_id,
            metadata,
        } => {
            let mut args = json!({"investigationId":investigation_id,"collectorId":collector_id,"submissionId":submission_id});
            if let Some(metadata) = metadata {
                args["metadata"] = object(&metadata)?;
            }
            ("diagnostics_analyze", args)
        }
        DiagnosticCommand::AnalysisStatus {
            investigation_id,
            analysis_id,
        } => (
            "diagnostics_analysis_status",
            json!({"investigationId":investigation_id,"analysisId":analysis_id}),
        ),
        DiagnosticCommand::AnalysisCancel {
            investigation_id,
            analysis_id,
        } => (
            "diagnostics_analysis_cancel",
            json!({"investigationId":investigation_id,"analysisId":analysis_id}),
        ),
        DiagnosticCommand::Query {
            investigation_id,
            analysis_id,
            filters,
            page,
        } => {
            let mut args = query_arguments(investigation_id, analysis_id, filters)?;
            page.add_to(&mut args);
            ("diagnostics_query", args)
        }
        DiagnosticCommand::Compare {
            investigation_id,
            analysis_id,
            other_investigation_id,
            other_analysis_id,
        } => (
            "diagnostics_compare",
            json!({"investigationId":investigation_id,"analysisId":analysis_id,"otherInvestigationId":other_investigation_id,"otherAnalysisId":other_analysis_id}),
        ),
        _ => unreachable!("Only analysis commands reach this conversion"),
    })
}

fn collector_arguments(command: DiagnosticCommand) -> Result<(&'static str, Value)> {
    Ok(match command {
        DiagnosticCommand::Open {
            submission_id,
            question,
            binding,
        } => {
            let mut args = json!({"submissionId":submission_id,"question":question});
            if let Some(binding) = binding {
                args["binding"] = object(&binding)?;
            }
            ("diagnostics_open", args)
        }
        DiagnosticCommand::Start {
            investigation_id,
            submission_id,
            source,
            limits,
        } => {
            let mut args = json!({"investigationId":investigation_id,"submissionId":submission_id,"source":object(&source)?});
            if let Some(limits) = limits {
                args["limits"] = object(&limits)?;
            }
            ("diagnostics_start", args)
        }
        DiagnosticCommand::Status { investigation_id } => (
            "diagnostics_status",
            json!({"investigationId":investigation_id}),
        ),
        DiagnosticCommand::Read {
            investigation_id,
            collector_id,
            page,
        } => ("diagnostics_read", {
            let mut args = json!({"investigationId":investigation_id,"collectorId":collector_id});
            page.add_to(&mut args);
            args
        }),
        DiagnosticCommand::Stop {
            investigation_id,
            collector_id,
        } => {
            let mut args = json!({"investigationId":investigation_id});
            if let Some(id) = collector_id {
                args["collectorId"] = json!(id);
            }
            ("diagnostics_stop", args)
        }
        DiagnosticCommand::Close { investigation_id } => (
            "diagnostics_close",
            json!({"investigationId":investigation_id}),
        ),
        DiagnosticCommand::Remove { investigation_id } => (
            "diagnostics_remove",
            json!({"investigationId":investigation_id}),
        ),
        _ => unreachable!("Planning and analysis commands are converted separately"),
    })
}

fn query_arguments(
    investigation_id: String,
    analysis_id: String,
    filters: Option<String>,
) -> Result<Value> {
    let mut args = filters
        .as_deref()
        .map(object)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    if !args.is_object()
        || args.get("investigationId").is_some()
        || args.get("analysisId").is_some()
    {
        return Err(Error::invalid(
            "Query filters must be an object without investigationId/analysisId",
        ));
    }
    args["investigationId"] = json!(investigation_id);
    args["analysisId"] = json!(analysis_id);
    if args.get("offset").is_some() {
        return Err(Error::invalid("Use --cursor for continuation"));
    }
    Ok(args)
}
fn projection_arguments(
    investigation: String,
    analysis: String,
    filters: Option<String>,
) -> Result<Value> {
    let mut args = filters
        .as_deref()
        .map(object)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    if !args.is_object()
        || ["investigationId", "analysisId", "path"]
            .iter()
            .any(|key| args.get(*key).is_some())
    {
        return Err(Error::invalid(
            "Projection filters cannot override investigationId, analysisId or path",
        ));
    }
    if ["offset", "cursor", "limit"]
        .iter()
        .any(|key| args.get(*key).is_some())
    {
        return Err(Error::invalid(
            "Use --cursor and --limit for projection pages",
        ));
    }
    args["investigationId"] = json!(investigation);
    args["analysisId"] = json!(analysis);
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::arguments::{Command, CommandLine};
    use clap::Parser;
    #[test]
    fn diagnostics_cli_preserves_explicit_sources_and_requires_stable_submissions() {
        let parsed = CommandLine::try_parse_from([
            "testrun",
            "diagnostics",
            "start",
            "diag-acme",
            "--submission-id",
            "copy",
            "--source",
            r#"{"kind":"import","path":"/tmp/acme","format":"bytes"}"#,
        ])
        .unwrap();
        let Command::Diagnostics(DiagnosticCommand::Start {
            submission_id,
            source,
            ..
        }) = parsed.command
        else {
            panic!("diagnostics route")
        };
        assert_eq!(submission_id, "copy");
        assert_eq!(object(&source).unwrap()["kind"], "import");
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "diagnostics",
                "start",
                "diag-acme",
                "--source",
                "{}"
            ])
            .is_err()
        );
    }

    #[test]
    fn diagnostics_cli_preserves_retained_reads_analysis_and_exports() {
        for verb in ["status", "close", "remove"] {
            assert!(
                CommandLine::try_parse_from(["testrun", "diagnostics", verb, "diag-acme"]).is_ok()
            );
        }
        for verb in ["analysis-status", "analysis-cancel", "query"] {
            assert!(
                CommandLine::try_parse_from([
                    "testrun",
                    "diagnostics",
                    verb,
                    "diag-acme",
                    "analysis-acme"
                ])
                .is_ok()
            );
        }
        assert!(CommandLine::try_parse_from(["testrun", "diagnostics", "capabilities"]).is_ok());
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "diagnostics",
                "prepare",
                "builtin-parser",
                "--version",
                "testrun-diagnostics/1"
            ])
            .is_ok()
        );
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "diagnostics",
                "project",
                "diag-acme",
                "analysis-acme",
                "--filters",
                "{\"scope\":\"all\"}"
            ])
            .is_ok()
        );
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "diagnostics",
                "export",
                "diag-acme",
                "analysis-acme",
                "--path",
                "/tmp/acme-report.json"
            ])
            .is_ok()
        );
        assert!(
            projection_arguments(
                "diag-acme".into(),
                "analysis-acme".into(),
                Some("{\"path\":\"/outside\"}".into())
            )
            .is_err()
        );
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "diagnostics",
                "analyze",
                "diag-acme",
                "collector-acme",
                "--submission-id",
                "parse",
                "--metadata",
                "{}"
            ])
            .is_ok()
        );
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "diagnostics",
                "compare",
                "diag-one",
                "analysis-one",
                "diag-two",
                "analysis-two"
            ])
            .is_ok()
        );
    }
}
