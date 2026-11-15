//! Canonical operator orchestration over project-owned policy, grants and state.
mod diagnostics;
pub(crate) mod integrations;
mod packs;
mod path_access;
mod presentation;
mod project_files;
mod registries;
mod rules;
mod sdk;
mod session_grants;
mod state;
mod terminal;
#[cfg(test)]
mod tests;

#[cfg(test)]
use super::history;
use super::{Context, Error, approvals, project};
use crate::cli::{Cli, PublicCommand};
use serde_json::{Value, json};
use std::{path::Path, process::ExitCode};

struct Output {
    value: Value,
    context: Option<Context>,
}
impl Output {
    fn independent(value: Value) -> Self {
        Self {
            value,
            context: None,
        }
    }
}

#[cfg(test)]
pub(crate) fn commit_proposal_fixture(context: &Context, id: &str) -> Result<Value, Error> {
    let mut before = crate::review::project::load(context)?;
    let index = before
        .proposals
        .iter()
        .position(|p| p.id == id)
        .ok_or(Error::Missing)?;
    super::storage::Store::open(Path::new(&context.project_root))?
        .hydrate_proposal(&mut before.proposals[index])?;
    let candidate = before.proposals[index].candidate.clone();
    let meta = before.proposals[index].candidate_metadata.clone();
    crate::review::files::write::commit_fixture(context, &before, candidate, meta, Some(id))
}

pub(crate) fn run(input: &Cli) -> ExitCode {
    super::retire_user_records();
    match dispatch(input) {
        Ok(Output { mut value, context }) => {
            if matches!(input.command, PublicCommand::History(_)) {
                super::history::redact_result(&mut value);
            } else if !matches!(input.command, PublicCommand::Docs(_)) {
                super::model::redact(&mut value);
            }
            // Every public operator result carries the schema it follows.
            if input.json
                && let Some(object) = value.as_object_mut()
            {
                object.entry("schema_version").or_insert_with(|| json!(1));
            }
            if input.json {
                println!("{value}");
            } else {
                presentation::print(input, &value);
            }
            if let Some(context) = context.as_ref() {
                pack_update_notice(input, &value, context);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            print_error(input, &error);
            ExitCode::from(if matches!(error, Error::Cancelled) {
                130
            } else {
                1
            })
        }
    }
}

pub(crate) fn startup_error(input: &Cli, error: &Error) -> ExitCode {
    if input.command.mcp_serve().is_some() {
        eprintln!("{error}");
    } else {
        print_error(input, error);
    }
    ExitCode::from(2)
}

fn print_error(input: &Cli, error: &Error) {
    if input.json {
        let mut value =
            json!({"schema_version":1,"error":{"code":error.code(),"message":error.to_string()}});
        let recovery = presentation::errors::recovery(&input.command, error);
        value["error"]["details"] = recovery["details"].clone();
        value["error"]["recovery"] = recovery["recovery"].clone();
        if let Error::Ambiguous {
            alternatives,
            total,
            ..
        } = error
        {
            value["error"]["alternatives"] = json!(alternatives);
            value["error"]["alternatives_total"] = json!(total);
        }
        println!("{value}");
    } else {
        presentation::errors::print(input, error);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one routing arm per public command keeps the command surface in one place"
)]
fn dispatch(input: &Cli) -> Result<Output, Error> {
    use crate::cli::public::{
        HistoryCommand, McpCommand, PackCommand, PolicyCommand, RegistryCommand, RequestCommand,
        RuleCommand, Scope,
    };
    let start = input.project.clone().unwrap_or(std::env::current_dir()?);
    match &input.command {
        PublicCommand::Docs(docs) => {
            return diagnostics::docs(&docs.command).map(Output::independent);
        }
        PublicCommand::Init {
            agents,
            no_setup,
            no_baseline,
            baseline_profile,
        } => {
            let choice = if *no_setup {
                Some(crate::cli::public::IntegrationChoice::Keep)
            } else if agents.is_empty() {
                None
            } else {
                Some(crate::cli::public::IntegrationChoice::Agents(
                    crate::cli::public::IntegrationChoice::bits(agents)?,
                ))
            };
            return integrations::initialize_in_cache(
                &start,
                choice,
                input.json,
                input.cache_dir.as_deref(),
                if *no_baseline {
                    integrations::Baseline::Skip
                } else {
                    baseline_profile.map_or(
                        integrations::Baseline::Choose,
                        integrations::Baseline::Selected,
                    )
                },
            )
            .map(Output::independent);
        }
        PublicCommand::Registry(registry) if registry.scope == Scope::User => {
            if let RegistryCommand::List { page } = &registry.command {
                let mut args = page.arguments();
                args["scope"] = json!("user");
                return super::registries::list_page(None, &args).map(Output::independent);
            }
            return registries::mutate(None, &super::registries::fetcher(), registry, input.json)
                .map(Output::independent);
        }
        _ => {}
    }
    let context = if matches!(
        input.command,
        PublicCommand::Status(_) | PublicCommand::Doctor(_)
    ) {
        match project_with(&start, super::open_diagnostics) {
            Ok(context) => context,
            // A folder with policy files and no review state, such as a new
            // clone, worktree or copy, gets its runtime state on first use.
            Err(error) => project(&start).map_err(|_| error)?,
        }
    } else {
        project(&start)?
    };
    if !input.json {
        match &input.command {
            PublicCommand::Pack(pack)
                if matches!(
                    pack.command,
                    PackCommand::Search { .. } | PackCommand::Info { .. }
                ) =>
            {
                println!("Checking registry metadata…");
            }
            _ => {}
        }
    }
    let mut value = match &input.command {
        PublicCommand::Sdk(crate::cli::public::Sdk {
            command: crate::cli::public::SdkCommand::Setup { adapter },
        }) => sdk::setup(&context, adapter, input.json),
        PublicCommand::Mcp(crate::cli::public::Mcp {
            command: McpCommand::Setup(setup),
        }) => {
            let choice = setup.choice()?;
            if let Some(action) = setup.pending {
                let action = match action {
                    crate::cli::public::PendingSetup::Review => {
                        super::integrations::recovery::Action::Review
                    }
                    crate::cli::public::PendingSetup::Cancel => {
                        super::integrations::recovery::Action::Cancel
                    }
                };
                return integrations::recover(&context, action, input.json, setup.dry_run).map(
                    |value| Output {
                        value,
                        context: Some(context),
                    },
                );
            }
            if setup.dry_run {
                return integrations::setup_review(
                    &context,
                    choice,
                    input.json,
                    setup.reset_tool_permissions,
                )
                .map(|value| Output {
                    value,
                    context: Some(context),
                });
            }
            let mut receipt =
                integrations::setup(&context, choice, input.json, setup.reset_tool_permissions)?;
            if receipt.get("setup_error").is_none()
                && let Some(directory) = input.cache_dir.as_deref()
            {
                receipt["cache_location"] = super::files::relocate_cache(&context, directory)?;
            }
            Ok(receipt)
        }
        PublicCommand::Protection(_) => path_access::configure(&context, input.json),
        PublicCommand::SessionGrant(command) => {
            session_grants::run(&context, &command.command, input.json)
        }
        PublicCommand::Status(args) => {
            let mut values = args.page.arguments();
            if let Some(section) = &args.section {
                values["section"] = json!(section);
            }
            super::page::validate(&context, "status", &values)?;
            super::page::sections(&context, "status", &values, project::status(&context)?)
        }
        PublicCommand::Suggestion(crate::cli::public::SuggestionCommand::List {
            session_id,
            page,
        }) => {
            let mut args = page.arguments();
            if let Some(id) = session_id {
                args["session_id"] = json!(id);
            }
            super::mcp::operator_list(&context, "suggestion_list", &args)
        }
        PublicCommand::PolicyDraft(crate::cli::public::DraftCommand::List { page }) => {
            super::mcp::operator_list(&context, "policy_draft_list", &page.arguments())
        }
        PublicCommand::Enable => administer(&context, "enable", &json!({}), input.json),
        PublicCommand::Disable => administer(&context, "disable", &json!({}), input.json),
        PublicCommand::Doctor(args) => {
            let mut values = args.page.arguments();
            if let Some(section) = &args.section {
                values["section"] = json!(section);
            }
            super::page::validate(&context, "doctor", &values)?;
            super::page::sections(&context, "doctor", &values, project::doctor(&context)?)
        }
        PublicCommand::Policy(policy) => match &policy.command {
            PolicyCommand::Show {
                proposal_id,
                section,
                page,
            } => {
                let mut args = page.arguments();
                if let Some(id) = proposal_id {
                    args["proposal_id"] = json!(id);
                }
                if let Some(section) = section {
                    args["section"] = json!(section);
                }
                super::mcp::operator_list(&context, "policy_show", &args)
            }
            PolicyCommand::Review => rules::policy(&context, input.json),
            PolicyCommand::Reject {
                proposal_id,
                reason,
            } => {
                let mut args = json!({"proposal_id":proposal_id});
                if let Some(reason) = reason {
                    args["reason"] = json!(reason);
                }
                administer(&context, "policy_reject", &args, input.json)
            }
            command => diagnostics::diagnostic(&context, command),
        },
        PublicCommand::Rule(rule) => match &rule.command {
            RuleCommand::List {
                page,
                section,
                pack_selector,
            } => {
                let mut args = page.arguments();
                if let Some(section) = section {
                    args["section"] = json!(section);
                }
                if let Some(selector) = pack_selector {
                    args["pack_selector"] = json!(selector);
                }
                super::mcp::operator_list(&context, "rule_list", &args)
            }
            command => rules::mutate(&context, command, input.json),
        },
        PublicCommand::Registry(registry) => match &registry.command {
            RegistryCommand::List { page } => {
                super::mcp::operator_list(&context, "registry_list", &page.arguments())
            }
            _ => registries::mutate(Some(&context), &context.fetcher, registry, input.json),
        },
        PublicCommand::Pack(pack) => match &pack.command {
            PackCommand::List { page } => {
                super::mcp::operator_list(&context, "pack_list", &page.arguments())
            }
            PackCommand::Show { reference } => super::packs::show(&context, reference),
            PackCommand::Search { query, page } => {
                let mut args = page.arguments();
                args["query"] = json!(query);
                super::mcp::operator_list(&context, "pack_search", &args)
            }
            PackCommand::Info {
                reference,
                view,
                section,
                page,
            } => {
                let mut args = page.arguments();
                args["reference"] = json!(reference);
                args["view"] = json!(view);
                if let Some(section) = section {
                    args["section"] = json!(section);
                }
                super::mcp::operator_list(&context, "pack_info", &args)
            }
            PackCommand::Configure {
                reference,
                set,
                unset,
                overrides,
                clear,
                local,
            } => packs::configure(
                &context,
                reference,
                &packs::setup::Edits {
                    set,
                    unset,
                    overrides,
                    clear,
                    local: *local,
                },
                input.json,
            ),
            PackCommand::Add {
                reference,
                profile,
                set,
                local,
            } => packs::add(&context, reference, *profile, set, *local, input.json),
            PackCommand::Update { reference } => {
                packs::update(&context, reference.as_deref(), input.json)
            }
            PackCommand::Remove { reference, local } => {
                packs::remove(&context, reference, *local, input.json)
            }
        },
        PublicCommand::Request(request) => match &request.command {
            RequestCommand::List { page } => {
                super::mcp::operator_list(&context, "request_list", &page.arguments())
            }
            RequestCommand::Show { request } => approvals::inspect(&context, Some(request)),
            RequestCommand::Approve { request } => decide(&context, request, false, input.json),
            RequestCommand::Deny { request } => decide(&context, request, true, input.json),
        },
        PublicCommand::History(crate::cli::public::History {
            command:
                HistoryCommand::Configure {
                    enabled,
                    limit,
                    purge,
                },
        }) => {
            let mut args = json!({"purge":purge});
            if let Some(enabled) = enabled {
                args["enabled"] = json!(enabled);
            }
            if let Some(limit) = limit {
                args["limit"] = json!(limit);
            }
            administer(&context, "history_configure", &args, input.json)
        }
        PublicCommand::History(history) => {
            if let HistoryCommand::List {
                page,
                denied,
                approved,
                agent,
                authorization_kind,
                grant_id,
            } = &history.command
            {
                let mut args = page.arguments();
                if *denied {
                    args["denied"] = json!(true);
                }
                if *approved {
                    args["approved"] = json!(true);
                }
                if let Some(value) = agent {
                    args["agent"] = json!(value);
                }
                if let Some(value) = authorization_kind {
                    args["authorization_kind"] = json!(value);
                }
                if let Some(value) = grant_id {
                    args["grant_id"] = json!(value);
                }
                super::mcp::operator_list(&context, "history_list", &args)
            } else if let HistoryCommand::Show {
                entry,
                section,
                page,
            } = &history.command
            {
                let mut args = page.arguments();
                args["entry_id"] = json!(entry);
                if let Some(section) = section {
                    args["section"] = json!(section);
                }
                super::mcp::operator_list(&context, "history_show", &args)
            } else {
                unreachable!("configuration handled above")
            }
        }
        _ => Err(Error::Invalid("invalid operator command")),
    }?;
    finish_pack_updates(&context, input, &mut value);
    Ok(Output {
        value,
        context: Some(context),
    })
}

fn finish_pack_updates(context: &Context, input: &Cli, value: &mut Value) {
    use crate::cli::public::PackCommand;
    let mutation = matches!(&input.command, PublicCommand::Pack(pack)
        if matches!(pack.command, PackCommand::Add { .. } | PackCommand::Update { .. }
            | PackCommand::Remove { .. } | PackCommand::Configure { .. }));
    if mutation && let Ok(state) = project::checkpoint(context) {
        value["pack_updates"] = super::pack_updates::inspect(context, &state, None);
    }
}

fn pack_update_notice(input: &Cli, value: &Value, context: &Context) {
    use crate::cli::public::PackCommand;
    use std::io::{IsTerminal as _, Write as _};
    let eligible = matches!(&input.command, PublicCommand::Status(_))
        || matches!(&input.command, PublicCommand::Pack(pack) if matches!(pack.command,
            PackCommand::List { .. } | PackCommand::Add { .. } | PackCommand::Update { .. }
            | PackCommand::Remove { .. } | PackCommand::Configure { .. }));
    if !eligible
        || input.json
        || !std::io::stdout().is_terminal()
        || !std::io::stderr().is_terminal()
        || value.get("pack_updates").is_none()
    {
        return;
    }
    let Some(pending) = super::pack_updates::pending(context, "cli", &value["pack_updates"]) else {
        return;
    };
    let text = presentation::pack_update_notice(&input.command, &pending.value);
    if writeln!(std::io::stderr().lock(), "{text}").is_ok() {
        pending.commit();
    }
}

fn administer(
    context: &Context,
    name: &str,
    args: &Value,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = terminal::Terminal::required(json_mode)?;
    let prepared = super::admin::prepare(context, name, args, &|| Ok(()))?;
    tty.confirm(&prepared.review())?;
    prepared.commit(context, &|| Ok(()))
}

pub(super) fn project(start: &Path) -> Result<Context, Error> {
    project_with(start, super::open)
}

fn project_with(start: &Path, open: fn(&Path) -> Result<Context, Error>) -> Result<Context, Error> {
    let path = start.canonicalize()?;
    for root in path.ancestors() {
        if configured(root) {
            return open(root);
        }
    }
    Err(Error::Invalid("no enrolled project; run shield init"))
}

fn configured(root: &Path) -> bool {
    crate::layout::discovery_root(root)
}

fn decide(context: &Context, id: &str, deny: bool, json_mode: bool) -> Result<Value, Error> {
    // Refuse JSON and missing terminals before reading any request.
    let mut tty = terminal::Terminal::required(json_mode)?;
    let (_, value) = approvals::request_view(context, id)?;
    if value["terminal"] == true {
        return Err(Error::Missing);
    }
    tty.confirm(
        &json!({"operation":if deny {"deny/revoke"} else {"approve once"},"request":value,"scope":if deny {"Deny the pending request or revoke an unused grant"} else {"One use; five-minute expiry; the exact stored request and resource binding"}}),
    )?;
    approvals::authorize(context, id, deny)
}
