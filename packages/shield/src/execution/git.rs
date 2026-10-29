//! Fixed Git inspection forms and deterministic launch settings, independent of policy.
use crate::policy::Request;

pub(super) const EXECUTABLE: &str = "/Library/Developer/CommandLineTools/usr/bin/git";

pub(super) fn supported(request: &Request) -> bool {
    let args = request
        .arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    matches!(args.as_slice(), ["status" | "diff"] | ["diff", "--cached"])
        || matches!(args.as_slice(), ["log", "-n", count] if count.parse::<u16>().is_ok_and(|n| (1..=100).contains(&n)) && count.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(target_os = "macos")]
pub(super) fn launch(request: &Request) -> Request {
    let mut launch = request.clone();
    launch.arguments = [
        "--no-pager",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "core.attributesFile=/dev/null",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "maintenance.auto=false",
        "-c",
        "gc.auto=0",
        "-c",
        "protocol.allow=never",
    ]
    .map(str::to_owned)
    .to_vec();
    launch.arguments.extend(request.arguments.clone());
    match request.arguments[0].as_str() {
        "status" => launch.arguments.extend(
            ["--short", "--untracked-files=no", "--ignore-submodules=all"].map(str::to_owned),
        ),
        "diff" => launch.arguments.extend(
            ["--no-ext-diff", "--no-textconv", "--ignore-submodules=all"].map(str::to_owned),
        ),
        "log" => launch.arguments.extend(
            ["--no-show-signature", "--no-decorate", "--format=oneline"].map(str::to_owned),
        ),
        _ => unreachable!("validated Git form"),
    }
    launch
}

#[cfg(target_os = "macos")]
pub(super) fn capture(request: &Request) -> Result<serde_json::Value, super::Error> {
    use std::{fs, os::unix::fs::MetadataExt, path::Path};
    let directory = Path::new(&request.project).join(".git");
    if directory.canonicalize()? != directory || !fs::symlink_metadata(&directory)?.is_dir() {
        return Err(super::Error::Unsupported);
    }
    let metadata = fs::metadata(&directory)?;
    if directory.join("info/attributes").try_exists()? {
        return Err(super::Error::Unsupported);
    }
    let mut files = Vec::new();
    for name in ["config", "HEAD"] {
        let path = directory.join(name);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() > 1_048_576 {
            return Err(super::Error::Unsupported);
        }
        let bytes = fs::read(&path)?;
        if bytes.len() > 1_048_576 {
            return Err(super::Error::Unsupported);
        }
        files.push(serde_json::json!({"name":name,"sha256":crate::policy::fingerprint(&bytes)}));
    }
    Ok(
        serde_json::json!({"directory_identity":[metadata.dev(),metadata.ino()],"files":files,"launch_arguments":launch(request).arguments,"environment":"git-inspection-v1"}),
    )
}
