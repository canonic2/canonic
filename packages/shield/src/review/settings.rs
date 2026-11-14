//! Shield's settings, which are keys of `config.yml` and `config.local.yml`:
//! reading them when review state loads, writing a command's change into
//! `config.local.yml`, and moving the settings an earlier checkpoint held into
//! that file once.
use super::{Context, Error, model::State, path_access::PathAccess};
use crate::configuration::settings::{
    self, Enforcement, Fallback, History, Layer, Protection, Values,
};
use canonic_registry_kit::registrations::Registrations;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[cfg(test)]
mod tests;

const SHARED: &str = ".shield/config.yml";
const LOCAL: &str = "config.local.yml";

/// The settings of the policy files under `root`, local layer applied, or
/// `None` for a folder without `config.yml`.
///
/// # Errors
/// Refuses unreadable files and invalid settings.
pub(in crate::review) fn read(root: &Path) -> Result<Option<Layer>, Error> {
    let Some(shared) = text(&root.join(SHARED))? else {
        return Ok(None);
    };
    let shared: Layer = crate::policy::parse(&shared)?;
    shared.validate()?;
    let local = match text(&root.join(".shield").join(LOCAL))? {
        Some(local) => crate::policy::parse(&local)?,
        None => Layer::default(),
    };
    local.validate()?;
    Ok(Some(shared.under(&local)))
}

fn text(path: &Path) -> Result<Option<String>, Error> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(_) => crate::application::read(path)
            .map(Some)
            .map_err(|_| Error::Invalid("policy file unreadable")),
    }
}

/// The settings in effect for the folder at `root`: the files' values, over
/// what an earlier checkpoint still holds for keys the files leave unset.
/// Files that cannot be read keep native enforcement on, since a request then
/// asks anyway.
pub(in crate::review) fn current(root: &Path, legacy: &Legacy) -> Values {
    match read(root) {
        Ok(Some(files)) => legacy.under(&files).values(),
        Ok(None) => legacy.layer().values(),
        Err(_) => {
            let mut values = legacy.layer().values();
            values.enforcement = Enforcement::Enabled;
            values
        }
    }
}

/// Read the settings for a state just loaded from its checkpoint: take the
/// settings records an earlier checkpoint held out of its operator metadata,
/// then read the files over them. Settings already taken by a conversion that
/// could not move them yet stay in effect.
pub(in crate::review) fn load(root: &Path, state: &mut State) {
    let taken = Legacy::take(state);
    if !taken.is_empty() {
        state.legacy_settings = taken;
    }
    state.settings = current(root, &state.legacy_settings);
}

/// Whether a checkpoint just decoded holds settings records, as earlier
/// formats did, without taking them.
pub(in crate::review) fn held(state: &State) -> bool {
    let operator = &state.operator;
    state.legacy_enforcement.is_some()
        || state.legacy_history_enabled.is_some()
        || state.legacy_history_limit.is_some()
        || !operator["path_access"].is_null()
        || operator["sources"]
            .as_object()
            .is_some_and(|sources| !sources.is_empty())
        || !operator["default"].is_null()
        || operator["integration"].get("ask_fallback").is_some()
}

/// The settings an earlier checkpoint held. A checkpoint written since holds
/// none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Legacy {
    enforcement: Option<bool>,
    history_enabled: Option<bool>,
    history_limit: Option<usize>,
    path_access: Option<PathAccess>,
    registries: Option<Registrations>,
    ask_fallback: BTreeMap<String, Fallback>,
}

impl Legacy {
    fn take(state: &mut State) -> Self {
        let mut legacy = Self {
            enforcement: state.legacy_enforcement.take().map(|mode| mode.enabled),
            history_enabled: state.legacy_history_enabled.take(),
            history_limit: state
                .legacy_history_limit
                .take()
                .filter(|limit| (1..=settings::MAX_HISTORY_LIMIT).contains(limit)),
            ..Self::default()
        };
        let Some(operator) = state.operator.as_object_mut() else {
            return legacy;
        };
        // An unreadable record stays out of effect, like an invalid file key.
        legacy.path_access = operator
            .remove("path_access")
            .filter(|value| !value.is_null())
            .and_then(|value| serde_json::from_value(value).ok());
        let sources = operator.remove("sources");
        let default = operator.remove("default");
        let named = sources
            .as_ref()
            .and_then(Value::as_object)
            .is_some_and(|sources| !sources.is_empty())
            || default.as_ref().is_some_and(|default| !default.is_null());
        if named {
            legacy.registries = serde_json::from_value(json!({
                "sources": sources.unwrap_or_else(|| json!({})),
                "default": default.unwrap_or(Value::Null),
            }))
            .ok();
        }
        if let Some(fallbacks) = operator
            .get_mut("integration")
            .and_then(Value::as_object_mut)
            .and_then(|integration| integration.remove("ask_fallback"))
        {
            legacy.ask_fallback = serde_json::from_value(fallbacks).unwrap_or_default();
        }
        legacy
    }

    pub(in crate::review) fn is_empty(&self) -> bool {
        self.layer().is_empty()
    }

    /// Put these settings back into an encoded checkpoint, as the earlier
    /// format held them, so values the files could not take yet are kept.
    /// Records the checkpoint already holds stay as they are.
    pub(in crate::review) fn keep(&self, checkpoint: &mut Value) {
        if let Some(enabled) = self.enforcement
            && checkpoint["enforcement"].is_null()
        {
            checkpoint["enforcement"] = json!(super::mode::Mode::with(enabled));
        }
        if let Some(enabled) = self.history_enabled
            && checkpoint["history_enabled"].is_null()
        {
            checkpoint["history_enabled"] = json!(enabled);
        }
        if let Some(limit) = self.history_limit
            && checkpoint["history_limit"].is_null()
        {
            checkpoint["history_limit"] = json!(limit);
        }
        if let Some(access) = self.path_access
            && checkpoint["operator"]["path_access"].is_null()
        {
            checkpoint["operator"]["path_access"] = json!(access);
        }
        if let Some(registries) = &self.registries {
            for (alias, source) in &registries.sources {
                if checkpoint["operator"]["sources"].get(alias).is_none() {
                    checkpoint["operator"]["sources"][alias] = json!(source);
                }
            }
            if checkpoint["operator"]["default"].is_null() {
                checkpoint["operator"]["default"] = json!(registries.default);
            }
        }
        if !self.ask_fallback.is_empty()
            && checkpoint["operator"]["integration"].is_object()
            && checkpoint["operator"]["integration"]["ask_fallback"].is_null()
        {
            checkpoint["operator"]["integration"]["ask_fallback"] = json!(self.ask_fallback);
        }
    }

    /// The policy files' settings over these. Registry aliases the files do
    /// not name stay enrolled, so moving them never drops a registry.
    fn under(&self, files: &Layer) -> Layer {
        let legacy = self.layer();
        let mut merged = legacy.under(files);
        if let (Some(earlier), Some(registries)) = (&legacy.registries, &mut merged.registries) {
            for (alias, source) in &earlier.sources {
                registries
                    .sources
                    .entry(alias.clone())
                    .or_insert_with(|| source.clone());
            }
            if registries.default.is_none() {
                registries.default.clone_from(&earlier.default);
            }
        }
        merged
    }

    /// These settings as a layer that sets only what the checkpoint held.
    fn layer(&self) -> Layer {
        let defaults = PathAccess::default();
        let protection = self
            .path_access
            .filter(|access| *access != defaults)
            .map(|access| Protection {
                agent_settings_read: Some(access.agent_settings_read),
                agent_settings_change: Some(access.agent_settings_change),
            });
        let history =
            (self.history_enabled.is_some() || self.history_limit.is_some()).then_some(History {
                enabled: self.history_enabled,
                limit: self.history_limit,
            });
        Layer {
            enforcement: self.enforcement.map(|enabled| {
                if enabled {
                    Enforcement::Enabled
                } else {
                    Enforcement::Paused
                }
            }),
            protection,
            ask_fallback: self.ask_fallback.clone(),
            history,
            registries: self.registries.clone(),
        }
    }
}

/// Whether `state` holds settings from an earlier checkpoint that its folder's
/// policy files can take.
pub(in crate::review) fn pending(state: &State) -> bool {
    !state.legacy_settings.is_empty() && Path::new(&state.project_root).join(SHARED).is_file()
}

/// Move the settings an earlier checkpoint held into `config.local.yml`,
/// for the keys the files leave unset, under the writer lock. Afterwards the
/// checkpoint written holds none of them.
///
/// # Errors
/// Refuses files Shield cannot read or write; the checkpoint keeps its
/// values and the next reader tries again.
pub(in crate::review) fn convert(state: &mut State) -> Result<(), Error> {
    let root = Path::new(&state.project_root);
    let files = read(root)?.ok_or(Error::Invalid("project configuration missing"))?;
    let target = state.legacy_settings.under(&files).values();
    let changes = files.changes_to(&target);
    if !changes.is_empty() {
        let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
        write_local(&context, root, &changes)?;
    }
    state.legacy_settings = Legacy::default();
    state.settings = current(root, &state.legacy_settings);
    Ok(())
}

/// Write `changes`, the keys a command sets, into `config.local.yml`, keeping
/// the file's text and formatting, after checking the result is valid
/// configuration. The setting in effect is then what the command chose.
///
/// # Errors
/// Refuses a folder without policy files, unreadable files and changes that
/// make the configuration invalid.
pub(in crate::review) fn write(context: &Context, changes: &Layer) -> Result<(), Error> {
    let root = Path::new(&context.project_root);
    if !root.join(SHARED).is_file() {
        return Err(Error::Invalid(
            "this project has no policy files; run shield init to create them",
        ));
    }
    write_local(context, root, changes)
}

fn write_local(context: &Context, root: &Path, changes: &Layer) -> Result<(), Error> {
    let shield = root.join(".shield");
    let shared =
        text(&shield.join("config.yml"))?.ok_or(Error::Invalid("project configuration missing"))?;
    let previous = text(&shield.join(LOCAL))?;
    let mut document: Value = match &previous {
        Some(previous) => crate::policy::parse(previous)?,
        None => json!({"schema_version":1}),
    };
    let fields = document
        .as_object_mut()
        .ok_or(Error::Invalid("config.local.yml must be a mapping"))?;
    settings::set_into(fields, changes);
    let written = crate::policy_files::write(previous.as_deref(), &document)?;
    let merged = crate::configuration::layers::merge(&shared, Some(&written.text))?;
    crate::configuration::Candidate::from_yaml(&merged)?;
    super::publication::projections::materialize(
        context,
        &BTreeMap::from([(LOCAL.to_owned(), written.text)]),
    )
}

/// The protected file access in effect.
pub(in crate::review) fn path_access(values: &Values) -> PathAccess {
    PathAccess {
        agent_settings_read: values.agent_settings_read,
        agent_settings_change: values.agent_settings_change,
        ..PathAccess::default()
    }
}

/// The layer that sets protected file access to `access`.
pub(in crate::review) fn protection(access: PathAccess) -> Layer {
    Layer {
        protection: Some(Protection {
            agent_settings_read: Some(access.agent_settings_read),
            agent_settings_change: Some(access.agent_settings_change),
        }),
        ..Layer::default()
    }
}

/// The layer that sets native enforcement.
pub(in crate::review) fn enforcement(enabled: bool) -> Layer {
    Layer {
        enforcement: Some(if enabled {
            Enforcement::Enabled
        } else {
            Enforcement::Paused
        }),
        ..Layer::default()
    }
}

/// The layer that sets request history.
pub(in crate::review) fn history(enabled: bool, limit: usize) -> Layer {
    Layer {
        history: Some(History {
            enabled: Some(enabled),
            limit: Some(limit),
        }),
        ..Layer::default()
    }
}

/// The layer that sets the project's registries, whole.
pub(in crate::review) fn registries(registries: Registrations) -> Layer {
    Layer {
        registries: Some(registries),
        ..Layer::default()
    }
}

/// The layer that sets agents' Ask fallbacks; Block for agents the setup no
/// longer covers, so their choice leaves the files too.
pub(in crate::review) fn fallbacks(fallbacks: BTreeMap<String, Fallback>) -> Layer {
    Layer {
        ask_fallback: fallbacks,
        ..Layer::default()
    }
}

/// Change the project's registries without review: in `config.local.yml`, as
/// a registry command does, or in a folder without policy files, where an
/// earlier checkpoint holds them.
#[cfg(test)]
pub(crate) fn registries_fixture(context: &Context, edit: impl FnOnce(&mut Registrations)) {
    let root = Path::new(&context.project_root);
    if root.join(SHARED).is_file() {
        let mut current = super::project::checkpoint(context)
            .unwrap()
            .settings
            .registries;
        edit(&mut current);
        write(context, &registries(current)).unwrap();
        return;
    }
    super::storage::Store::open(root)
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            edit(&mut state.settings.registries);
            state.legacy_settings.registries = Some(state.settings.registries.clone());
            Ok(json!({}))
        })
        .unwrap();
}

/// Turn request history on or off without review, like
/// [`registries_fixture`].
#[cfg(test)]
pub(crate) fn history_fixture(context: &Context, enabled: bool) {
    let root = Path::new(&context.project_root);
    if root.join(SHARED).is_file() {
        let limit = super::project::checkpoint(context)
            .unwrap()
            .settings
            .history_limit;
        write(context, &history(enabled, limit)).unwrap();
        return;
    }
    super::storage::Store::open(root)
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            state.settings.history_enabled = enabled;
            state.legacy_settings.history_enabled = Some(enabled);
            Ok(json!({}))
        })
        .unwrap();
}

/// Set an agent's Ask fallback without review, like [`registries_fixture`].
#[cfg(test)]
pub(crate) fn fallback_fixture(context: &Context, agent: &str, fallback: Fallback) {
    let root = Path::new(&context.project_root);
    if root.join(SHARED).is_file() {
        write(
            context,
            &fallbacks(BTreeMap::from([(agent.to_owned(), fallback)])),
        )
        .unwrap();
        return;
    }
    super::storage::Store::open(root)
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            state
                .settings
                .ask_fallback
                .insert(agent.to_owned(), fallback);
            state
                .legacy_settings
                .ask_fallback
                .insert(agent.to_owned(), fallback);
            Ok(json!({}))
        })
        .unwrap();
}
