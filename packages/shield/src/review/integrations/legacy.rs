//! Retained Claude mod ownership from the installed 0.1.0 integration codec.
use super::{Error, MAX_CONFIG, Path, Plan, Registration, Value, files};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const BASE: &str = ".claude/skills/shield";
const SETTINGS: &str = ".claude/settings.local.json";
const KEY: &str = "shield@skills-dir";
const ASSETS: [&str; 5] = [
    ".claude/skills/shield/.claude-plugin/plugin.json",
    ".claude/skills/shield/hooks/hooks.json",
    ".claude/skills/shield/hooks/register.mjs",
    ".claude/skills/shield/hooks/config.mjs",
    ".claude/skills/shield/hooks/protocol.mjs",
];

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Installation {
    assets: BTreeMap<String, String>,
}

pub(super) fn asset_path(name: &str) -> bool {
    ASSETS.contains(&name)
}

/// These installed prereleases admitted omitted file lists. Current writers
/// always retain their complete list; its absence is not a current no-op plan.
pub(super) fn historical_plan(value: &Value) -> bool {
    ["previous", "desired"]
        .into_iter()
        .any(|field| historical_registration(&value[field]))
}

fn historical_registration(value: &Value) -> bool {
    value.as_object().is_some_and(|record| {
        record.contains_key("interceptor")
            && record.get("version").and_then(Value::as_str) == Some("0.1.0")
    })
}

pub(super) fn normalize_registration(value: &mut Value) {
    if value.get("agents").is_none() && historical_registration(value) {
        value["agents"] = serde_json::json!({});
    }
}

impl Installation {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.assets.len() != ASSETS.len()
            || self
                .assets
                .iter()
                .any(|(name, bytes)| !asset_path(name) || bytes.len() as u64 > MAX_CONFIG)
        {
            return Err(Error::Invalid(
                "invalid retained Claude interceptor ownership",
            ));
        }
        Ok(())
    }
}

/// Legacy asset publication requires the exact bytes captured by the old
/// installation, not a filename or a plugin marker. Modern file callers retain
/// their narrower allowlist.
pub(super) fn validate_plan(plan: &Plan) -> Result<(), Error> {
    let previous = plan.previous.as_ref().and_then(|r| r.interceptor.as_ref());
    let desired = plan.desired.as_ref().and_then(|r| r.interceptor.as_ref());
    for installation in [previous, desired].into_iter().flatten() {
        installation.validate()?;
    }
    for change in &plan.files {
        if !asset_path(&change.path) {
            continue;
        }
        let old = previous.and_then(|i| i.assets.get(&change.path));
        let new = desired.and_then(|i| i.assets.get(&change.path));
        if (old.is_none() && new.is_none())
            || change
                .before
                .as_ref()
                .is_some_and(|bytes| Some(bytes) != old)
            || change.after.as_ref() != new
        {
            return Err(Error::Invalid(
                "legacy asset change lacks exact retained ownership",
            ));
        }
    }
    Ok(())
}

/// Preserve old interception until a human-reviewed setup explicitly replaces
/// or removes it. The historical SDK consumer is not native-hook authority.
pub(super) fn prepare(
    root: &Path,
    previous: Option<&Registration>,
    desired: Option<&mut Registration>,
    replace: bool,
    changes: &mut Vec<files::Change>,
) -> Result<(), Error> {
    let old = previous.and_then(|r| r.interceptor.as_ref());
    let Some(old) = old else { return Ok(()) };
    old.validate()?;
    if !replace {
        if let Some(desired) = desired {
            desired.interceptor = Some(old.clone());
        }
        return Ok(());
    }
    // Disable first. Composed native settings edits retain one before image
    // and keep unrelated hooks, plugins and settings in the reviewed result.
    files::edit(root, changes, SETTINGS, |source| {
        let mut config: Value = crate::json::parse(source.unwrap_or("{}").as_bytes())
            .map_err(|_| Error::Invalid("invalid legacy Claude settings JSON"))?;
        if !config.is_object()
            || config
                .get("enabledPlugins")
                .is_some_and(|plugins| !plugins.is_object())
        {
            return Err(Error::Invalid("invalid legacy Claude settings shape"));
        }
        if let Some(plugins) = config
            .get_mut("enabledPlugins")
            .and_then(Value::as_object_mut)
        {
            if plugins.get(KEY).is_some_and(|value| !value.is_boolean()) {
                return Err(Error::Invalid("owned legacy plugin setting was changed"));
            }
            plugins.remove(KEY);
        }
        serde_json::to_string_pretty(&config)
            .map(|text| format!("{text}\n"))
            .map_err(|_| Error::Invalid("legacy settings encoding"))
    })?;
    for (name, owned) in &old.assets {
        let before = files::read_legacy(root, name)?;
        if before.as_ref().is_some_and(|bytes| bytes != owned) {
            return Err(Error::Invalid(
                "owned legacy plugin asset was manually changed",
            ));
        }
        changes.push(files::Change {
            path: name.clone(),
            before,
            after: None,
        });
    }
    if let Some(desired) = desired {
        desired.interceptor = None;
    }
    Ok(())
}

pub(super) fn diagnose(
    root: &Path,
    owned: &Installation,
    diagnostics: &mut Vec<&'static str>,
) -> Result<(), Error> {
    owned.validate()?;
    for (name, bytes) in &owned.assets {
        if files::read_legacy(root, name)?.as_ref() != Some(bytes) {
            diagnostics.push("owned legacy Claude interceptor asset missing or changed");
            break;
        }
    }
    let settings = files::read(root, SETTINGS)?;
    let settings: Value = crate::json::parse(settings.as_deref().unwrap_or("{}").as_bytes())
        .map_err(|_| Error::Invalid("invalid legacy Claude settings JSON"))?;
    if settings["enabledPlugins"][KEY] != true {
        diagnostics.push("owned legacy Claude interceptor is not enabled");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
