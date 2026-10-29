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
            bind, prepare_add, prepare_configure, prepare_remove, prepare_update, rebuild, unbind,
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
    let before = crate::review::project::load(context)?;
    let edits = setup::Edits {
        set,
        local,
        ..setup::Edits::default()
    };
    // Going back from the first setup question asks for the profile again.
    loop {
        let mut asked = false;
        let (candidate, meta) = prepare_add(
            context,
            &before,
            reference,
            profile,
            local,
            |id, version| {
                asked = true;
                tty.choose_profile(
                    "Choose a profile",
                    &format!(
                        "{id} {version} · change it later with shield pack add {id} --profile <profile>"
                    ),
                )
            },
        )?;
        let settled = settle(
            &mut tty,
            &crate::review::project::root(context),
            &crate::review::project::metadata(&before),
            candidate,
            meta,
            &edits,
            asked,
        )?;
        match settled {
            Step::Done((candidate, meta)) => {
                return finish(context, &before, candidate, meta, &mut tty);
            }
            Step::Back if asked => {}
            Step::Back => return Err(Error::Cancelled),
        }
    }
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
    let before = crate::review::project::load(context)?;
    let project = crate::review::project::root(context);
    let (candidate, meta) = prepare_configure(&before, reference, |artifact, settings| {
        if !edits.is_empty() {
            return settings.edit(edits.local, |binding| {
                setup::apply(artifact, binding, edits)
            });
        }
        let id = policy::Pack::from_yaml(artifact)?.id().to_owned();
        let questions = !policy::declarations(artifact)?.is_empty();
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
            if setup::walk(&mut tty, &project, &id, artifact, settings, options)? == Step::Back {
                return Err(Error::Cancelled);
            }
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
    finish(context, &before, candidate, meta, &mut tty)
}

/// [`settle`] for a command whose first step is pack setup, such as update or
/// init's pack questions: going back from the first question cancels.
pub(super) fn settle_first(
    tty: &mut Terminal,
    project: &std::path::Path,
    previous: &Value,
    candidate: Value,
    meta: Value,
    edits: &setup::Edits<'_>,
) -> Result<(Value, Value), Error> {
    settle(tty, project, previous, candidate, meta, edits, false)?.or_cancel()
}

/// Ask for the variables of selections new to `previous` (or apply `edits` to
/// new direct ones), and for required values still missing elsewhere, then
/// rebuild the candidate when any value changed. Going back from one pack's
/// first question returns to the previous pack's last step; from the first
/// pack it returns [`Step::Back`], labeled "back" when an `earlier` step exists.
pub(super) fn settle(
    tty: &mut Terminal,
    project: &std::path::Path,
    previous: &Value,
    candidate: Value,
    mut meta: Value,
    edits: &setup::Edits<'_>,
    earlier: bool,
) -> Result<Step<(Value, Value)>, Error> {
    let known: Vec<&Value> = previous["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| &row["selector"])
        .collect();
    let artifacts: Vec<String> = serde_json::from_value(candidate["packs"].clone())
        .map_err(|_| Error::Invalid("pack artifacts missing"))?;
    let rows = meta["installed"]
        .as_array_mut()
        .ok_or(Error::Invalid("installed packs missing"))?;
    let mut edited = edits.is_empty();
    // Each selection's settings once visited, kept when going back across packs.
    let mut visited: Vec<Option<Settings>> = vec![None; rows.len()];
    // Selections walked interactively, in order; going back pops one.
    let mut walked: Vec<usize> = Vec::new();
    let mut resume = false;
    let mut index = 0;
    while index < rows.len() {
        if policy::declarations(&artifacts[index])?.is_empty() {
            index += 1;
            continue;
        }
        let settings = match &mut visited[index] {
            Some(settings) => settings,
            slot => slot.insert(crate::review::inspection::selection_settings(
                &candidate, index,
            )?),
        };
        let row = &rows[index];
        let fresh = !known.contains(&&row["selector"]);
        let direct = row["direct"] == true;
        if fresh && direct && !edits.is_empty() {
            settings.edit(edits.local, |binding| {
                setup::apply(&artifacts[index], binding, edits)
            })?;
            edited = true;
            index += 1;
            continue;
        }
        let options = setup::Options {
            scope: if fresh && direct && edits.is_empty() {
                setup::Walk::All
            } else {
                setup::Walk::Missing
            },
            local: edits.local,
            earlier: earlier || !walked.is_empty(),
            resume,
        };
        let id = row["id"].as_str().unwrap_or_default().to_owned();
        match setup::walk(tty, project, &id, &artifacts[index], settings, options)? {
            Step::Done(()) => {
                walked.push(index);
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
    if !edited {
        return Err(Error::Invalid(
            "--set applies to a newly added pack that declares variables; use shield pack configure",
        ));
    }
    let mut changed = false;
    for (index, settings) in visited.into_iter().enumerate() {
        let Some(settings) = settings else {
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

pub(super) fn remove(context: &Context, reference: &str, json_mode: bool) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let before = crate::review::project::load(context)?;
    let (candidate, meta) = prepare_remove(&before, reference)?;
    finish(context, &before, candidate, meta, &mut tty)
}
pub(super) fn update(
    context: &Context,
    reference: Option<&str>,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let before = crate::review::project::load(context)?;
    let skipped = crate::review::packs::update_skipped(&before.operator, reference)?;
    tty.progress(&super::presentation::update_progress(&skipped))?;
    let (candidate, meta) = prepare_update(context, &before, reference)?;
    let (candidate, meta) = settle_first(
        &mut tty,
        &crate::review::project::root(context),
        &crate::review::project::metadata(&before),
        candidate,
        meta,
        &setup::Edits::default(),
    )?;
    let mut result = finish(context, &before, candidate, meta, &mut tty)?;
    result["skipped_pinned"] = json!(skipped);
    Ok(result)
}
fn finish(
    context: &Context,
    before: &crate::review::model::State,
    candidate: Value,
    meta: Value,
    tty: &mut Terminal,
) -> Result<Value, Error> {
    if candidate == before.candidates["project"] && meta == crate::review::project::metadata(before)
    {
        return Ok(
            json!({"committed":false,"unchanged":true,"policy_revision":before.policy_revision}),
        );
    }
    state::review_candidate(context, before, &candidate, &meta, tty)?;
    crate::review::publication::commit(context, before, candidate, meta, None)
}
