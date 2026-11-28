//! Release-selected source identities; no runtime latest/version resolution.
use super::*;

mod linux;
mod windows;

#[derive(Clone, Copy)]
pub(super) enum Host {
    Macos15X64,
    Ubuntu26X64,
    WindowsX64,
}

impl Host {
    /// The supported bundle host, detected once per process.
    pub(super) fn current() -> Option<Self> {
        static HOST: std::sync::OnceLock<Option<Host>> = std::sync::OnceLock::new();
        *HOST.get_or_init(Self::detect)
    }

    fn detect() -> Option<Self> {
        if super::super::processes::initial_host() {
            return Some(Self::Macos15X64);
        }
        if cfg!(all(windows, target_arch = "x86_64")) {
            return Some(Self::WindowsX64);
        }
        if cfg!(all(target_os = "linux", target_arch = "x86_64"))
            && std::fs::read_to_string("/etc/os-release").is_ok_and(|release| {
                os_release_field(&release, "ID=") == Some("ubuntu")
                    && os_release_field(&release, "VERSION_ID=") == Some("26.04")
            })
        {
            return Some(Self::Ubuntu26X64);
        }
        None
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Macos15X64 => "darwin-x64-macos15",
            Self::Ubuntu26X64 => "linux-x64-ubuntu26.04",
            Self::WindowsX64 => "win32-x64",
        }
    }

    pub(super) const fn node(self) -> &'static Artifact {
        match self {
            Self::Macos15X64 => &NODE,
            Self::Ubuntu26X64 => &linux::NODE,
            Self::WindowsX64 => &windows::NODE,
        }
    }

    pub(super) const fn node_executable_sha256(self) -> &'static str {
        match self {
            Self::Macos15X64 => super::super::contract::NODE_SHA256,
            Self::Ubuntu26X64 => linux::NODE_EXECUTABLE_SHA256,
            Self::WindowsX64 => windows::NODE_EXECUTABLE_SHA256,
        }
    }

    pub(super) const fn chrome(self) -> &'static Artifact {
        match self {
            Self::Macos15X64 => &CHROME,
            Self::Ubuntu26X64 => &linux::CHROME,
            Self::WindowsX64 => &windows::CHROME,
        }
    }

    fn firefox(self) -> &'static Artifact {
        match self {
            Self::Macos15X64 => &FIREFOX,
            Self::Ubuntu26X64 => &linux::FIREFOX,
            Self::WindowsX64 => &windows::FIREFOX,
        }
    }

    fn webkit(self) -> &'static Artifact {
        match self {
            Self::Macos15X64 => &WEBKIT,
            Self::Ubuntu26X64 => &linux::WEBKIT,
            Self::WindowsX64 => &windows::WEBKIT,
        }
    }
}

fn os_release_field<'a>(release: &'a str, key: &str) -> Option<&'a str> {
    release
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .map(|value| value.trim_matches('"'))
}

pub(super) struct Artifact {
    pub id: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub directory: &'static str,
    pub entry: &'static str,
    pub zip: bool,
    pub license: &'static str,
}
pub(super) static NODE: Artifact = Artifact {
    id: "node-24.12.0-darwin-x64",
    url: "https://nodejs.org/dist/v24.12.0/node-v24.12.0-darwin-x64.tar.gz",
    sha256: "b82ea4c62fd08e250cab59d625e75d77cc5b0a3d60c6698ebee4545c88a169c5",
    bytes: 52376978,
    directory: "node-24.12.0",
    entry: "node-v24.12.0-darwin-x64/bin/node",
    zip: false,
    license: "node-v24.12.0-darwin-x64/LICENSE",
};
pub(super) static CORE: Artifact = Artifact {
    id: "playwright-core-1.64.1",
    url: "https://registry.npmjs.org/playwright-core/-/playwright-core-1.64.1.tgz",
    sha256: "3af03703c967c95037454f2283f709d53ce804a4776d4219d319f5bbcc2a0185",
    bytes: 3172161,
    directory: "playwright-core-1.64.1",
    entry: "package/cli.js",
    zip: false,
    license: "package/LICENSE",
};
static FIREFOX: Artifact = Artifact {
    id: "firefox-1555-mac",
    url: "https://cdn.playwright.dev/dbazure/download/playwright/builds/firefox/1555/firefox-mac.zip",
    sha256: "ae2c6e477d4c5f59e76be39ca0db6c003e9bcf9bb9b3c4e13cf545275d6be388",
    bytes: 118829566,
    directory: "browsers/firefox-1555",
    entry: "firefox/Nightly.app/Contents/MacOS/firefox",
    zip: true,
    license: "about:license (inside the Firefox distribution); https://www.mozilla.org/MPL/2.0/",
};
static WEBKIT: Artifact = Artifact {
    id: "webkit-2370-mac15",
    url: "https://cdn.playwright.dev/dbazure/download/playwright/builds/webkit/2370/webkit-mac-15.zip",
    sha256: "2b43c865db2c3a408736ff4a822f060570a6e3e52df7ed25d2ed5bc231be7e41",
    bytes: 88587279,
    directory: "browsers/webkit-2370",
    entry: "pw_run.sh",
    zip: true,
    license: "https://webkit.org/licensing-webkit/; bundled WebInspectorUI.framework resources include third-party licenses",
};
pub(super) static CHROME: Artifact = Artifact {
    id: "chrome-156.0.8078.47-mac-x64",
    url: "https://storage.googleapis.com/chrome-for-testing-public/156.0.8078.47/mac-x64/chrome-mac-x64.zip",
    sha256: "5a38b8e3d621514fff0775dcae678a152ce378890188c88eef79fc97aa59fccd",
    bytes: 205405847,
    directory: "browsers/chrome-156.0.8078.47",
    entry: "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    zip: true,
    license: "https://www.google.com/intl/en/chrome/terms/; bundled chrome-mac-x64/ABOUT",
};
pub(super) fn selected(host: Host, engine: Engine) -> Result<Vec<&'static Artifact>> {
    let browser = match engine {
        Engine::Firefox => host.firefox(),
        Engine::Webkit => host.webkit(),
        Engine::Chrome => return Ok(vec![host.chrome()]),
        Engine::Chromium => {
            return Err(Error::new(
                "BACKEND_UNAVAILABLE",
                "Select chrome for the pinned Chrome for Testing build",
            ));
        }
    };
    let mut artifacts = vec![host.node(), &CORE, browser];
    if matches!(host, Host::WindowsX64) {
        artifacts.push(&windows::WINLDD);
    }
    Ok(artifacts)
}

pub(super) fn has_dependency_marker(artifact: &Artifact) -> bool {
    [
        FIREFOX.id,
        WEBKIT.id,
        linux::FIREFOX.id,
        linux::WEBKIT.id,
        windows::FIREFOX.id,
        windows::WEBKIT.id,
    ]
    .contains(&artifact.id)
}

pub(super) fn external_extensions_directory(artifact: &Artifact) -> Option<&'static str> {
    (artifact.id == linux::CHROME.id).then_some("chrome-linux64/extensions")
}
