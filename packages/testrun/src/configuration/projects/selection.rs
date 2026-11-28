//! Explicit application selection; platform adapters remain operation-specific.
use super::*;
pub(crate) struct Selected {
    pub loaded: Loaded,
    pub name: String,
    pub project: Project,
    pub root: PathBuf,
}
#[cfg(test)]
pub(crate) fn select(root: &Path, name: &str) -> Result<Selected> {
    select_at(root, None, name)
}
/// Selects a named project from the configuration chosen for `root`.
pub(crate) fn select_at(root: &Path, config: Option<&Path>, name: &str) -> Result<Selected> {
    if !root.is_absolute() || !super::name(name) {
        return Err(Error::invalid(
            "Use an absolute root and a symbolic project name",
        ));
    }
    let loaded = super::super::load_at(root, config)?;
    let value = normalized(&loaded);
    let selected = value["projects"]
        .get(name)
        .ok_or_else(|| Error::new("PROJECT_NOT_FOUND", "Unknown project; use project_list"))?;
    // An overlay may specialize a valid shared project, never repair an invalid one.
    if loaded.document.version == 2
        && let Some(base) = loaded.base["projects"].get(name)
    {
        decode_project(base)
            .and_then(|p| super::references(&p, &loaded.base))
            .map_err(|error| argument(error, name))?;
    }
    let project = decode_project(selected).map_err(|error| argument(error, name))?;
    super::references(&project, &value).map_err(|error| argument(error, name))?;
    let directory = loaded
        .root
        .join(&project.root)
        .canonicalize()
        .map_err(|_| Error::new("CONFIG_PATH", "Application root does not exist"))?;
    if !directory.is_dir() {
        return Err(Error::new(
            "CONFIG_PATH",
            "Application root must be a directory",
        ));
    }
    Ok(Selected {
        loaded,
        name: name.into(),
        project,
        root: directory,
    })
}
impl Selected {
    pub(crate) fn profile(&self, name: &str) -> Result<&NamedProfile> {
        self.project.profiles.get(name).ok_or_else(|| {
            Error::new(
                "BUILD_PROFILE_NOT_FOUND",
                "Unknown profile in selected project",
            )
        })
    }
    pub(crate) fn show(&self) -> Value {
        let mut value = serde_json::to_value(&self.project).unwrap();
        super::super::redact(&mut value);
        json!({"root":self.loaded.root,"location":self.loaded.describe(),"project":self.name,"applicationRoot":self.root,"configuration":value,"sources":self.loaded.sources,"provenance":self.loaded.provenance})
    }
    pub(crate) fn native(&self, name: &str) -> Result<Value> {
        // Retain all v1 development settings, including exact argv, when selecting legacy.
        if self.loaded.document.version == 1 {
            return self.loaded.value["builds"]
                .get(name)
                .cloned()
                .ok_or_else(|| {
                    Error::new("BUILD_PROFILE_NOT_FOUND", "Unknown legacy build profile")
                });
        }
        let profile = self.profile(name)?;
        if !["ios", "android"].contains(&profile.platform.as_str())
            || !["device", "simulator", "emulator"].contains(&profile.target_kind.as_str())
        {
            return Err(Error::new("CAPABILITY_UNAVAILABLE","Configuration is valid; this operation requires an implemented mobile build adapter").details(json!({"platform":profile.platform,"targetKind":profile.target_kind})));
        }
        let mut build = profile.build.clone().ok_or_else(|| {
            Error::new("PROJECT_TASK_MISSING", "Selected profile declares no build")
        })?;
        let builder = build["builder"].as_str().unwrap_or_default();
        if ![
            "xcode",
            "gradle",
            "react-native",
            "expo",
            "flutter",
            "custom",
        ]
        .contains(&builder)
            || build.get("options").is_some()
            || build.get("resources").is_some()
        {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "The selected build adapter or its extension options are unavailable",
            ));
        }
        if let Some(artifact) = build.get_mut("artifact").and_then(Value::as_object_mut) {
            if artifact.contains_key("validator") {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Custom artifact validators are unavailable",
                ));
            }
            if artifact.get("kind").is_some_and(|kind| {
                !if profile.platform == "android" {
                    kind == "android-apk"
                } else {
                    kind == "ios-app" || profile.target_kind == "device" && kind == "ios-ipa"
                }
            }) {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Artifact kind is not supported by the selected mobile validator",
                ));
            }
            artifact.remove("kind");
        }
        build["platform"] = json!(profile.platform);
        build["target_kind"] = json!(profile.target_kind);
        if let Some(application) = &profile.application {
            let identity = self.project.applications[application]
                .get(&profile.platform)
                .ok_or_else(|| {
                    Error::new(
                        "CONFIG_REFERENCE",
                        "Selected application has no identity for this platform",
                    )
                })?;
            let app_id = identity["app_id"].as_str().ok_or_else(|| {
                Error::new(
                    "CONFIG_REFERENCE",
                    "Selected native application needs an app_id for this platform",
                )
            })?;
            if identity.get("adapter").is_some()
                || identity.get("options").is_some()
                || identity.get("version").is_some()
                || identity.get("build").is_some()
            {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Native application identity extensions are not implemented",
                ));
            }
            build["development"] = json!({"app_id":app_id,"variant":application});
        }
        super::super::schema::validate_profile(&build)?;
        let native: super::super::Profile = serde_json::from_value(build.clone())
            .map_err(|_| Error::new("CONFIG_INVALID", "Invalid mobile build profile"))?;
        native.validate()?;
        Ok(build)
    }
}
