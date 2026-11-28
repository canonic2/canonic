//! Private durable command outcomes for retained CLI sessions; never restores input authority.
use super::arguments::{field, session_ref};
use super::{Server, id};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::{fs, io::Read, path::Path};

#[derive(Clone)]
pub(crate) struct Archive(PathBuf);
impl Archive {
    /// Raw MCP journals belong to run evidence, independently of a target lane.
    pub(super) fn for_session(evidence: &Path, session: &str) -> Result<Self> {
        crate::artifacts::check_private_directory(evidence)?;
        let archive = Self::at(evidence);
        crate::managed::storage::directory(&archive.0)?;
        archive.prepare_scoped(session)?;
        Ok(archive.scoped(session))
    }
    pub(crate) fn scoped(&self, session: &str) -> Self {
        Self(
            self.0
                .join(format!("{:x}", Sha256::digest(session.as_bytes()))),
        )
    }
    pub(super) fn prepare_scoped(&self, session: &str) -> Result<()> {
        crate::artifacts::check_private_directory(&self.0)?;
        crate::artifacts::directory(&self.scoped(session).0)
    }
    #[cfg(any(unix, test))]
    pub(crate) fn create(directory: &Path) -> Result<Self> {
        let archive = Self(directory.join("outcomes"));
        crate::artifacts::directory(&archive.0)?;
        Ok(archive)
    }
    pub(crate) fn at(directory: &Path) -> Self {
        Self(directory.join("outcomes"))
    }
    /// The session directory this archive persists into.
    pub(crate) fn session_directory(&self) -> Option<&Path> {
        self.0.parent()
    }
    fn path(&self, command: &str, batch: bool) -> PathBuf {
        self.0.join(format!(
            "{}-{:x}.json",
            if batch { "batch" } else { "action" },
            Sha256::digest(command.as_bytes())
        ))
    }
    pub(crate) fn record(&self, command: &str, status: &Value) -> Result<()> {
        let temporary = self.0.join(format!("{}.partial", id()));
        let bytes = serde_json::to_vec(status)
            .map_err(|_| Error::new("JOURNAL_IO", "Cannot encode command outcome"))?;
        crate::artifacts::write(&temporary, &bytes)?;
        let result = fs::rename(
            &temporary,
            self.path(command, status.get("steps").is_some()),
        )
        .and_then(|()| crate::artifacts::sync(&fs::File::open(&self.0)?));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|_| Error::new("JOURNAL_IO", "Cannot publish durable command outcome"))
    }
    fn read(&self, command: &str, batch: bool) -> Result<Value> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let directory = fs::symlink_metadata(&self.0)
                .map_err(|_| Error::new("NOT_FOUND", "No archived journal is available"))?;
            // SAFETY: read-only query of this process's effective user identity.
            if !directory.is_dir()
                || directory.uid() != unsafe { libc::geteuid() }
                || directory.permissions().mode() & 0o077 != 0
            {
                return Err(Error::new(
                    "UNAUTHORIZED",
                    "Archived journal must be private and owned by this user",
                ));
            }
        }
        let path = self.path(command, batch);
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            Error::new(
                "NOT_FOUND",
                "No archived outcome; do not infer non-dispatch or replay",
            )
        })?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(Error::new("JOURNAL_IO", "Invalid archived outcome file"));
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .and_then(|file| file.take(1024 * 1024 + 1).read_to_end(&mut bytes))
            .map_err(|_| Error::new("JOURNAL_IO", "Cannot read archived outcome"))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new("JOURNAL_IO", "Cannot decode archived outcome"))?;
        if bytes.len() > 1024 * 1024 || !value.is_object() {
            return Err(Error::new("JOURNAL_IO", "Invalid archived outcome"));
        }
        Ok(value)
    }
    pub(crate) fn status(&self, command: &str) -> Result<Value> {
        let mut status = recovered(match self.read(command, false) {
            Err(error) if error.code == "NOT_FOUND" => self.read(command, true)?,
            result => result?,
        });
        if status.get("steps").is_some() {
            let mut outcomes = Vec::new();
            for index in 0..8 {
                match self.read(&format!("{command}:{index}"), false) {
                    Ok(outcome) => {
                        let mut outcome = recovered(outcome);
                        outcome["index"] = json!(index);
                        outcomes.push(outcome);
                    }
                    Err(error) if error.code == "NOT_FOUND" => {}
                    Err(error) => return Err(error),
                }
            }
            status["actionOutcomes"] = json!(outcomes);
        }
        Ok(status)
    }
}

fn recovered(mut status: Value) -> Value {
    if status["state"] == "dispatched" {
        status["state"] = json!("outcome_unknown");
        status["reason"] = json!("terminal_outcome_not_recorded");
    }
    status["journalSource"] = json!("archive");
    status
}

impl Server {
    pub(super) fn journal_archive(&self, session: &str) -> Option<Archive> {
        self.sessions
            .get(session)
            .and_then(|session| session.archive.clone())
            .or_else(|| self.archive.clone())
    }
    #[cfg(any(unix, test))]
    pub(crate) fn archive_outcomes(&mut self, archive: Archive) {
        self.archive = Some(archive);
    }
    pub(super) fn record_command(
        &mut self,
        session: &str,
        command: String,
        signature: Value,
        status: Value,
    ) -> Result<()> {
        if let Some(archive) = self.journal_archive(session) {
            archive.record(&command, &status)?;
        }
        self.sessions
            .get_mut(session)
            .ok_or_else(|| Error::new("SESSION_NOT_FOUND", "Session ended"))?
            .journal
            .insert(command, (signature, status));
        Ok(())
    }
    pub(super) fn record_batch(
        &mut self,
        session: &str,
        command: String,
        signature: Value,
        status: Value,
    ) -> Result<()> {
        if let Some(archive) = self.journal_archive(session) {
            archive.record(&command, &status)?;
        }
        if let Some(session) = self.sessions.get_mut(session) {
            session.batches.insert(command, (signature, status));
        }
        Ok(())
    }
    pub(super) fn command_status(&self, args: Value) -> Result<Value> {
        let reference = session_ref(&args)?;
        let session = self.session(&reference)?;
        if let Some(archive) = self.journal_archive(&reference.session_id) {
            return archive.status(field(&args, "commandId")?);
        }
        session
            .journal
            .get(field(&args, "commandId")?)
            .or_else(|| {
                session
                    .batches
                    .get(args["commandId"].as_str().unwrap_or(""))
            })
            .map(|(_, status)| status.clone())
            .ok_or_else(|| Error::new("NOT_FOUND", "Command is not in this session journal"))
    }
}

#[cfg(test)]
mod tests;
