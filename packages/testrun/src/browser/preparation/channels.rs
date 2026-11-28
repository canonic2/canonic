//! Installed Stable channel discovery from fixed platform paths and metadata.

use super::{CHROME_VERSION, ChromeBundle};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(any(not(windows), test))]
use std::{fs, io::Read, path::Path};

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Channel {
    Chrome,
    Msedge,
}

#[derive(Clone)]
pub(in crate::browser) struct Installed {
    pub channel: Channel,
    pub version: String,
}

impl Channel {
    pub(super) const ALL: [Self; 2] = [Self::Chrome, Self::Msedge];

    pub(in crate::browser) fn product(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Msedge => "Microsoft Edge",
        }
    }

    #[cfg(any(not(windows), test))]
    fn bundle_id(self) -> &'static str {
        match self {
            Self::Chrome => "com.google.Chrome",
            Self::Msedge => "com.microsoft.edgemac",
        }
    }

    pub(super) fn status(self, supported: bool) -> Value {
        let mut value = json!({"channel":self,"product":self.product()});
        if supported {
            match self.inspect_host() {
                Ok((_, version)) => {
                    value["version"] = json!(version);
                    value["state"] = json!(if compatible(&version) {
                        "installed_unverified"
                    } else {
                        "build_unsupported"
                    });
                }
                Err(error) => value["state"] = json!(error.code),
            }
        } else {
            value["state"] = json!("unsupported_host");
        }
        value
    }

    pub(crate) fn select(self) -> Result<ChromeBundle> {
        let (executable, version) = self.inspect_host()?;
        if !compatible(&version) {
            return Err(Error::new(
                "BROWSER_CHANNEL_BUILD_UNSUPPORTED",
                format!(
                    "{} {version} predates the pinned CDP build family",
                    self.product()
                ),
            ));
        }
        Ok(ChromeBundle {
            executable,
            installed: Some(Installed {
                channel: self,
                version,
            }),
        })
    }

    fn inspect_host(self) -> Result<(std::path::PathBuf, String)> {
        #[cfg(windows)]
        return windows::inspect(self);
        #[cfg(not(windows))]
        if cfg!(target_os = "linux") {
            self.inspect_linux(Path::new("/var/lib/dpkg/status"), Path::new("/opt"))
        } else {
            self.inspect(Path::new("/Applications"))
        }
    }

    #[cfg(any(not(windows), test))]
    fn inspect_linux(self, packages: &Path, opt: &Path) -> Result<(std::path::PathBuf, String)> {
        let (package, binary) = match self {
            Self::Chrome => ("google-chrome-stable", "google/chrome/chrome"),
            Self::Msedge => ("microsoft-edge-stable", "microsoft/msedge/msedge"),
        };
        let unavailable = || {
            Error::new(
                "BROWSER_CHANNEL_UNAVAILABLE",
                format!("{} Stable is unavailable", self.product()),
            )
        };
        let invalid = || {
            Error::new(
                "BROWSER_CHANNEL_INVALID",
                format!("{} Stable package identity is invalid", self.product()),
            )
        };
        const MAX_BYTES: usize = 16 * 1024 * 1024;
        let status = fs::File::open(packages).map_err(|_| unavailable())?;
        let mut contents = String::new();
        status
            .take(MAX_BYTES as u64 + 1)
            .read_to_string(&mut contents)
            .map_err(|_| invalid())?;
        if contents.len() > MAX_BYTES {
            return Err(invalid());
        }
        let record = contents
            .split("\n\n")
            .find(|record| package_field(record, "Package: ") == Some(package))
            .ok_or_else(unavailable)?;
        if package_field(record, "Status: ") != Some("install ok installed") {
            return Err(unavailable());
        }
        if package_field(record, "Architecture: ") != Some("amd64") {
            return Err(invalid());
        }
        let version = package_field(record, "Version: ").ok_or_else(invalid)?;
        // Stable packages put the Debian packaging revision after the browser version.
        let version = version
            .split_once('-')
            .map_or(version, |(browser, _)| browser);
        if version_major(version).is_none() {
            return Err(invalid());
        }
        let executable = opt.join(binary);
        if !executable.is_file() {
            return Err(unavailable());
        }
        Ok((executable, version.into()))
    }

    #[cfg(any(not(windows), test))]
    fn inspect(self, applications: &Path) -> Result<(std::path::PathBuf, String)> {
        let product = self.product();
        let bundle = applications.join(format!("{product}.app/Contents"));
        let unavailable = || {
            Error::new(
                "BROWSER_CHANNEL_UNAVAILABLE",
                format!("{product} Stable is unavailable"),
            )
        };
        let invalid = || {
            Error::new(
                "BROWSER_CHANNEL_INVALID",
                format!("{product} Stable bundle identity is invalid"),
            )
        };
        let plist = fs::File::open(bundle.join("Info.plist")).map_err(|_| unavailable())?;
        if !plist.metadata().is_ok_and(|meta| meta.is_file()) {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        plist
            .take(128 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        let info = plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_| invalid())?;
        let get = |key| info.as_dictionary()?.get(key)?.as_string();
        if get("CFBundleIdentifier") != Some(self.bundle_id())
            || get("CFBundleExecutable") != Some(product)
        {
            return Err(invalid());
        }
        let version = get("CFBundleShortVersionString")
            .filter(|version| version_major(version).is_some())
            .ok_or_else(invalid)?;
        let executable = bundle.join("MacOS").join(product);
        if !executable.is_file() {
            return Err(unavailable());
        }
        Ok((executable, version.into()))
    }
}

#[cfg(any(not(windows), test))]
fn package_field<'a>(record: &'a str, key: &str) -> Option<&'a str> {
    record.lines().find_map(|line| line.strip_prefix(key))
}

pub(in crate::browser) fn version_major(version: &str) -> Option<u32> {
    if version.len() > 64
        || version.split('.').count() != 4
        || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return None;
    }
    let mut parts = version.split('.').map(str::parse::<u32>);
    let major = parts.next()?.ok()?;
    parts.all(|part| part.is_ok()).then_some(major)
}

fn compatible(version: &str) -> bool {
    version_major(version)
        .is_some_and(|major| major >= version_major(CHROME_VERSION).expect("Pinned version"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_stable_discovery_requires_an_installed_amd64_package() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("google/chrome/chrome");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, []).unwrap();
        let packages = root.path().join("status");
        for (record, code) in [
            (
                "Package: google-chrome-beta\nStatus: install ok installed\nArchitecture: amd64\nVersion: 158.0.0.4-1\n",
                "BROWSER_CHANNEL_UNAVAILABLE",
            ),
            (
                "Package: google-chrome-stable\nStatus: deinstall ok config-files\nArchitecture: amd64\nVersion: 158.0.0.4-1\n",
                "BROWSER_CHANNEL_UNAVAILABLE",
            ),
            (
                "Package: google-chrome-stable\nStatus: install ok installed\nArchitecture: arm64\nVersion: 158.0.0.4-1\n",
                "BROWSER_CHANNEL_INVALID",
            ),
        ] {
            fs::write(&packages, record).unwrap();
            assert_eq!(
                Channel::Chrome
                    .inspect_linux(&packages, root.path())
                    .unwrap_err()
                    .code,
                code
            );
        }
        fs::write(&packages, "Package: google-chrome-stable\nStatus: install ok installed\nArchitecture: amd64\nVersion: 158.0.0.4-1\n").unwrap();
        assert_eq!(
            Channel::Chrome
                .inspect_linux(&packages, root.path())
                .unwrap(),
            (binary, "158.0.0.4".into())
        );
    }

    #[test]
    fn stable_discovery_requires_exact_bundle_identity_and_compatible_version() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("Microsoft Edge.app/Contents");
        fs::create_dir_all(app.join("MacOS")).unwrap();
        fs::write(app.join("MacOS/Microsoft Edge"), []).unwrap();
        let info = app.join("Info.plist");
        for (id, version, expected) in [
            ("com.microsoft.edgemac.Beta", "157.0.0.0", None),
            ("com.microsoft.edgemac", "128.0.2739.79", Some(false)),
            ("com.microsoft.edgemac", "157.0.0.1", Some(true)),
        ] {
            plist::to_file_xml(&info, &json!({"CFBundleIdentifier":id,"CFBundleExecutable":"Microsoft Edge","CFBundleShortVersionString":version})).unwrap();
            assert_eq!(
                Channel::Msedge
                    .inspect(root.path())
                    .ok()
                    .map(|(_, version)| compatible(&version)),
                expected
            );
        }
        assert!(Channel::Chrome.inspect(root.path()).is_err());
    }

    #[test]
    fn public_connect_accepts_channels_only_as_native_chrome_launch_selection() {
        use crate::browser::contract::{ConnectOptions, Mode, schemas};
        for (input, expected) in [
            (json!({"engine":"chrome","channel":"msedge"}), Ok(())),
            (
                json!({"engine":"firefox","channel":"chrome"}),
                Err("BROWSER_CHANNEL_UNSUPPORTED"),
            ),
            (json!({"engine":"webkit"}), Ok(())),
            (
                json!({"engine":"chrome","profile":{"kind":"disposable"}}),
                if cfg!(unix) || crate::browser::processes::windows_bundle_host() {
                    Ok(())
                } else {
                    Err("DISPOSABLE_PROFILE_UNSUPPORTED")
                },
            ),
            (
                json!({"engine":"firefox","profile":{"kind":"disposable"}}),
                Err("DISPOSABLE_PROFILE_UNSUPPORTED"),
            ),
            (
                json!({"engine":"chrome","channel":"chrome","profile":{"kind":"disposable"}}),
                Err("DISPOSABLE_PROFILE_UNSUPPORTED"),
            ),
        ] {
            crate::schema::validate(&schemas()["launch"], &input).unwrap();
            let options: ConnectOptions = serde_json::from_value(input).unwrap();
            assert_eq!(options.launch.mode, Mode::Headless);
            assert_eq!(options.validate(), expected);
        }
        for invalid in [
            json!({"engine":"chrome","channel":"chrome-beta"}),
            json!({"engine":"chrome","executablePath":"/Applications/Acme"}),
        ] {
            assert!(crate::schema::validate(&schemas()["launch"], &invalid).is_err());
            assert!(serde_json::from_value::<ConnectOptions>(invalid).is_err());
        }
    }
}
