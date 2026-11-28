//! Optional project build configuration, strict layering and source provenance.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

mod files;
pub(crate) mod onboarding;
pub(crate) mod projects;
pub(crate) mod provenance;
pub(crate) mod schema;
#[cfg(test)]
pub(crate) use files::load;
pub(crate) use files::{Location, init, inspect, load_at};
#[cfg(test)]
mod tests;

pub(crate) type Env = BTreeMap<String, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    pub version: u32,
    #[serde(default)]
    pub builds: BTreeMap<String, Profile>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Launch {
    pub args: Vec<String>,
    pub env: Env,
    pub inherit_env: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub path: Option<String>,
    pub glob: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub builder: String,
    pub platform: String,
    pub target_kind: String,
    #[serde(default = "dot")]
    pub cwd: String,
    #[serde(default)]
    pub env: Env,
    #[serde(default)]
    pub inherit_env: Vec<String>,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub launch: Launch,
    pub xcode: Option<Xcode>,
    pub gradle: Option<Gradle>,
    #[serde(rename = "react-native")]
    pub react_native: Option<ReactNative>,
    pub expo: Option<Expo>,
    pub flutter: Option<Flutter>,
    pub custom: Option<Custom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub development: Option<crate::project::Development>,
    /// Claims about an explicitly selected build, never automatic compiler flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<DiagnosticBuild>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiagnosticBuild {
    pub symbols: String,
    pub profileable: bool,
    pub sanitizers: Vec<String>,
    pub coverage: bool,
    pub probes: Vec<String>,
    pub timing: String,
}
fn dot() -> String {
    ".".into()
}
fn timeout() -> u64 {
    600_000
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Xcode {
    pub workspace: Option<String>,
    pub project: Option<String>,
    pub scheme: String,
    pub configuration: String,
    pub sdk: Option<String>,
    #[serde(default)]
    pub build_settings: Env,
    pub development_team: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Gradle {
    pub tasks: Vec<String>,
    #[serde(default)]
    pub properties: Env,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReactNative {
    pub configuration: Option<String>,
    pub variant: Option<String>,
    pub scheme: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expo {
    pub profile: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Flutter {
    pub mode: String,
    pub flavor: Option<String>,
    pub entry_point: Option<String>,
    #[serde(default)]
    pub dart_defines: Env,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Custom {
    pub command: Vec<String>,
}

pub(crate) struct Loaded {
    pub root: PathBuf,
    pub location: Location,
    pub document: Document,
    pub sources: Value,
    pub provenance: BTreeMap<String, String>,
    pub value: Value,
    pub base: Value,
}

pub(crate) fn decode(value: Value) -> Result<Document> {
    schema::validate_document(&value, false)?;
    let document: Document = serde_json::from_value(value)
        .map_err(|_| Error::new("CONFIG_INVALID", "Invalid build configuration"))?;
    for (name, profile) in &document.builds {
        profile.validate().map_err(|mut error| {
            let mut details = error.details.take().unwrap_or(json!({}));
            let suffix = details["pointer"].as_str().unwrap_or("");
            details["pointer"] = json!(format!("/builds/{name}{suffix}"));
            error.details(details)
        })?;
    }
    Ok(document)
}

pub(crate) fn variable(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || i > 0 && b.is_ascii_digit())
}
pub(crate) fn literal(value: &str) -> Result<()> {
    if value.len() > 16_384 || value.contains('\0') {
        return Err(Error::invalid(
            "Build value exceeds its limit or contains NUL",
        ));
    }
    Ok(())
}
pub(crate) fn path(value: &str) -> Result<()> {
    literal(value)?;
    if value.is_empty() || value.contains("${") || value.starts_with('~') {
        return Err(Error::invalid(
            "Use a nonempty literal path without interpolation",
        ));
    }
    Ok(())
}
fn named(value: &str) -> Result<()> {
    literal(value)?;
    if value.is_empty() || value.starts_with('-') || value.contains("${") {
        return Err(Error::invalid(
            "Use an explicit nonempty identifier without flags or interpolation",
        ));
    }
    Ok(())
}
impl Profile {
    #[expect(
        clippy::too_many_lines,
        reason = "Pure semantic validation keeps the complete profile and its builder-specific constraints in one pre-dispatch check"
    )]
    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(development) = &self.development {
            development.validate()?;
        }
        path(&self.cwd)?;
        for values in [&self.env, &self.launch.env] {
            for (key, value) in values {
                if !variable(key) {
                    return Err(Error::invalid("Invalid environment variable name"));
                }
                literal(value)?;
            }
        }
        for names in [&self.inherit_env, &self.launch.inherit_env] {
            let unique: std::collections::BTreeSet<_> = names.iter().collect();
            if unique.len() != names.len()
                || names.len() > 128
                || names.iter().any(|v| !variable(v))
            {
                return Err(Error::invalid("Invalid or duplicate inherit_env name"));
            }
        }
        if self.launch.args.len() > 64 {
            return Err(Error::invalid("At most 64 launch arguments"));
        }
        for argument in &self.launch.args {
            literal(argument)?;
            if argument.len() > 4096 {
                return Err(Error::invalid("Launch arguments exceed 4096 bytes"));
            }
        }
        if let Some(artifact) = &self.artifact {
            match (&artifact.path, &artifact.glob) {
                (Some(p), None) | (None, Some(p)) => path(p)?,
                _ => {
                    return Err(Error::invalid(
                        "artifact requires exactly one of path or glob",
                    ));
                }
            }
        }
        if let Some(x) = &self.xcode {
            match (&x.workspace, &x.project) {
                (Some(p), None) | (None, Some(p)) => path(p)?,
                _ => {
                    return Err(Error::invalid(
                        "xcode requires exactly one workspace or project",
                    ));
                }
            }
            named(&x.scheme)?;
            named(&x.configuration)?;
            if let Some(sdk) = &x.sdk
                && sdk
                    != if self.target_kind == "simulator" {
                        "iphonesimulator"
                    } else {
                        "iphoneos"
                    }
            {
                return Err(Error::invalid("sdk must match target_kind"));
            }
            for key in x.build_settings.keys() {
                if !variable(key)
                    || [
                        "SDKROOT",
                        "CONFIGURATION_BUILD_DIR",
                        "SYMROOT",
                        "OBJROOT",
                        "DSTROOT",
                    ]
                    .contains(&key.as_str())
                {
                    return Err(Error::invalid(
                        "Invalid or output-overriding Xcode build setting",
                    ));
                }
            }
            for value in x.build_settings.values() {
                literal(value)?;
            }
            if let Some(team) = &x.development_team
                && (team.is_empty() || !team.bytes().all(|b| b.is_ascii_alphanumeric()))
            {
                return Err(Error::invalid("Invalid development_team"));
            }
        }
        if let Some(g) = &self.gradle {
            for task in &g.tasks {
                named(task)?;
                if !task
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
                {
                    return Err(Error::invalid("Invalid Gradle task name"));
                }
            }
            for (key, value) in &g.properties {
                named(key)?;
                literal(value)?;
                if key.contains('=') {
                    return Err(Error::invalid("Gradle property names cannot contain ="));
                }
            }
        }
        if let Some(r) = &self.react_native {
            if self.platform == "ios" {
                named(r.configuration.as_deref().ok_or_else(|| {
                    Error::invalid("React Native iOS requires configuration and scheme")
                })?)?;
                named(
                    r.scheme
                        .as_deref()
                        .ok_or_else(|| Error::invalid("React Native iOS requires scheme"))?,
                )?;
                if r.variant.is_some() {
                    return Err(Error::invalid("variant applies to Android"));
                }
            } else {
                let variant = r
                    .variant
                    .as_deref()
                    .ok_or_else(|| Error::invalid("React Native Android requires variant"))?;
                named(variant)?;
                if !variant.bytes().all(|b| b.is_ascii_alphanumeric())
                    || r.configuration.is_some()
                    || r.scheme.is_some()
                {
                    return Err(Error::invalid(
                        "Android variant must be alphanumeric; configuration/scheme apply to iOS",
                    ));
                }
            }
        }
        if let Some(e) = &self.expo {
            named(&e.profile)?;
            if self.artifact.is_some() {
                return Err(Error::invalid(
                    "Expo output is owned by its local build; omit artifact",
                ));
            }
        }
        if let Some(f) = &self.flutter {
            if self.platform == "ios" && self.target_kind == "simulator" && f.mode != "debug" {
                return Err(Error::invalid(
                    "Flutter iOS simulator builds require debug mode",
                ));
            }
            if let Some(flavor) = &f.flavor {
                named(flavor)?;
            }
            if let Some(entry) = &f.entry_point {
                path(entry)?;
            }
        }
        if let Some(f) = &self.flutter {
            for (key, value) in &f.dart_defines {
                named(key)?;
                literal(value)?;
                if key.contains('=') {
                    return Err(Error::invalid("Dart define names cannot contain ="));
                }
            }
        }
        if let Some(c) = &self.custom {
            for arg in &c.command {
                literal(arg)?;
            }
            if c.command[0].is_empty() {
                return Err(Error::invalid("custom.command needs an executable"));
            }
        }
        Ok(())
    }
    pub(crate) fn directory(&self, root: &Path) -> Result<PathBuf> {
        let directory = root
            .join(&self.cwd)
            .canonicalize()
            .map_err(|_| Error::new("CONFIG_PATH", "Build working directory does not exist"))?;
        if !directory.is_dir() {
            return Err(Error::new("CONFIG_PATH", "Build cwd must be a directory"));
        }
        Ok(directory)
    }
    pub(crate) fn launch_supported(&self) -> Result<()> {
        if self.platform == "android"
            && (!self.launch.args.is_empty()
                || !self.launch.env.is_empty()
                || !self.launch.inherit_env.is_empty())
        {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Android does not support generic process launch args/env; configure build-time values instead",
            ));
        }
        Ok(())
    }
}

pub(crate) fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if ["env", "build_settings", "properties", "dart_defines"].contains(&key.as_str()) {
                    if let Some(values) = value.as_object_mut() {
                        for v in values.values_mut() {
                            *v = json!("<redacted>");
                        }
                    }
                } else if ["command", "args", "options", "context"].contains(&key.as_str()) {
                    *value = json!("<redacted>");
                } else {
                    redact(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact(item);
            }
        }
        _ => {}
    }
}
