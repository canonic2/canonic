//! Test-app build operations, retained outcomes and composition with device workflows.
mod adapters;
mod artifacts;
mod environment;
mod provenance;
mod runtime;
pub(crate) mod surface;
pub(crate) use environment::capture_cli;
pub(crate) use runtime::Owners;
pub(crate) use runtime::Ticket;
#[cfg(test)]
mod tests;

use crate::{
    configuration::{Env, Profile},
    error::{Error, Result},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prepare: bool,
    #[serde(default, rename = "submissionId")]
    pub submission_id: Option<String>,
    /// CLI-captured transport values are frozen execution inputs, not retry identity.
    #[serde(default, rename = "clientCapture")]
    pub client_capture: bool,
    pub project: PathBuf,
    /// Explicit absolute base file; omitted keeps the default location and request signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<PathBuf>,
    pub profile: Option<String>,
    pub inline: Option<Value>,
    #[serde(default)]
    pub env: Env,
    #[serde(default)]
    pub bindings: Env,
    #[serde(default)]
    pub baseline: Env,
    #[serde(default)]
    pub overrides: Overrides,
    #[serde(default)]
    pub install: bool,
    #[serde(default)]
    pub launch: bool,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub boot: bool,
    #[serde(rename = "targetId")]
    pub target: Option<String>,
}
impl Request {
    fn validate_envelope(&self) -> Result<()> {
        let task = self.task.as_deref().unwrap_or("build");
        if !["build", "preflight", "prepare", "server", "check"].contains(&task)
            || (task == "check") != self.check.is_some()
            || (task != "build"
                && (self.install || self.launch || self.replace || self.boot || self.prepare))
        {
            return Err(Error::invalid(
                "Select a project task; check requires its name, and only builds accept prepare/install/launch/replace/boot",
            ));
        }
        if let Some(config) = &self.config {
            if self.inline.is_some() {
                return Err(Error::invalid(
                    "Inline profiles cannot combine with a config file",
                ));
            }
            if !config.is_absolute() {
                return Err(Error::invalid("config must be an absolute file path"));
            }
        }
        for map in [
            &self.env,
            &self.bindings,
            &self.baseline,
            &self.overrides.launch_env,
        ] {
            if map.len() > 128 {
                return Err(Error::invalid(
                    "Build environment maps permit at most 128 entries",
                ));
            }
            for (key, value) in map {
                if !crate::configuration::variable(key) {
                    return Err(Error::invalid("Invalid build environment key"));
                }
                crate::configuration::literal(value)?;
            }
        }
        Ok(())
    }
    fn operation_id(&self, transport: &str) -> Result<String> {
        use sha2::{Digest, Sha256};
        let key = self.submission_id.as_deref().unwrap_or(transport);
        if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err(Error::invalid(
                "submissionId must be 1..128 bytes without control characters",
            ));
        }
        Ok(format!(
            "build-{}",
            &format!("{:x}", Sha256::digest(key.as_bytes()))[..32]
        ))
    }
    fn signature(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut request = self.clone();
        if request.client_capture {
            request.baseline.clear();
            request.bindings.clear();
        }
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&request).unwrap())
        )
    }
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Overrides {
    pub configuration: Option<String>,
    pub variant: Option<String>,
    pub flavor: Option<String>,
    pub entry_point: Option<String>,
    pub mode: Option<String>,
    pub timeout_ms: Option<u64>,
    pub launch_args: Option<Vec<String>>,
    pub launch_env: Env,
}
#[derive(Clone)]
pub(crate) struct Prepared {
    pub task: String,
    pub check: Option<String>,
    pub prepare: bool,
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub profile: Profile,
    pub env: Env,
    pub launch_env: Env,
    pub source: Value,
}
#[expect(
    clippy::too_many_lines,
    reason = "One preparation transaction resolves source layers, typed overrides, target compatibility and private environment inputs before admission"
)]
pub(crate) fn prepare(request: &Request) -> Result<Prepared> {
    request.validate_envelope()?;
    if let Some(root) = &request.root {
        if request.inline.is_some() {
            return Err(Error::invalid(
                "Named projects do not accept inline profiles",
            ));
        }
        let name = request
            .project
            .to_str()
            .ok_or_else(|| Error::invalid("Invalid project name"))?;
        let selected =
            crate::configuration::projects::select_at(root, request.config.as_deref(), name)?;
        if selected.loaded.document.version == 2
            && (request.prepare || request.task.as_deref().is_some_and(|task| task != "build"))
        {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Named task and service execution is not available in this adapter; inspect project_show",
            ));
        }
        let profile = request
            .profile
            .as_deref()
            .ok_or_else(|| Error::invalid("Select a project-local profile"))?;
        let mut legacy = request.clone();
        legacy.root = None;
        legacy.config = None;
        legacy.project = selected.root.clone();
        legacy.inline = Some(selected.native(profile)?);
        legacy.profile = None;
        let mut prepared = prepare(&legacy)?;
        prepared.source["configurationRoot"] = json!(selected.loaded.root);
        prepared.source["project"] = json!(name);
        prepared.source["profile"] = json!(profile);
        prepared.source["location"] = selected.loaded.describe();
        prepared.source["sources"] = selected.loaded.sources;
        prepared.source["provenance"] = json!(selected.loaded.provenance);
        prepared.source["sourceVersion"] = json!(selected.loaded.document.version);
        prepared.source.as_object_mut().unwrap().remove("inline");
        provenance::complete(request, &prepared.profile, &mut prepared.source);
        return Ok(prepared);
    }
    if !request.project.is_absolute() {
        return Err(Error::invalid(
            "Build project must be an absolute host path",
        ));
    }
    let (root, mut profile, mut source) = match (&request.profile, &request.inline) {
        (Some(name), None) => {
            let loaded =
                crate::configuration::load_at(&request.project, request.config.as_deref())?;
            let profile =
                loaded.document.builds.get(name).cloned().ok_or_else(|| {
                    Error::new("BUILD_PROFILE_NOT_FOUND", "Unknown build profile")
                })?;
            (
                loaded.root.clone(),
                profile,
                json!({"profile":name,"location":loaded.describe(),"sources":loaded.sources,"provenance":loaded.provenance}),
            )
        }
        (None, Some(value)) => {
            crate::configuration::schema::validate_profile(value)?;
            let profile: Profile = serde_json::from_value(value.clone())
                .map_err(|_| Error::invalid("Invalid inline profile"))?;
            profile.validate()?;
            (
                request
                    .project
                    .canonicalize()
                    .map_err(|_| Error::invalid("Project directory is unavailable"))?,
                profile,
                json!({"inline":true}),
            )
        }
        _ => return Err(Error::invalid("Select exactly one of profile or inline")),
    };
    let overrides = &request.overrides;
    if let Some(value) = &overrides.configuration {
        if let Some(x) = &mut profile.xcode {
            x.configuration = value.clone();
        } else if profile.platform == "ios"
            && let Some(r) = &mut profile.react_native
        {
            r.configuration = Some(value.clone());
        } else {
            return Err(Error::invalid(
                "configuration applies to Xcode or iOS React Native",
            ));
        }
    }
    if let Some(value) = &overrides.variant {
        if profile.platform == "android"
            && let Some(r) = &mut profile.react_native
        {
            r.variant = Some(value.clone());
        } else {
            return Err(Error::invalid("variant applies to Android React Native"));
        }
    }
    if overrides.flavor.is_some() || overrides.entry_point.is_some() || overrides.mode.is_some() {
        let f = profile
            .flutter
            .as_mut()
            .ok_or_else(|| Error::invalid("flavor/entry_point/mode overrides apply to Flutter"))?;
        if let Some(v) = &overrides.flavor {
            f.flavor = Some(v.clone());
        }
        if let Some(v) = &overrides.entry_point {
            f.entry_point = Some(v.clone());
        }
        if let Some(v) = &overrides.mode {
            f.mode = v.clone();
        }
    }
    if let Some(value) = overrides.timeout_ms {
        profile.timeout_ms = value;
    }
    if let Some(value) = &overrides.launch_args {
        profile.launch.args = value.clone();
    }
    profile.launch.env.extend(overrides.launch_env.clone());
    // Validate the same schema after typed invocation changes, omitting absent optional fields.
    let mut effective = serde_json::to_value(&profile).unwrap();
    fn omit_nulls(value: &mut Value) {
        if let Some(map) = value.as_object_mut() {
            map.retain(|_, v| !v.is_null());
            for value in map.values_mut() {
                omit_nulls(value);
            }
        }
    }
    omit_nulls(&mut effective);
    crate::configuration::schema::validate_profile(&effective)?;
    profile.validate()?;
    if (request.launch || request.replace) && !request.install {
        return Err(Error::invalid("launch and replace require install"));
    }
    if request.install {
        let target = request
            .target
            .as_deref()
            .ok_or_else(|| Error::invalid("Installation requires an explicit targetId"))?;
        crate::applications::validate(target)?;
        let matches = if profile.platform == "ios" {
            target.starts_with(if profile.target_kind == "simulator" {
                "simulator:"
            } else {
                "device:"
            })
        } else {
            let virtual_target =
                target.starts_with("android:emulator-") || target.starts_with("android:avd:");
            target.starts_with("android:") && (profile.target_kind == "emulator") == virtual_target
        };
        if !matches {
            return Err(Error::new(
                "PACKAGE_PLATFORM_MISMATCH",
                "Build profile does not match selected target",
            ));
        }
    } else if request.target.is_some() {
        return Err(Error::invalid("targetId requires install"));
    }
    if request.launch {
        profile.launch_supported()?;
    }
    let task = request.task.as_deref().unwrap_or("build");
    let cwd = if task != "build" || request.prepare {
        root.join(&profile.cwd)
    } else {
        profile.directory(&root)?
    };
    if task != "build" || request.prepare {
        let development = profile.development.as_ref().ok_or_else(|| {
            Error::new(
                "PROJECT_NOT_CONFIGURED",
                "Add development settings to this profile",
            )
        })?;
        if task != "build" {
            development.task(task, request.check.as_deref())?;
        }
        if request.prepare {
            development.task("prepare", None)?;
        }
    }
    let env = environment::resolve(
        &request.baseline,
        &profile.inherit_env,
        &request.bindings,
        &profile.env,
        &request.env,
    )?;
    let launch_env = if request.launch {
        environment::resolve(
            &Env::new(),
            &profile.launch.inherit_env,
            &request.bindings,
            &profile.launch.env,
            &Env::new(),
        )?
    } else {
        Env::new()
    };
    provenance::complete(request, &profile, &mut source);
    Ok(Prepared {
        task: task.into(),
        check: request.check.clone(),
        prepare: request.prepare,
        root,
        cwd,
        profile,
        env,
        launch_env,
        source,
    })
}

pub(crate) fn handles(name: &str) -> bool {
    matches!(
        name,
        "build_start" | "build_status" | "build_cancel" | "build_log" | "project_start"
    )
}
pub(crate) fn config_handles(name: &str) -> bool {
    name.starts_with("config_")
        || matches!(name, "project_list" | "project_show")
        || crate::configuration::onboarding::api::handles(name)
}
/// The optional absolute `config` argument of a configuration-consuming request.
pub(crate) fn config_file(args: &Value) -> Result<Option<PathBuf>> {
    match args.get("config") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(path)) if Path::new(path).is_absolute() => Ok(Some(path.into())),
        Some(_) => Err(Error::invalid("config must be an absolute file path")),
    }
}
pub(crate) fn config(name: &str, args: &Value) -> Result<Value> {
    if crate::configuration::onboarding::api::handles(name) {
        return crate::configuration::onboarding::api::call(name, args);
    }
    let root = args
        .get("root")
        .unwrap_or(&args["project"])
        .as_str()
        .map(Path::new)
        .ok_or_else(|| Error::invalid("project must name an absolute directory"))?;
    if !root.is_absolute() {
        return Err(Error::invalid("project must be absolute"));
    }
    let config = config_file(args)?;
    let config = config.as_deref();
    match name {
        "project_list" => crate::configuration::projects::list(
            &crate::configuration::load_at(root, config)?,
            args,
        ),
        "project_show" => Ok(crate::configuration::projects::select_at(
            root,
            config,
            args["project"]
                .as_str()
                .ok_or_else(|| Error::invalid("project is required"))?,
        )?
        .show()),
        "config_init" => crate::configuration::init(root, config),
        "config_check" | "config_show" => {
            let result = crate::configuration::inspect(root, config)?;
            if name == "config_check" && result["valid"] == false {
                return Err(Error::new(
                    "CONFIG_INVALID",
                    "Configuration contains invalid projects; inspect config_show",
                )
                .details(json!({"errors":result["errors"]})));
            }
            Ok(result)
        }
        "config_schema" => match args["version"].as_u64().unwrap_or(2) {
            1 => Ok(crate::configuration::schema::document(
                args["local"].as_bool().unwrap_or(false),
            )),
            2 => Ok(crate::configuration::projects::schema::document(
                args["local"].as_bool().unwrap_or(false),
            )),
            _ => Err(Error::invalid(
                "Supported configuration versions are 1 and 2",
            )),
        },
        _ => Err(Error::invalid("Unknown config operation")),
    }
}
