//! Operator recovery guidance: a few lines for people, and the cause and every
//! recovery step for JSON errors beside their existing code and message.
use crate::{
    cli::{
        Cli, PublicCommand,
        public::{
            History, HistoryCommand, Pack, PackCommand, Request, RequestCommand, Rule, RuleCommand,
        },
    },
    review::Error,
};
use std::io::{self, IsTerminal, Write};

/// What failed, why and how to recover. Humans see a few lines of it; JSON
/// errors carry the cause and every recovery step.
struct Report {
    heading: String,
    message: &'static str,
    context: Vec<(String, String)>,
    help: Vec<String>,
    /// The underlying reason explains this failure better than `message`.
    detailed: bool,
    details: String,
}
impl Report {
    fn new(heading: &str, message: &'static str, help: &[&str]) -> Self {
        Self {
            heading: heading.into(),
            message,
            context: vec![],
            help: help.iter().map(|s| (*s).into()).collect(),
            detailed: false,
            details: String::new(),
        }
    }
    /// A report whose cause line is the underlying reason.
    fn detailed(heading: &str, message: &'static str, help: &[&str]) -> Self {
        Self {
            detailed: true,
            ..Self::new(heading, message, help)
        }
    }
    fn field(&mut self, key: &str, value: impl Into<String>) {
        self.context.push((key.into(), value.into()));
    }
    fn context(&self, key: &str) -> Option<&str> {
        self.context
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    /// The one-line cause: the classification's explanation, or the
    /// underlying reason when it says more.
    fn cause(&self) -> String {
        let mut cause = if self.detailed && !self.details.is_empty() {
            let mut chars = self.details.chars();
            chars.next().map_or_else(String::new, |first| {
                format!(
                    "{}{}.",
                    first.to_uppercase(),
                    chars.as_str().trim_end_matches('.')
                )
            })
        } else {
            self.message.to_owned()
        };
        if let Some(file) = self.context("File") {
            cause = format!(
                "{} Check {file}.",
                cause.trim_end_matches('.').to_owned() + "."
            );
        }
        cause
    }
}
fn report(command: &PublicCommand, error: &Error) -> Report {
    let mut result = classify(command, error);
    result.details = match error {
        Error::Cancelled => String::new(),
        Error::Invalid(reason) => (*reason).into(),
        Error::Io(error) => error.to_string(),
        other => other.to_string(),
    };
    result
}
/// The cause and recovery steps that JSON errors carry beside their code and message.
pub(in crate::review::operator) fn recovery(
    command: &PublicCommand,
    error: &Error,
) -> serde_json::Value {
    let report = report(command, error);
    serde_json::json!({"details":report.cause(),"recovery":report.help})
}
/// A rule naming this machine's folders; the cause says what to write instead.
fn not_portable() -> Report {
    Report::detailed(
        "The rule names a folder on this machine",
        "A project rule must match on every checkout and machine.",
        &["Write the rule as the cause describes, then retry."],
    )
}

fn classify(command: &PublicCommand, error: &Error) -> Report {
    match error {
        Error::Cancelled => Report::new("Cancelled", "The pending change was not confirmed.", &[]),
        Error::Invalid(reason) => invalid(command, reason),
        Error::Ambiguous { .. } => Report::detailed(
            "The pack reference is ambiguous",
            "Several packs match this reference.",
            &["Retry with one of the listed references."],
        ),
        Error::Missing => missing(command),
        Error::CacheUnavailable => Report::new(
            "Compiled policy is not ready",
            "Shield needs a valid compiled plan for the committed policy.",
            &[
                "Run shield cache prepare in the owning project, then retry.",
                "If preparation fails, inspect shield doctor.",
            ],
        ),
        Error::Locked => Report::new(
            "Another Shield operation is writing",
            "The project state is locked by its current writer.",
            &["Wait for that operation to finish, then retry."],
        ),
        Error::StaleSnapshot => Report::new(
            "The project changed during this operation",
            "The request's policy revision is no longer current.",
            &["Inspect shield status, then retry and review the current policy."],
        ),
        Error::RequiredPack { pack, dependents } => {
            let name = |id: &String| super::pack_name(&serde_json::Value::String(id.clone()));
            let mut r = Report::new(
                &format!(
                    "{} is still required by {}",
                    name(pack),
                    dependents.iter().map(name).collect::<Vec<_>>().join(", ")
                ),
                "Removing it would break that dependency.",
                &["Remove or change the packs that depend on it first; see shield pack list."],
            );
            r.field("Pack", pack.clone());
            r.field("Required by", dependents.join(", "));
            r
        }
        Error::UnsupportedReleases(pack) => unsupported_releases(pack),
        Error::NotPortable(_) => not_portable(),
        Error::Dependency(error) => dependency(error),
        Error::Policy(error) => policy(error),
        Error::Io(error) => io_report(error),
        Error::McpSetupRequired => Report::new(
            "MCP setup requires terminal review",
            "Connection settings or defaults changed.",
            &["Run shield policy review in your terminal, then shield mcp setup."],
        ),
        Error::ApprovalRequired => Report::new(
            "Operator approval is required",
            "An inspection or proposal cannot activate this change.",
            &["Review the pending proposal with shield policy review in an operator terminal."],
        ),
        Error::CandidateUnavailable | Error::CursorUnavailable => Report::new(
            "Inspection result is no longer available",
            "The retained diagnostic handle cannot be used for this query.",
            &["Run the inspection again to obtain current results."],
        ),
        Error::Overloaded => Report::new(
            "Shield's connection capacity is full",
            "This connection cannot retain more inspection work.",
            &["Finish outstanding work or reconnect, then retry."],
        ),
        Error::Capacity {
            kind,
            pending,
            retained,
            limit,
        } => capacity(*kind, *pending, *retained, *limit),
    }
}

fn dependency(error: &canonic_registry::resolution::Error) -> Report {
    match error {
        canonic_registry::resolution::Error::Limit => Report::new(
            "Dependency resolution reached its limit",
            "The dependency graph exceeds the bounded resolver's capacity.",
            &["Reduce the requested dependency closure or choose a simpler release selection."],
        ),
        canonic_registry::resolution::Error::Unenrolled(source) => {
            let mut r = Report::new(
                &format!(
                    "A dependency needs registry {}, which is not enrolled",
                    super::short(&serde_json::Value::String(source.clone()))
                ),
                "This release depends on a pack from a registry this project has not added.",
                &["Add that registry with shield registry add, then retry."],
            );
            r.field("Source identity", source.clone());
            r
        }
        _ => Report::detailed(
            "Pack dependencies could not be resolved",
            "No complete selection can be installed under the requested versions, profiles and enrolled sources.",
            &[
                "Check versions and profiles with shield pack info and shield pack list, then retry.",
                "Resolve version/profile conflicts or enroll the required source, then retry.",
            ],
        ),
    }
}

fn policy(error: &crate::policy::Error) -> Report {
    match error {
        crate::policy::Error::Interrupted => Report::new(
            "Operation interrupted",
            "Shield did not produce a completed result.",
            &["Retry when the operation can finish."],
        ),
        crate::policy::Error::Limit => Report::new(
            "A resource limit was reached",
            "The supplied policy or request exceeds a supported bound.",
            &["Reduce the input size or complexity, then retry."],
        ),
        _ => Report::detailed(
            "Policy needs correction",
            "The supplied policy does not satisfy Shield's structured v1 contract.",
            &[
                "Inspect the policy's rule, profile and requirement fields.",
                "Validate the corrected candidate before confirming activation.",
            ],
        ),
    }
}

fn capacity(
    kind: crate::review::CapacityKind,
    pending: usize,
    retained: usize,
    limit: usize,
) -> Report {
    use crate::review::CapacityKind;
    let mut r = match kind {
        CapacityKind::ReadRequests | CapacityKind::RunRequests => Report::new(
            &format!("Too many open {}s", kind.noun()),
            "Pending and approved requests that have not expired count toward this bound.",
            &[],
        ),
        CapacityKind::RetainedProposals => Report::new(
            "Retained policy proposal storage is full",
            "It cannot be cleared yet, so new proposals cannot be saved.",
            &[],
        ),
        CapacityKind::Proposals => Report::new(
            &format!("Too many {}s are waiting", kind.noun()),
            "Shield retains a bounded number of these until they are reviewed.",
            &[],
        ),
    };
    if let Some(route) = kind.operator_route() {
        r.help
            .push(format!("Run {route} to review them, then retry."));
    }
    r.field("Pending", pending.to_string());
    r.field("Retained", retained.to_string());
    r.field("Limit", limit.to_string());
    r
}
fn unsupported_releases(pack: &str) -> Report {
    let mut r = Report::new(
        &format!(
            "{} has no release for Shield {}",
            super::pack_name(&serde_json::Value::String(pack.into())),
            env!("CARGO_PKG_VERSION")
        ),
        "Every published release needs a different Shield version or policy format.",
        &["Update Shield, then retry."],
    );
    r.field("Pack", pack);
    r
}
fn invalid(command: &PublicCommand, reason: &str) -> Report {
    if reason.contains("interactive TUI") {
        return Report::new(
            "The terminal cannot host this interaction",
            "Interactive prompts require terminal stdin/stdout and cursor control.",
            &["Run the command in a cursor-capable terminal without redirecting stdin/stdout."],
        );
    }
    if reason.contains("operator terminal") || reason.contains("JSON is read-only") {
        return Report::new(
            "Interactive confirmation is required",
            "Policy-changing commands need confirmation from the operator.",
            &[
                "Run the command in an interactive terminal without --json.",
                "Use --json for inspection; it cannot confirm a change.",
            ],
        );
    }
    if reason.contains("no enrolled project") {
        return Report::new(
            "Shield is not initialized here",
            "No enrolled project was found for the selected directory.",
            &[
                "Run shield init in the intended project.",
                "To use another enrolled project, select it with --project PATH.",
            ],
        );
    }
    if reason.contains("unowned") || reason.contains("manually changed") {
        let mut r = Report::new(
            "Existing Shield configuration needs review",
            "An existing entry does not match the recorded or generated Shield configuration. Mismatched entries are not overwritten.",
            &[
                "Inspect the indicated file and resolve the conflicting entry.",
                retry(command),
            ],
        );
        let files = if reason.contains("Codex") && reason.contains("MCP") {
            ".codex/config.toml"
        } else if reason.contains("MCP") {
            ".mcp.json"
        } else {
            ".claude/settings.local.json, .codex/hooks.json"
        };
        r.field("File", files);
        return r;
    }
    if reason.contains("project already initialized") {
        return Report::new(
            "This project is already initialized",
            "Agent changes use the existing enrollment rather than reinitializing policy.",
            &["Run shield mcp setup to configure agents."],
        );
    }
    if reason.contains("changed during")
        || reason.contains("changed after")
        || reason.contains("repair plan changed")
    {
        return Report::new(
            "Configuration changed during review",
            "The captured files or state no longer match this operation's review.",
            &[
                "Inspect the current files and shield doctor, then repeat the review.",
                "For a saved partial setup, run shield mcp setup to resume its confirmed plan.",
            ],
        );
    }
    invalid_source(reason)
}
fn invalid_source(reason: &str) -> Report {
    if reason.contains("alias already enrolled") {
        return Report::new(
            "This registry alias is already registered",
            "Each alias names one registry in its scope.",
            &["Choose another alias, or remove the existing one with shield registry remove."],
        );
    }
    if reason.contains("signature")
        || reason.contains("signing")
        || reason.contains("rollback")
        || reason.contains("different bytes")
        || reason.contains("digest mismatch")
        || reason.contains("unsigned")
    {
        return Report::detailed(
            "Registry verification failed",
            "The release or catalog could not be accepted under its source and integrity binding.",
            &[
                "Check the registry's published release metadata, then retry.",
                "Retry after correcting the source registration or publication.",
            ],
        );
    }
    if reason.contains("acquisition failed") || reason.contains("request timed out") {
        return Report::detailed(
            "Registry download failed",
            "Shield could not retrieve the required registry resource.",
            &["Check the connection, source address and published resource, then retry."],
        );
    }
    if reason.contains("registry") || reason.contains("catalog") {
        return Report::detailed(
            "Registry input needs correction",
            "The registry address, metadata or response does not meet Shield's requirements.",
            &[
                "Check the registry with shield registry list; see shield registry --help for input.",
                "Use shield registry --help for supported registration input.",
            ],
        );
    }
    if reason.contains("linked")
        || reason.contains("cannot be links")
        || reason.contains("escapes project")
    {
        return Report::detailed(
            "Configuration path is not permitted",
            "Shield requires contained regular configuration files, with valid ownership.",
            &["Use a regular file inside the project instead of a link or outside path."],
        );
    }
    if reason.contains("configuration") || reason.contains("config") || reason.contains("artifact")
    {
        return Report::detailed(
            "Project configuration needs correction",
            "Shield could not use the supplied configuration or retained artifact.",
            &[
                "Correct .shield/config.yml or its artifacts, then review with shield policy review.",
                "Correct the candidate and review it with shield policy review; cache preparation cannot activate edits.",
            ],
        );
    }
    Report::detailed(
        "The command could not finish",
        "Shield rejected the supplied input or current state.",
        &["Correct the input described above, then retry; see the command's --help."],
    )
}
fn retry(command: &PublicCommand) -> &'static str {
    if matches!(command, PublicCommand::Init { .. }) {
        "Retry shield init after resolving the conflict."
    } else {
        "Retry the setup or command after resolving the conflict."
    }
}
fn missing(command: &PublicCommand) -> Report {
    let (heading, message, help, reference) = match command {
        PublicCommand::Request(Request {
            command:
                RequestCommand::Approve { request }
                | RequestCommand::Deny { request }
                | RequestCommand::Show { request },
        }) => (
            "Request is not available",
            "The request may have expired, been consumed or belong to another project.",
            "Run shield request list and select a current request; request a fresh attempt when needed.",
            Some(request),
        ),
        PublicCommand::History(History {
            command: HistoryCommand::Show { entry },
        }) => (
            "History entry is not available",
            "This entry is not retained in the selected project's history.",
            "Run shield history list to select an available entry.",
            Some(entry),
        ),
        PublicCommand::Rule(Rule {
            command: RuleCommand::Edit { id } | RuleCommand::Remove { id },
        }) => (
            "Rule was not found",
            "No current project rule has this identifier.",
            "Run shield rule list to select the exact rule identifier.",
            Some(id),
        ),
        PublicCommand::Pack(Pack {
            command:
                PackCommand::Remove { reference }
                | PackCommand::Update {
                    reference: Some(reference),
                }
                | PackCommand::Show { reference }
                | PackCommand::Configure { reference, .. },
        }) => (
            "Installed pack was not found",
            "No retained selection matches this reference.",
            "Run shield pack list to select a current pack reference.",
            Some(reference),
        ),
        PublicCommand::Pack(Pack {
            command: PackCommand::Add { reference, .. } | PackCommand::Info { reference },
        }) => (
            "Permission pack was not found",
            "No available release matches this reference in the selected source.",
            "Use shield pack search to find the publisher/name and source, then retry.",
            Some(reference),
        ),
        _ => (
            "Requested object is not available",
            "Shield could not locate the object needed for this operation.",
            "Inspect the current project and the command's --help, then retry.",
            None,
        ),
    };
    let mut r = Report::new(heading, message, &[help]);
    if let Some(reference) = reference {
        r.field("Reference", reference.clone());
    }
    r
}
fn io_report(error: &io::Error) -> Report {
    if error.to_string().contains("cursor position") || error.to_string().contains("cursor report")
    {
        return Report::new(
            "The terminal did not provide a usable cursor position",
            "Shield cannot safely anchor its interactive prompt.",
            &["Retry in a terminal that supports cursor position reports."],
        );
    }
    if error.to_string().contains("window too small") {
        return Report::new(
            "The terminal is too small",
            "There is not enough visible space for this interaction.",
            &["Enlarge the terminal and retry."],
        );
    }
    match error.kind() {
        io::ErrorKind::PermissionDenied => Report::detailed(
            "Shield could not access a file",
            "The current user lacks the required filesystem access.",
            &[
                "Check ownership and permissions of the project and its cache, then retry.",
                "Use the intended user account and retry; inspect the original I/O details.",
            ],
        ),
        io::ErrorKind::NotFound => Report::detailed(
            "A required file or directory is missing",
            "Shield could not open a path needed by this operation.",
            &[
                "Check the selected project; shield doctor reports what needs repair.",
                "Use shield doctor in an enrolled project to identify repair needs.",
            ],
        ),
        _ => Report::detailed(
            "An input/output operation failed",
            "Shield could not complete the required I/O.",
            &["Check the terminal, project and cache, then retry."],
        ),
    }
}
/// Prints a human error to stderr: what failed, why, and one way to fix it.
pub(in crate::review::operator) fn print(input: &Cli, error: &Error) {
    let report = report(&input.command, error);
    let mut stderr = io::stderr().lock();
    let styled = stderr.is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").is_ok_and(|v| !v.is_empty() && v != "dumb");
    let _ =
        stderr.write_all(compact(&report, matches!(error, Error::Cancelled), styled).as_bytes());
}
/// At most four lines: the failure and the value it concerns, the cause and
/// one recovery step.
fn compact(report: &Report, cancelled: bool, styled: bool) -> String {
    use std::fmt::Write;
    let clean = |s: &str| super::label(&serde_json::Value::String(s.into()));
    if cancelled {
        return "Cancelled; nothing changed.\n".into();
    }
    let prefix = if styled {
        "\x1b[1;31mError:\x1b[0m"
    } else {
        "Error:"
    };
    let mut output = format!("{prefix} {}", clean(&report.heading));
    if let Some(reference) = report.context("Reference") {
        let _ = write!(output, ": {}", clean(reference));
    }
    let _ = writeln!(output, "\n{}", clean(&report.cause()));
    if let Some(help) = report.help.first() {
        let _ = writeln!(output, "{}", clean(help));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn init() -> PublicCommand {
        PublicCommand::Init {
            agents: vec![],
            no_setup: false,
            no_baseline: false,
            baseline_profile: None,
        }
    }
    fn pack(command: PackCommand) -> PublicCommand {
        PublicCommand::Pack(Pack { command })
    }
    fn approve(request: &str) -> PublicCommand {
        PublicCommand::Request(Request {
            command: RequestCommand::Approve {
                request: request.into(),
            },
        })
    }
    #[test]
    fn terminal_prerequisites_explain_recovery_without_plain_input() {
        let command = init();
        let output = compact(
            &report(
                &command,
                &Error::Invalid(
                    "interactive TUI requires terminal stdin/stdout and a cursor-capable TERM",
                ),
            ),
            false,
            false,
        );
        assert!(output.contains("cursor-capable terminal"), "{output}");
        assert!(output.contains("without redirecting"), "{output}");
        let output = compact(
            &io_report(&io::Error::other("The cursor position could not be read")),
            false,
            false,
        );
        assert!(
            output.contains("supports cursor position reports"),
            "{output}"
        );
    }
    #[test]
    fn configuration_conflict_has_file_context_and_no_unrelated_registry_route() {
        let command = init();
        let output = compact(
            &report(
                &command,
                &Error::Invalid("Shield hook group is unowned or manually changed"),
            ),
            false,
            false,
        );
        assert!(output.contains("Existing Shield configuration needs review"));
        assert!(output.contains(".claude/settings.local.json"));
        assert!(!output.contains("registry connection"));
        assert!(!output.contains("--no-baseline"));
        assert!(!output.contains("invalid review input or state"));
    }
    #[test]
    fn missing_request_uses_approval_recovery_and_escapes_untrusted_reference() {
        let command = approve("request-\u{1b}[2J\u{202e}");
        let output = compact(&report(&command, &Error::Missing), false, false);
        assert!(output.contains("shield request list"));
        assert!(!output.contains("shield pack list"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\u{202e}'));
    }
    #[test]
    fn configuring_a_missing_pack_points_to_the_installed_packs() {
        let command = pack(PackCommand::Configure {
            reference: "acme/unknown".into(),
            set: vec![],
            unset: vec![],
            overrides: vec![],
            clear: vec![],
            local: false,
        });
        let output = compact(&report(&command, &Error::Missing), false, false);
        assert!(
            output.starts_with("Error: Installed pack was not found: acme/unknown\n"),
            "{output}"
        );
        assert!(output.contains("Run shield pack list"), "{output}");
    }
    #[test]
    fn failed_policy_validation_is_not_reported_as_a_registry_connection_failure() {
        let command = init();
        let output = compact(
            &report(
                &command,
                &Error::Policy(crate::policy::Error::Contract("invalid requirement")),
            ),
            false,
            false,
        );
        assert!(output.contains("Policy needs correction"));
        assert!(output.contains("invalid requirement"));
        assert!(!output.contains("registry connection"));
    }
    #[test]
    fn ambiguous_references_list_bounded_alternatives() {
        let command = pack(PackCommand::Remove {
            reference: "test".into(),
        });
        let names: Vec<String> = (0..10).map(|i| format!("acme-{i}/acme/test")).collect();
        let error = Error::ambiguous("installed reference is ambiguous", names);
        let output = compact(&report(&command, &error), false, false);
        assert!(output.lines().count() <= 4, "{output}");
        assert!(
            output.contains("acme-0/acme/test, acme-1/acme/test"),
            "{output}"
        );
        assert!(output.contains("acme-7/acme/test and 2 more"), "{output}");
        assert!(!output.contains("acme-8/acme/test"), "{output}");
    }
    #[test]
    fn every_classification_fits_the_four_line_budget() {
        let commands = [
            PublicCommand::Status,
            pack(PackCommand::Add {
                reference: "acme/git".into(),
                profile: None,
                set: vec![],
                local: false,
            }),
            approve("run-1"),
        ];
        let mut errors = classified_errors();
        let full = Error::Capacity {
            kind: crate::review::CapacityKind::RetainedProposals,
            pending: 0,
            retained: 1024,
            limit: 1024,
        };
        let retained = compact(&report(&commands[0], &full), false, false);
        assert!(!retained.contains("retry"), "{retained}");
        errors.insert(1, full);
        for command in &commands {
            for error in &errors {
                let output = compact(
                    &report(command, error),
                    matches!(error, Error::Cancelled),
                    false,
                );
                assert!(output.lines().count() <= 4, "{output}");
                assert!(
                    !output.contains("Code:") && !output.contains("Operation:"),
                    "{output}"
                );
            }
        }
        let pack = errors
            .iter()
            .find(|e| matches!(e, Error::RequiredPack { .. }))
            .unwrap();
        let required = compact(&report(&commands[0], pack), false, false);
        assert!(
            required.starts_with("Error: acme/git is still required by acme/workflow\n"),
            "{required}"
        );
    }

    /// One error of every classification family, with long and terminal variants.
    fn classified_errors() -> Vec<Error> {
        let reasons = [
            "operator terminal required; run this command interactively",
            "no enrolled project; run shield init",
            "Shield MCP entry is unowned or manually changed",
            "project already initialized",
            "repair plan changed during review",
            "registry alias already enrolled",
            "registry catalog signature invalid",
            "registry HTTPS acquisition failed",
            "invalid registry catalog",
            "linked integration configuration",
            "invalid project configuration",
            "something else entirely",
        ];
        let mut errors: Vec<Error> = reasons.iter().map(|r| Error::Invalid(r)).collect();
        errors.extend([
            Error::Cancelled,
            Error::Missing,
            Error::Locked,
            Error::StaleSnapshot,
            Error::CacheUnavailable,
            Error::RequiredPack {
                pack: "0123456789abcdef/acme/git".into(),
                dependents: vec!["0123456789abcdef/acme/workflow".into()],
            },
            Error::Dependency(canonic_registry::resolution::Error::Conflict),
            Error::Dependency(canonic_registry::resolution::Error::Unenrolled(
                "0123456789abcdef".into(),
            )),
            Error::Policy(crate::policy::Error::Limit),
            Error::Io(io::Error::new(io::ErrorKind::NotFound, "acme.yml missing")),
            Error::UnsupportedReleases("0123456789abcdef/acme/git".into()),
            Error::NotPortable(
                "the rule names /Users/acme/project; write it relative to the project".into(),
            ),
            Error::Dependency(canonic_registry::resolution::Error::Limit),
            Error::Policy(crate::policy::Error::Interrupted),
            Error::Policy(crate::policy::Error::Contract("invalid requirement")),
            Error::Policy(crate::policy::Error::binding(
                "acme_paths",
                "value is not a list",
            )),
            Error::Policy(crate::policy::Error::DirectoryOptionConflict(
                "acme_root is declared by acme/git and acme/workflow".into(),
            )),
            Error::ApprovalRequired,
            Error::CandidateUnavailable,
            Error::CursorUnavailable,
            Error::Overloaded,
            Error::Capacity {
                kind: crate::review::CapacityKind::RunRequests,
                pending: 64,
                retained: 64,
                limit: 64,
            },
            Error::ambiguous(
                "installed reference is ambiguous",
                (0..12).map(|i| format!("acme-{i}/acme/test")).collect(),
            ),
            Error::Invalid(
                "interactive TUI requires terminal stdin/stdout and a cursor-capable TERM",
            ),
            Error::Io(io::Error::other("The cursor position could not be read")),
            Error::Io(io::Error::other("no cursor report from the terminal")),
            Error::Io(io::Error::other("terminal window too small")),
            Error::Io(io::Error::new(io::ErrorKind::PermissionDenied, "acme.yml")),
        ]);
        errors.insert(
            0,
            Error::Capacity {
                kind: crate::review::CapacityKind::Proposals,
                pending: 128,
                retained: 300,
                limit: 128,
            },
        );
        errors
    }
}
