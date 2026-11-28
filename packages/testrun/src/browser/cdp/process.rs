//! Private Chrome process capsule retained across bootstrap and cleanup failures.

use super::{Connection, Events};
use crate::browser::{
    contract::{ContextProxy, Mode},
    preparation::ChromeBundle,
    processes::LaunchScope,
};
#[cfg(unix)]
use crate::browser::{driver::OwnedChild, processes::Tracker};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Stdio;
#[cfg(unix)]
use tokio::{io::AsyncReadExt, task::JoinHandle};
use tokio::{process::Command, time::Instant};

#[cfg(target_os = "linux")]
mod linux;
pub(super) mod media_fixtures;
#[cfg(windows)]
mod windows;

pub(super) struct Process {
    #[cfg(unix)]
    owned: OwnedChild,
    profile: PathBuf,
    preserve_profile: bool,
    #[cfg(unix)]
    stderr: Option<JoinHandle<()>>,
    #[cfg(unix)]
    witnesses: Tracker,
    #[cfg(unix)]
    reaped: bool,
    #[cfg(unix)]
    inspection_failed: bool,
    #[cfg(unix)]
    pipe: Option<tokio::net::UnixStream>,
    #[cfg(windows)]
    owned: windows::Child,
}

type Result<T> = std::result::Result<T, &'static str>;

impl Process {
    #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
    pub(super) fn pid(&self) -> Option<u32> {
        #[cfg(unix)]
        return self.owned.child.id();
        #[cfg(windows)]
        return Some(self.owned.id());
    }

    pub(super) fn profile_path(directory: &Path) -> PathBuf {
        directory.join("chrome-profile")
    }

    #[cfg(unix)]
    pub(super) async fn remove_profile(profile: &Path, _: Instant) -> Result<()> {
        match tokio::fs::remove_dir_all(profile).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("CLEANUP_PENDING"),
        }
    }

    #[cfg(windows)]
    pub(super) async fn remove_profile(profile: &Path, deadline: Instant) -> Result<()> {
        crate::artifacts::windows::remove_tree(profile, deadline)
            .await
            .map_err(|_| "CLEANUP_PENDING")
    }

    /// Once Chrome spawns, return its capsule without an await boundary.
    /// The prepared profile transfers to the process for verified cleanup.
    pub(super) fn spawn(
        bundle: ChromeBundle,
        profile: PathBuf,
        mode: Mode,
        proxy: Option<&ContextProxy>,
        fixtures: media_fixtures::Staged,
        preserve_profile: bool,
        scope: LaunchScope,
    ) -> Result<Self> {
        let mut command = launch_command(&bundle.executable, &profile, mode, proxy, &scope)?;
        #[cfg(windows)]
        windows::environment(&mut command, &profile, bundle.installed.is_some())?;
        #[cfg(target_os = "linux")]
        if bundle.installed.is_none()
            && let Some(helper) = linux::private_chrome_sandbox()
        {
            command.env("CHROME_DEVEL_SANDBOX", helper);
        }
        fixtures.configure(&mut command)?;
        #[cfg(windows)]
        {
            let owned = windows::Child::spawn(command).map_err(|_| "BROWSER_LAUNCH_FAILED")?;
            Ok(Self {
                owned,
                profile,
                preserve_profile,
            })
        }
        #[cfg(unix)]
        {
            let pipe = Some(
                super::connection::pipe::configure(&mut command)
                    .map_err(|_| "BROWSER_LAUNCH_FAILED")?,
            );
            let mut owned = OwnedChild::spawn_scoped(&mut command, scope)
                .map_err(|_| "BROWSER_LAUNCH_FAILED")?;
            let mut errors = owned.child.stderr.take().ok_or("BROWSER_LAUNCH_FAILED")?;
            // Drain continuously without exposing endpoint/profile data or retaining
            // an unbounded stderr log. Process status supplies public failure codes.
            let stderr = Some(tokio::spawn(async move {
                let mut bytes = [0; 4096];
                while matches!(errors.read(&mut bytes).await, Ok(n) if n > 0) {}
            }));
            Ok(Self {
                owned,
                profile,
                preserve_profile,
                stderr,
                witnesses: Tracker::default(),
                reaped: false,
                inspection_failed: false,
                pipe,
            })
        }
    }

    #[cfg(unix)]
    pub(super) async fn connect(&mut self, _: Instant) -> Result<(Connection, Events)> {
        let pipe = self.pipe.take().ok_or("BROWSER_ENDPOINT_UNAVAILABLE")?;
        self.observe().await?;
        let (reader, writer) = tokio::io::split(pipe);
        Ok(super::connection::pipe::open(reader, writer))
    }

    #[cfg(windows)]
    pub(super) async fn connect(&mut self, _: Instant) -> Result<(Connection, Events)> {
        let (reader, writer) = self
            .owned
            .take_pipe()
            .ok_or("BROWSER_ENDPOINT_UNAVAILABLE")?;
        Ok(super::connection::pipe::open(reader, writer))
    }

    #[cfg(unix)]
    pub(super) async fn observe(&mut self) -> Result<()> {
        if self.reaped {
            return Ok(());
        }
        let root = self.owned.child.id().ok_or("CLEANUP_PENDING")?;
        let mut tracker = self.witnesses.clone();
        match tokio::task::spawn_blocking(move || {
            tracker.observe(root)?;
            Ok::<_, std::io::Error>(tracker)
        })
        .await
        {
            Ok(Ok(tracker)) => {
                self.witnesses = tracker;
                Ok(())
            }
            _ => {
                self.inspection_failed = true;
                Err("CLEANUP_PENDING")
            }
        }
    }

    /// Forced group cleanup is limited to this retained, unreaped Child. An
    /// escaped descendant remains a release obligation, never a stale-PID kill.
    #[cfg(unix)]
    pub(super) async fn shutdown(
        &mut self,
        connection: Option<&Connection>,
        deadline: Instant,
    ) -> Result<()> {
        if !self.reaped {
            let _ = self.observe().await;
            if let Some(connection) = connection {
                let graceful = deadline.min(Instant::now() + std::time::Duration::from_secs(2));
                let _ = connection
                    .call(
                        None,
                        "Browser.close",
                        serde_json::json!({}),
                        super::Lane::Control,
                        graceful,
                    )
                    .await;
                let _ = connection.close(graceful).await;
            }
            let graceful = deadline.min(Instant::now() + std::time::Duration::from_secs(2));
            match tokio::time::timeout_at(graceful, self.owned.child.wait()).await {
                Ok(result) => {
                    result.map_err(|_| "CLEANUP_PENDING")?;
                }
                Err(_) => {
                    if Instant::now() >= deadline {
                        return Err("CLEANUP_PENDING");
                    }
                    self.owned.signal().map_err(|_| "CLEANUP_PENDING")?;
                    tokio::time::timeout_at(deadline, self.owned.child.wait())
                        .await
                        .map_err(|_| "CLEANUP_PENDING")?
                        .map_err(|_| "CLEANUP_PENDING")?;
                }
            }
            self.reaped = true;
        }
        let scope_released = self
            .owned
            .scope
            .release(deadline)
            .await
            .map_err(|_| "CLEANUP_PENDING")?;
        if let Some(reader) = self.stderr.as_mut() {
            tokio::time::timeout_at(deadline, reader)
                .await
                .map_err(|_| "CLEANUP_PENDING")?
                .map_err(|_| "CLEANUP_PENDING")?;
            self.stderr = None;
        }
        if self.inspection_failed && !scope_released {
            return Err("CLEANUP_PENDING");
        }
        while !scope_released && !self.witnesses.released().map_err(|_| "CLEANUP_PENDING")? {
            if Instant::now() >= deadline {
                return Err("CLEANUP_PENDING");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        if self.preserve_profile {
            Ok(())
        } else {
            Self::remove_profile(&self.profile, deadline).await
        }
    }

    #[cfg(windows)]
    pub(super) async fn shutdown(
        &mut self,
        connection: Option<&Connection>,
        deadline: Instant,
    ) -> Result<()> {
        self.owned.shutdown(connection, deadline).await?;
        if self.preserve_profile {
            Ok(())
        } else {
            Self::remove_profile(&self.profile, deadline).await
        }
    }
}

#[cfg(unix)]
impl Drop for Process {
    fn drop(&mut self) {
        if let Some(task) = self.stderr.take() {
            task.abort();
        }
    }
}

// This policy is only for disposable owned profiles. Mock keychain access keeps
// Chrome from consulting the login keychain when HOME points at its private root.
fn launch_command(
    executable: &Path,
    profile: &Path,
    mode: Mode,
    proxy: Option<&ContextProxy>,
    scope: &LaunchScope,
) -> Result<Command> {
    let mut command = scope.command(executable);
    #[cfg(unix)]
    command
        .env("HOME", profile)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(profile)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
        .arg(format!("--user-data-dir={}", profile.display()))
        .args([
            // The session creates its own first page through CDP.
            "--no-startup-window",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-background-timer-throttling",
            "--disable-backgrounding-occluded-windows",
            "--disable-renderer-backgrounding",
            "--disable-component-update",
            "--disable-sync",
            "--metrics-recording-only",
            "--force-color-profile=srgb",
            "--enable-features=WebMCP",
            "--password-store=basic",
            "--use-mock-keychain",
        ]);
    if mode == Mode::Headless {
        command.arg("--headless=new");
    }
    #[cfg(target_os = "linux")]
    if mode == Mode::Headed {
        crate::browser::driver::display_environment(&mut command);
    }
    if let Some(proxy) = proxy {
        let proxy = proxy.normalized()?;
        if proxy.server.starts_with("socks5://") {
            let url = url::Url::parse(&proxy.server).map_err(|_| "HTTP_PROXY_INVALID")?;
            command.arg(format!(
                "--host-resolver-rules=\"MAP * ~NOTFOUND , EXCLUDE {}\"",
                url.host_str().ok_or("HTTP_PROXY_INVALID")?
            ));
        }
        command.arg(format!("--proxy-server={}", proxy.server));
        let mut bypass = proxy
            .bypass
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .split(',')
                    .map(|rule| {
                        if rule.starts_with('.') {
                            format!("*{rule}")
                        } else {
                            rule.to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if proxy.proxies_loopback() {
            bypass.push("<-loopback>".into());
        }
        if !bypass.is_empty() {
            command.arg(format!("--proxy-bypass-list={}", bypass.join(";")));
        }
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_socks_native_launch_owns_the_pinned_dns_rule_without_credentials() {
        for scheme in ["socks4", "socks5"] {
            let proxy:ContextProxy=serde_json::from_value(serde_json::json!({"server":format!("{scheme}://AcmeIgnored:example@127.0.0.1:1080/ignored"),"bypass":" .example.com "})).unwrap();
            let command = launch_command(
                Path::new("/private/Acme/chrome"),
                Path::new("/private/Acme/profile"),
                Mode::Headless,
                Some(&proxy),
                &LaunchScope::default(),
            )
            .unwrap();
            let args = command
                .as_std()
                .get_args()
                .map(|value| value.to_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                args.iter()
                    .filter(|value| value.starts_with("--host-resolver-rules="))
                    .copied()
                    .collect::<Vec<_>>(),
                if scheme == "socks5" {
                    vec!["--host-resolver-rules=\"MAP * ~NOTFOUND , EXCLUDE 127.0.0.1\""]
                } else {
                    vec![]
                }
            );
            assert!(args.contains(&format!("--proxy-server={scheme}://127.0.0.1:1080").as_str()));
            assert!(args.contains(&"--proxy-bypass-list=*.example.com;<-loopback>"));
            assert!(!args.iter().any(|value| value.contains("AcmeIgnored")
                || value.contains("example@")
                || value.contains("ignored")));
        }
    }
    #[test]
    fn launch_proxy_native_flags_normalize_authority_without_exposing_credentials() {
        for (bypass, expected) in [
            (None, "<-loopback>"),
            (Some(" localhost , 127.0.0.1 "), "localhost;127.0.0.1"),
            (
                Some(" .example.com , acme.test "),
                "*.example.com;acme.test;<-loopback>",
            ),
        ] {
            let proxy:ContextProxy=serde_json::from_value(serde_json::json!({"server":"http://AcmeIgnored:private@127.0.0.1:8080/ignored?Acme=1","bypass":bypass,"username":"Acme global","password":"global example"})).unwrap();
            for mode in [Mode::Headless, Mode::Headed] {
                let command = launch_command(
                    Path::new("/private/Acme/chrome"),
                    Path::new("/private/Acme/profile"),
                    mode,
                    Some(&proxy),
                    &LaunchScope::default(),
                )
                .unwrap();
                let args = command
                    .as_std()
                    .get_args()
                    .map(|value| value.to_str().unwrap())
                    .collect::<Vec<_>>();
                assert!(args.contains(&"--proxy-server=http://127.0.0.1:8080"));
                assert!(args.contains(&format!("--proxy-bypass-list={expected}").as_str()));
                for private in [
                    "AcmeIgnored",
                    "private@",
                    "Acme global",
                    "global example",
                    "ignored?",
                    "Acme=1",
                ] {
                    assert!(!args.iter().any(|arg| arg.contains(private)));
                }
            }
        }
    }
    #[test]
    fn chrome_launch_leaves_the_first_page_to_cdp() {
        for mode in [Mode::Headed, Mode::Headless] {
            let command = launch_command(
                Path::new("/private/Acme/chrome"),
                Path::new("/private/Acme/profile"),
                mode,
                None,
                &LaunchScope::default(),
            )
            .unwrap();
            let args = command.as_std().get_args().collect::<Vec<_>>();
            assert!(args.contains(&std::ffi::OsStr::new("--no-startup-window")));
            assert!(
                args.iter()
                    .all(|arg| arg.as_encoded_bytes().starts_with(b"--"))
            );
        }
    }

    #[cfg(feature = "acceptance-browser-lifecycle")]
    #[tokio::test]
    async fn native_http_state_hidden_target_identity_diagnostic() {
        use futures_util::FutureExt;
        use serde_json::json;
        use std::time::Duration;
        assert!(std::env::var_os("TESTRUN_BROWSER_ROOT").is_some());
        for mode in [Mode::Headless, Mode::Headed] {
            let root = tempfile::tempdir().unwrap();
            let bundle = crate::browser::preparation::prepare_chrome(|_| {})
                .await
                .unwrap();
            let profile = Process::profile_path(root.path());
            crate::artifacts::directory(&profile).unwrap();
            let mut process = Process::spawn(
                bundle,
                profile,
                mode,
                None,
                media_fixtures::Staged::default(),
                false,
                LaunchScope::default(),
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            let (connection, mut events) = process.connect(deadline).await.unwrap();
            let result = std::panic::AssertUnwindSafe(async {
                super::super::Bootstrap::initialize(&connection, crate::browser::preparation::CHROME_VERSION, deadline).await.unwrap();
                let context = connection.call(None,"Target.createBrowserContext",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                connection.call(None,"Target.setAutoAttach",json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true,"filter":[{"type":"page"},{"exclude":true}]}),super::super::Lane::Ordinary,deadline).await.unwrap();
                while events.try_next().is_some() {}
                let created = connection.call(None,"Target.createTarget",json!({"url":"about:blank#testrun-storage-probe","browserContextId":context["browserContextId"],"hidden":true,"background":true}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let mut observed = Vec::new();
                while let Some(item) = events.try_next() {
                    if let super::super::EventItem::Event(event) = item {
                        if event.method.starts_with("Target.") { observed.push(json!({"method":event.method,"params":event.params})); }
                    }
                }
                let info = connection.call(None,"Target.getTargetInfo",json!({"targetId":created["targetId"]}),super::super::Lane::Ordinary,deadline).await.unwrap();
                eprintln!("{mode:?} hidden-target reply={created}; pre-reply events={observed:?}; info={info}");
                assert!(created["targetId"].is_string());
                assert_eq!(info["targetInfo"]["targetId"],created["targetId"]);
                assert_eq!(info["targetInfo"]["browserContextId"],context["browserContextId"]);
                assert_eq!(info["targetInfo"]["type"],"other");
                assert_eq!(info["targetInfo"]["attached"],false);
                let attached = connection.call(None,"Target.attachToTarget",json!({"targetId":created["targetId"],"flatten":true}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let attached = attached["sessionId"].as_str().unwrap();
                connection.call(Some(attached),"Page.enable",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                connection.call(Some(attached),"Runtime.enable",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let close_started = Instant::now();
                let close = connection.call(None,"Target.closeTarget",json!({"targetId":created["targetId"]}),super::super::Lane::Ordinary,deadline).await;
                let after = connection.call(None,"Target.getTargets",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let present = after["targetInfos"].as_array().unwrap().iter().any(|info|info["targetId"]==created["targetId"]);
                eprintln!("{mode:?} root-created hidden close={close:?}; stillPresent={present}");
                let observation_end = deadline.min(Instant::now()+Duration::from_secs(2));
                let destroyed = loop {
                    match tokio::time::timeout_at(observation_end,events.next()).await {
                        Ok(Some(super::super::EventItem::Event(event))) if event.method=="Target.targetDestroyed" && event.params["targetId"]==created["targetId"] => break true,
                        Ok(Some(_))=>{},
                        _=>break false,
                    }
                };
                let after_wait = connection.call(None,"Target.getTargets",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let still_present = after_wait["targetInfos"].as_array().unwrap().iter().any(|info|info["targetId"]==created["targetId"]);
                eprintln!("{mode:?} attached root hidden destroyedEvent={destroyed}; stillPresentAfterObservation={still_present}; elapsedMs={}",close_started.elapsed().as_millis());
                let owner = connection.call(None,"Target.attachToBrowserTarget",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let owner = owner["sessionId"].as_str().unwrap();
                let owned = connection.call(Some(owner),"Target.createTarget",json!({"url":"about:blank","browserContextId":context["browserContextId"],"hidden":true,"background":true}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let close = connection.call(None,"Target.closeTarget",json!({"targetId":owned["targetId"]}),super::super::Lane::Ordinary,deadline).await;
                let before_detach = connection.call(None,"Target.getTargets",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let present_before = before_detach["targetInfos"].as_array().unwrap().iter().any(|info|info["targetId"]==owned["targetId"]);
                let detached = connection.call(None,"Target.detachFromTarget",json!({"sessionId":owner}),super::super::Lane::Control,deadline).await;
                let after = connection.call(None,"Target.getTargets",json!({}),super::super::Lane::Ordinary,deadline).await.unwrap();
                let present_after = after["targetInfos"].as_array().unwrap().iter().any(|info|info["targetId"]==owned["targetId"]);
                eprintln!("{mode:?} session-created hidden close={close:?}; presentBeforeDetach={present_before}; detach={detached:?}; presentAfterDetach={present_after}");
                assert!(detached.is_ok());
                assert!(!present_after,"Creator-session release must close its hidden target");
            }).catch_unwind().await;
            let closed = process
                .shutdown(Some(&connection), Instant::now() + Duration::from_secs(30))
                .await;
            assert!(closed.is_ok(), "{closed:?}");
            if let Err(panic) = result {
                std::panic::resume_unwind(panic);
            }
        }
    }
    #[test]
    fn both_launch_modes_keep_credentials_inside_the_disposable_profile() {
        let profile = Path::new("/private/acme/chrome-profile");
        let executable = Path::new("/private/acme/Chrome for Testing");
        for mode in [Mode::Headless, Mode::Headed] {
            let command =
                launch_command(executable, profile, mode, None, &LaunchScope::default()).unwrap();
            let command = command.as_std();
            assert_eq!(command.get_program(), executable);
            assert_eq!(command.get_current_dir(), Some(profile));
            let args = command.get_args().collect::<Vec<_>>();
            assert!(args.contains(&std::ffi::OsStr::new("--use-mock-keychain")));
            assert!(args.contains(&std::ffi::OsStr::new("--password-store=basic")));
            assert!(args.contains(&std::ffi::OsStr::new("--force-color-profile=srgb")));
            assert!(args.contains(&std::ffi::OsStr::new("--enable-features=WebMCP")));
            for flag in [
                "--disable-background-timer-throttling",
                "--disable-backgrounding-occluded-windows",
                "--disable-renderer-backgrounding",
            ] {
                assert!(args.contains(&std::ffi::OsStr::new(flag)));
            }
            assert!(args.contains(&std::ffi::OsStr::new(
                "--user-data-dir=/private/acme/chrome-profile"
            )));
            assert_eq!(
                args.contains(&std::ffi::OsStr::new("--headless=new")),
                mode == Mode::Headless
            );
            let environment = command
                .get_envs()
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(
                environment.get(std::ffi::OsStr::new("HOME")),
                Some(&Some(profile.as_os_str()))
            );
            #[cfg(target_os = "linux")]
            for key in [
                "DISPLAY",
                "WAYLAND_DISPLAY",
                "XAUTHORITY",
                "XDG_RUNTIME_DIR",
            ] {
                let expected = if mode == Mode::Headed {
                    std::env::var_os(key)
                } else {
                    None
                };
                let value = environment.get(std::ffi::OsStr::new(key)).and_then(|v| *v);
                assert_eq!(value, expected.as_deref());
            }
        }
    }
}
