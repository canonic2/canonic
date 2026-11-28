//! Selected regular files become bounded immutable payloads before HTTP dispatch.
use super::{Error, Fetch, MAX_BODY, MultipartPart, Payload};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::Semaphore, time::Instant};

static READERS: Semaphore = Semaphore::const_new(4);
pub(super) async fn reader(
    deadline: Instant,
) -> Result<tokio::sync::SemaphorePermit<'static>, Error> {
    tokio::time::timeout_at(deadline, READERS.acquire())
        .await
        .map_err(|_| Error::from("HTTP_DEADLINE"))?
        .map_err(|_| Error::from("HTTP_FILE_IO"))
}
#[cfg(test)]
pub(crate) async fn hold_readers() -> tokio::sync::SemaphorePermit<'static> {
    READERS.acquire_many(4).await.expect("File reader capacity")
}
struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) async fn prepare(mut fetch: Fetch, deadline: Instant) -> Result<Fetch, Error> {
    fetch.validate()?;
    if !matches!(&fetch.body, Some(Payload::Multipart { fields }) if fields.iter().any(|part|matches!(part, MultipartPart::FilePath {..})))
    {
        return Ok(fetch);
    }
    let cancel = Cancel(Arc::new(AtomicBool::new(false)));
    let cancelled = cancel.0.clone();
    let permit = reader(deadline).await?;
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut remaining = MAX_BODY;
        let Some(Payload::Multipart { fields }) = &mut fetch.body else {
            unreachable!("Selected file payload");
        };
        for part in fields {
            checkpoint(&cancelled, deadline)?;
            match part {
                MultipartPart::Text { value, .. } => {
                    remaining = remaining
                        .checked_sub(value.len())
                        .ok_or("HTTP_REQUEST_BODY_BOUNDS")?
                }
                MultipartPart::File { data, .. } => {
                    let size = STANDARD
                        .decode(data.as_bytes())
                        .map_err(|_| Error::from("HTTP_REQUEST_BODY_INVALID"))?
                        .len();
                    remaining = remaining
                        .checked_sub(size)
                        .ok_or("HTTP_REQUEST_BODY_BOUNDS")?;
                }
                MultipartPart::FilePath {
                    name,
                    path,
                    filename,
                    mime_type,
                } => {
                    let selected_name =
                        super::multipart::filename(path, filename.as_deref())?.to_owned();
                    let bytes = read(path, remaining, &cancelled, deadline)?;
                    remaining -= bytes.len();
                    *part = MultipartPart::File {
                        name: name.clone(),
                        filename: selected_name,
                        mime_type: mime_type.clone(),
                        data: STANDARD.encode(bytes),
                    };
                }
            }
        }
        // Framing, all parts together and inferred/explicit MIME values must fit
        // before the request store can acquire an HTTP dispatch lease.
        fetch.validate()?;
        checkpoint(&cancelled, deadline)?;
        Ok(fetch)
    });
    tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| Error::from("HTTP_DEADLINE"))?
        .map_err(|error| Error::caused("HTTP_FILE_IO", error))?
}
fn checkpoint(cancelled: &AtomicBool, deadline: Instant) -> Result<(), Error> {
    if cancelled.load(Ordering::Acquire) {
        return Err("CANCELLED".into());
    }
    if Instant::now() >= deadline {
        return Err("HTTP_DEADLINE".into());
    }
    Ok(())
}
fn read(
    path: &str,
    max: usize,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, Error> {
    checkpoint(cancelled, deadline)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options
        .open(path)
        .map_err(|error| Error::caused("HTTP_FILE_IO", error))?;
    let before = file
        .metadata()
        .map_err(|error| Error::caused("HTTP_FILE_IO", error))?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err("HTTP_FILE_NOT_REGULAR".into());
    }
    if before.len() > max as u64 {
        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut buffer = [0; 16384];
    loop {
        checkpoint(cancelled, deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| Error::caused("HTTP_FILE_IO", error))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > max {
            return Err("HTTP_REQUEST_BODY_BOUNDS".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|error| Error::caused("HTTP_FILE_IO", error))?;
    if after.len() != bytes.len() as u64
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("HTTP_FILE_CHANGED".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request(path: &std::path::Path) -> Fetch {
        serde_json::from_value(json!({"url":"https://example.com/acme","method":"POST","body":{"kind":"multipart","fields":[{"kind":"file_path","name":"upload","path":path}]}})).unwrap()
    }
    #[tokio::test]
    async fn file_preparation_snapshots_bytes_and_infers_the_selected_basename() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("Acme.json");
        std::fs::write(&path, b"{\"Acme\":true}").unwrap();
        let fetch = request(&path);
        fetch.validate().unwrap();
        assert_eq!(
            fetch.body.as_ref().unwrap().encode().unwrap_err().code,
            "HTTP_FILE_PREPARATION_REQUIRED"
        );
        let fetch = prepare(fetch, Instant::now() + std::time::Duration::from_secs(5))
            .await
            .unwrap();
        std::fs::write(path, b"changed").unwrap();
        let (bytes, _) = fetch.body.unwrap().encode().unwrap();
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("filename=\"Acme.json\""));
        assert!(body.contains("Content-Type: application/json"));
        assert!(body.contains("{\"Acme\":true}"));
        assert!(!body.contains("changed"));
    }
    #[tokio::test]
    async fn invalid_files_and_expired_preparation_refuse_before_dispatch() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            prepare(
                request(root.path()),
                Instant::now() + std::time::Duration::from_secs(5)
            )
            .await
            .unwrap_err()
            .code,
            "HTTP_FILE_NOT_REGULAR"
        );
        let path = root.path().join("large.bin");
        std::fs::write(&path, vec![0; MAX_BODY + 1]).unwrap();
        assert_eq!(
            prepare(
                request(&path),
                Instant::now() + std::time::Duration::from_secs(5)
            )
            .await
            .unwrap_err()
            .code,
            "HTTP_REQUEST_BODY_BOUNDS"
        );
        assert_eq!(
            prepare(request(&path), Instant::now())
                .await
                .unwrap_err()
                .code,
            "HTTP_DEADLINE"
        );
        #[cfg(unix)]
        {
            let link = root.path().join("link.bin");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert_eq!(
                prepare(
                    request(&link),
                    Instant::now() + std::time::Duration::from_secs(5)
                )
                .await
                .unwrap_err()
                .code,
                "HTTP_FILE_IO"
            );
        }
    }
}
