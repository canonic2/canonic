//! Read-only descendant witnesses for verified owned browser teardown.
//!
//! PID reuse never grants signal authority. An inspection error is unknown,
//! not proof that a process exited. Other hosts have no lifecycle acceptance.

use std::collections::BTreeMap;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::collections::BTreeSet;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_scope;
#[cfg(target_os = "linux")]
use linux::{children, info};

#[derive(Default)]
pub(crate) struct LaunchScope {
    #[cfg(target_os = "linux")]
    scope: Option<linux_scope::Scope>,
}

impl LaunchScope {
    pub async fn prepare(deadline: tokio::time::Instant) -> std::io::Result<Self> {
        #[cfg(target_os = "linux")]
        if super::preparation::ubuntu_bundle_host() {
            return Ok(Self {
                scope: Some(linux_scope::Scope::start(deadline).await?),
            });
        }
        let _ = deadline;
        Ok(Self::default())
    }

    pub fn command(&self, executable: &std::path::Path) -> tokio::process::Command {
        #[cfg(target_os = "linux")]
        if let Some(scope) = &self.scope {
            return scope.command(executable);
        }
        let mut command = tokio::process::Command::new(executable);
        command.env_clear();
        command
    }

    #[cfg(unix)]
    pub fn end(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(scope) = &mut self.scope {
            scope.end();
        }
    }

    #[cfg(unix)]
    pub async fn release(&mut self, deadline: tokio::time::Instant) -> std::io::Result<bool> {
        #[cfg(target_os = "linux")]
        if let Some(scope) = &mut self.scope {
            scope.release(deadline).await?;
            return Ok(true);
        }
        let _ = deadline;
        Ok(false)
    }
}

pub(super) const fn windows_bundle_host() -> bool {
    cfg!(all(windows, target_arch = "x86_64"))
}

pub(super) fn windows_mode_supported(mode: super::contract::Mode) -> bool {
    windows_bundle_host() && (mode == super::contract::Mode::Headless || windows_console_session())
}

pub(super) fn windows_console_session() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::RemoteDesktop::{
            ProcessIdToSessionId, WTSGetActiveConsoleSessionId,
        };
        let mut session = 0;
        // SAFETY: the output is writable and the input identifies this process.
        let found = unsafe { ProcessIdToSessionId(std::process::id(), &raw mut session) };
        // SAFETY: this Windows query has no input or pointer arguments.
        let console = unsafe { WTSGetActiveConsoleSessionId() };
        found != 0 && session != 0 && session != u32::MAX && session == console
    }
    #[cfg(not(windows))]
    false
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Info {
    parent: u32,
    start: u64,
    zombie: bool,
}

pub(crate) fn initial_host() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        let mut name = std::mem::MaybeUninit::<libc::utsname>::zeroed();
        // SAFETY: uname fills this correctly sized writable native structure.
        if unsafe { libc::uname(name.as_mut_ptr()) } != 0 {
            return false;
        }
        // SAFETY: uname initialized the complete NUL-terminated release field.
        let name = unsafe { name.assume_init() };
        unsafe { std::ffi::CStr::from_ptr(name.release.as_ptr()) }
            .to_bytes()
            .starts_with(b"24.")
    }
    #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
    {
        false
    }
}

#[derive(Clone, Default)]
pub(crate) struct Tracker {
    processes: BTreeMap<u32, u64>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Snapshot {
    pub driver_alive: Option<bool>,
    pub descendants_alive: usize,
    pub descendants_zombies: usize,
    pub descendants_absent: usize,
    pub descendants_reused: usize,
}

#[cfg(target_os = "macos")]
fn info(pid: u32) -> std::io::Result<Option<Info>> {
    let pid = i32::try_from(pid).map_err(|_| std::io::Error::other("Process identity overflow"))?;
    let mut result = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size =
        i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).expect("Small native structure");
    // SAFETY: the zeroed structure is writable for exactly the supplied size.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            result.as_mut_ptr().cast(),
            size,
        )
    };
    if written == size {
        // SAFETY: the whole structure was filled by proc_pidinfo.
        let result = unsafe { result.assume_init() };
        return Ok(Some(Info {
            parent: result.pbi_ppid,
            start: result.pbi_start_tvsec * 1_000_000 + result.pbi_start_tvusec,
            zombie: result.pbi_status == 5,
        }));
    }
    let error = std::io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(target_os = "macos")]
fn children(pid: u32) -> std::io::Result<Vec<u32>> {
    let mut children = [0i32; 1024];
    let bytes = i32::try_from(std::mem::size_of_val(&children)).expect("Bounded child table");
    // SAFETY: errno is thread-local, and the PID query writes at most bytes.
    unsafe { *libc::__error() = 0 };
    // SAFETY: selector 6 observes children using the writable PID buffer.
    let written = unsafe { libc::proc_listpids(6, pid, children.as_mut_ptr().cast(), bytes) };
    if written < 0
        || (written == 0 && std::io::Error::last_os_error().raw_os_error().unwrap_or(0) != 0)
    {
        return Err(std::io::Error::last_os_error());
    }
    if written >= bytes {
        return Err(std::io::Error::other("Owned child table exceeds capacity"));
    }
    Ok(children
        .into_iter()
        .take(written as usize / std::mem::size_of::<i32>())
        .filter_map(|pid| u32::try_from(pid).ok().filter(|pid| *pid != 0))
        .collect())
}

impl Tracker {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn observe(&mut self, root: u32) -> std::io::Result<()> {
        if let std::collections::btree_map::Entry::Vacant(entry) = self.processes.entry(root) {
            let root_info =
                info(root)?.ok_or_else(|| std::io::Error::other("Driver identity unavailable"))?;
            entry.insert(root_info.start);
        }
        let mut pending: Vec<_> = self
            .processes
            .iter()
            .map(|(pid, start)| (*pid, *start))
            .collect();
        let mut seen = BTreeSet::new();
        while let Some((pid, start)) = pending.pop() {
            if !seen.insert(pid) {
                continue;
            }
            let Some(parent) = info(pid)? else {
                continue;
            };
            if parent.start != start || parent.zombie {
                continue;
            }
            let visible_children = children(pid)?;
            let Some(after) = info(pid)? else {
                continue;
            };
            if after.start != start {
                continue;
            }
            for child in visible_children {
                if let Some(child_info) = info(child)?
                    && child_info.parent == pid
                    && child_info.start >= start
                {
                    self.processes.insert(child, child_info.start);
                    pending.push((child, child_info.start));
                    if self.processes.len() > 4096 {
                        return Err(std::io::Error::other(
                            "Owned process table exceeds capacity",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
    pub fn observe(&mut self, _root: u32) -> std::io::Result<()> {
        Err(std::io::Error::other("Process witness host unsupported"))
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn released(&self) -> std::io::Result<bool> {
        for (pid, start) in &self.processes {
            if let Some(current) = info(*pid)?
                && current.start == *start
                && !current.zombie
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub fn released(&self) -> std::io::Result<bool> {
        Err(std::io::Error::other("Process witness host unsupported"))
    }
    /// Witnessed processes still alive (zombies count as released), for diagnostics.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn remaining(&self) -> std::io::Result<usize> {
        Ok(self.snapshot(None)?.descendants_alive)
    }
    /// Counts only birth-fenced witnesses; a missing root is unknown, not dead.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn snapshot(&self, root: Option<u32>) -> std::io::Result<Snapshot> {
        let mut snapshot = Snapshot {
            driver_alive: None,
            descendants_alive: 0,
            descendants_zombies: 0,
            descendants_absent: 0,
            descendants_reused: 0,
        };
        for (pid, start) in &self.processes {
            let current = info(*pid)?;
            let alive = current
                .as_ref()
                .is_some_and(|current| current.start == *start && !current.zombie);
            if root == Some(*pid) {
                snapshot.driver_alive = Some(alive);
            } else if alive {
                snapshot.descendants_alive += 1;
            } else {
                match current {
                    None => snapshot.descendants_absent += 1,
                    Some(current) if current.start != *start => snapshot.descendants_reused += 1,
                    Some(_) => snapshot.descendants_zombies += 1,
                }
            }
        }
        Ok(snapshot)
    }
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
    pub fn snapshot(&self, _root: Option<u32>) -> std::io::Result<Snapshot> {
        Err(std::io::Error::other("Process witness host unsupported"))
    }
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
    pub fn remaining(&self) -> std::io::Result<usize> {
        Err(std::io::Error::other("Process witness host unsupported"))
    }
    /// All witnessed processes, including the driver, for diagnostics.
    pub fn witnessed(&self) -> usize {
        self.processes.len()
    }
    #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
    pub fn count(&self) -> usize {
        self.processes.len()
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn driver_lifecycle_process_witness_refresh_captures_late_owned_descendants() {
        let root = std::process::id();
        let mut initial = Tracker::default();
        initial.observe(root).unwrap();
        let mut command = tokio::process::Command::new("/bin/cat");
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // This helper can only wait for stdin EOF; it never launches a browser.
        let mut child = crate::browser::driver::OwnedChild::spawn(&mut command).unwrap();
        let input = child.child.stdin.take().unwrap();
        let pid = child.child.id().unwrap();
        assert!(initial.snapshot(Some(pid)).unwrap().driver_alive.is_none());
        let mut refreshed = initial.clone();
        refreshed.observe(root).unwrap();
        assert_eq!(
            refreshed.snapshot(Some(pid)).unwrap().driver_alive,
            Some(true)
        );
        assert!(
            initial.snapshot(Some(pid)).unwrap().driver_alive.is_none(),
            "An initial launch snapshot cannot account for the later child"
        );
        drop(input);
        child.child.wait().await.unwrap();
        assert_eq!(
            refreshed.snapshot(Some(pid)).unwrap().driver_alive,
            Some(false)
        );
        assert_eq!(
            refreshed.snapshot(Some(root)).unwrap().driver_alive,
            Some(true)
        );
        assert!(
            refreshed
                .snapshot(Some(u32::MAX))
                .unwrap()
                .driver_alive
                .is_none()
        );
    }
}
