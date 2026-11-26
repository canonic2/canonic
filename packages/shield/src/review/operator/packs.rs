//! Interactive pack selection, setup questions and confirmed CLI changes.
pub(super) mod setup;
use super::{
    state,
    terminal::{Step, Terminal},
};
use crate::{
    policy::{self, Profile},
    review::{
        Context, Error,
        inspection::Settings,
        packs::{
            bind, prepare_add_unconfigured as prepare_add, prepare_configure, prepare_remove,
            prepare_update, rebuild, unbind,
        },
    },
};
use serde_json::{Value, json};

pub(super) fn add(
    context: &Context,
    reference: &str,
    profile: Option<Profile>,
    set: &[String],
    local: bool,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    tty.progress(&format!("Resolving {reference} and its dependencies…"))?;
    let (before, base) = crate::review::project::load_prepared(context)?;
    let edits = setup::Edits {
        set,
        local,
        ..setup::Edits::default()
    };
    let (candidate, meta) = prepare_add(context, &before, reference, profile, local)?;
    let target = (!set.is_empty())
        .then(|| crate::review::packs::addition_selection(&meta, &before.settings, reference))
        .transpose()?;
    let (candidate, meta) = settle(
        &mut tty,
        &crate::review::project::root(context),
        candidate,
        meta,
        &edits,
        target,
    )?
    .or_cancel()?;
    finish(context, &before, candidate, meta, &mut tty, &base)
}

/// Change one installed selection's variable values and rule overrides: the
/// command-line edits when given, otherwise the guided walk.
pub(super) fn configure(
    context: &Context,
    reference: &str,
    edits: &setup::Edits<'_>,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let (before, base) = crate::review::project::load_prepared(context)?;
    let project = crate::review::project::root(context);
    let (candidate, meta) = prepare_configure(&before, reference, |artifact, settings| {
        if !edits.is_empty() {
            return settings.edit(edits.local, |binding| {
                setup::apply(artifact, binding, edits)
            });
        }
        let scope = crate::review::packs::discovery::Scope::for_candidate(
            &project,
            &before.candidates["project"],
        )?;
        if crate::review::packs::discovery::settings(artifact, settings, &scope)? {
            return Ok(());
        }
        let id = policy::Pack::from_retained_yaml(artifact)?.id().to_owned();
        let mut navigation = setup::Navigation::default();
        let mut resume = false;
        // Setup questions, then rule overrides; going back from the override
        // list returns to the walk's last step.
        loop {
            let options = setup::Options {
                scope: setup::Walk::All,
                local: edits.local,
                earlier: false,
                resume,
            };
            if setup::walk(
                &mut tty,
                &project,
                &id,
                artifact,
                settings,
                options,
                &mut navigation,
            )? == Step::Back
            {
                return Err(Error::Cancelled);
            }
            let questions = navigation.has_questions();
            let mut step = Step::Done(());
            settings.edit(edits.local, |binding| {
                step = setup::walk_overrides(&mut tty, artifact, binding, questions)?;
                Ok(())
            })?;
            match step {
                Step::Done(()) => return Ok(()),
                Step::Back if questions => resume = true,
                Step::Back => return Err(Error::Cancelled),
            }
        }
    })?;
    finish(context, &before, candidate, meta, &mut tty, &base)
}

/// [`settle`] for a command whose first step is pack setup, such as update or
/// init's pack questions: going back from the first question cancels.
pub(super) fn settle_first(
    tty: &mut Terminal,
    project: &std::path::Path,
    candidate: Value,
    meta: Value,
) -> Result<(Value, Value), Error> {
    settle(
        tty,
        project,
        candidate,
        meta,
        &setup::Edits::default(),
        None,
    )?
    .or_cancel()
}

/// Apply explicit settings to the named selection and ask for required values
/// still missing in the captured closure.
pub(super) fn settle(
    tty: &mut Terminal,
    project: &std::path::Path,
    candidate: Value,
    mut meta: Value,
    edits: &setup::Edits<'_>,
    target: Option<usize>,
) -> Result<Step<(Value, Value)>, Error> {
    struct Visit {
        base: Settings,
        result: Option<Settings>,
        navigation: setup::Navigation,
    }
    let artifacts: Vec<String> = serde_json::from_value(candidate["packs"].clone())
        .map_err(|_| Error::Invalid("pack artifacts missing"))?;
    let scope = crate::review::packs::discovery::Scope::for_candidate(project, &candidate)?;
    let rows = meta["installed"]
        .as_array_mut()
        .ok_or(Error::Invalid("installed packs missing"))?;
    let mut visited: Vec<Option<Visit>> = (0..rows.len()).map(|_| None).collect();
    // Selections walked interactively, in order; going back pops one.
    let mut walked: Vec<usize> = Vec::new();
    let mut resume = false;
    let mut index = 0;
    while index < rows.len() {
        if Some(index) != target && policy::declarations(&artifacts[index])?.is_empty() {
            index += 1;
            continue;
        }
        let row = &rows[index];
        let local = row["layer"] == "local";
        let visit = match &mut visited[index] {
            Some(visit) => visit,
            slot => {
                let mut base = crate::review::inspection::selection_settings(&candidate, index)?;
                if Some(index) == target {
                    base.edit(edits.local || local, |binding| {
                        setup::apply(&artifacts[index], binding, edits)
                    })?;
                }
                crate::review::packs::discovery::settings(&artifacts[index], &mut base, &scope)?;
                slot.insert(Visit {
                    base,
                    result: None,
                    navigation: setup::Navigation::default(),
                })
            }
        };
        // The missing-question scope and new bindings both start from the
        // values before the walk, including explicit and discovered settings.
        let mut settings = visit.base.clone();
        let options = setup::Options {
            scope: setup::Walk::Missing,
            local: local || (Some(index) == target && edits.local),
            earlier: !walked.is_empty(),
            resume,
        };
        let id = row["id"].as_str().unwrap_or_default().to_owned();
        match setup::walk(
            tty,
            project,
            &id,
            &artifacts[index],
            &mut settings,
            options,
            &mut visit.navigation,
        )? {
            Step::Done(()) => {
                if visit.navigation.has_questions() {
                    walked.push(index);
                }
                visit.result = Some(settings);
                index += 1;
                resume = false;
            }
            Step::Back => match walked.pop() {
                Some(previous) => {
                    index = previous;
                    resume = true;
                }
                None => return Ok(Step::Back),
            },
        }
    }
    let mut changed = false;
    for (index, visit) in visited.into_iter().enumerate() {
        let Some(settings) = visit.and_then(|visit| visit.result) else {
            continue;
        };
        if settings != crate::review::inspection::selection_settings(&candidate, index)? {
            bind(&mut rows[index], &settings);
            changed = true;
        }
    }
    if !changed {
        return Ok(Step::Done((candidate, meta)));
    }
    let candidate = rebuild(&candidate, rows, &artifacts, None)?;
    unbind(rows);
    Ok(Step::Done((candidate, meta)))
}

pub(super) fn remove(
    context: &Context,
    reference: &str,
    local: bool,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let (before, base) = crate::review::project::load_prepared(context)?;
    let (candidate, meta) = if local {
        crate::review::packs::prepare_remove_local(&before, reference)?
    } else {
        prepare_remove(&before, reference)?
    };
    finish(context, &before, candidate, meta, &mut tty, &base)
}
pub(super) fn update(
    context: &Context,
    reference: Option<&str>,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let (before, base) = crate::review::project::load_prepared(context)?;
    tty.progress("Checking for newer releases…")?;
    let (candidate, meta) = prepare_update(context, &before, reference)?;
    let (candidate, meta) = settle_first(
        &mut tty,
        &crate::review::project::root(context),
        candidate,
        meta,
    )?;
    finish(context, &before, candidate, meta, &mut tty, &base)
}
fn finish(
    context: &Context,
    before: &crate::review::model::State,
    candidate: Value,
    meta: Value,
    tty: &mut Terminal,
    base: &crate::configuration::Evaluator,
) -> Result<Value, Error> {
    if candidate == before.candidates["project"] && meta == crate::review::project::metadata(before)
    {
        return Ok(
            json!({"committed":false,"unchanged":true,"policy_revision":before.policy_revision}),
        );
    }
    state::write_reviewed(context, before, candidate, meta, tty, base)
}
