//! Native approval resource schema and operator revalidation.
/// Strict retained resource shape. Digests are evidence; native verification is
/// still mandatory at issuance and at the locked handoff.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalBinding {
    #[serde(default)]
    schema_version: Option<u8>,
    timeout_ms: u64,
    resources: serde_json::Value,
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[serde(default)]
    submitted: Option<crate::policy::Request>,
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[serde(default)]
    launch: Option<super::launch::EffectiveLaunch>,
}
#[cfg(not(any(windows, target_os = "linux")))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Resources {
    model: String,
    executable: String,
    sha256: String,
    executable_identity: (u64, u64, u64, i64, i64),
    loader: String,
    loader_sha256: String,
    loader_identity: (u64, u64, u64, i64, i64),
    cwd_identity: (u64, u64),
    library_contract: String,
    environment: String,
    #[serde(default)]
    git: Option<serde_json::Value>,
}

pub(crate) fn valid_approval_binding(
    request: &crate::policy::Request,
    value: &serde_json::Value,
) -> bool {
    let Ok(binding) = serde_json::from_value::<ApprovalBinding>(value.clone()) else {
        return false;
    };
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    let request = match (&binding.schema_version, &binding.submitted, &binding.launch) {
        (None, None, None) if value.as_object().is_some_and(|object| object.len() == 2) => request,
        (Some(2), Some(submitted), Some(launch))
            if launch.matches(submitted)
                && serde_json::to_value(request).ok()
                    == serde_json::to_value(launch.request()).ok() =>
        {
            submitted
        }
        _ => return false,
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    if binding.schema_version.is_some() {
        return false;
    }
    if !(1..=3_600_000).contains(&binding.timeout_ms)
        || !std::path::Path::new(&request.cwd).starts_with(&request.project)
        || super::protected_cwd(
            std::path::Path::new(&request.cwd),
            std::path::Path::new(&request.project),
        )
    {
        return false;
    }
    #[cfg(windows)]
    {
        super::windows_binding::valid(request, &binding.resources)
    }
    #[cfg(target_os = "linux")]
    {
        super::linux_binding::valid(request, &binding.resources)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let Ok(resources) = serde_json::from_value::<Resources>(binding.resources) else {
            return false;
        };
        super::supported(request)
            && (if request.executable == super::git::EXECUTABLE {
                resources.model == "macos-clt-git-inspection-v1"
                    && resources.git.is_some()
                    && resources.library_contract == "OS Git libraries/shared cache"
                    && resources.environment == "cleared-git-inspection-v1"
            } else {
                resources.model == "macos-readonly-system-leaf-v1"
                    && resources.git.is_none()
                    && resources.library_contract == "OS libSystem/shared cache"
                    && resources.environment == "cleared-lc-all-c"
            })
            && resources.executable == request.executable
            && resources.loader == "/usr/lib/dyld"
            && [&resources.sha256, &resources.loader_sha256]
                .iter()
                .all(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            && resources.executable_identity.2 <= 16_777_216
            && resources.loader_identity.2 <= 16_777_216
            && resources.cwd_identity.1 != 0
    }
}

pub(crate) fn verify_approval_binding(
    request: &crate::policy::Request,
    resource: &serde_json::Value,
) -> Result<(), crate::review::Error> {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    {
        let timeout = resource["timeout_ms"]
            .as_u64()
            .ok_or(crate::review::Error::Invalid("missing deadline"))?;
        if !valid_current_approval_binding(request, resource) {
            return Err(crate::review::Error::Invalid(
                "native launch binding requires a fresh request and review",
            ));
        }
        let submitted = serde_json::from_value::<ApprovalBinding>(resource.clone())
            .ok()
            .and_then(|binding| binding.submitted)
            .ok_or(crate::review::Error::Invalid("missing submitted launch"))?;
        if *resource == capture(&submitted, timeout)? {
            return Ok(());
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let _ = (request, resource);
    Err(crate::review::Error::Invalid(
        "unsupported or changed native approval binding",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
pub(crate) fn capture(
    request: &crate::policy::Request,
    timeout: u64,
) -> Result<serde_json::Value, crate::review::Error> {
    let binding = super::binding::Binding::open(request)
        .map_err(|_| crate::review::Error::Invalid("native resource cannot be verified"))?;
    binding
        .verify()
        .map_err(|_| crate::review::Error::Invalid("native resource changed"))?;
    let launch = super::launch::EffectiveLaunch::prepare(request)
        .map_err(|_| crate::review::Error::Invalid("unsupported effective launch"))?;
    Ok(resource(request, &launch, timeout, &binding.metadata()))
}

pub(crate) fn valid_current_approval_binding(
    request: &crate::policy::Request,
    value: &serde_json::Value,
) -> bool {
    value["schema_version"] == 2 && valid_approval_binding(request, value)
}

pub(crate) fn valid_evaluated_approval_binding(
    evidence: &crate::runtime::EvaluatedRequest,
    value: &serde_json::Value,
) -> bool {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    if valid_current_approval_binding(evidence.request(), value)
        && let Ok(binding) = serde_json::from_value::<ApprovalBinding>(value.clone())
        && let (Some(submitted), Some(launch), Some(actual)) =
            (binding.submitted, binding.launch, evidence.resource())
    {
        let expected = launch.resource(&submitted);
        return serde_json::json!(actual.input) == serde_json::json!(expected.input)
            && actual.context.cwd == expected.context.cwd
            && actual.context.environment == expected.context.environment
            && actual.mcp.is_none();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let _ = (evidence, value);
    false
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
pub(super) fn resource(
    submitted: &crate::policy::Request,
    launch: &super::launch::EffectiveLaunch,
    timeout: u64,
    resources: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({"schema_version":2,"timeout_ms":timeout,"resources":resources,"submitted":submitted,"launch":launch})
}

pub(crate) fn evaluate_approval(
    snapshot: &crate::runtime::Snapshot,
    request: &crate::policy::Request,
    value: &serde_json::Value,
) -> Result<crate::runtime::EvaluatedRequest, crate::review::Error> {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    if valid_current_approval_binding(request, value) {
        let binding: ApprovalBinding = serde_json::from_value(value.clone())
            .map_err(|_| crate::review::Error::Invalid("invalid launch binding"))?;
        if let (Some(submitted), Some(launch)) = (binding.submitted, binding.launch) {
            return snapshot
                .evaluate_resource(request.clone(), launch.resource(&submitted))
                .map_err(|_| crate::review::Error::Invalid("request cannot be evaluated"));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let _ = (snapshot, request, value);
    Err(crate::review::Error::Invalid(
        "native launch binding requires a fresh request and review",
    ))
}
