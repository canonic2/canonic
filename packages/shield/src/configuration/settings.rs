//! Shield's settings, held as keys of the configuration files: native
//! enforcement, protected file access, per-agent Ask fallback, request history
//! and project registries. A key may appear in the shared or the local layer;
//! the local value wins, leaf by leaf, except `registries`, which the local
//! layer replaces whole so it can also remove a shared registry.
use crate::policy::Error;
use canonic_registry_kit::registrations::Registrations;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

/// The most request history entries a project may keep.
pub(crate) const MAX_HISTORY_LIMIT: usize = 10_000;
/// History entries kept when no layer sets `history.limit`.
pub(crate) const DEFAULT_HISTORY_LIMIT: usize = 5000;

/// Whether native hooks apply policy or pause it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Enforcement {
    #[default]
    Enabled,
    Paused,
}

/// Reading agent settings files, which may hold credentials.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Read {
    /// Ask where the host can prompt, otherwise block.
    #[default]
    Ask,
    Allow,
}

/// Changing a group of protected files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Change {
    #[default]
    Block,
    Allow,
}

/// What a host that cannot ask does with a request policy asks about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Fallback {
    #[default]
    Block,
    Allow,
}

/// Protected file access, each choice optional in a layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Protection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent_settings_read: Option<Read>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent_settings_change: Option<Change>,
}

/// Request history, each choice optional in a layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct History {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<usize>,
}

/// The settings one file holds, or the merge of both layers. Absent keys take
/// their defaults. Reading a whole configuration document into a layer
/// ignores its other keys; the configuration reader refuses unknown ones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Layer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) enforcement: Option<Enforcement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) protection: Option<Protection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) ask_fallback: BTreeMap<String, Fallback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) history: Option<History>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) registries: Option<Registrations>,
}

/// Every setting with its value in effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Values {
    pub(crate) enforcement: Enforcement,
    pub(crate) agent_settings_read: Read,
    pub(crate) agent_settings_change: Change,
    pub(crate) ask_fallback: BTreeMap<String, Fallback>,
    pub(crate) history_enabled: bool,
    pub(crate) history_limit: usize,
    pub(crate) registries: Registrations,
}

impl Default for Values {
    fn default() -> Self {
        Layer::default().values()
    }
}

impl Values {
    pub(crate) fn enabled(&self) -> bool {
        self.enforcement == Enforcement::Enabled
    }
    /// An agent's Ask fallback; absent means Block.
    pub(crate) fn fallback(&self, agent: &str) -> Fallback {
        self.ask_fallback.get(agent).copied().unwrap_or_default()
    }
}

impl Layer {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Refuse values no layer may hold.
    ///
    /// # Errors
    /// Refuses a history limit outside 1 to [`MAX_HISTORY_LIMIT`], Ask
    /// fallback agent IDs that are not short identifiers, and more registries
    /// than one scope holds.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self
            .history
            .and_then(|history| history.limit)
            .is_some_and(|limit| !(1..=MAX_HISTORY_LIMIT).contains(&limit))
        {
            return Err(Error::Contract("history.limit must be from 1 to 10000"));
        }
        if self.ask_fallback.len() > 64
            || self.ask_fallback.keys().any(|agent| {
                agent.is_empty()
                    || agent.len() > 64
                    || !agent
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err(Error::Contract(
                "ask_fallback keys must be agent IDs such as codex",
            ));
        }
        if self.registries.as_ref().is_some_and(|registries| {
            registries.sources.len() > canonic_registry_kit::registrations::MAX_REGISTRATIONS
        }) {
            return Err(Error::Limit);
        }
        Ok(())
    }

    /// This layer with `local` applied over it.
    #[must_use]
    pub(crate) fn under(&self, local: &Self) -> Self {
        let protection = match (self.protection, local.protection) {
            (Some(shared), Some(local)) => Some(Protection {
                agent_settings_read: local.agent_settings_read.or(shared.agent_settings_read),
                agent_settings_change: local.agent_settings_change.or(shared.agent_settings_change),
            }),
            (shared, local) => local.or(shared),
        };
        let history = match (self.history, local.history) {
            (Some(shared), Some(local)) => Some(History {
                enabled: local.enabled.or(shared.enabled),
                limit: local.limit.or(shared.limit),
            }),
            (shared, local) => local.or(shared),
        };
        let mut ask_fallback = self.ask_fallback.clone();
        ask_fallback.extend(local.ask_fallback.clone());
        Self {
            enforcement: local.enforcement.or(self.enforcement),
            protection,
            ask_fallback,
            history,
            registries: local.registries.clone().or_else(|| self.registries.clone()),
        }
    }

    /// The value of every setting, with defaults for those absent.
    #[must_use]
    pub(crate) fn values(&self) -> Values {
        let protection = self.protection.unwrap_or_default();
        let history = self.history.unwrap_or_default();
        Values {
            enforcement: self.enforcement.unwrap_or_default(),
            agent_settings_read: protection.agent_settings_read.unwrap_or_default(),
            agent_settings_change: protection.agent_settings_change.unwrap_or_default(),
            ask_fallback: self.ask_fallback.clone(),
            history_enabled: history.enabled.unwrap_or(true),
            history_limit: history.limit.unwrap_or(DEFAULT_HISTORY_LIMIT),
            registries: self.registries.clone().unwrap_or_default(),
        }
    }

    /// The keys a local layer must set so that, over `self`, the settings in
    /// effect become `target`. Ask fallbacks `target` names are written unless
    /// `self` names the same one, so a confirmed Block stays confirmed; those
    /// it does not name are left as they are.
    #[must_use]
    pub(crate) fn changes_to(&self, target: &Values) -> Self {
        let current = self.values();
        let mut changes = Self::default();
        if current.enforcement != target.enforcement {
            changes.enforcement = Some(target.enforcement);
        }
        let mut protection = Protection::default();
        if current.agent_settings_read != target.agent_settings_read {
            protection.agent_settings_read = Some(target.agent_settings_read);
        }
        if current.agent_settings_change != target.agent_settings_change {
            protection.agent_settings_change = Some(target.agent_settings_change);
        }
        if protection != Protection::default() {
            changes.protection = Some(protection);
        }
        for (agent, fallback) in &target.ask_fallback {
            if current.ask_fallback.get(agent) != Some(fallback) {
                changes.ask_fallback.insert(agent.clone(), *fallback);
            }
        }
        let mut history = History::default();
        if current.history_enabled != target.history_enabled {
            history.enabled = Some(target.history_enabled);
        }
        if current.history_limit != target.history_limit {
            history.limit = Some(target.history_limit);
        }
        if history != History::default() {
            changes.history = Some(history);
        }
        if current.registries != target.registries {
            changes.registries = Some(target.registries.clone());
        }
        changes
    }
}

/// The keys of a configuration document that hold settings, including the
/// local layer's settings as the layer merge carries them.
pub(crate) const KEYS: [&str; 6] = [
    "enforcement",
    "protection",
    "ask_fallback",
    "history",
    "registries",
    "local_settings",
];

/// The digest a merged configuration document contributes to the policy
/// revision: its bytes, or, when it holds settings, the document without
/// them. Settings are not policy, so changing one leaves the revision alone.
///
/// # Errors
/// Refuses text that is not a configuration document.
pub(crate) fn config_digest(merged: &str) -> Result<String, Error> {
    let mut document: Value = crate::policy::parse(merged)?;
    // Every key is removed; counting keeps the removal from short-circuiting.
    let removed = document.as_object_mut().is_some_and(|fields| {
        KEYS.iter()
            .filter(|key| fields.remove(**key).is_some())
            .count()
            > 0
    });
    Ok(if removed {
        crate::policy::fingerprint(document.to_string().as_bytes())
    } else {
        crate::policy::fingerprint(merged.as_bytes())
    })
}

/// Set `changes` into a configuration document, leaf by leaf, keeping its
/// other keys and the leaves `changes` does not name.
pub(crate) fn set_into(document: &mut Map<String, Value>, changes: &Layer) {
    let Value::Object(changes) = json!(changes) else {
        return;
    };
    for (key, value) in changes {
        match (document.get_mut(&key), value) {
            // `registries` is replaced whole, like the layer merge.
            (Some(Value::Object(existing)), Value::Object(leaves)) if key != "registries" => {
                existing.extend(leaves);
            }
            (_, value) => {
                document.insert(key, value);
            }
        }
    }
}
