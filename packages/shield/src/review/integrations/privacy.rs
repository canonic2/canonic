//! Refuse private launcher metadata in tracked project host files.
use super::{Error, Path, Plan, agents};
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[cfg_attr(
    not(windows),
    expect(
        clippy::unnecessary_wraps,
        reason = "Windows must discover a native Git executable before creating the shared command"
    )
)]
fn git(root: &Path) -> Result<Command, Error> {
    #[cfg(windows)]
    let executable = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .filter(|directory| directory.is_absolute())
        .filter_map(|directory| directory.join("git.exe").canonicalize().ok())
        .find(|path| path.is_file() && !path.starts_with(root))
        .ok_or(Error::Invalid(
            "Git for Windows is required to verify private MCP destinations",
        ))?;
    #[cfg(not(windows))]
    let executable = std::path::PathBuf::from("/usr/bin/git");
    let mut command = Command::new(executable);
    command
        .current_dir(root)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    Ok(command)
}

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
    let mut child = git(root)?
        .args([
            "--literal-pathspecs",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_git_tracking_refuses_private_projection_into_the_index() {
        let root = assert_fs::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        assert!(untracked(&root, ".mcp.json").is_err());
        assert!(git(&root).unwrap().arg("init").status().unwrap().success());
        std::fs::write(root.join(".mcp.json"), "{}").unwrap();
        assert!(untracked(&root, ".mcp.json").is_ok());
        assert!(
            git(&root)
                .unwrap()
                .args(["add", "--", ".mcp.json"])
                .status()
                .unwrap()
                .success()
        );
        assert!(untracked(&root, ".mcp.json").is_err());
        assert!(untracked(&root, ".claude/settings.local.json").is_ok());
    }
}
