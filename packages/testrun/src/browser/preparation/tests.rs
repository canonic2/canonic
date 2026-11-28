//! Real archive fixtures verify extraction containment and cache corruption.
use super::*;
use std::io::Write;

#[test]
fn chrome_preparation_has_no_node_or_playwright_dependency() {
    for host in [Host::Macos15X64, Host::Ubuntu26X64, Host::WindowsX64] {
        let artifacts = manifest::selected(host, Engine::Chrome).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].id, host.chrome().id);
        assert!(manifest::selected(host, Engine::Chromium).is_err());
    }
}

#[test]
fn windows_pins_native_executables_and_the_shared_playwright_driver() {
    for (engine, entry) in [
        (Engine::Firefox, "firefox/firefox.exe"),
        (Engine::Webkit, "Playwright.exe"),
    ] {
        let artifacts = manifest::selected(Host::WindowsX64, engine).unwrap();
        assert_eq!(artifacts.len(), 4);
        assert_eq!(artifacts[0].entry, "node-v24.12.0-win-x64/node.exe");
        assert_eq!(artifacts[1].id, manifest::CORE.id);
        assert_eq!(artifacts[2].entry, entry);
        assert_eq!(artifacts[3].directory, "browsers/winldd-1007");
        assert_eq!(artifacts[3].entry, "PrintDeps.exe");
    }
    assert_eq!(Host::WindowsX64.chrome().entry, "chrome-win64/chrome.exe");
}

#[test]
fn ubuntu_prepares_pinned_linux_browser_and_shared_driver() {
    for (engine, entry, archive) in [
        (
            Engine::Firefox,
            "firefox/firefox",
            "firefox-ubuntu-24.04.zip",
        ),
        (Engine::Webkit, "pw_run.sh", "webkit-ubuntu-26.04.zip"),
    ] {
        let artifacts = manifest::selected(Host::Ubuntu26X64, engine).unwrap();
        assert_eq!(artifacts.len(), 3);
        assert_eq!(artifacts[0].entry, "node-v24.12.0-linux-x64/bin/node");
        assert_eq!(artifacts[1].id, manifest::CORE.id);
        assert_eq!(artifacts[2].entry, entry);
        assert!(artifacts[2].url.ends_with(archive));
        assert!(manifest::has_dependency_marker(artifacts[2]));
    }
}

#[test]
fn dependency_validation_marker_is_optional_but_cannot_hide_bundle_mutation() {
    for engine in [Engine::Firefox, Engine::Webkit] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("contents");
        crate::artifacts::directory(&root).unwrap();
        let artifact = *manifest::selected(Host::Macos15X64, engine)
            .unwrap()
            .last()
            .unwrap();
        let file = temporary.path().join("fixture.zip");
        archive(&file, artifact.entry, b"Acme browser");
        super::archive::extract(&file, &root, true).unwrap();
        super::archive::seal(&root, artifact).unwrap();
        let marker = root.join("DEPENDENCIES_VALIDATED");
        fs::write(&marker, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&marker, fs::Permissions::from_mode(0o664)).unwrap();
            assert!(super::archive::verify_install(&root, artifact).is_err());
            fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
        }
        super::archive::verify_install(&root, artifact).unwrap();
        fs::write(&marker, b"unexpected content").unwrap();
        assert!(super::archive::verify_install(&root, artifact).is_err());
        fs::remove_file(&marker).unwrap();
        #[cfg(unix)]
        {
            let outside = temporary.path().join("outside");
            fs::write(&outside, b"").unwrap();
            std::os::unix::fs::symlink(outside, &marker).unwrap();
            assert!(super::archive::verify_install(&root, artifact).is_err());
            fs::remove_file(&marker).unwrap();
        }
        super::archive::verify_install(&root, artifact).unwrap();
        fs::write(root.join(artifact.entry), b"changed").unwrap();
        assert_eq!(
            super::archive::verify_install(&root, artifact)
                .unwrap_err()
                .code,
            "BUNDLE_CORRUPT"
        );
    }
}

#[test]
fn linux_chrome_reuses_the_cache_only_with_an_empty_external_extensions_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("contents");
    crate::artifacts::directory(&root).unwrap();
    let artifact = Host::Ubuntu26X64.chrome();
    let file = temporary.path().join("fixture.zip");
    archive(&file, artifact.entry, b"Acme browser");
    super::archive::extract(&file, &root, true).unwrap();
    super::archive::seal(&root, artifact).unwrap();

    let extensions = root.join("chrome-linux64/extensions");
    crate::artifacts::directory(&extensions).unwrap();
    super::archive::verify_install(&root, artifact).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&extensions, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(super::archive::verify_install(&root, artifact).is_err());
        fs::set_permissions(&extensions, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(extensions.join("acme.json"), b"{}").unwrap();
    assert!(super::archive::verify_install(&root, artifact).is_err());
}

fn archive(path: &Path, name: &str, bytes: &[u8]) {
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap();
}
#[test]
fn unsafe_paths_links_and_corrupt_installations_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("contents");
    crate::artifacts::directory(&root).unwrap();
    let file = temp.path().join("fixture.zip");
    archive(&file, "../escaped", b"Acme");
    assert!(super::archive::extract(&file, &root, true).is_err());
    assert!(!temp.path().join("escaped").exists());
    {
        let mut writer = zip::ZipWriter::new(fs::File::create(&file).unwrap());
        writer
            .add_symlink(
                "foreign-link",
                "../../escaped",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.finish().unwrap();
        assert!(super::archive::extract(&file, &root, true).is_err());
    }
    archive(&file, "node-v24.12.0-darwin-x64/bin/node", b"Acme");
    super::archive::extract(&file, &root, true).unwrap();
    super::archive::seal(&root, &manifest::NODE).unwrap();
    super::archive::verify_install(&root, &manifest::NODE).unwrap();
    fs::write(root.join(manifest::NODE.entry), b"changed").unwrap();
    assert_eq!(
        super::archive::verify_install(&root, &manifest::NODE)
            .unwrap_err()
            .code,
        "BUNDLE_CORRUPT"
    );
}
#[test]
fn checksum_and_size_are_both_required_before_extraction() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("archive");
    fs::write(&path, b"Acme").unwrap();
    assert_eq!(
        super::archive::verify_download(&path, &manifest::CORE)
            .unwrap_err()
            .code,
        "BUNDLE_CHECKSUM"
    );
}

fn fixture_artifact(path: &Path) -> &'static Artifact {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).unwrap();
    Box::leak(Box::new(Artifact {
        id: "acme-chrome-fixture",
        url: "https://example.com/acme.zip",
        sha256: Box::leak(format!("{:x}", Sha256::digest(&bytes)).into_boxed_str()),
        bytes: bytes.len() as u64,
        directory: "chrome-fixture",
        entry: "Acme/browser",
        zip: true,
        license: "Acme fixture",
    }))
}

#[tokio::test]
async fn concurrent_preparation_publishes_once_and_offline_reuse_verifies_the_tree() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("cache");
    private_tree(&root).unwrap();
    let source = temporary.path().join("fixture.zip");
    archive(&source, "Acme/browser", b"Acme browser fixture");
    let artifact = fixture_artifact(&source);
    let acquired = Arc::new(AtomicUsize::new(0));
    let download = |path: PathBuf, _: &'static Artifact| {
        let source = source.clone();
        let acquired = acquired.clone();
        async move {
            acquired.fetch_add(1, Ordering::SeqCst);
            tokio::fs::copy(source, path)
                .await
                .map_err(super::archive::io)?;
            Ok(())
        }
    };
    let (first, second) = tokio::join!(
        install_with(&root, artifact, &download),
        install_with(&root, artifact, &download)
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(acquired.load(Ordering::SeqCst), 1);
    install_with(&root, artifact, |_, _| async {
        panic!("Offline reuse cannot download")
    })
    .await
    .unwrap();
    fs::write(
        root.join(artifact.directory).join(artifact.entry),
        b"changed",
    )
    .unwrap();
    let error = install_with(&root, artifact, |_, _| async {
        panic!("Corruption cannot redownload over an active build")
    })
    .await
    .unwrap_err();
    assert_eq!(error.code, "BUNDLE_CORRUPT");
}

#[tokio::test]
async fn interrupted_or_failed_acquisition_never_publishes_and_can_be_retried() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("cache");
    private_tree(&root).unwrap();
    let source = temporary.path().join("fixture.zip");
    archive(&source, "Acme/browser", b"Acme browser fixture");
    let artifact = fixture_artifact(&source);
    let (entered, receiver) = tokio::sync::oneshot::channel();
    let installing = tokio::spawn({
        let root = root.clone();
        async move {
            install_with(&root, artifact, |path, _| async move {
                tokio::fs::write(path, b"partial").await.unwrap();
                entered.send(()).unwrap();
                std::future::pending::<Result<()>>().await
            })
            .await
        }
    });
    receiver.await.unwrap();
    installing.abort();
    assert!(installing.await.unwrap_err().is_cancelled());
    assert_no_incomplete_candidate(&root);
    let error = install_with(&root, artifact, |_, _| async {
        Err(Error::new(
            "BROWSER_DOWNLOAD_FAILED",
            "Acme acquisition failure",
        ))
    })
    .await
    .unwrap_err();
    assert_eq!(error.code, "BROWSER_DOWNLOAD_FAILED");
    assert_no_incomplete_candidate(&root);
    install_with(&root, artifact, |path, _| async {
        tokio::fs::copy(source, path)
            .await
            .map_err(super::archive::io)?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        root.join(artifact.directory)
            .join("verified.json")
            .is_file()
    );
}

fn assert_no_incomplete_candidate(root: &Path) {
    let remaining = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    #[cfg(unix)]
    assert!(remaining.is_empty(), "{remaining:?}");
    #[cfg(windows)]
    {
        // Windows keeps the exclusive-lock anchor so concurrent preparers
        // cannot start locking different inodes after an aborted download.
        assert_eq!(
            remaining,
            [std::ffi::OsString::from(".acme-chrome-fixture.lock")]
        );
        assert!(root.join(".acme-chrome-fixture.lock").is_file());
    }
}

#[tokio::test]
#[cfg(feature = "acceptance-downloads")]
async fn private_downloads_publish_and_reuse_both_browser_bundles() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("private");
    let (firefox, webkit) = tokio::try_join!(
        prepare_in(&root, Engine::Firefox, |_| {}),
        prepare_in(&root, Engine::Webkit, |_| {})
    )
    .unwrap();
    assert!(matches_node_identity(&firefox));
    assert!(matches_node_identity(&webkit));
    // Existing complete installations run through full integrity validation
    // without invoking curl; the same routine supports offline reuse.
    prepare_in(&root, Engine::Firefox, |_| {}).await.unwrap();
    prepare_in(&root, Engine::Webkit, |_| {}).await.unwrap();
}

#[tokio::test]
#[cfg(all(target_os = "linux", feature = "acceptance-downloads"))]
async fn ubuntu_private_chrome_archive_publishes_and_reuses_without_node() {
    let root = tempfile::tempdir().unwrap();
    let host = bundle_host().unwrap();
    let chrome = host.chrome();
    install(root.path(), chrome).await.unwrap();
    assert!(
        root.path()
            .join(chrome.directory)
            .join(chrome.entry)
            .is_file()
    );
    install_with(root.path(), chrome, |_, _| async {
        panic!("Offline reuse cannot download Chrome")
    })
    .await
    .unwrap();
    assert!(!root.path().join(host.node().directory).exists());
    assert!(!root.path().join(manifest::CORE.directory).exists());
}
