//! Named project configuration and selection, independent of runtime adapter availability.
use super::*;
use serde::{Deserialize, Serialize};
mod legacy;
pub(crate) mod schema;
mod selection;
#[cfg(test)]
pub(crate) use selection::select;
pub(crate) use selection::select_at;
#[cfg(test)]
mod tests;

pub(crate) const CORE_KINDS: &[&str] = &[
    "web",
    "vite",
    "vue",
    "react",
    "react-native",
    "expo",
    "flutter",
    "xcode",
    "studio",
    "android-studio",
    "gradle",
    "native",
    "cli",
    "service",
    "library",
    "custom",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Project {
    pub root: String,
    pub kind: String,
    #[serde(default)]
    pub applications: BTreeMap<String, BTreeMap<String, Value>>,
    #[serde(default)]
    pub tasks: BTreeMap<String, Command>,
    #[serde(default)]
    pub checks: BTreeMap<String, Command>,
    #[serde(default)]
    pub services: BTreeMap<String, Value>,
    #[serde(default)]
    pub profiles: BTreeMap<String, NamedProfile>,
    #[serde(default)]
    pub options: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedProfile {
    pub platform: String,
    pub target_kind: String,
    pub driver: Option<Value>,
    pub application: Option<String>,
    pub build: Option<Value>,
    #[serde(default)]
    pub prepare: Vec<String>,
    #[serde(default)]
    pub preflight: Vec<String>,
    #[serde(default)]
    pub checks: BTreeMap<String, String>,
    #[serde(default)]
    pub services: Vec<String>,
    pub open: Option<Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Command {
    pub command: Vec<String>,
    #[serde(default = "super::dot")]
    pub cwd: String,
    #[serde(default = "super::timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub env: Env,
    #[serde(default)]
    pub inherit_env: Vec<String>,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    pub expected_output: Option<String>,
}
pub(crate) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().filter(|v| *v == b'/').count() <= 1
        && value.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
pub(crate) fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
pub(crate) fn argument(error: Error, project: &str) -> Error {
    let mut error = error;
    let mut details = error.details.take().unwrap_or(json!({}));
    let suffix = details["pointer"].as_str().unwrap_or("");
    details["pointer"] = json!(format!("/projects/{project}{suffix}"));
    error.details(details)
}
pub(crate) fn decode_project(value: &Value) -> Result<Project> {
    canonic_mcp_kit::schema::validate(&schema::project(false), value)
        .map_err(|e| super::provenance::argument(e, value))?;
    let project: Project = serde_json::from_value(value.clone())
        .map_err(|_| Error::new("CONFIG_INVALID", "Invalid project configuration"))?;
    path(&project.root)?;
    for (group, keys) in [
        ("tasks", project.tasks.keys().collect::<Vec<_>>()),
        ("checks", project.checks.keys().collect()),
        ("profiles", project.profiles.keys().collect()),
        ("services", project.services.keys().collect()),
        ("applications", project.applications.keys().collect()),
    ] {
        if let Some(key) = keys.into_iter().find(|key| !name(key)) {
            return Err(Error::new("CONFIG_INVALID", "Invalid project object name")
                .details(json!({"pointer":format!("/{group}/{key}")})));
        }
    }
    if !identifier(&project.kind) {
        return Err(Error::new("CONFIG_INVALID", "Invalid project kind"));
    }
    for (group, commands) in [("tasks", &project.tasks), ("checks", &project.checks)] {
        for (key, command) in commands {
            command
                .validate()
                .map_err(|e| e.details(json!({"pointer":format!("/{group}/{key}")})))?;
        }
    }
    for (key, profile) in &project.profiles {
        if let Some(build) = &profile.build {
            let builder = build["builder"].as_str().unwrap_or_default();
            let builders = [
                "xcode",
                "gradle",
                "react-native",
                "expo",
                "flutter",
                "custom",
            ];
            if builders.contains(&builder) && build.get(builder).is_none() {
                return Err(Error::new(
                    "CONFIG_INVALID",
                    "Build requires its selected builder block",
                )
                .details(json!({"pointer":format!("/profiles/{key}/build/{builder}")})));
            }
            if builders
                .iter()
                .any(|candidate| *candidate != builder && build.get(*candidate).is_some())
            {
                return Err(Error::new(
                    "CONFIG_INVALID",
                    "Only the selected builder block is allowed",
                )
                .details(json!({"pointer":format!("/profiles/{key}/build")})));
            }
            if let Some(artifact) = build.get("artifact")
                && artifact.get("path").is_some() == artifact.get("glob").is_some()
            {
                return Err(Error::new(
                    "CONFIG_INVALID",
                    "Artifact requires exactly one path or glob",
                )
                .details(json!({"pointer":format!("/profiles/{key}/build/artifact")})));
            }
        }
        if !identifier(&profile.platform) || !identifier(&profile.target_kind) {
            return Err(Error::new(
                "CONFIG_INVALID",
                "Invalid platform or target-kind identifier",
            )
            .details(json!({"pointer":format!("/profiles/{key}")})));
        }
        for task in profile.prepare.iter().chain(profile.preflight.iter()) {
            if !project.tasks.contains_key(task) {
                return Err(Error::new("CONFIG_REFERENCE", "Unknown project task")
                    .details(json!({"pointer":format!("/profiles/{key}"),"task":task})));
            }
        }
        for check in profile.checks.values() {
            if !project.checks.contains_key(check) {
                return Err(Error::new("CONFIG_REFERENCE", "Unknown project check")
                    .details(json!({"pointer":format!("/profiles/{key}/checks")})));
            }
        }
        if let Some(application) = &profile.application
            && !project.applications.contains_key(application)
        {
            return Err(
                Error::new("CONFIG_REFERENCE", "Unknown application variant")
                    .details(json!({"pointer":format!("/profiles/{key}/application")})),
            );
        }
        for service in &profile.services {
            if !service.starts_with("workspace/") && !project.services.contains_key(service) {
                return Err(Error::new("CONFIG_REFERENCE", "Unknown project service")
                    .details(json!({"pointer":format!("/profiles/{key}/services")})));
            }
        }
    }
    Ok(project)
}
fn references(project: &Project, document: &Value) -> Result<()> {
    let mut services = project.services.clone();
    for (name, service) in document["services"].as_object().into_iter().flatten() {
        services.insert(format!("workspace/{name}"), service.clone());
    }
    let resource = |name: &str| document["resources"].get(name).is_some();
    for command in project.tasks.values().chain(project.checks.values()) {
        if command.resources.iter().any(|name| !resource(name)) {
            return Err(Error::new("CONFIG_REFERENCE", "Unknown shared resource"));
        }
    }
    for profile in project.profiles.values() {
        if profile
            .services
            .iter()
            .any(|name| !services.contains_key(name))
        {
            return Err(Error::new("CONFIG_REFERENCE", "Unknown profile service"));
        }
        if profile
            .build
            .as_ref()
            .and_then(|build| build["resources"].as_array())
            .is_some_and(|names| {
                names
                    .iter()
                    .any(|name| !name.as_str().is_some_and(resource))
            })
        {
            return Err(Error::new("CONFIG_REFERENCE", "Unknown build resource"));
        }
        if let Some(service) = profile
            .open
            .as_ref()
            .and_then(|open| open["service"].as_str())
            && !services.contains_key(service)
        {
            return Err(Error::new("CONFIG_REFERENCE", "Unknown opening service"));
        }
    }
    let mut dependencies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, service) in &services {
        validate_service_execution(service, document)?;
        let mut edges = Vec::new();
        for dependency in service["depends_on"].as_array().into_iter().flatten() {
            let dependency = dependency.as_str().unwrap_or_default();
            let dependency =
                if name.starts_with("workspace/") && !dependency.starts_with("workspace/") {
                    format!("workspace/{dependency}")
                } else {
                    dependency.into()
                };
            if !services.contains_key(&dependency) {
                return Err(Error::new("CONFIG_REFERENCE", "Unknown service dependency"));
            }
            edges.push(dependency);
        }
        dependencies.insert(name.clone(), edges);
    }
    while !dependencies.is_empty() {
        let ready: Vec<_> = dependencies
            .iter()
            .filter(|(_, edges)| edges.is_empty())
            .map(|(name, _)| name.clone())
            .collect();
        if ready.is_empty() {
            return Err(Error::new("CONFIG_REFERENCE", "Service dependency cycle"));
        }
        for name in &ready {
            dependencies.remove(name);
        }
        for edges in dependencies.values_mut() {
            edges.retain(|name| !ready.contains(name));
        }
    }
    Ok(())
}
fn validate_service_execution(service: &Value, document: &Value) -> Result<()> {
    let resource = |name: &str| document["resources"].get(name).is_some();
    if let Some(run) = service["execution"].get("run") {
        let command: Command = serde_json::from_value(run.clone())
            .map_err(|_| Error::new("CONFIG_INVALID", "Invalid service command"))?;
        command.validate()?;
        if command.resources.iter().any(|name| !resource(name)) {
            return Err(Error::new("CONFIG_REFERENCE", "Unknown service resource"));
        }
    }
    if let Some(endpoint) = service["url"].as_str() {
        let endpoint = url::Url::parse(endpoint)
            .map_err(|_| Error::new("CONFIG_INVALID", "Invalid service endpoint"))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Error::new(
                "CONFIG_INVALID",
                "Service endpoints require HTTP(S) without credentials, queries or fragments",
            ));
        }
    }
    let kind = service["execution"]["kind"].as_str().unwrap_or_default();
    let execution = &service["execution"];
    let incompatible = match kind {
        "command" => ["workspace", "service", "reference"]
            .iter()
            .any(|key| execution.get(key).is_some()),
        "external" => ["run", "workspace", "service"]
            .iter()
            .any(|key| execution.get(key).is_some()),
        "box" => ["run", "reference"]
            .iter()
            .any(|key| execution.get(key).is_some()),
        _ => true,
    };
    if incompatible {
        return Err(Error::new(
            "CONFIG_INVALID",
            "Service declares incompatible execution owners",
        ));
    }
    Ok(())
}

impl Command {
    fn validate(&self) -> Result<()> {
        path(&self.cwd)?;
        if self.command.first().is_none_or(String::is_empty) {
            return Err(Error::invalid("Command needs an executable"));
        }
        for argument in &self.command {
            literal(argument)?;
        }
        for (key, value) in &self.env {
            if !variable(key) {
                return Err(Error::invalid("Invalid task environment key"));
            }
            literal(value)?;
        }
        let unique: std::collections::BTreeSet<_> = self.inherit_env.iter().collect();
        if unique.len() != self.inherit_env.len()
            || self.inherit_env.iter().any(|key| !variable(key))
        {
            return Err(Error::invalid("Invalid or duplicate task inherited key"));
        }
        for effect in &self.effects {
            if effect.is_empty()
                || Path::new(effect)
                    .components()
                    .any(|p| !matches!(p, std::path::Component::Normal(_)))
            {
                return Err(Error::invalid(
                    "File effects must be project-relative paths",
                ));
            }
        }
        Ok(())
    }
}
pub(crate) fn normalized(loaded: &Loaded) -> Value {
    if loaded.document.version == 2 {
        loaded.value.clone()
    } else {
        legacy::normalize(&loaded.value)
    }
}
pub(crate) fn inspect(loaded: &Loaded, selected: Option<&str>) -> Result<Value> {
    let value = normalized(loaded);
    let mut errors = Vec::new();
    let shared = decode_project(&json!({"root":".","kind":"custom"})).unwrap();
    if let Err(error) = references(&shared, &value) {
        errors.push(error);
    }
    for (name, project) in value["projects"].as_object().into_iter().flatten() {
        if selected.is_none_or(|selected| selected == name) {
            let shared = if loaded.document.version == 2 {
                loaded.base["projects"].get(name)
            } else {
                None
            };
            if let Err(error) = shared
                .map_or(Ok(()), |value| {
                    decode_project(value).and_then(|p| references(&p, &loaded.base))
                })
                .and_then(|()| decode_project(project).and_then(|p| references(&p, &value)))
            {
                errors.push(argument(error, name));
            }
        }
    }
    let mut configuration = value;
    super::redact(&mut configuration);
    Ok(
        json!({"valid":errors.is_empty(),"root":loaded.root,"location":loaded.describe(),"sourceVersion":loaded.document.version,"configuration":configuration,"errors":errors,"sources":loaded.sources,"provenance":loaded.provenance,"coreKinds":CORE_KINDS}),
    )
}
pub(crate) fn list(loaded: &Loaded, args: &Value) -> Result<Value> {
    let legacy;
    let value = if loaded.document.version == 2 {
        &loaded.value
    } else {
        legacy = legacy::normalize(&loaded.value);
        &legacy
    };
    let projects: std::collections::BTreeMap<_, _> = value["projects"]
        .as_object()
        .into_iter()
        .flatten()
        .collect();
    let page = crate::pagination::collect(
        "project_list",
        &(&loaded.root, &loaded.location.base),
        args,
        projects
            .iter()
            .map(|(name, project)| (name.as_bytes().to_vec(), (*name, *project))),
        |(name, project)| {
            let shared = if loaded.document.version == 2 {
                loaded.base["projects"].get(name)
            } else {
                None
            };
            let decoded = shared
                .map_or(Ok(()), |project| {
                    decode_project(project).and_then(|project| references(&project, &loaded.base))
                })
                .and_then(|()| decode_project(project))
                .and_then(|project| {
                    references(&project, value)?;
                    Ok(project)
                });
            Ok(match decoded {
                Ok(project) => {
                    json!({"project":name,"kind":project.kind,"core":CORE_KINDS.contains(&project.kind.as_str()),"root":loaded.root.join(&project.root),"profiles":project.profiles.keys().collect::<Vec<_>>(),"valid":true})
                }
                Err(error) => json!({"project":name,"valid":false,"error":argument(error,name)}),
            })
        },
        "project_show",
    )?;
    Ok(
        json!({"root":loaded.root,"location":loaded.describe(),"sourceVersion":loaded.document.version,"projects":page.items,"total":projects.len(),"nextCursor":page.next_cursor}),
    )
}
