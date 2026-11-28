//! Explicit client environment capture and per-operation environment construction.
use super::*;
pub(crate) const BASELINE: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMPDIR",
    "TMP",
    "TEMP",
    "DEVELOPER_DIR",
    "SDKROOT",
    "JAVA_HOME",
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "FLUTTER_ROOT",
    "PUB_CACHE",
];
pub(crate) fn capture(request: &mut Request) -> Result<()> {
    request.baseline = BASELINE
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|v| ((*key).into(), v)))
        .collect();
    let profile = if let Some(root) = &request.root {
        let selected = crate::configuration::projects::select_at(
            root,
            request.config.as_deref(),
            request
                .project
                .to_str()
                .ok_or_else(|| Error::invalid("Invalid project name"))?,
        )?;
        serde_json::from_value(
            selected.native(
                request
                    .profile
                    .as_deref()
                    .ok_or_else(|| Error::invalid("Select a profile"))?,
            )?,
        )
        .map_err(|_| Error::invalid("Invalid native profile"))?
    } else {
        match (&request.profile, &request.inline) {
            (Some(name), None) => {
                match crate::configuration::load_at(&request.project, request.config.as_deref())
                    .ok()
                    .and_then(|loaded| loaded.document.builds.get(name).cloned())
                {
                    Some(profile) => profile,
                    None => return Ok(()),
                }
            }
            (None, Some(value)) => serde_json::from_value(value.clone())
                .map_err(|_| Error::invalid("Invalid inline profile"))?,
            _ => return Err(Error::invalid("Select profile or inline")),
        }
    };
    for key in profile.inherit_env.iter().chain(if request.launch {
        profile.launch.inherit_env.iter()
    } else {
        [].iter()
    }) {
        if !request.bindings.contains_key(key)
            && let Ok(value) = std::env::var(key)
        {
            request.bindings.insert(key.clone(), value);
        }
    }
    Ok(())
}
pub(crate) fn resolve(
    baseline: &Env,
    inherited: &[String],
    bindings: &Env,
    configured: &Env,
    overrides: &Env,
) -> Result<Env> {
    if baseline.keys().any(|key| !BASELINE.contains(&key.as_str())) {
        return Err(Error::invalid("baseline contains an unsupported host key"));
    }
    let mut result = baseline.clone();
    for key in inherited {
        result.insert(
            key.clone(),
            bindings.get(key).cloned().ok_or_else(|| {
                Error::new(
                    "BUILD_ENV_MISSING",
                    "Supply the requested inherited variable through bindings",
                )
                .details(json!({"key":key}))
            })?,
        );
    }
    result.extend(configured.clone());
    result.extend(overrides.clone());
    for (key, value) in &result {
        if !crate::configuration::variable(key) {
            return Err(Error::invalid("Invalid environment name"));
        }
        crate::configuration::literal(value)?;
    }
    Ok(result)
}
pub(crate) use capture as capture_cli;
