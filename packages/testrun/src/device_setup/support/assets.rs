//! Immutable upstream Cryptex assets cached privately and verified against manifest digests.
use super::*;
use sha2::{Digest, Sha384};

const REVISION: &str = "6eae353ae694bda1c421d4a3eee5459ae59c99a1";
pub(super) const BUILD: &str = "27A5228h";
const PAYLOADS: [(&str, &str); 4] = [
    ("Cryptex1,GenericDmg", "Image.dmg"),
    ("Cryptex1,GenericTrustCache", "Image.dmg.trustcache"),
    ("Cryptex1,CryptexInfoPlist", "Image.dmg.cryptex_info"),
    ("Cryptex1,GenericVolume", "Image.dmg.root_hash"),
];

pub(super) async fn load() -> Result<Cryptex1Assets> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::new("HOST_IO", "Cannot locate the Testrun data directory"))?;
    let root = std::path::PathBuf::from(home)
        .join(".local/share/testrun/developer-support")
        .join(REVISION);
    std::fs::create_dir_all(&root)
        .map_err(|_| Error::new("HOST_IO", "Cannot create the developer-support cache"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::new("HOST_IO", "Cannot restrict the developer-support cache"))?;
    }
    crate::artifacts::check_private_directory(&root)?;
    let _cache =
        crate::lease::ControlLease::acquire(&format!("developer-support-cache:{REVISION}"))?;
    download(&root, "BuildManifest.plist").await?;
    let bytes = std::fs::read(root.join("BuildManifest.plist"))
        .map_err(|_| Error::new("HOST_IO", "Cannot read cached developer manifest"))?;
    let manifest: plist::Dictionary = plist::from_bytes(&bytes)
        .map_err(|_| Error::new("INVALID_DEVELOPER_SUPPORT", "Developer manifest is invalid"))?;
    if manifest
        .get("ProductBuildVersion")
        .and_then(plist::Value::as_string)
        != Some(BUILD)
    {
        return Err(Error::new(
            "INVALID_DEVELOPER_SUPPORT",
            "Developer manifest has an unexpected build",
        ));
    }
    let identity = idevice::tss::select_cryptex_build_identity(&manifest)
        .map_err(platform)?
        .clone();
    let mut payloads = Vec::new();
    for (key, file) in PAYLOADS {
        download(&root, file).await?;
        let mut bytes = std::fs::read(root.join(file))
            .map_err(|_| Error::new("HOST_IO", "Cannot read cached developer payload"))?;
        let expected = identity
            .get("Manifest")
            .and_then(plist::Value::as_dictionary)
            .and_then(|m| m.get(key))
            .and_then(plist::Value::as_dictionary)
            .and_then(|m| m.get("Digest"))
            .and_then(plist::Value::as_data)
            .ok_or_else(|| {
                Error::new(
                    "INVALID_DEVELOPER_SUPPORT",
                    "Developer manifest omitted a payload digest",
                )
            })?;
        if verify(&bytes, expected).is_err() {
            std::fs::remove_file(root.join(file)).map_err(|_| {
                Error::new(
                    "HOST_IO",
                    "Cannot replace a damaged developer-support cache payload",
                )
            })?;
            download(&root, file).await?;
            bytes = std::fs::read(root.join(file))
                .map_err(|_| Error::new("HOST_IO", "Cannot read downloaded developer payload"))?;
        }
        verify(&bytes, expected)?;
        payloads.push(bytes);
    }
    let mut payloads = payloads.into_iter();
    Ok(Cryptex1Assets::from_parts(
        payloads.next().unwrap(),
        payloads.next().unwrap(),
        payloads.next().unwrap(),
        payloads.next().unwrap(),
        identity,
    ))
}
fn verify(bytes: &[u8], expected: &[u8]) -> Result<()> {
    if Sha384::digest(bytes).as_slice() != expected {
        return Err(Error::new(
            "INVALID_DEVELOPER_SUPPORT",
            "Downloaded developer payload digest does not match its manifest; existing device support was preserved",
        ));
    }
    Ok(())
}
async fn download(root: &std::path::Path, file: &str) -> Result<()> {
    let path = root.join(file);
    if path.is_file() {
        return Ok(());
    }
    let partial = Partial(root.join(format!("{}.partial", uuid::Uuid::new_v4())));
    let temporary = &partial.0;
    let url = format!(
        "https://raw.githubusercontent.com/doronz88/DeveloperDiskImage/{REVISION}/PersonalizedImages/Xcode_iOS_DDI_Cryptex/{file}"
    );
    let result = crate::process::run(
        "/usr/bin/curl",
        &[
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
            "1073741824",
            "--output",
            temporary
                .to_str()
                .ok_or_else(|| Error::invalid("Cache path is not UTF-8"))?,
            &url,
        ],
        Duration::from_secs(245),
        8192,
    )
    .await;
    if let Err(error) = result {
        return Err(Error::new("DEVELOPER_SUPPORT_DOWNLOAD_FAILED","Cannot download pinned developer support; check host internet access and prepare again").details(json!({"file":file,"cause":error.code,"details":error.details})));
    }
    std::fs::rename(temporary, &path)
        .map_err(|_| Error::new("HOST_IO", "Cannot publish cached developer payload"))?;
    Ok(())
}
struct Partial(std::path::PathBuf);
impl Drop for Partial {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[cfg(feature = "acceptance-downloads")]
    async fn downloads_and_verifies_pinned_developer_support() {
        let assets = load().await.unwrap();
        assert!(!assets.image.is_empty());
        assert!(!assets.info.is_empty());
        assert!(!assets.volumehash.is_empty());
    }
    #[test]
    fn payload_corruption_is_rejected_before_personalization() {
        let digest = Sha384::digest(b"Acme developer payload");
        verify(b"Acme developer payload", &digest).unwrap();
        assert_eq!(
            verify(b"corrupt", &digest).unwrap_err().code,
            "INVALID_DEVELOPER_SUPPORT"
        );
    }
}
