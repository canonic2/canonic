//! App deployment transactions, verified package identity and durable outcomes.
use crate::{
    error::{Error, Result},
    managed::storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

mod adapter;
mod package;
mod physical;
mod play_protect;
/// Validate and retain a build artifact using the installer's package contract.
pub(crate) async fn retain_build_checked(
    path: &Path,
    destination: &Path,
    platform: &str,
    simulator: bool,
    env: &std::collections::BTreeMap<String, String>,
    cancel: crate::process::Cancellation,
) -> Result<Value> {
    let source = path.to_owned();
    let worker_cancel = cancel.clone();
    let snapshot = tokio::task::spawn_blocking(move || {
        package::Snapshot::create_checked(&source, &worker_cancel)
    })
    .await
    .map_err(|_| Error::new("BUILD_IO", "Artifact snapshot stopped"))??;
    let target = if platform == "ios" {
        if simulator {
            "simulator:build"
        } else {
            "device:build"
        }
    } else {
        "android:build"
    };
    let mut tools = adapter::Tools::default();
    if platform == "android" {
        // Resolve SDK tools from this build's client environment, never the owner's startup env.
        let sdk = env
            .get("ANDROID_HOME")
            .or_else(|| env.get("ANDROID_SDK_ROOT"));
        let sdk = sdk.ok_or_else(|| {
            Error::new(
                "PREREQUISITE_MISSING",
                "APK validation needs ANDROID_HOME or ANDROID_SDK_ROOT in the build environment",
            )
        })?;
        let mut versions = std::fs::read_dir(Path::new(sdk).join("build-tools"))
            .map_err(|_| {
                Error::new(
                    "PREREQUISITE_MISSING",
                    "Android SDK Build Tools are unavailable",
                )
            })?
            .filter_map(|e| e.ok().map(|v| v.path()))
            .collect::<Vec<_>>();
        versions.sort();
        tools.aapt2 = versions
            .into_iter()
            .rev()
            .map(|v| v.join("aapt2"))
            .find(|v| v.is_file());
        if tools.aapt2.is_none() {
            return Err(Error::new(
                "PREREQUISITE_MISSING",
                "Android SDK aapt2 is unavailable",
            ));
        }
    }
    let identity = tools.identity(target, &snapshot.path).await?;
    cancel.check()?;
    package::validate_id(&identity.bundle_id)?;
    let result = json!({"path":destination.join(snapshot.path.file_name().unwrap()),"sha256":snapshot.sha256,"identity":identity});
    snapshot.retain(destination)?;
    Ok(result)
}
#[cfg(all(test, unix))]
mod tests;

pub(crate) async fn inventory(target: &str) -> Result<Value> {
    validate(target)?;
    adapter::Tools::default().inventory(target).await
}

/// Resolves the installed executable for exact physical process identity checks.
pub(crate) async fn physical_executable(serial: &str, bundle: &str) -> Result<String> {
    package::validate_id(bundle)?;
    physical::executable(serial, bundle).await
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub target_id: String,
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
    #[serde(rename = "appId")]
    pub bundle_id: Option<String>,
    #[serde(default)]
    pub replace: bool,
    /// Android only: the answer to a Play Protect security-check prompt.
    #[serde(default, skip_serializing_if = "play_protect::Answer::is_fail")]
    pub play_protect: play_protect::Answer,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    #[serde(rename = "appId")]
    bundle_id: String,
    version: String,
    build: String,
}

pub(crate) fn validate(target: &str) -> Result<()> {
    let valid = target.split_once(':').is_some_and(|(kind, id)| {
        matches!(kind, "device" | "simulator" | "android")
            && !id.is_empty()
            && id.len() <= 256
            && !id.chars().any(char::is_control)
    });
    if !valid {
        return Err(Error::invalid("Choose an explicit discovered targetId"));
    }
    Ok(())
}
pub(crate) fn record_path(root: &Path, reference: &str) -> Result<PathBuf> {
    if !reference.starts_with("app-")
        || reference.len() > 100
        || !reference
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::invalid("Invalid app operation reference"));
    }
    Ok(root.join(format!("{reference}.json")))
}
pub(crate) fn last(root: &Path) -> Result<Value> {
    if !root.exists() {
        return Ok(Value::Null);
    }
    let Some(pointer) = storage::read::<Value>(&root.join("last.json"))? else {
        return Ok(Value::Null);
    };
    status(
        root,
        pointer["actionId"]
            .as_str()
            .ok_or_else(|| Error::new("STATE_IO", "Invalid app operation pointer"))?,
    )
}
pub(crate) fn status(root: &Path, reference: &str) -> Result<Value> {
    let mut record = storage::read::<Value>(&record_path(root, reference)?)?.ok_or_else(|| {
        Error::new(
            "OUTCOME_UNKNOWN",
            "App operation record is unavailable; do not replay it",
        )
    })?;
    record.as_object_mut().unwrap().remove("signature");
    if matches!(record["status"].as_str(), Some("preparing" | "dispatched")) {
        record["status"] = json!("outcome_unknown");
        record["message"] = json!(
            "No terminal result is recorded; the operation may still be running. Inspect installed app state before another mutation"
        );
    }
    Ok(record)
}
pub(crate) async fn inspect(root: &Path, reference: &str) -> Result<Value> {
    inspect_with(root, reference, &adapter::Tools::default()).await
}
async fn inspect_with(root: &Path, reference: &str, tools: &adapter::Tools) -> Result<Value> {
    let mut record = status(root, reference)?;
    if let (Some(target), Some(bundle)) = (
        record["targetId"].as_str().map(str::to_owned),
        record["appId"].as_str().map(str::to_owned),
    ) {
        let (target, bundle) = (target.as_str(), bundle.as_str());
        record["observation"] = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tools.installed(target, bundle),
        )
        .await
        {
            Ok(Ok(Some(app))) => json!({"installed":true,"app":app}),
            Ok(Ok(None)) => json!({"installed":false}),
            Ok(Err(error)) => json!({"error":error}),
            Err(_) => {
                json!({"error":{"code":"TIMEOUT","message":"Installed-app observation exceeded its budget; the retained outcome is unchanged"}})
            }
        };
        if record["status"] == "outcome_unknown"
            && let Some(sha) = record["packageSha256"].as_str()
            && let Some(installed) = tools.reconcile(target, bundle, sha).await
        {
            record["reconciliation"] = installed;
        }
    }
    Ok(record)
}
pub(crate) async fn execute(
    root: &Path,
    reference: &str,
    operation: &str,
    request: Request,
) -> Result<Value> {
    execute_with(
        root,
        reference,
        operation,
        request,
        &adapter::Tools::default(),
    )
    .await
}
async fn execute_with(
    root: &Path,
    reference: &str,
    operation: &str,
    request: Request,
    tools: &adapter::Tools,
) -> Result<Value> {
    validate_request(&request, operation)?;
    storage::directory(root)?;
    let path = record_path(root, reference)?;
    let signature = json!({"operation":operation,"request":request});
    if let Some(record) = storage::read::<Value>(&path)? {
        if record["signature"] != signature {
            return Err(Error::new(
                "COMMAND_ID_CONFLICT",
                "App operation reference already has different arguments",
            ));
        }
        return if record["status"] == "completed" {
            Ok(record["data"].clone())
        } else {
            Err(Error::new(if record["status"]=="failed" {record["error"]["code"].as_str().unwrap_or("APP_OPERATION_FAILED")}else{"APP_OUTCOME_UNKNOWN"}, "This app operation already has a retained outcome; inspect status before another mutation")
                .details(json!({"actionId":reference,"result":status(root,reference)?})))
        };
    }
    let mut record = json!({"actionId":reference,"targetId":request.target_id,"operation":operation,"signature":signature,"status":"preparing","dispatched":false});
    storage::write(&path, &record)?;
    storage::write(&root.join("last.json"), &json!({"actionId":reference}))?;
    let result = perform(tools, &request, operation, &path, &mut record).await;
    match result {
        Ok(mut value) => {
            value["actionId"] = json!(reference);
            record["status"] = json!("completed");
            record["data"] = value.clone();
            storage::write(&path, &record)?;
            Ok(value)
        }
        Err(mut error) => {
            let uncertain = record["dispatched"] == true;
            record["status"] = json!(if uncertain {
                "outcome_unknown"
            } else {
                "failed"
            });
            record["error"] = json!(error);
            storage::write(&path, &record)?;
            if uncertain {
                let installed = match (record["appId"].as_str(), record["packageSha256"].as_str()) {
                    (Some(bundle), Some(sha)) => {
                        tools.reconcile(&request.target_id, bundle, sha).await
                    }
                    _ => None,
                };
                error = if error.code == "INSTALL_AWAITING_PLAY_PROTECT" {
                    // The device may still finish this install once the prompt is answered.
                    let mut details = error.details.take().unwrap_or_default();
                    details["actionId"] = json!(reference);
                    details["targetId"] = json!(request.target_id);
                    details["outcome"] = json!("unknown");
                    error.details(details)
                } else {
                    Error::new("APP_OUTCOME_UNKNOWN","App mutation was dispatched but completion could not be verified; inspect status and installed app state before retrying").details(json!({"cause":error,"actionId":reference,"targetId":request.target_id}))
                };
                if let Some(installed) = installed {
                    error.details.as_mut().unwrap()["reconciliation"] = installed;
                }
            } else {
                error.details =
                    Some(json!({"cause":error.details,"actionId":reference,"dispatched":false}));
            }
            Err(error)
        }
    }
}
fn validate_request(request: &Request, operation: &str) -> Result<bool> {
    validate(&request.target_id)?;
    let install = match operation {
        "install" => true,
        "uninstall" => false,
        _ => return Err(Error::invalid("Unknown app operation")),
    };
    if install && (request.path.is_none() || request.bundle_id.is_some())
        || !install && (request.path.is_some() || request.replace || request.bundle_id.is_none())
    {
        return Err(Error::invalid(
            "Install requires path; uninstall requires appId and accepts no path or replace flag",
        ));
    }
    if request
        .path
        .as_ref()
        .is_some_and(|p| p.as_os_str().as_encoded_bytes().len() > 4096)
    {
        return Err(Error::invalid("App path exceeds 4096 bytes"));
    }
    Ok(install)
}
async fn prepare_package(request: &Request) -> Result<package::Snapshot> {
    let source = request.path.clone().unwrap();
    let snapshot = tokio::task::spawn_blocking(move || package::Snapshot::create(&source))
        .await
        .map_err(|_| Error::new("PACKAGE_IO", "Could not prepare the app package"))??;
    if let Some(expected) = &request.expected_sha256
        && &snapshot.sha256 != expected
    {
        return Err(Error::new(
            "BUILD_ARTIFACT_CHANGED",
            "Retained artifact bytes no longer match the build; no installation was dispatched",
        ));
    }
    Ok(snapshot)
}

async fn perform(
    tools: &adapter::Tools,
    request: &Request,
    operation: &str,
    path: &Path,
    record: &mut Value,
) -> Result<Value> {
    let install = validate_request(request, operation)?;
    tools.check_target(&request.target_id).await?;
    let snapshot = if install {
        Some(prepare_package(request).await?)
    } else {
        None
    };
    let identity = if let Some(snapshot) = &snapshot {
        tools.identity(&request.target_id, &snapshot.path).await?
    } else {
        Identity {
            bundle_id: request
                .bundle_id
                .clone()
                .ok_or_else(|| Error::invalid("appId is required"))?,
            version: String::new(),
            build: String::new(),
        }
    };
    package::validate_id(&identity.bundle_id)?;
    if identity.version.len() > 1024 || identity.build.len() > 1024 {
        return Err(Error::invalid("App version metadata is too long"));
    }
    if identity.bundle_id.starts_with("com.canonic.testrun.Runner") {
        return Err(Error::new(
            "APP_IN_USE",
            "Testrun's own inspection runner cannot be managed as a test app",
        ));
    }
    let before = tools
        .installed(&request.target_id, &identity.bundle_id)
        .await?;
    if install && before.is_some() && !request.replace {
        return Err(Error::new(
            "APP_ALREADY_INSTALLED",
            "The app is installed; use replace: true to update it without uninstalling first",
        ));
    }
    if !install && before.is_none() {
        return Ok(
            json!({"targetId":request.target_id,"appId":identity.bundle_id,"removed":true,"changed":false,"verified":true}),
        );
    }
    record["appId"] = json!(identity.bundle_id);
    record["expected"] = json!(identity);
    if let Some(snapshot) = &snapshot {
        record["packageSha256"] = json!(snapshot.sha256);
    }
    record["status"] = json!("dispatched");
    record["dispatched"] = json!(true);
    storage::write(path, record)?;
    let prompt = tools
        .mutate(
            request,
            operation,
            snapshot.as_ref().map(|s| s.path.as_path()),
            &identity.bundle_id,
        )
        .await?;
    let after = tools
        .installed(&request.target_id, &identity.bundle_id)
        .await?;
    if install {
        let actual = after.ok_or_else(|| {
            Error::new(
                "APP_VERIFICATION_FAILED",
                "App is not present after installation",
            )
        })?;
        if (!identity.version.is_empty() && actual.version != identity.version)
            || (!actual.build.is_empty() && actual.build != identity.build)
        {
            return Err(Error::new(
                "APP_VERIFICATION_FAILED",
                "Installed app version does not match the submitted build",
            ).details(json!({"submittedVersion":identity.version,"submittedBuild":identity.build,"installedVersion":actual.version,"installedBuild":actual.build})));
        }
        let mut result = json!({"targetId":request.target_id,"appId":identity.bundle_id,"version":actual.version,"build":actual.build,"submittedBuild":identity.build,"packageSha256":snapshot.as_ref().unwrap().sha256,"updated":before.is_some(),"verified":true,"buildVerified":!actual.build.is_empty(),"launched":false,"dataPolicy":"no_uninstall_performed"});
        if !prompt.is_null() {
            result["playProtect"] = prompt;
        }
        Ok(result)
    } else {
        if after.is_some() {
            return Err(Error::new(
                "APP_VERIFICATION_FAILED",
                "App remains installed after uninstall",
            ));
        }
        Ok(
            json!({"targetId":request.target_id,"appId":identity.bundle_id,"removed":true,"changed":true,"verified":true,"dataPolicy":"platform_uninstall"}),
        )
    }
}
