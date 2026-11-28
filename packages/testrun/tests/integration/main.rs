//! Acceptance through the Rust product against explicitly owned platform fixtures.

/// An absent AVD home: fake-adb fixtures never list the host user's AVDs.
#[cfg(unix)]
pub(crate) const NO_AVDS: &str = "/var/empty/testrun-test-no-avds";
#[cfg(feature = "acceptance-android")]
mod android;
#[cfg(unix)]
mod android_contract;
#[cfg(unix)]
mod applications;
#[cfg(all(unix, feature = "acceptance-browser-public"))]
mod browser;
mod browser_api;
#[path = "../../build_support.rs"]
mod build_support;
#[cfg(all(unix, feature = "acceptance-builds"))]
mod builds_native;
#[cfg(all(target_os = "macos", feature = "acceptance-ios-device"))]
mod device_media;
#[cfg(unix)]
mod emulator;
mod mcp;
#[cfg(unix)]
mod playback;
#[cfg(target_os = "macos")]
mod simulator;
mod startup;

/// A test's own short temporary directory and Testrun data root. Every
/// `testrun` a test starts gets `TESTRUN_DATA_ROOT` from it, so the test runs
/// its own manager, which inherits that test's fake tools and never reaches
/// the user's. Dropping it stops that manager.
#[cfg(unix)]
pub(crate) struct Workspace {
    directory: tempfile::TempDir,
}
#[cfg(unix)]
#[allow(dead_code, reason = "Only some feature combinations use every helper")]
impl Workspace {
    pub(crate) fn new() -> Self {
        Self {
            // Short enough for device actor socket paths inside the data root.
            directory: tempfile::Builder::new()
                .prefix("tr-")
                .tempdir_in("/tmp")
                .unwrap(),
        }
    }
    pub(crate) fn path(&self) -> &std::path::Path {
        self.directory.path()
    }
    pub(crate) fn data_root(&self) -> std::path::PathBuf {
        self.path().join("state")
    }
    /// The manager's pool records directory, shared by every client. Mirrors
    /// the product's layout: the pool key under the current protocol version.
    pub(crate) fn pool_directory(&self) -> std::path::PathBuf {
        use sha2::{Digest, Sha256};
        let hash = format!("{:x}", Sha256::digest(b"manager"));
        self.data_root().join(format!("v7-{}", &hash[..16]))
    }
    /// The records directory of the one connected target: its selection,
    /// actions and device actor records.
    pub(crate) fn target_records(&self) -> std::path::PathBuf {
        let session = self.pool_directory();
        let mut targets: Vec<_> = std::fs::read_dir(&session)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("t-"))
                    && path.join("selected.json").exists()
            })
            .collect();
        assert_eq!(targets.len(), 1, "One connected target in {session:?}");
        targets.remove(0)
    }
    /// Confirm the owned actor closed or recovery reconciled it, retaining its cleanup evidence.
    pub(crate) fn closed_cleanup(&self) -> serde_json::Value {
        let selected: serde_json::Value = serde_json::from_slice(
            &std::fs::read(self.target_records().join("selected.json")).unwrap(),
        )
        .unwrap();
        let directory = std::path::Path::new(selected["directory"].as_str().unwrap());
        let state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("state.json")).unwrap()).unwrap();
        let cleanup: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("cleanup.json")).unwrap())
                .unwrap();
        assert_eq!(cleanup["complete"], true, "{cleanup}");
        assert!(
            state["state"] == "closed" || cleanup.get("reconciled").is_some(),
            "{state}: {cleanup}"
        );
        cleanup
    }
    /// The running manager's PID from its own record.
    pub(crate) fn owner_pid(&self) -> Option<i32> {
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(self.data_root().join("owner.json")).ok()?)
                .ok()?;
        let pid = i32::try_from(record["pid"].as_u64()?).ok()?;
        // SAFETY: signal 0 only checks that this test's owner still exists.
        (unsafe { libc::kill(pid, 0) } == 0).then_some(pid)
    }
}
/// One data root for a hardware acceptance run, which those suites' CLI
/// commands and MCP servers share; such suites run one test at a time.
#[cfg(unix)]
#[allow(
    dead_code,
    reason = "Only hardware acceptance features share a data root"
)]
pub(crate) fn acceptance_workspace() -> &'static Workspace {
    static SHARED: std::sync::OnceLock<Workspace> = std::sync::OnceLock::new();
    SHARED.get_or_init(Workspace::new)
}
#[cfg(unix)]
impl Drop for Workspace {
    fn drop(&mut self) {
        // Diagnostics only: keep the private data root and its session journals.
        if std::env::var_os("TESTRUN_TEST_KEEP_WORKSPACE").is_some() {
            self.directory.disable_cleanup(true);
            eprintln!("Keeping test workspace {}", self.path().display());
        }
        let Some(pid) = self.owner_pid() else {
            return;
        };
        // SAFETY: the PID is this test's own owner process, from its record.
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        // SAFETY: signal 0 checks existence only.
        while unsafe { libc::kill(pid, 0) } == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // SAFETY: signal 0 checks existence only.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if alive {
            // SAFETY: this test's own owner is still running past its exit.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}
