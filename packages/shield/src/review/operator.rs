//! Canonical operator orchestration over project-owned policy, grants and state.
pub(crate) mod integrations;
mod packs;
mod path_access;
mod presentation;
mod project_files;
mod registries;
mod rules;
mod state;
mod terminal;
#[cfg(test)]
mod tests;

use super::{Context, Error, approvals, history, project};
use crate::cli::{Cli, PublicCommand};
use serde_json::{Value, json};
use std::{path::Path, process::ExitCode};

#[cfg(test)]
pub(super) fn commit_proposal_fixture(context: &Context, id: &str) -> Result<Value, Error> {
    let before = crate::review::project::load(context)?;
    let row = before
        .proposals
        .iter()
        .find(|p| p.id == id)
        .ok_or(Error::Missing)?;
    crate::review::publication::commit(
        context,
        &before,
        row.candidate.clone(),
        row.candidate_metadata.clone(),
        Some(id),
    )
}

pub(crate) fn run(input: &Cli) -> ExitCode {
    match dispatch(input) {
        Ok(mut value) => {
            super::model::redact(&mut value);
            // Every public operator result carries the schema it follows.
            if input.json
                && let Some(object) = value.as_object_mut()
            {
                object.entry("schema_version").or_insert_with(|| json!(1));
            }
            if input.json {
                println!("{value}");
            } else {
                presentation::print(&input.command, &value);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if input.json {
                let mut value = json!({"schema_version":1,"error":{"code":error.code(),"message":error.to_string()}});
                let recovery = presentation::errors::recovery(&input.command, &error);
                value["error"]["details"] = recovery["details"].clone();
                value["error"]["recovery"] = recovery["recovery"].clone();
                if let Error::Ambiguous {
                    alternatives,
                    total,
                    ..
                } = &error
                {
                    value["error"]["alternatives"] = json!(alternatives);
                    value["error"]["alternatives_total"] = json!(total);
                }
                println!("{value}");
            } else {
                presentation::errors::print(input, &error);
            }
            ExitCode::from(if matches!(error, Error::Cancelled) {
                130
            } else {
                1
            })
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one routing arm per public command keeps the command surface in one place"
)]
fn dispatch(input: &Cli) -> Result<Value, Error> {
    use crate::cli::public::{
        HistoryCommand, McpCommand, PackCommand, PolicyCommand, RegistryCommand, RequestCommand,
        RuleCommand, Scope,
    };
    let start = input.project.clone().unwrap_or(std::env::current_dir()?);
    match &input.command {
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
            );
        }
        PublicCommand::Registry(registry) if registry.scope == Scope::User => {
            if matches!(registry.command, RegistryCommand::List) {
                return super::registries::list(None);
            }
            return registries::mutate(None, &super::registries::fetcher(), registry, input.json);
        }
        _ => {}
    }
    let context = project(&start)?;
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
            PublicCommand::Cache { .. } => {
                println!("Preparing the committed policy's compiled cache…");
            }
            _ => {}
        }
    }
    match &input.command {
        PublicCommand::Mcp(crate::cli::public::Mcp {
            command: McpCommand::Setup(setup),
        }) => {
            let choice = setup.choice()?;
            if setup.dry_run {
                return integrations::setup_review(&context, choice, input.json);
            }
            let mut receipt = integrations::setup(&context, choice, input.json)?;
            if receipt.get("setup_error").is_none()
                && let Some(directory) = input.cache_dir.as_deref()
            {
                receipt["cache_preparation"] = super::cache::prepare(&context, Some(directory))?;
            }
            Ok(receipt)
        }
        PublicCommand::Cache { .. } => super::cache::prepare(&context, input.cache_dir.as_deref()),
        PublicCommand::Protection(_) => path_access::configure(&context, input.json),
        PublicCommand::Status => project::status(&context),
        PublicCommand::Doctor => project::doctor(&context),
        PublicCommand::Policy(policy) => match policy.command {
            PolicyCommand::Show => project::policy(&context),
            PolicyCommand::Review => rules::policy(&context, input.json),
        },
        PublicCommand::Rule(rule) => match &rule.command {
            RuleCommand::List => super::inspection::rules(&context, None),
            command => rules::mutate(&context, command, input.json),
        },
        PublicCommand::Registry(registry) => match registry.command {
            RegistryCommand::List => super::registries::list(Some(&context)),
            _ => registries::mutate(Some(&context), &context.fetcher, registry, input.json),
        },
        PublicCommand::Pack(pack) => match &pack.command {
            PackCommand::List => super::packs::list(&context),
            PackCommand::Show { reference } => super::packs::show(&context, reference),
            PackCommand::Search { query } => super::packs::search(&context, query),
            PackCommand::Info { reference } => super::packs::info(&context, reference),
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
            PackCommand::Remove { reference } => packs::remove(&context, reference, input.json),
        },
        PublicCommand::Request(request) => match &request.command {
            RequestCommand::List => approvals::inspect(&context, None),
            RequestCommand::Show { request } => approvals::inspect(&context, Some(request)),
            RequestCommand::Approve { request } => decide(&context, request, false, input.json),
            RequestCommand::Deny { request } => decide(&context, request, true, input.json),
        },
        PublicCommand::History(history) => history::inspect(
            &context,
            &match &history.command {
                HistoryCommand::List {
                    denied,
                    approved,
                    agent,
                } => history::Query {
                    entry: None,
                    denied: *denied,
                    approved: *approved,
                    agent: agent.as_deref(),
                },
                HistoryCommand::Show { entry } => history::Query {
                    entry: Some(entry.as_str()),
                    denied: false,
                    approved: false,
                    agent: None,
                },
            },
        ),
        _ => Err(Error::Invalid("invalid operator command")),
    }
}

pub(super) fn project(start: &Path) -> Result<Context, Error> {
    let path = start.canonicalize()?;
    for root in path.ancestors() {
        if root.join(".shield/review").exists() {
            return super::open(root);
        }
    }
    Err(Error::Invalid("no enrolled project; run shield init"))
}

fn decide(context: &Context, id: &str, deny: bool, json_mode: bool) -> Result<Value, Error> {
    let mut tty = terminal::Terminal::required(json_mode)?;
    let (read, value) = approvals::request_view(context, id)?;
    tty.confirm(
        &json!({"operation":if deny {"deny/revoke"} else {"approve once"},"request":value,"scope":if deny {"Deny the pending request or revoke an unused grant"} else {"One use; five-minute expiry; the exact stored request and resource binding"}}),
    )?;
    if read {
        super::read_approval::authorize(context, id, 300, None, deny)
    } else {
        super::run_approval::authorize(context, id, 300, None, deny)
    }
}
