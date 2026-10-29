//! Refuse private launcher metadata in tracked project host files.
use super::{Error, Path, Plan, agents};
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) fn private_mcp_destination(root: &Path, agent: &str) -> Result<(), Error> {
    let adapter = agents::find(agent).ok_or(Error::Invalid("unsupported MCP agent"))?;
    let path = adapter.spec().mcp_path.unwrap_or(".mcp.json");
    untracked(root, path)?;
    // Claude also projects local aliases into its enablement settings.
    if adapter.spec().mcp_path.is_none() {
        untracked(root, adapter.spec().hooks_path)?;
    }
    Ok(())
}

pub(super) fn validate(root: &Path, plan: &Plan) -> Result<(), Error> {
    if let Some(desired) = &plan.desired {
        for (agent, owned) in &desired.connections {
            let local = owned["review"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["source"] == "local" && row["enabled"] == true)
                .collect::<Vec<_>>();
            if !local.is_empty() {
                for row in local {
                    let id = row["id"]
                        .as_str()
                        .ok_or(Error::Invalid("invalid private MCP review record"))?;
                    let binding: crate::mcp_connections::launcher::Binding =
                        serde_json::from_value(owned["launches"][id].clone()).map_err(|_| {
                            Error::Invalid("private MCP launch binding needs fresh reviewed setup")
                        })?;
                    if !crate::mcp_connections::launcher::valid_token(&binding.token) {
                        return Err(Error::Invalid("invalid private MCP launch token"));
                    }
                }
                crate::mcp_connections::launcher::private_state(root)?;
                private_mcp_destination(root, agent)?;
            }
        }
    }
    Ok(())
}

fn untracked(root: &Path, path: &str) -> Result<(), Error> {
    // Do not inherit GIT_DIR, GIT_WORK_TREE or an alternate index: the check
    // must describe the destination's actual repository, not the caller's.
    let mut child = Command::new("/usr/bin/git")
        .current_dir(root)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args([
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "ls-files",
            "--cached",
            "--error-unmatch",
            "--",
            path,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            Error::Invalid(
                "cannot verify private MCP destination tracking; no host files were changed",
            )
        })?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(1) => Ok(()),
                    Some(0) => Err(Error::Invalid(
                        "private MCP destination is tracked; use an untracked project host file before setup",
                    )),
                    _ => Err(Error::Invalid(
                        "cannot verify private MCP destination tracking; setup refuses this projection",
                    )),
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Invalid(
                    "private MCP destination tracking check did not complete",
                ));
            }
        }
    }
}
