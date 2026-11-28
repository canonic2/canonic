//! Pinned private runtime acquisition; discovery never downloads or executes.

use super::{contract::Engine, driver::DiagnosticBundle};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::Instant;

mod archive;
mod channels;
mod manifest;
#[cfg(test)]
mod tests;
#[cfg(windows)]
pub(super) mod windows;
pub(crate) use channels::Channel;
pub(super) use channels::version_major;
use manifest::{Artifact, Host};

#[cfg(target_os = "linux")]
pub(super) fn ubuntu_bundle_host() -> bool {
    matches!(Host::current(), Some(Host::Ubuntu26X64))
}

pub(crate) fn root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("TESTRUN_BROWSER_ROOT") {
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            return Err(Error::invalid("TESTRUN_BROWSER_ROOT must be absolute"));
        }
        return Ok(root);
    }
    let host = Host::current().unwrap_or(Host::Macos15X64);
    let root = if cfg!(windows) {
        dirs::data_local_dir().map(|path| path.join("Testrun/browser-bundles"))
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".local/share/testrun/browser-bundles"))
    };
    root.map(|root| root.join(format!("v1-{}", host.label())))
        .ok_or_else(|| Error::new("HOST_IO", "Cannot locate the private browser cache"))
}

pub(super) fn host_capabilities(engine: Engine) -> (bool, Value) {
    let macos = super::processes::initial_host();
    let engine_bundled = engine.revision().is_some() || engine == Engine::Chrome;
    let windows_supported = super::processes::windows_bundle_host() && engine_bundled;
    let supported = (macos && engine_bundled) || windows_supported;
    let mut capabilities = super::contract::capabilities(engine, macos && engine_bundled);
    if windows_supported {
        capabilities["host"] = json!(Host::WindowsX64.label());
        let mut modes = vec!["headless"];
        if super::processes::windows_mode_supported(super::contract::Mode::Headed) {
            modes.push("headed");
        }
        capabilities["lifecycle"] = json!({"available":true,"scope":"owned-launch","modes":modes,"acceptance":"fixture-only"});
        capabilities["connectionModes"]["profiles"]
            .as_array_mut()
            .expect("Browser profile capability list")
            .insert(0, json!("isolated"));
        capabilities["persistent"] = json!(true);
        capabilities["remoteAttachment"] = json!(true);
        capabilities["pageInteraction"] = json!(true);
        capabilities["streaming"] = json!(true);
        capabilities["preview"]["available"] = json!(true);
        capabilities["preview"]["acceptance"] = json!("pending-installed-windows");
        capabilities["inputScope"]["viewerControl"] = json!(true);
        capabilities["sharedApi"]["viewerControl"] = json!(true);
        #[cfg(windows)]
        {
            let available = super::recording::available();
            capabilities["recording"] = json!(available);
            capabilities["recordingScope"]["available"] = json!(available);
            capabilities["recordingScope"]["backend"] = json!("windows-media-foundation");
            capabilities["recordingScope"]["acceptance"] = json!("pending-installed-windows");
        }
        if engine == Engine::Chrome {
            capabilities["pageReading"]["available"] = json!(true);
            capabilities["navigation"]["available"] = json!(true);
        }
    }
    (supported, capabilities)
}

pub(crate) fn doctor(engine: Engine) -> Result<Value> {
    let (supported, capabilities) = host_capabilities(engine);
    let root = root()?;
    let artifacts = manifest::selected(Host::current().unwrap_or(Host::Macos15X64), engine)?;
    let missing: Vec<_> = artifacts
        .iter()
        .filter(|artifact| {
            !root
                .join(artifact.directory)
                .join("verified.json")
                .is_file()
        })
        .collect();
    let mut result = json!({"engine":engine,"backend":engine.backend(),"hostSupported":supported,
        "state":if !supported{"unsupported_host"} else if missing.is_empty(){"prepared_unchecked"}else{"not_prepared"},
        "missing":missing.iter().map(|a|json!({"id":a.id,"bytes":a.bytes,"url":a.url})).collect::<Vec<_>>(),
        "downloadBytes":missing.iter().map(|a|a.bytes).sum::<u64>(),
        "capabilities":capabilities});
    if engine == Engine::Chrome {
        let installed = super::processes::initial_host() || super::processes::windows_bundle_host();
        result["installedChannels"] = json!(Channel::ALL.map(|channel| channel.status(installed)));
    }
    Ok(result)
}

pub(crate) async fn prepare(engine: Engine, progress: impl Fn(&str)) -> Result<DiagnosticBundle> {
    prepare_in(&root()?, engine, progress).await
}

pub(super) fn matches_node_identity(bundle: &DiagnosticBundle) -> bool {
    Host::current()
        .is_some_and(|host| bundle.identity().node_sha256 == host.node_executable_sha256())
}

/// A verified private Chrome executable. Construction is restricted to the
/// acquisition owner so ambient installed browsers cannot become launch input.
#[derive(Clone)]
pub(crate) struct ChromeBundle {
    pub(super) executable: PathBuf,
    pub(super) installed: Option<channels::Installed>,
}

pub(crate) const CHROME_VERSION: &str = "156.0.8078.47";

#[cfg(feature = "acceptance-browser-public")]
#[doc(hidden)]
pub fn checkpoint_artifacts(root: &Path) -> [PathBuf; 3] {
    let host = Host::current().expect("Checkpoint runs on a pinned browser bundle host");
    let node = host.node();
    let chrome = host.chrome();
    [
        root.join(node.directory).join(node.entry),
        root.join(manifest::CORE.directory).join("package"),
        root.join(chrome.directory).join(chrome.entry),
    ]
}

pub(crate) fn chrome_identity() -> Value {
    let Some(host) = Host::current() else {
        return Value::Null;
    };
    let chrome = host.chrome();
    json!({"version":CHROME_VERSION,"archiveSha256":chrome.sha256,"archiveBytes":chrome.bytes,
        "source":chrome.url,"host":host.label(),"artifact":chrome.id})
}

pub(crate) async fn prepare_chrome(progress: impl Fn(&str)) -> Result<ChromeBundle> {
    let host = bundle_host()?;
    let chrome = host.chrome();
    let root = root()?;
    private_tree(&root)?;
    progress(chrome.id);
    install(&root, chrome).await?;
    Ok(ChromeBundle {
        executable: root.join(chrome.directory).join(chrome.entry),
        installed: None,
    })
}

async fn prepare_in(
    root: &Path,
    engine: Engine,
    progress: impl Fn(&str),
) -> Result<DiagnosticBundle> {
    if engine.backend() != super::contract::Backend::Playwright {
        return Err(Error::new(
            "BACKEND_UNAVAILABLE",
            "Select native Chrome preparation for this engine",
        ));
    }
    let host = bundle_host()?;
    let node = host.node();
    let artifacts = manifest::selected(host, engine)?;
    private_tree(root)?;
    for artifact in artifacts {
        progress(artifact.id);
        install(root, artifact).await?;
    }
    DiagnosticBundle::inspect(
        &root.join(node.directory).join(node.entry),
        &root.join(manifest::CORE.directory).join("package"),
        &root.join("browsers"),
        Instant::now() + Duration::from_secs(30),
    )
    .await
    .map_err(|error| {
        Error::new(
            "BUNDLE_INVALID",
            "The prepared driver does not match its pinned identity",
        )
        .details(json!({"cause":error.to_string()}))
    })
}

fn bundle_host() -> Result<Host> {
    Host::current().ok_or_else(|| {
        Error::new(
            "HOST_UNAVAILABLE",
            "No pinned browser distribution for this host",
        )
    })
}

#[cfg(windows)]
pub(super) fn private_tree(path: &Path) -> Result<()> {
    canonic_process_kit::endpoint::prepare_directory(path)
        .map(drop)
        .map_err(|_| Error::new("BUNDLE_IO", "Browser cache must be private to this user"))
}

#[cfg(unix)]
pub(super) fn private_tree(path: &Path) -> Result<()> {
    if path.exists() {
        return crate::artifacts::check_private_directory(path);
    }
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        private_tree(parent)?;
    }
    match crate::artifacts::directory(path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => crate::artifacts::check_private_directory(path),
        Err(error) => Err(error),
    }
}

async fn install(root: &Path, artifact: &'static Artifact) -> Result<()> {
    install_with(root, artifact, download).await
}

async fn download(path: PathBuf, artifact: &'static Artifact) -> Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| Error::invalid("Private cache path is not UTF-8"))?;
    #[cfg(unix)]
    let curl = Path::new("/usr/bin/curl");
    #[cfg(windows)]
    let curl = windows::system_directory()?.join("curl.exe");
    let arguments = [
        #[cfg(windows)]
        "--disable",
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-time",
        "240",
        "--max-filesize",
        &artifact.bytes.to_string(),
        "--output",
        path,
        artifact.url,
    ];
    crate::process::run(curl,&arguments,Duration::from_secs(245),8192).await
        .map(|_| ())
        .map_err(|cause|Error::new("BROWSER_DOWNLOAD_FAILED","Cannot download the pinned browser dependency; inspect the host network, proxy and disk space").details(json!({"artifact":artifact.id,"cause":cause})))
}

/// Tests replace only the byte-acquisition boundary; locking, checksum,
/// extraction, publication and offline verification remain the production path.
async fn install_with<F, Fut>(root: &Path, artifact: &'static Artifact, download: F) -> Result<()>
where
    F: FnOnce(PathBuf, &'static Artifact) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let deadline = Instant::now() + Duration::from_secs(600);
    let _lock = loop {
        #[cfg(unix)]
        let locked = crate::lease::ControlLease::acquire(&format!(
            "browser-install:{}:{}",
            root.display(),
            artifact.id
        ));
        #[cfg(windows)]
        let locked = windows::lock(root, artifact.id);
        match locked {
            Ok(lock) => break lock,
            Err(error) if error.code == "TARGET_BUSY" && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            Err(error) => return Err(error),
        }
    };
    let destination = root.join(artifact.directory);
    if destination.exists() {
        let path = destination.clone();
        return tokio::task::spawn_blocking(move || archive::verify_install(&path, artifact))
            .await
            .map_err(|_| Error::new("BUNDLE_INVALID", "Private browser validation stopped"))?;
    }
    let staging = root.join(format!(".{}-{}.partial", artifact.id, uuid::Uuid::new_v4()));
    private_tree(&staging)?;
    let guard = Staging(staging);
    let downloaded = guard.0.join("archive");
    download(downloaded.clone(), artifact).await?;
    // The blocking worker owns staging and the installation lock across caller
    // cancellation; a second extractor cannot publish over unfinished work.
    tokio::task::spawn_blocking(move || {
        let _lock = _lock;
        let guard = guard;
        archive::verify_download(&downloaded, artifact)?;
        let extracted = guard.0.join("contents");
        private_tree(&extracted)?;
        archive::extract(&downloaded, &extracted, artifact.zip)?;
        archive::seal(&extracted, artifact)?;
        let parent = destination
            .parent()
            .ok_or_else(|| Error::invalid("Missing cache parent"))?;
        private_tree(parent)?;
        #[cfg(windows)]
        crate::artifacts::windows::publish(&extracted, &destination, false)
            .map_err(|error| Error::new("BUNDLE_PUBLISH_FAILED", error.to_string()))?;
        #[cfg(unix)]
        {
            fs::rename(&extracted, &destination)
                .map_err(|error| Error::new("BUNDLE_PUBLISH_FAILED", error.to_string()))?;
            crate::artifacts::sync(&fs::File::open(parent).map_err(archive::io)?)
                .map_err(archive::io)?;
            crate::artifacts::sync(
                &fs::File::open(guard.0.parent().expect("Browser cache parent"))
                    .map_err(archive::io)?,
            )
            .map_err(archive::io)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| {
        Error::new(
            "BUNDLE_PUBLISH_FAILED",
            "Private browser publication stopped",
        )
    })?
}

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
