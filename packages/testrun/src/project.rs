//! Project association, Expo readiness and explicitly selected development tasks.
use crate::{
    configuration,
    error::{Error, Result},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

mod expo;
pub(crate) mod files;
mod network;
pub(crate) mod schema;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Development {
    pub app_id: String,
    pub variant: Option<String>,
    pub expo: Option<Expo>,
    pub preflight: Option<Task>,
    pub prepare: Option<Task>,
    pub server: Option<Task>,
    #[serde(default)]
    pub checks: BTreeMap<String, Task>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expo {
    pub url: String,
    pub slug: String,
    pub connection_url: Option<String>,
    pub owner: Option<String>,
    pub connection_mode: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Task {
    pub command: Vec<String>,
    #[serde(default = "cwd")]
    pub cwd: String,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub effects: Vec<String>,
    pub expected_output: Option<String>,
}
fn cwd() -> String {
    ".".into()
}
fn timeout() -> u64 {
    600_000
}
impl Development {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.app_id.is_empty()
            || self.app_id.len() > 256
            || !self
                .app_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err(Error::invalid(
                "development.app_id must be an application identifier",
            ));
        }
        if let Some(expo) = &self.expo {
            expo::endpoint(&expo.url)?;
            if expo.slug.is_empty() {
                return Err(Error::invalid("development.expo.slug is required"));
            }
            if let Some(url) = &expo.connection_url {
                expo::connection(url)?;
            }
            if expo
                .connection_mode
                .as_deref()
                .is_some_and(|mode| !["lan", "tunnel", "forwarded"].contains(&mode))
                || (expo.connection_mode.as_deref() == Some("forwarded")
                    && expo.connection_url.is_none())
            {
                return Err(Error::invalid(
                    "Expo connection_mode is lan, tunnel or forwarded; forwarded requires an explicit connection_url",
                ));
            }
            if expo.owner.is_some() && self.server.is_some() {
                return Err(Error::invalid(
                    "An externally owned server cannot also declare a server command",
                ));
            }
        }
        for task in self
            .preflight
            .iter()
            .chain(self.prepare.iter())
            .chain(self.server.iter())
            .chain(self.checks.values())
        {
            configuration::path(&task.cwd)?;
            if task.command.is_empty()
                || task.command.len() > 128
                || task.command[0].is_empty()
                || !(1000..=3_600_000).contains(&task.timeout_ms)
            {
                return Err(Error::invalid(
                    "Development tasks require exact argv and a 1000..3600000 ms deadline",
                ));
            }
            for value in task.command.iter().chain(task.effects.iter()) {
                configuration::literal(value)?;
            }
            for effect in &task.effects {
                files::relative(effect)?;
            }
            if let Some(expected) = &task.expected_output {
                configuration::literal(expected)?;
            }
        }
        if self.checks.len() > 128
            || self.checks.keys().any(|k| {
                k.is_empty()
                    || k.len() > 64
                    || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(Error::invalid("Invalid development check name"));
        }
        Ok(())
    }
    pub(crate) fn task(&self, kind: &str, check: Option<&str>) -> Result<&Task> {
        let task = match kind {
            "preflight" => self.preflight.as_ref(),
            "prepare" => self.prepare.as_ref(),
            "server" => self.server.as_ref(),
            "check" => check.and_then(|name| self.checks.get(name)),
            _ => None,
        };
        task.ok_or_else(|| Error::new("PROJECT_TASK_MISSING", "Declare the selected development task in this profile; external services remain owned by their execution tool"))
    }
}

pub(crate) fn selected(
    root: &Path,
    config: Option<&Path>,
    name: &str,
) -> Result<(configuration::Loaded, configuration::Profile)> {
    if !root.is_absolute() {
        return Err(Error::invalid("project must be an absolute directory"));
    }
    let loaded = configuration::load_at(root, config)?;
    let profile = loaded
        .document
        .builds
        .get(name)
        .cloned()
        .ok_or_else(|| Error::new("BUILD_PROFILE_NOT_FOUND", "Unknown build profile"))?;
    if profile.development.is_none() {
        return Err(Error::new(
            "PROJECT_NOT_CONFIGURED",
            "Add development settings to the selected build profile",
        ));
    }
    Ok((loaded, profile))
}

pub(crate) async fn status(args: &Value) -> Result<Value> {
    let config = crate::builds::config_file(args)?;
    let config = config.as_deref();
    if let Some(root) = args["root"].as_str() {
        let selected = configuration::projects::select_at(
            Path::new(root),
            config,
            args["project"]
                .as_str()
                .ok_or_else(|| Error::invalid("project is required"))?,
        )?;
        let profile = args["profile"]
            .as_str()
            .ok_or_else(|| Error::invalid("profile is required"))?;
        let _ = selected.profile(profile)?;
        let build = selected.native(profile);
        let mut result = selected.show();
        result["profile"] = json!(profile);
        result["checks"] = json!({"configuration":{"state":"ready"},"runtime":{"state":"not_checked","reason":"Named-project readiness adapters are not implemented"}});
        result["capabilities"] = json!({"build":{"available":build.is_ok(),"error":build.err()},"tasks":{"available":false},"open":{"available":false}});
        return Ok(result);
    }
    let root = args["project"]
        .as_str()
        .ok_or_else(|| Error::invalid("project is required"))?;
    let name = args["profile"]
        .as_str()
        .ok_or_else(|| Error::invalid("profile is required"))?;
    let (loaded, profile) = selected(Path::new(root), config, name)?;
    let development = profile.development.as_ref().unwrap();
    let directory = loaded.root.join(&profile.cwd);
    let mut effective = serde_json::to_value(&profile).unwrap();
    configuration::redact(&mut effective);
    let package = std::fs::File::open(directory.join("package.json"))
        .ok()
        .and_then(|f| {
            use std::io::Read;
            serde_json::from_reader::<_, Value>(f.take(1_048_577)).ok()
        });
    let mut checks = json!({
        "toolchain":{"state":"unknown","declaredPackageManager":package.as_ref().and_then(|p|p["packageManager"].as_str()),"nextAction":"Run project_start with task preflight to check project toolchain prerequisites"},
        "preparation":{"state":"not_checked","nativeProjectExists":directory.join(if profile.platform == "ios" {"ios"} else {"android"}).is_dir(),"nextAction":"Run the declared preflight; select preparation explicitly when required"},
        "nativeCompatibility":{"state":"unknown","reason":"App identity and server response do not establish native-module compatibility"},
        "connection":{"state":"unknown","reason":"Host HTTP inspection cannot attribute a bundle load to the selected device"},
        "rendering":{"state":"not_checked","nextAction":"Observe the selected app and verify the required interaction"},
        "verification":{"state":"not_checked","checks":development.checks.keys().collect::<Vec<_>>(),"reason":"Project checks and preview captures do not establish native scrolling or accessibility"}
    });
    checks["server"] = match &development.expo {
        Some(expo) => {
            expo::inspect(
                expo.clone(),
                profile.platform.clone(),
                args["targetId"].is_string() && profile.target_kind == "device",
            )
            .await
        }
        None => json!({"state":"not_checked","reason":"No Expo server association"}),
    };
    if checks["server"]["manifestAppId"]
        .as_str()
        .is_some_and(|app| app != development.app_id)
    {
        checks["server"]["state"] = json!("blocked");
        checks["server"]["reason"] =
            json!("Manifest application ID differs from the selected profile");
    }
    if let Some(expo) = &development.expo
        && args["targetId"].is_string()
        && profile.target_kind == "device"
        && checks["server"]["connectionState"] != "candidate"
    {
        checks["connection"]["hostInterfaces"] = network::inspect(&expo.url);
    }
    checks["installedApp"] = if let Some(target) = args["targetId"].as_str() {
        crate::applications::validate(target)?;
        if (profile.platform == "ios")
            != (target.starts_with("device:") || target.starts_with("simulator:"))
        {
            return Err(Error::new(
                "PACKAGE_PLATFORM_MISMATCH",
                "Profile and selected target have different platforms",
            ));
        }
        match crate::applications::inventory(target).await {
            Ok(inventory) => installed(&inventory, &development.app_id),
            Err(error) => {
                json!({"state":"unknown","error":error,"nextAction":"Inspect the selected target's host and app-inventory prerequisites"})
            }
        }
    } else {
        json!({"state":"not_checked","nextAction":"Select targetId to inspect the expected installed app"})
    };
    Ok(
        json!({"project":loaded.root,"profile":name,"targetId":args["targetId"],"observedAtMs":now(),"appId":development.app_id,"variant":development.variant,"cwd":directory,"configuration":effective,"location":loaded.describe(),"sources":loaded.sources,"provenance":loaded.provenance,"checks":checks,"nextAction":"Resolve blocked checks before building; missing server attachment alone does not require a rebuild"}),
    )
}
fn installed(inventory: &Value, app: &str) -> Value {
    let Some(apps) = inventory["apps"].as_array() else {
        return json!({"state":"unknown","reason":"Inventory did not expose an app list"});
    };
    match apps.iter().find(|v| v["appId"] == app) {
        Some(value) => json!({"state":"ready","app":value}),
        None => {
            json!({"state":"blocked","appId":app,"reason":"Expected app is absent","nextAction":"Build and install the selected profile on this target"})
        }
    }
}
pub(crate) fn now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn diagnostic(mut error: Error) -> Error {
    if error
        .details
        .as_ref()
        .and_then(|d| d["privateOutput"].as_str())
        .is_some_and(|text| text.contains("ERR_PNPM_BAD_PM_VERSION"))
    {
        error.code = "PACKAGE_MANAGER_MISMATCH".into();
        error.message="A nested pnpm command refused the project package-manager version; inspect build_log and correct the child's executable resolution".into();
    }
    error
}

pub(crate) async fn handoff(args: &Value) -> Result<Value> {
    if args.get("root").is_some() {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Named-project opening adapters are not implemented; inspect project_show",
        ));
    }
    let project = args["project"]
        .as_str()
        .ok_or_else(|| Error::invalid("project is required"))?;
    let profile = args["profile"]
        .as_str()
        .ok_or_else(|| Error::invalid("profile is required"))?;
    let target = args["targetId"]
        .as_str()
        .ok_or_else(|| Error::invalid("targetId is required"))?;
    crate::applications::validate(target)?;
    let config = crate::builds::config_file(args)?;
    let (_, profile) = selected(Path::new(project), config.as_deref(), profile)?;
    if (profile.platform == "ios")
        != (target.starts_with("device:") || target.starts_with("simulator:"))
    {
        return Err(Error::new(
            "PACKAGE_PLATFORM_MISMATCH",
            "Profile and target have different platforms",
        ));
    }
    let development = profile.development.as_ref().unwrap();
    if target.starts_with("device:") {
        return Err(Error::new("CAPABILITY_UNAVAILABLE","Open the configured connection URL in the iPhone development client, then observe the device; generic physical iOS URL opening is unavailable").details(json!({"nextAction":"project_status","targetId":target})));
    }
    let server = expo::inspect(
        development.expo.clone().ok_or_else(|| {
            Error::new(
                "PROJECT_NOT_CONFIGURED",
                "Configure an Expo server association",
            )
        })?,
        profile.platform,
        profile.target_kind == "device",
    )
    .await;
    if server["manifestAppId"]
        .as_str()
        .is_some_and(|app| app != development.app_id)
    {
        return Err(Error::new(
            "EXPO_PROJECT_MISMATCH",
            "Manifest app ID differs from the selected development app",
        ));
    }
    if server["state"] != "ready" || !server["connectionUrl"].is_string() {
        return Err(Error::new(
            "PROJECT_NOT_READY",
            "No validated connection URL is available; inspect project_status",
        )
        .details(json!({"server":server})));
    }
    Ok(json!({"appId":development.app_id,"url":server["connectionUrl"]}))
}

pub(crate) async fn admit_server(development: &Development, platform: &str) -> Result<()> {
    if let Some(expo) = &development.expo {
        let state = expo::inspect(expo.clone(), platform.into(), false).await;
        if state["error"]["code"] != "EXPO_UNREACHABLE" {
            return Err(Error::new(
                "PROJECT_SERVER_EXISTS",
                "The configured endpoint already responds or cannot be safely classified; inspect project_status and use its existing execution owner",
            ));
        }
    }
    Ok(())
}
