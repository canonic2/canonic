//! Equivalent local launcher maintenance after operation-epoch rotation.
use super::{
    Context, Error, Plan, Registration, State, activation, agents, binary_digest, files, json,
    pending, portable, registration,
};
use crate::{
    mcp_connections::{
        Connection, Transport,
        launcher::{Binding, valid_token},
    },
    review::{runtime, storage::Store},
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub(super) const REASON: &str = "equivalent private launcher rebind";

/// Runtime startup may repair exact owned launchers; diagnostics never call it.
/// Failure leaves that launcher unavailable without revoking portable policy.
pub(in crate::review) fn resume(context: &Context, initial: &State) {
    let _trace = crate::diagnostics::trace::span("integration.rebind");
    crate::diagnostics::trace::count("integration.rebind_attempts", 1);
    if let Err(error) = resume_inner(context, initial)
        && !matches!(error, Error::PolicyFileConflict(_))
    {
        eprintln!("Shield launcher maintenance unavailable ({})", error.code());
    }
}

fn resume_inner(context: &Context, initial: &State) -> Result<(), Error> {
    if let Some(plan) = pending(&initial.operator)? {
        // The owner is a policy revision, which only the files give.
        if matches!(
            plan.scope_reason.as_deref(),
            Some(REASON | portable::REASON | portable::ADOPT_REASON)
        ) && let Some(owner) = activation::current_owner(
            &Store::open(Path::new(&context.project_root))?.read_following_files()?,
        ) {
            let guard = crate::review::publication::ProjectionLease::acquire(Path::new(
                &context.project_root,
            ))?;
            activation::resume_guarded(context, None, &owner, &guard)?;
        }
        return Ok(());
    }
    let Some(previous) = registration(&initial.operator)? else {
        return adopt_host(context, initial);
    };
    let migrate_native = portable::needed(initial);
    if initial.operator["mcp_rebind_pending"] != true && !migrate_native {
        return Ok(());
    }
    initial.check_operations()?;
    let guard =
        crate::review::publication::ProjectionLease::acquire(Path::new(&context.project_root))?;
    guard.check(context)?;
    let root = Path::new(&context.project_root);
    let store = Store::open(root)?;
    // Upkeep requires valid current source. Refuse before hashing the binary
    // when source is broken, rather than repeating that cost on every open
    // made by an otherwise policy-independent recovery request.
    let state = store.read_following_files()?;
    state.check(context)?;
    state.check_operations()?;
    // A different executable is an actual administrative change, not relocation.
    let executable = std::env::current_exe()?.canonicalize()?;
    if binary_digest(&executable)? != previous.binary_digest {
        return Ok(());
    }
    let mut native_retry = false;
    if migrate_native {
        match portable::prepare_pin(root, &state, &executable) {
            Ok(Some(plan)) => publish(context, &store, &state, &plan, &guard)?,
            Ok(None) => {}
            Err(error) => native_retry |= !portable::manual(&error),
        }
        for agent in previous.agents.keys() {
            let state = store.read_following_files()?;
            state.check(context)?;
            state.check_operations()?;
            if pending(&state.operator)?.is_some() {
                return Ok(());
            }
            // Unknown/edited callbacks are retained for explicit publisher review.
            let plan = match portable::prepare(root, &state, agent) {
                Ok(Some(plan)) => plan,
                Ok(None) => continue,
                Err(error) => {
                    native_retry |= !portable::manual(&error);
                    continue;
                }
            };
            publish(context, &store, &state, &plan, &guard)?;
        }
    }
    let snapshot = runtime::consumer::load(context, &|| Ok(()))?;
    for adapter in agents::ADAPTERS
        .iter()
        .filter(|adapter| adapter.portable_mcp_launcher())
    {
        for (id, connection) in snapshot.mcp_connections() {
            let Some(alias) = connection.agents.get(adapter.spec().id) else {
                continue;
            };
            let state = store.read_following_files()?;
            state.check(context)?;
            state.check_operations()?;
            // The snapshot is the policy files' plan; the state must follow them.
            if state.policy_revision != snapshot.identity().policy_revision() {
                return Err(Error::StaleSnapshot);
            }
            if pending(&state.operator)?.is_some() {
                return Ok(());
            }
            // One edited/missing alias does not prevent an unrelated exact rebind.
            let Ok(Some(plan)) = prepare(root, &state, *adapter, id, alias, connection) else {
                continue;
            };
            publish(context, &store, &state, &plan, &guard)?;
        }
    }
    finish_maintenance(
        context,
        &store,
        &executable,
        &previous.binary_digest,
        migrate_native,
        native_retry,
    )
}

/// Record the committed Hub or Box definition of a folder that has no
/// registration, such as a fresh worktree or clone, so its agents are set up
/// as the definition says. A folder without one, or with an edited one, is unchanged.
fn adopt_host(context: &Context, initial: &State) -> Result<(), Error> {
    let root = Path::new(&context.project_root);
    let executable = std::env::current_exe()?.canonicalize()?;
    // An attempt that did not adopt these exact files with this executable is
    // not repeated on every request; a change to either tries again.
    let Some(key) = portable::adoption_key(root, &executable) else {
        return Ok(());
    };
    if initial.operator[ADOPTION_CHECKED] == key.as_str() {
        return Ok(());
    }
    match portable::adopt_host(root, initial, &executable) {
        Ok(Some(_)) => {}
        outcome => {
            record_adoption_checked(context, &key)?;
            return outcome.map(|_| ());
        }
    }
    initial.check_operations()?;
    let guard = crate::review::publication::ProjectionLease::acquire(root)?;
    guard.check(context)?;
    let store = Store::open(root)?;
    let state = store.read_following_files()?;
    state.check(context)?;
    state.check_operations()?;
    match portable::adopt_host(root, &state, &executable)? {
        Some(plan) => publish(context, &store, &state, &plan, &guard),
        None => Ok(()),
    }
}

/// Operator metadata naming the last Hub or Box adoption attempt that adopted
/// nothing. The key predates Hub.
const ADOPTION_CHECKED: &str = "box_adoption_checked";

fn record_adoption_checked(context: &Context, key: &str) -> Result<(), Error> {
    Store::open(Path::new(&context.project_root))?.update_checkpoint_if_changed(
        context,
        &|| Ok(()),
        |state| {
            if state.operator[ADOPTION_CHECKED] == key {
                return Ok(false);
            }
            state.operator[ADOPTION_CHECKED] = json!(key);
            Ok(true)
        },
    )
}

fn finish_maintenance(
    context: &Context,
    store: &Store,
    executable: &Path,
    expected_binary: &str,
    migrate_native: bool,
    native_retry: bool,
) -> Result<(), Error> {
    store.update_checkpoint_if_changed(context, &|| Ok(()), |state| {
        state.check_operations()?;
        if registration(&state.operator)?
            .is_none_or(|current| current.binary_digest != expected_binary)
        {
            return Err(Error::StaleSnapshot);
        }
        if (state.operator["mcp_rebind_pending"] == true || migrate_native)
            && pending(&state.operator)?.is_none()
        {
            let receipt = json!({"schema_version":1,"path":executable,"digest":expected_binary});
            let changed = state.operator["current_executable"] != receipt
                || state.operator["mcp_rebind_pending"] == true
                || (migrate_native
                    && !native_retry
                    && state.operator["portable_registration_epoch"] != state.project_id);
            state.operator["current_executable"] = receipt;
            state
                .operator
                .as_object_mut()
                .ok_or(Error::Invalid("operator metadata"))?
                .remove("mcp_rebind_pending");
            if migrate_native && !native_retry {
                state.operator["portable_registration_epoch"] = json!(state.project_id);
            }
            return Ok(changed);
        }
        Ok(false)
    })
}

fn publish(
    context: &Context,
    store: &Store,
    state: &State,
    plan: &Plan,
    guard: &crate::review::publication::ProjectionLease,
) -> Result<(), Error> {
    let revision = state.policy_revision.clone();
    let expected = state.operator.clone();
    store.update_checkpoint(context, &|| Ok(()), |current| {
        current.check_operations()?;
        if current.operator != expected || current.policy_revision != revision {
            return Err(Error::StaleSnapshot);
        }
        activation::stage(&mut current.operator, plan, None)?;
        activation::own(&mut current.operator, &revision);
        Ok(json!({}))
    })?;
    activation::resume_guarded(context, None, &revision, guard)?;
    Ok(())
}

fn prepare(
    root: &Path,
    state: &State,
    adapter: &dyn crate::agents::AgentAdapter,
    id: &str,
    alias: &str,
    connection: &Connection,
) -> Result<Option<Plan>, Error> {
    if !connection.local || !connection.enabled {
        return Ok(None);
    }
    let Some(previous) = registration(&state.operator)? else {
        return Ok(None);
    };
    let agent = adapter.spec().id;
    let Some(owned) = previous.connections.get(agent) else {
        return Ok(None);
    };
    if !owned["launches"][id].is_null() {
        return Ok(None);
    }
    let reviewed = owned["review"].as_array().into_iter().flatten().any(|row| {
        row["id"] == id
            && row["alias"] == alias
            && row["source"] == "local"
            && row["enabled"] == true
            && row["settings"] == json!(connection.transport)
            && row["packs"] == json!(connection.packs)
            && row["projection"] == "private Shield launcher"
    });
    let Some(entry) = owned["entries"].get(alias) else {
        return Ok(None);
    };
    if !reviewed {
        return Ok(None);
    }
    adapter.check_mcp_scopes(root, root, alias)?;
    let source = if let Some(path) = adapter.spec().mcp_path {
        files::read(root, path)?
    } else {
        super::read_file(root)?
    };
    let Some(source) = source else {
        return Ok(None);
    };
    let Some(actual) = adapter.connection(&source, alias)? else {
        return Ok(None);
    };
    if !known_launcher(
        &actual,
        &previous,
        &state.operator["portable_launch_roots"],
        root,
    ) {
        return Ok(None);
    }
    let binding = Binding::new(connection)?;
    let transport = binding.portable_projected(connection, &previous.binary, root)?;
    // The adapter checks its complete exact ownership encoding before replacement.
    let update = adapter.merge_connections(
        &source,
        &json!({alias:entry}),
        &BTreeMap::from([(alias.to_owned(), transport)]),
    )?;
    let replacement = update
        .ownership
        .as_ref()
        .and_then(|owned| owned.get(alias))
        .ok_or(Error::Invalid("rebound MCP ownership missing"))?;
    let mut desired = previous.clone();
    let next = desired.connections.get_mut(agent).ok_or(Error::Missing)?;
    next["entries"][alias] = replacement.clone();
    next["launches"][id] = json!(binding);
    let (before, after, changes) = if let Some(path) = adapter.spec().mcp_path {
        (
            None,
            String::new(),
            vec![files::Change {
                path: path.into(),
                before: Some(source),
                after: Some(update.source),
            }],
        )
    } else {
        (Some(source), update.source, Vec::new())
    };
    let plan = Plan {
        targets: Some(vec![agent.into()]),
        scope_reason: Some(REASON.into()),
        policy_revision: Some(state.policy_revision.clone()),
        before,
        after,
        previous: Some(previous),
        desired: Some(desired),
        files: changes,
        handed_over: Vec::new(),
    };
    super::maintenance::validate(&plan)?;
    super::privacy::validate(root, &plan)?;
    files::validate_plan_before(root, &plan)?;
    Ok(Some(plan))
}

fn known_launcher(
    transport: &Transport,
    registration: &Registration,
    roots: &Value,
    root: &Path,
) -> bool {
    let Transport::Stdio { command, args, env } = transport else {
        return false;
    };
    // Launchers carry the project's own `--config` selection after the token,
    // and only that one.
    let config = crate::layout::launch_args(root).unwrap_or_default();
    if args.len() < 6 || args[6..] != config[..] {
        return false;
    }
    if args[0] != "__internal"
        || args[1] != "mcp-connect"
        || args[2] != "--project"
        || args[4] != "--token"
        || !valid_token(&args[5])
    {
        return false;
    }
    if env
        .iter()
        .any(|(key, value)| key != "SHIELD_MCP_GENERATION" || !valid_token(value))
    {
        return false;
    }
    command == "shield" && args[3] == "."
        || registration.binary.to_str() == Some(command.as_str())
            && roots
                .as_array()
                .is_some_and(|roots| roots.iter().any(|root| root == &args[3]))
}

/// Preserve unavailable neighboring launchers rather than adopting or repairing them.
pub(super) fn unchanged(plan: &Plan, agent: &str, id: &str, alias: &str) -> bool {
    if !matches!(
        plan.scope_reason.as_deref(),
        Some(REASON | portable::REASON)
    ) {
        return false;
    }
    let old = plan
        .previous
        .as_ref()
        .and_then(|r| r.connections.get(agent));
    let new = plan.desired.as_ref().and_then(|r| r.connections.get(agent));
    old.zip(new).is_some_and(|(old, new)| {
        old["entries"][alias] == new["entries"][alias]
            && old["launches"][id] == new["launches"][id]
            && old["review"] == new["review"]
    })
}
