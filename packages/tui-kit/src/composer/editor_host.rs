//! Optional explicit host adapter; composer state never starts a process itself.

use super::{EditorRequest, EditorResult};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(Option<PathBuf>);
impl Drop for Temporary {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Resolve an explicitly supplied environment's editor command, VISUAL before EDITOR.
#[must_use]
pub fn editor_command(visual: Option<&str>, editor: Option<&str>) -> Option<String> {
    visual
        .filter(|value| !value.trim().is_empty())
        .or_else(|| editor.filter(|value| !value.trim().is_empty()))
        .map(str::to_owned)
}

/// Run a configured editor after the host releases its terminal lease.
/// The command is trusted host configuration interpreted by the supplied shell;
/// the temporary path is passed as a separate argument, never interpolated.
/// The host owns cancellation/GUI scheduling; this foreground helper waits for exit.
/// # Errors
/// Returns temp-file, spawn, wait or read errors. Owned temporary files are cleaned up.
pub fn run_editor(
    request: &EditorRequest,
    command: &str,
    shell: &Path,
    directory: &Path,
    limit: usize,
) -> io::Result<EditorResult> {
    if command.trim().is_empty() {
        return Ok(EditorResult::Failed(
            "No VISUAL or EDITOR configured".into(),
        ));
    }
    let path = directory.join(format!(
        "canonic-composer-{}-{}.txt",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    let mut temporary = Temporary(Some(path.clone()));
    let result = (|| {
        file.write_all(request.text.as_bytes())?;
        // Closing makes the bytes visible to the editor process. The file is a
        // short-lived handoff, not durable state, so it receives no storage flush.
        drop(file);
        let status = Command::new(shell)
            .arg("-c")
            .arg(format!("{command} \"$1\""))
            .arg("canonic-editor")
            .arg(&path)
            .status()?;
        if !status.success() {
            return Ok(EditorResult::Failed(format!("Editor exited with {status}")));
        }
        let mut text = String::new();
        fs::File::open(&path)?
            .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
            .read_to_string(&mut text)?;
        if text.len() > limit {
            return Ok(EditorResult::Failed(
                "Editor result exceeds the draft limit".into(),
            ));
        }
        if text.is_empty() {
            return Ok(EditorResult::Failed("Editor returned an empty file".into()));
        }
        Ok(EditorResult::Text(text))
    })();
    let cleanup = temporary.0.take().map_or(Ok(()), fs::remove_file);
    match (result, cleanup) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!(
            "{error}; temporary file cleanup: {cleanup}"
        ))),
    }
}
