//! Verified signed XCTest products, serialized per build key and retained across sessions.
use super::{
    preparation::{RUNNER_BUNDLE, StepLogs, utf8},
    signing,
};
use crate::{
    artifacts,
    error::{Error, Result},
    lease::ControlLease,
    process,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub(super) struct Products {
    pub(super) app: PathBuf,
    pub(super) descriptor: PathBuf,
    pub(super) signing: Value,
    pub(super) lease: ControlLease,
}

/// Hash file names, bytes and symlink targets, without traversing linked directories.
fn digest(path: &Path) -> Result<String> {
    fn walk(root: &Path, path: &Path, hash: &mut Sha256) -> Result<()> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|_| Error::new("RUNNER_CACHE_INVALID", "Cannot inspect runner inputs"))?;
        let name = path
            .strip_prefix(root)
            .map_err(|_| Error::invalid("Runner input escaped its root"))?
            .as_os_str()
            .as_encoded_bytes();
        hash.update((name.len() as u64).to_be_bytes());
        hash.update(name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hash.update(metadata.permissions().mode().to_be_bytes());
        }
        if metadata.is_symlink() {
            hash.update(b"link");
            let target = std::fs::read_link(path)
                .map_err(|_| Error::new("RUNNER_CACHE_INVALID", "Cannot read runner link"))?;
            hash.update(target.as_os_str().as_encoded_bytes());
        } else if metadata.is_dir() {
            hash.update(b"directory");
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .map_err(|_| Error::new("RUNNER_CACHE_INVALID", "Cannot enumerate runner inputs"))?
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|_| {
                    Error::new("RUNNER_CACHE_INVALID", "Cannot enumerate runner inputs")
                })?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                walk(root, &entry.path(), hash)?;
            }
        } else if metadata.is_file() {
            use std::io::Read;
            hash.update(b"file");
            hash.update(metadata.len().to_be_bytes());
            let mut file = std::fs::File::open(path)
                .map_err(|_| Error::new("RUNNER_CACHE_INVALID", "Cannot read runner input"))?;
            let mut bytes = [0; 64 * 1024];
            loop {
                let count = file
                    .read(&mut bytes)
                    .map_err(|_| Error::new("RUNNER_CACHE_INVALID", "Cannot hash runner input"))?;
                if count == 0 {
                    break;
                }
                hash.update(&bytes[..count]);
            }
        } else {
            return Err(Error::new(
                "RUNNER_CACHE_INVALID",
                "Runner inputs contain a special file",
            ));
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    walk(path, path, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn products(entry: &Path) -> Result<(PathBuf, PathBuf)> {
    let root = entry.join("build/Build/Products");
    let app = root.join("Release-iphoneos/TestrunXCTest-Runner.app");
    let mut descriptors: Vec<_> = std::fs::read_dir(&root)
        .map_err(|_| Error::new("RUNNER_CACHE_INVALID", "Runner products are missing"))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|s| s == "xctestrun"))
        .collect();
    if !app.is_dir() || descriptors.len() != 1 {
        return Err(Error::new(
            "RUNNER_CACHE_INVALID",
            "Expected one runner app and test descriptor",
        ));
    }
    Ok((app, descriptors.remove(0)))
}

async fn verify(entry: &Path, team: &str, serial: &str, logs: &StepLogs) -> Result<Value> {
    let (app, _) = products(entry)?;
    let info: plist::Value = plist::from_file(app.join("Info.plist")).map_err(|_| {
        Error::new(
            "RUNNER_CACHE_INVALID",
            "Cached runner app metadata is invalid",
        )
    })?;
    if info
        .as_dictionary()
        .and_then(|info| info.get("CFBundleIdentifier"))
        .and_then(plist::Value::as_string)
        != Some(RUNNER_BUNDLE)
    {
        return Err(Error::new(
            "RUNNER_CACHE_INVALID",
            "Cached app is not the owned Testrun runner",
        ));
    }
    logs.run("verify", "/usr/bin/codesign", &["--verify", "--deep", "--strict", utf8(&app)?], Duration::from_secs(20)).await
        .map_err(|cause|Error::new("RUNNER_SIGNATURE_INVALID", "The signed runner failed code-signature verification; rebuild it with a valid development certificate").details(json!({"cause":cause})))?;
    let profile = process::run(
        "/usr/bin/security",
        &[
            "cms",
            "-D",
            "-i",
            utf8(&app.join("embedded.mobileprovision"))?,
        ],
        Duration::from_secs(10),
        4 * 1024 * 1024,
    )
    .await
    .map_err(|_| {
        Error::new(
            "RUNNER_PROFILE_INVALID",
            "Cannot decode the signed runner's provisioning profile",
        )
    })?;
    let profile: plist::Value = plist::from_bytes(&profile.bytes).map_err(|_| {
        Error::new(
            "RUNNER_PROFILE_INVALID",
            "Runner provisioning profile is malformed",
        )
    })?;
    let report = signing::profile(&profile, team, serial, RUNNER_BUNDLE, SystemTime::now())?;
    let certificate = entry.join(format!("certificate-{}-", uuid::Uuid::new_v4()));
    logs.run(
        "verify",
        "/usr/bin/codesign",
        &[
            "--display",
            &format!("--extract-certificates={}", utf8(&certificate)?),
            utf8(&app)?,
        ],
        Duration::from_secs(10),
    )
    .await?;
    let leaf = PathBuf::from(format!("{}0", utf8(&certificate)?));
    let result = std::fs::read(&leaf).ok().is_some_and(|leaf| {
        profile
            .as_dictionary()
            .and_then(|p| p.get("DeveloperCertificates"))
            .and_then(plist::Value::as_array)
            .is_some_and(|certificates| {
                certificates
                    .iter()
                    .any(|certificate| certificate.as_data() == Some(leaf.as_slice()))
            })
    });
    // codesign writes a bounded certificate chain with this unique prefix.
    for index in 0..16 {
        let _ = std::fs::remove_file(format!("{}{index}", certificate.display()));
    }
    if !result {
        return Err(Error::new(
            "RUNNER_SIGNATURE_INVALID",
            "Runner signing certificate is not authorized by its provisioning profile",
        ));
    }
    Ok(report)
}

pub(super) async fn acquire(
    source: &Path,
    serial: &str,
    team: &str,
    logs: &StepLogs,
) -> Result<Products> {
    let xcode = process::run(
        "/usr/bin/xcodebuild",
        &["-version"],
        Duration::from_secs(10),
        4096,
    )
    .await?;
    let sdk = process::run(
        "/usr/bin/xcrun",
        &["--sdk", "iphoneos", "--show-sdk-path"],
        Duration::from_secs(10),
        4096,
    )
    .await?;
    let sdk_build = process::run(
        "/usr/bin/xcrun",
        &["--sdk", "iphoneos", "--show-sdk-build-version"],
        Duration::from_secs(10),
        4096,
    )
    .await?;
    let identity = process::run(
        "/usr/bin/security",
        &["find-identity", "-v", "-p", "codesigning"],
        Duration::from_secs(10),
        64 * 1024,
    )
    .await?;
    let key = key(
        &digest(source)?,
        serial,
        team,
        &[&xcode.bytes, &sdk.bytes, &sdk_build.bytes, &identity.bytes],
    );
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::new("HOST_IO", "Cannot locate runner cache"))?;
    let root = PathBuf::from(home).join(".local/share/testrun/signed-runners");
    acquire_at(
        &root,
        &key,
        team,
        async |entry: &Path| build(entry, source, serial, team, logs).await,
        async |entry: &Path| verify(entry, team, serial, logs).await,
    )
    .await
}

async fn acquire_at(
    root: &Path,
    key: &str,
    team: &str,
    build: impl AsyncFn(&Path) -> Result<()>,
    verify: impl AsyncFn(&Path) -> Result<Value>,
) -> Result<Products> {
    std::fs::create_dir_all(root.parent().unwrap())
        .map_err(|_| Error::new("HOST_IO", "Cannot create runner cache parent"))?;
    if !root.exists() {
        artifacts::directory(root)?;
    }
    artifacts::check_private_directory(root)?;
    let lease = ControlLease::acquire(&format!("signed-runner-cache:{key}"))?;
    let entry = root.join(key);
    let mut invalidation = None;
    if entry.exists() {
        artifacts::check_private_directory(&entry)?;
        let cached = async {
            let record: Value =
                serde_json::from_slice(&std::fs::read(entry.join("verified.json")).map_err(
                    |_| Error::new("RUNNER_CACHE_INVALID", "No completed runner cache record"),
                )?)
                .map_err(|_| Error::new("RUNNER_CACHE_INVALID", "Malformed runner cache record"))?;
            if record["key"] != key
                || record["digest"] != digest(&entry.join("build/Build/Products"))?
            {
                return Err(Error::new(
                    "RUNNER_CACHE_INVALID",
                    "Cached runner products changed",
                ));
            }
            verify(&entry).await
        }
        .await;
        match cached {
            Ok(mut signing) => {
                signing["runnerTeamId"] = json!(team);
                signing["cacheReused"] = json!(true);
                let (app, descriptor) = products(&entry)?;
                return Ok(Products {
                    app,
                    descriptor,
                    signing,
                    lease,
                });
            }
            Err(error) => {
                invalidation = Some(error.code);
            }
        }
        std::fs::remove_dir_all(&entry)
            .map_err(|_| Error::new("HOST_IO", "Cannot replace invalid runner cache"))?;
    }
    artifacts::directory(&entry)?;
    if let Err(mut error) = build(&entry).await {
        let mut details = error.details.take().unwrap_or_default();
        details["cacheInvalidation"] = json!(invalidation);
        details["warning"] = json!(signing::PERSONAL_WARNING);
        return Err(error.details(details));
    }
    let mut signing = verify(&entry).await?;
    artifacts::write(
        &entry.join("verified.json"),
        &serde_json::to_vec(
            &json!({"key":key,"digest":digest(&entry.join("build/Build/Products"))?}),
        )
        .map_err(|_| Error::new("INTERNAL", "Cannot encode runner cache record"))?,
    )?;
    signing["runnerTeamId"] = json!(team);
    signing["cacheReused"] = json!(false);
    signing["cacheInvalidation"] = json!(invalidation);
    let (app, descriptor) = products(&entry)?;
    Ok(Products {
        app,
        descriptor,
        signing,
        lease,
    })
}

async fn build(
    entry: &Path,
    source: &Path,
    serial: &str,
    team: &str,
    logs: &StepLogs,
) -> Result<()> {
    let arguments = build_arguments(
        utf8(&source.join("TestrunRunner.xcodeproj"))?,
        utf8(&entry.join("build"))?,
        serial,
        team,
    );
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    logs.run(
        "build",
        "/usr/bin/xcodebuild",
        &arguments,
        Duration::from_secs(180),
    )
    .await
}

/// `xcodebuild` arguments for one signed runner build.
fn build_arguments(project: &str, derived_data: &str, serial: &str, team: &str) -> Vec<String> {
    [
        "build-for-testing",
        "-project",
        project,
        "-scheme",
        "TestrunRunner",
        "-configuration",
        "Release",
        "-sdk",
        "iphoneos",
        "-destination",
        &format!("id={serial}"),
        "-derivedDataPath",
        derived_data,
        // Let Xcode register a new phone with the team and refresh the
        // profile; without these, a phone missing from the team's profile
        // fails the build.
        "-allowProvisioningUpdates",
        "-allowProvisioningDeviceRegistration",
        &format!("DEVELOPMENT_TEAM={team}"),
    ]
    .map(str::to_owned)
    .to_vec()
}

fn key(source: &str, serial: &str, team: &str, toolchain: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for input in [
        b"testrun-signed-runner-v1:Release:iphoneos".as_slice(),
        source.as_bytes(),
        serial.as_bytes(),
        team.as_bytes(),
    ]
    .into_iter()
    .chain(toolchain.iter().copied())
    {
        hash.update((input.len() as u64).to_be_bytes());
        hash.update(input);
    }
    format!("{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runner_build_lets_xcode_register_the_phone_with_the_team() {
        let arguments = build_arguments("Acme.xcodeproj", "build", "acme-phone", "ACMETEAM");
        for expected in [
            "-allowProvisioningUpdates",
            "-allowProvisioningDeviceRegistration",
            "id=acme-phone",
            "DEVELOPMENT_TEAM=ACMETEAM",
        ] {
            assert!(arguments.iter().any(|a| a == expected), "{expected}");
        }
    }

    #[test]
    fn cache_identity_changes_with_source_device_team_or_toolchain() {
        let baseline = key("source", "phone", "team", &[b"xcode", b"sdk"]);
        for other in [
            key("new", "phone", "team", &[b"xcode", b"sdk"]),
            key("source", "other", "team", &[b"xcode", b"sdk"]),
            key("source", "phone", "other", &[b"xcode", b"sdk"]),
            key("source", "phone", "team", &[b"new", b"sdk"]),
        ] {
            assert_ne!(baseline, other);
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("runner"), b"one").unwrap();
        let first = digest(root.path()).unwrap();
        std::fs::write(root.path().join("runner"), b"two").unwrap();
        assert_ne!(first, digest(root.path()).unwrap());
    }

    #[tokio::test]
    async fn reuses_only_verified_complete_products_and_rebuilds_after_tampering_or_expiry() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let key = uuid::Uuid::new_v4().to_string();
        let builds = AtomicUsize::new(0);
        let checks = AtomicUsize::new(0);
        let build = async |entry: &Path| {
            builds.fetch_add(1, Ordering::SeqCst);
            let products = entry.join("build/Build/Products");
            std::fs::create_dir_all(products.join("Release-iphoneos/TestrunXCTest-Runner.app"))
                .unwrap();
            std::fs::write(products.join("fixture.xctestrun"), b"descriptor").unwrap();
            Ok(())
        };
        let verify = async |_: &Path| {
            checks.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"verified":true}))
        };
        let first = acquire_at(&cache, &key, "ACME000001", &build, &verify)
            .await
            .unwrap();
        assert_eq!(first.signing["cacheReused"], false);
        let descriptor = first.descriptor.clone();
        drop(first);
        let second = acquire_at(&cache, &key, "ACME000001", &build, &verify)
            .await
            .unwrap();
        assert_eq!(second.signing["cacheReused"], true);
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(checks.load(Ordering::SeqCst), 2);
        drop(second);
        std::fs::write(descriptor, b"tampered").unwrap();
        let replaced = acquire_at(&cache, &key, "ACME000001", &build, &verify)
            .await
            .unwrap();
        assert_eq!(
            replaced.signing["cacheInvalidation"],
            "RUNNER_CACHE_INVALID"
        );
        assert_eq!(builds.load(Ordering::SeqCst), 2);
        drop(replaced);
        let attempts = AtomicUsize::new(0);
        let expired = async |_: &Path| {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(Error::new("RUNNER_PROFILE_EXPIRED", "expired"))
            } else {
                Ok(json!({}))
            }
        };
        let refreshed = acquire_at(&cache, &key, "ACME000001", &build, &expired)
            .await
            .unwrap();
        assert_eq!(refreshed.signing["cacheReused"], false);
        assert_eq!(
            refreshed.signing["cacheInvalidation"],
            "RUNNER_PROFILE_EXPIRED"
        );
        assert_eq!(builds.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn failed_build_never_publishes_a_reusable_cache() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let key = uuid::Uuid::new_v4().to_string();
        let failed =
            async |_: &Path| Err(Error::new("RUNNER_BUILD_FAILED", "fixture build failed"));
        let verify = async |_: &Path| Ok(json!({}));
        assert_eq!(
            acquire_at(&cache, &key, "ACME000001", failed, verify)
                .await
                .err()
                .unwrap()
                .code,
            "RUNNER_BUILD_FAILED"
        );
        assert!(!cache.join(key).join("verified.json").exists());
    }
}
