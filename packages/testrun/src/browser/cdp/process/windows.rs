//! Windows ownership and pipe setup for native Chrome processes.
use super::{Connection, Result};
use canonic_process_kit::launch::{OwnedTree, spawn_owned_pipe_tree};
use std::{
    io,
    os::windows::io::{AsHandle, OwnedHandle},
    path::{Component, Path, Prefix},
    time::Duration,
};
use tokio::{process::Command, time::Instant};

pub(super) struct Child {
    tree: OwnedTree,
    pipe: Option<(tokio::fs::File, tokio::fs::File)>,
}

impl Child {
    pub(super) fn spawn(mut command: Command) -> io::Result<Self> {
        let (child_read, write) = io::pipe()?;
        let (read, child_write) = io::pipe()?;
        command.arg("--remote-debugging-pipe");
        let tree = spawn_owned_pipe_tree(
            command.into_std(),
            child_read.as_handle(),
            child_write.as_handle(),
            |read, write| format!("--remote-debugging-io-pipes={read},{write}").into(),
        )?;
        let read = tokio::fs::File::from_std(OwnedHandle::from(read).into());
        let write = tokio::fs::File::from_std(OwnedHandle::from(write).into());
        Ok(Self {
            tree,
            pipe: Some((read, write)),
        })
    }

    #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
    pub(super) fn id(&self) -> u32 {
        self.tree.id()
    }

    pub(super) fn take_pipe(&mut self) -> Option<(tokio::fs::File, tokio::fs::File)> {
        self.pipe.take()
    }

    pub(super) async fn shutdown(
        &self,
        connection: Option<&Connection>,
        deadline: Instant,
    ) -> Result<()> {
        if self.tree.empty().map_err(|_| "CLEANUP_PENDING")? {
            return Ok(());
        }
        let graceful = deadline.min(Instant::now() + Duration::from_secs(2));
        if let Some(connection) = connection {
            let _ = connection
                .call(
                    None,
                    "Browser.close",
                    serde_json::json!({}),
                    super::super::Lane::Control,
                    graceful,
                )
                .await;
            let _ = connection.close(graceful).await;
        }
        if self.empty_by(graceful).await? {
            return Ok(());
        }
        self.tree.terminate().map_err(|_| "CLEANUP_PENDING")?;
        if self.empty_by(deadline).await? {
            Ok(())
        } else {
            Err("CLEANUP_PENDING")
        }
    }

    async fn empty_by(&self, deadline: Instant) -> Result<bool> {
        while !self.tree.empty().map_err(|_| "CLEANUP_PENDING")? {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            tokio::time::sleep(remaining.min(Duration::from_millis(10))).await;
        }
        Ok(true)
    }
}

pub(super) fn environment(command: &mut Command, profile: &Path, installed: bool) -> Result<()> {
    let system =
        crate::browser::preparation::windows::system_directory().map_err(|_| "HOST_UNAVAILABLE")?;
    let root = system.parent().ok_or("HOST_UNAVAILABLE")?;
    let Some(Component::Prefix(prefix)) = root.components().next() else {
        return Err("HOST_UNAVAILABLE");
    };
    let drive = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
        _ => return Err("HOST_UNAVAILABLE"),
    };
    command
        .env("SystemRoot", root)
        // Windows shell caches expand this variable even with an explicit
        // browser profile. Leaving it unset creates a relative cache path.
        .env("SystemDrive", format!("{}:", char::from(drive)))
        .env("PATH", &system)
        .current_dir(profile);
    for key in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
    ] {
        command.env(key, profile);
    }
    if installed {
        // Edge resolves the account's known folder to distinguish its default
        // profile from the explicit --user-data-dir before enabling CDP.
        let account = dirs::home_dir().ok_or("HOST_UNAVAILABLE")?;
        command.env("USERPROFILE", account);
    }
    Ok(())
}
