//! Request-owned observations of Docker environments for project review.

use crate::policy::{
    Decision, Request,
    resolution::normalized::{Context, Input},
};
use crate::review::runtime::consumer::ResourceRequest;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[cfg(unix)]
mod engine;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("select the full lowercase Docker container ID")]
    ContainerId,
    #[error("Docker Engine observation failed: {0}")]
    Observation(&'static str),
    #[error("supply an absolute container path without parent traversal")]
    ContainerPath,
    #[error("select at most 64 container paths")]
    PathCount,
    #[error("unable to evaluate the enrolled project's host file policy")]
    Policy,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "PascalCase"))]
struct Mount {
    #[serde(rename(deserialize = "Type"))]
    kind: String,
    #[serde(default)]
    source: String,
    destination: String,
    #[serde(rename(deserialize = "RW"))]
    writable: bool,
}

#[cfg(any(unix, test))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct State {
    running: bool,
    paused: bool,
    restarting: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "PascalCase"))]
struct HostConfig {
    privileged: bool,
    network_mode: String,
}

#[cfg(any(unix, test))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Container {
    id: String,
    image: String,
    platform: String,
    state: State,
    host_config: HostConfig,
    mounts: Vec<Mount>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Review {
    engine_id: String,
    container_id: String,
    image_id: String,
    running: bool,
    host_config: HostConfig,
    mounts: BTreeMap<String, Mount>,
    paths: Vec<PathReview>,
}

#[derive(Debug, Serialize)]
struct PathReview {
    container_path: String,
    mount_target: Option<String>,
    candidate_host_path: Option<String>,
    writable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_host_policy: Option<HostPolicy>,
}

#[derive(Debug, Serialize)]
struct HostPolicy {
    read: Decision,
    write: Decision,
}

impl Review {
    pub(crate) fn preview_policy(&mut self, host_project: &Path) -> Result<(), Error> {
        let project = crate::runtime::Project::open(host_project).map_err(|_| Error::Policy)?;
        let snapshot = project.snapshot().map_err(|_| Error::Policy)?;
        let root = snapshot
            .identity()
            .project_root()
            .to_str()
            .ok_or(Error::Policy)?;
        for item in &mut self.paths {
            let Some(path) = &item.candidate_host_path else {
                continue;
            };
            item.candidate_host_policy = Some(HostPolicy {
                read: file_policy(&snapshot, root, path, "read")?,
                write: file_policy(&snapshot, root, path, "write")?,
            });
        }
        Ok(())
    }
}

fn file_policy(
    snapshot: &crate::runtime::Snapshot,
    root: &str,
    path: &str,
    action: &str,
) -> Result<Decision, Error> {
    let read = action == "read";
    let request = Request {
        executable: format!("shield:file-{action}"),
        arguments: vec![path.into()],
        cwd: root.into(),
        project: root.into(),
    };
    let resource = ResourceRequest {
        mcp: None,
        input: Input::File {
            action: action.into(),
            sources: if read { vec![path.into()] } else { vec![] },
            destination: (!read).then(|| path.into()),
            recursive: false,
            original_input: serde_json::json!({}),
        },
        context: Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    };
    let evaluation = snapshot
        .evaluate_resource(request, resource)
        .map_err(|_| Error::Policy)?;
    Ok(evaluation.evaluation().decision)
}

fn id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
}

pub(crate) fn inspect(socket: &Path, container: &str, paths: &[String]) -> Result<Review, Error> {
    if !id(container) {
        return Err(Error::ContainerId);
    }
    if paths.len() > 64 {
        return Err(Error::PathCount);
    }
    for path in paths {
        clean_absolute(path).ok_or(Error::ContainerPath)?;
    }
    #[cfg(unix)]
    {
        let engine = engine::Engine::open(socket)?;
        let identity = engine.identity()?;
        let observed = engine.container(container)?;
        engine.revalidate()?;
        review(identity, container, observed, paths)
    }
    #[cfg(not(unix))]
    {
        let _ = socket;
        Err(Error::Observation("local Unix Docker socket required"))
    }
}

fn clean_absolute(value: &str) -> Option<String> {
    if !value.starts_with('/') || value.contains('\0') || value.len() > 4096 {
        return None;
    }
    let mut path = String::new();
    for part in value.split('/') {
        match part {
            "" | "." => (),
            ".." => return None,
            part => {
                path.push('/');
                path.push_str(part);
            }
        }
    }
    if path.is_empty() {
        path.push('/');
    }
    Some(path)
}

#[cfg(any(unix, test))]
fn review(
    engine_id: String,
    selected: &str,
    observed: Container,
    paths: &[String],
) -> Result<Review, Error> {
    if observed.id != selected
        || observed.platform != "linux"
        || !observed.image.strip_prefix("sha256:").is_some_and(id)
    {
        return Err(Error::Observation(
            "unexpected container ID, image or platform",
        ));
    }
    if observed.mounts.len() > 256 {
        return Err(Error::Observation("mount inventory exceeds its limit"));
    }
    let mut mounts = BTreeMap::new();
    for mut mount in observed.mounts {
        mount.destination = clean_absolute(&mount.destination)
            .ok_or(Error::Observation("invalid mount destination"))?;
        if mount.kind == "bind" {
            mount.source =
                clean_absolute(&mount.source).ok_or(Error::Observation("invalid bind source"))?;
        } else {
            // Volume backing paths belong to the daemon. They are never host paths.
            mount.source.clear();
        }
        if mounts.insert(mount.destination.clone(), mount).is_some() {
            return Err(Error::Observation("repeated mount destination"));
        }
    }
    let paths = paths
        .iter()
        .map(|path| map_path(path, &mounts))
        .collect::<Result<_, _>>()?;
    Ok(Review {
        engine_id,
        container_id: observed.id,
        image_id: observed.image,
        running: observed.state.running && !observed.state.paused && !observed.state.restarting,
        host_config: observed.host_config,
        mounts,
        paths,
    })
}

#[cfg(any(unix, test))]
fn map_path(path: &str, mounts: &BTreeMap<String, Mount>) -> Result<PathReview, Error> {
    let path = clean_absolute(path).ok_or(Error::ContainerPath)?;
    let selected = mounts
        .values()
        .filter_map(|mount| {
            let suffix = path.strip_prefix(mount.destination.trim_end_matches('/'))?;
            (suffix.is_empty() || suffix.starts_with('/')).then_some((mount, suffix))
        })
        .max_by_key(|(mount, _)| mount.destination.len());
    let candidate_host_path =
        selected
            .filter(|(mount, _)| mount.kind == "bind")
            .map(|(mount, suffix)| {
                if suffix.is_empty() {
                    mount.source.clone()
                } else {
                    format!("{}{suffix}", mount.source.trim_end_matches('/'))
                }
            });
    Ok(PathReview {
        mount_target: selected.map(|(mount, _)| mount.destination.clone()),
        candidate_host_path,
        writable: selected.map(|(mount, _)| mount.writable),
        container_path: path,
        candidate_host_policy: None,
    })
}

#[cfg(test)]
mod tests;
