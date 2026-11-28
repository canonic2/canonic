//! Playwright's private stdio and Windows job ownership.

use canonic_process_kit::launch::{OwnedTree, spawn_owned_stdio_tree};
use std::{
    io,
    os::windows::io::{AsHandle, OwnedHandle},
    process::ExitStatus,
    time::Duration,
};
use tokio::{fs::File, process::Command, time::Instant};

use super::Error;

pub(crate) struct OwnedChild {
    pub(super) child: Child,
    pub(super) forced: bool,
}

pub(super) struct Child {
    tree: OwnedTree,
    reaped: bool,
    pub(super) stdin: Option<File>,
    pub(super) stdout: Option<File>,
    pub(super) stderr: Option<File>,
}

impl OwnedChild {
    pub(super) fn spawn(command: Command) -> Result<Self, Error> {
        let (input, stdin) = io::pipe()?;
        let (stdout, output) = io::pipe()?;
        let (stderr, errors) = io::pipe()?;
        let tree = spawn_owned_stdio_tree(
            command.into_std(),
            [input.as_handle(), output.as_handle(), errors.as_handle()],
        )?;
        Ok(Self {
            child: Child {
                tree,
                reaped: false,
                stdin: Some(File::from_std(OwnedHandle::from(stdin).into())),
                stdout: Some(File::from_std(OwnedHandle::from(stdout).into())),
                stderr: Some(File::from_std(OwnedHandle::from(stderr).into())),
            },
            forced: false,
        })
    }

    pub(super) fn signal(&mut self) -> Result<bool, Error> {
        if self.forced || self.child.tree.empty()? {
            return Ok(false);
        }
        self.child.tree.terminate()?;
        self.forced = true;
        Ok(true)
    }

    pub(super) fn snapshot(&self) -> io::Result<super::super::processes::Snapshot> {
        let count = self.child.tree.active_processes()?;
        let alive = !self.child.tree.exited()?;
        Ok(super::super::processes::Snapshot {
            driver_alive: Some(alive),
            descendants_alive: count.saturating_sub(u32::from(alive)) as usize,
            descendants_zombies: 0,
            descendants_absent: 0,
            descendants_reused: 0,
        })
    }

    pub(super) async fn release(&mut self, deadline: Instant) -> io::Result<bool> {
        while !self.child.tree.empty()? {
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(10)))
                .await;
        }
        Ok(true)
    }

    pub(super) async fn exit_after_eof(&mut self, deadline: Instant) -> Result<ExitStatus, Error> {
        let mut lone_since = None;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(Error::CleanupPending);
            }
            if self.is_lone()? {
                let since = *lone_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= super::waiting::RELEASED_DRIVER_GRACE && self.is_lone()? {
                    self.signal()?;
                }
            } else {
                lone_since = None;
            }
            tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(25)))
                .await;
        }
    }

    fn is_lone(&self) -> io::Result<bool> {
        Ok(self.child.tree.active_processes()? == 1 && !self.child.tree.exited()?)
    }
}

impl Child {
    pub(super) fn id(&self) -> Option<u32> {
        (!self.reaped).then(|| self.tree.id())
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.tree.try_wait()?;
        self.reaped |= status.is_some();
        Ok(status)
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
