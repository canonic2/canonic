//! Caller-selected files become inline immutable material before allocation.
use super::*;
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) struct Cancellation(pub Arc<AtomicBool>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub(super) fn checkpoint(cancelled: &AtomicBool, deadline: Instant) -> Result<(), Error> {
    if cancelled.load(Ordering::Acquire) {
        return Err("CANCELLED".into());
    }
    if Instant::now() >= deadline {
        return Err("HTTP_DEADLINE".into());
    }
    Ok(())
}

pub(super) fn materialize(
    input: &mut [ClientCertificate],
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), Error> {
    let mut remaining = MAX_BYTES;
    for item in input {
        for (inline, path) in [
            (&mut item.cert, &mut item.cert_path),
            (&mut item.key, &mut item.key_path),
            (&mut item.pfx, &mut item.pfx_path),
        ] {
            checkpoint(cancelled, deadline)?;
            // An explicitly supplied empty buffer is still selected. Never fall
            // back to the path merely because its base64 encoding is empty.
            let size = if let Some(value) = inline {
                STANDARD
                    .decode(value)
                    .map_err(|_| Error::from("HTTP_CLIENT_CERTIFICATE_ENCODING"))?
                    .len()
            } else if let Some(selected) = path.as_deref().filter(|path| !path.is_empty()) {
                let bytes = read(selected, remaining, cancelled, deadline)?;
                let size = bytes.len();
                *inline = Some(STANDARD.encode(bytes));
                size
            } else {
                0
            };
            remaining = remaining
                .checked_sub(size)
                .ok_or("HTTP_CLIENT_CERTIFICATE_BOUNDS")?;
            *path = None;
        }
    }
    checkpoint(cancelled, deadline)
}

fn read(
    path: &str,
    max: usize,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, Error> {
    checkpoint(cancelled, deadline)?;
    if path.len() > 4096 || path.contains('\0') || !std::path::Path::new(path).is_absolute() {
        return Err("HTTP_CLIENT_CERTIFICATE_PATH_INVALID".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Follow caller-selected links like Node's readFile, but do not block
        // on a selected FIFO before its non-regular-file refusal.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = options
        .open(path)
        .map_err(|_| Error::from("HTTP_CLIENT_CERTIFICATE_FILE_IO"))?;
    let before = file
        .metadata()
        .map_err(|_| Error::from("HTTP_CLIENT_CERTIFICATE_FILE_IO"))?;
    if !before.is_file() {
        return Err("HTTP_CLIENT_CERTIFICATE_FILE_NOT_REGULAR".into());
    }
    if before.len() > max as u64 {
        return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS".into());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut buffer = [0; 16384];
    loop {
        checkpoint(cancelled, deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| Error::from("HTTP_CLIENT_CERTIFICATE_FILE_IO"))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > max {
            return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| Error::from("HTTP_CLIENT_CERTIFICATE_FILE_IO"))?;
    if before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok()
    {
        return Err("HTTP_CLIENT_CERTIFICATE_FILE_CHANGED".into());
    }
    checkpoint(cancelled, deadline)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_certificate_paths_encrypted_formats_preserve_the_prepared_identity() {
        use openssl::{pkcs12::Pkcs12, pkey::PKey, symm::Cipher, x509::X509};
        let root = tempfile::tempdir().unwrap();
        let original = files(root.path());
        let private =
            PKey::private_key_from_pem(include_bytes!("../fixtures/server-key.pem")).unwrap();
        let leaf = X509::from_pem(include_bytes!("../fixtures/server.pem")).unwrap();
        let mut archive = Pkcs12::builder();
        archive.name("Acme").pkey(&private).cert(&leaf);
        let pfx = root.path().join("Acme encrypted archive.pfx");
        std::fs::write(
            &pfx,
            archive.build2("Acme secret").unwrap().to_der().unwrap(),
        )
        .unwrap();
        let encrypted = root.path().join("Acme encrypted key.pem");
        std::fs::write(
            &encrypted,
            private
                .private_key_to_pem_pkcs8_passphrase(Cipher::aes_256_cbc(), b"Acme secret")
                .unwrap(),
        )
        .unwrap();
        for mut input in [
            ClientCertificate {
                origin: original.origin.clone(),
                pfx_path: Some(pfx.to_str().unwrap().into()),
                passphrase: Some("Acme secret".into()),
                ..Default::default()
            },
            ClientCertificate {
                key_path: Some(encrypted.to_str().unwrap().into()),
                passphrase: Some("Acme secret".into()),
                ..original.clone()
            },
        ] {
            validate(&[input.clone()]).unwrap();
            materialize(
                std::slice::from_mut(&mut input),
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
            let prepared = prepare(&[input]).unwrap();
            assert_eq!(
                prepared[0].resolver.keys[0].cert[0].as_ref(),
                leaf.to_der().unwrap()
            );
        }
    }
    #[test]
    fn client_certificate_paths_aggregate_budget_includes_inline_and_file_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("Acme aggregate key");
        std::fs::write(&path, vec![0; MAX_BYTES / 2 + 1]).unwrap();
        let mut input = ClientCertificate {
            origin: "https://example.com".into(),
            cert: Some(STANDARD.encode(vec![0; MAX_BYTES / 2 + 1])),
            key_path: Some(path.to_str().unwrap().into()),
            ..Default::default()
        };
        let error = materialize(
            std::slice::from_mut(&mut input),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(error.code, "HTTP_CLIENT_CERTIFICATE_BOUNDS");
    }
    fn files(root: &std::path::Path) -> ClientCertificate {
        let cert = root.join("Acme certificate.pem");
        let key = root.join("Acme key.pem");
        std::fs::write(&cert, include_bytes!("../fixtures/server.pem")).unwrap();
        std::fs::write(&key, include_bytes!("../fixtures/server-key.pem")).unwrap();
        ClientCertificate {
            origin: "https://example.com".into(),
            cert_path: Some(cert.to_str().unwrap().into()),
            key_path: Some(key.to_str().unwrap().into()),
            ..Default::default()
        }
    }
    fn options(input: ClientCertificate) -> Options {
        Options {
            client_certificates: Some(vec![input]),
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn client_certificate_paths_snapshot_bytes_and_reuse_them_after_source_deletion() {
        let root = tempfile::tempdir().unwrap();
        let input = files(root.path());
        validate(std::slice::from_ref(&input)).unwrap();
        assert_eq!(
            prepare(std::slice::from_ref(&input)).err(),
            Some("HTTP_CLIENT_CERTIFICATE_PREPARATION_REQUIRED")
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let prepared = prepare_options(options(input.clone()), deadline)
            .await
            .unwrap();
        let material = &prepared.client_certificates.as_ref().unwrap()[0];
        assert!(material.cert_path.is_none() && material.key_path.is_none());
        assert_eq!(
            STANDARD.decode(material.cert()).unwrap(),
            include_bytes!("../fixtures/server.pem")
        );
        assert_eq!(
            STANDARD.decode(material.key()).unwrap(),
            include_bytes!("../fixtures/server-key.pem")
        );
        std::fs::write(input.cert_path.as_ref().unwrap(), b"Acme replaced source").unwrap();
        std::fs::remove_file(input.key_path.as_ref().unwrap()).unwrap();
        let readers = super::super::super::files::hold_readers().await;
        let mut reused = prepare_options(prepared, deadline).await.unwrap();
        assert_eq!(take_prepared(&mut reused).unwrap().len(), 1);
        drop(readers);
        assert!(prepare_options(options(input), deadline).await.is_err());
    }
    #[tokio::test]
    async fn client_certificate_paths_inline_material_wins_without_opening_fallback_files() {
        let root = tempfile::tempdir().unwrap();
        let mut input = files(root.path());
        input.cert = Some(STANDARD.encode(include_bytes!("../fixtures/server.pem")));
        input.key = Some(STANDARD.encode(include_bytes!("../fixtures/server-key.pem")));
        std::fs::remove_file(input.cert_path.as_ref().unwrap()).unwrap();
        std::fs::remove_file(input.key_path.as_ref().unwrap()).unwrap();
        let prepared = prepare_options(
            options(input.clone()),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(prepared.client_certificates.unwrap()[0].cert_path.is_none());
        input.cert = Some(String::new());
        // Preserve explicit emptiness: do not retry the missing file. Testrun
        // retains its pre-allocation material refusal, unlike SDK fetch staging.
        assert_eq!(
            prepare_options(options(input), Instant::now() + Duration::from_secs(5))
                .await
                .unwrap_err()
                .code,
            "HTTP_CLIENT_CERTIFICATE_INVALID"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn client_certificate_paths_follow_selected_regular_file_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let mut input = files(root.path());
        let link = root.path().join("Acme selected link.pem");
        std::os::unix::fs::symlink(input.cert_path.as_ref().unwrap(), &link).unwrap();
        input.cert_path = Some(link.to_str().unwrap().into());
        let prepared = prepare_options(options(input), Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        assert!(prepared.client_certificates.unwrap()[0].cert.is_some());
    }
    #[tokio::test]
    async fn client_certificate_paths_fail_privately_before_allocation_for_missing_nonregular_and_oversized_files()
     {
        let root = tempfile::tempdir().unwrap();
        let original = files(root.path());
        let large = root.path().join("Acme private oversized.pem");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(MAX_BYTES as u64 + 1)
            .unwrap();
        for (path, code) in [
            (
                root.path().join("Acme private absent.pem"),
                "HTTP_CLIENT_CERTIFICATE_FILE_IO",
            ),
            (
                root.path().to_path_buf(),
                "HTTP_CLIENT_CERTIFICATE_FILE_NOT_REGULAR",
            ),
            (large, "HTTP_CLIENT_CERTIFICATE_BOUNDS"),
        ] {
            let mut input = original.clone();
            input.cert_path = Some(path.to_str().unwrap().into());
            let error = prepare_options(options(input), Instant::now() + Duration::from_secs(5))
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            assert!(!format!("{error:?}").contains("Acme private"));
        }
    }
    #[tokio::test]
    async fn client_certificate_paths_deadline_while_waiting_has_no_materialized_result() {
        let root = tempfile::tempdir().unwrap();
        let input = files(root.path());
        let readers = super::super::super::files::hold_readers().await;
        let error = prepare_options(options(input), Instant::now() + Duration::from_millis(20))
            .await
            .unwrap_err();
        assert_eq!(error.code, "HTTP_DEADLINE");
        drop(readers);
        let cancel = Cancellation(Arc::new(AtomicBool::new(false)));
        let flag = cancel.0.clone();
        drop(cancel);
        assert_eq!(
            checkpoint(&flag, Instant::now() + Duration::from_secs(5))
                .unwrap_err()
                .code,
            "CANCELLED"
        );
    }
}
