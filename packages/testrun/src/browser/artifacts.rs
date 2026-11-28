//! Private immutable browser evidence. Caller-owned stores define retention.
use super::session::Error;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
use uuid::Uuid;
#[derive(Clone)]
pub(crate) struct Artifact {
    pub id: Uuid,
    path: PathBuf,
    bytes: usize,
    digest: String,
    pub summary: Value,
}
#[derive(Clone)]
pub(crate) struct Format {
    pub prefix: &'static str,
    pub extension: &'static str,
    pub mime: &'static str,
    pub name: &'static str,
    pub max_bytes: usize,
}
impl Format {
    pub(crate) fn write(
        &self,
        directory: &Path,
        bytes: &[u8],
        mut metadata: Value,
    ) -> Result<Artifact, Error> {
        if bytes.len() > self.max_bytes || bytes.len() > 64 * 1024 * 1024 {
            return Err("EVIDENCE_BOUNDS".into());
        }
        let id = Uuid::new_v4();
        let path = directory.join(format!("{}-{id}.{}", self.prefix, self.extension));
        crate::artifacts::write(&path, bytes)
            .map_err(|error| Error::caused("EVIDENCE_IO", error))?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        metadata["evidenceId"] = json!(id);
        metadata["attachment"] =
            json!({"path":path,"bytes":bytes.len(),"sha256":digest,"mimeType":self.mime});
        metadata["format"] = json!(self.name);
        Ok(Artifact {
            id,
            path,
            bytes: bytes.len(),
            digest,
            summary: metadata,
        })
    }
}
impl Artifact {
    pub(crate) fn read(&self) -> Result<Vec<u8>, Error> {
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|error| Error::caused("EVIDENCE_IO", error))?;
        if !metadata.is_file() || metadata.len() != self.bytes as u64 {
            return Err("EVIDENCE_CHANGED".into());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(&self.path)
            .map_err(|error| Error::caused("EVIDENCE_IO", error))?;
        let mut bytes = Vec::new();
        file.take(self.bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| Error::caused("EVIDENCE_IO", error))?;
        if format!("{:x}", Sha256::digest(&bytes)) != self.digest {
            return Err("EVIDENCE_CHANGED".into());
        }
        Ok(bytes)
    }
    pub(crate) fn remove(&self) -> Result<(), Error> {
        self.read()?;
        std::fs::remove_file(&self.path).map_err(|error| Error::caused("EVIDENCE_IO", error))?;
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Store {
    items: BTreeMap<Uuid, Artifact>,
    reservations: BTreeMap<Uuid, (usize, usize)>,
    pending: BTreeMap<Uuid, Vec<(Format, Vec<u8>, Value)>>,
}
impl Store {
    #[cfg(test)]
    pub(crate) fn capacity(&self, bytes: usize) -> Result<(), Error> {
        self.capacity_for(1, bytes)
    }
    pub(crate) fn capacity_for(&self, count: usize, bytes: usize) -> Result<(), Error> {
        if self.items.len().saturating_add(count).saturating_add(
            self.reservations
                .values()
                .map(|(count, _)| count)
                .sum::<usize>(),
        ) > 32
            || self.items.values().map(|item| item.bytes).sum::<usize>()
                + self
                    .reservations
                    .values()
                    .map(|(_, bytes)| bytes)
                    .sum::<usize>()
                + bytes
                > 128 * 1024 * 1024
        {
            Err("EVIDENCE_CAPACITY".into())
        } else {
            Ok(())
        }
    }
    #[cfg(test)]
    pub(crate) fn insert(&mut self, artifact: Artifact) -> Result<Value, Error> {
        self.capacity(artifact.bytes)?;
        let summary = artifact.summary.clone();
        self.items.insert(artifact.id, artifact);
        Ok(summary)
    }
    pub(crate) fn reserve(&mut self, owner: Uuid, count: usize, bytes: usize) -> Result<(), Error> {
        if count == 0 {
            return Err("EVIDENCE_BOUNDS".into());
        }
        if self.reservations.contains_key(&owner) {
            return Err("EVIDENCE_RESERVATION_CONFLICT".into());
        }
        self.capacity_for(count, bytes)?;
        self.reservations.insert(owner, (count, bytes));
        Ok(())
    }
    pub(crate) fn release(&mut self, owner: Uuid) {
        if !self.pending.contains_key(&owner) {
            self.reservations.remove(&owner);
        }
    }
    pub(crate) fn stage(
        &mut self,
        owner: Uuid,
        format: Format,
        bytes: Vec<u8>,
        mut metadata: Value,
    ) -> Result<(), Error> {
        let (slots, budget) = self
            .reservations
            .get(&owner)
            .ok_or("EVIDENCE_RESERVATION_REQUIRED")?;
        let count = self.pending.get(&owner).map_or(0, Vec::len);
        let held = self
            .pending
            .get(&owner)
            .map_or(0, |p| p.iter().map(|(_, b, _)| b.len()).sum::<usize>());
        if count >= *slots
            || held + bytes.len() > *budget
            || bytes.len() > format.max_bytes
            || bytes.len() > 64 * 1024 * 1024
        {
            return Err("EVIDENCE_BOUNDS".into());
        }
        metadata["captureId"] = json!(owner);
        self.pending
            .entry(owner)
            .or_default()
            .push((format, bytes, metadata));
        Ok(())
    }
    pub(crate) fn finalize(&mut self, directory: &Path, owner: Uuid) -> Result<Value, Error> {
        if !self.pending.contains_key(&owner) {
            return Err("UNKNOWN_CAPTURE".into());
        }
        let mut reports = Vec::new();
        while let Some((format, bytes, metadata)) = self.pending.get(&owner).and_then(|p| p.first())
        {
            let artifact = match format.write(directory, bytes, metadata.clone()) {
                Ok(artifact) => artifact,
                Err(mut error) => {
                    error.partial = Some(
                        json!({"captureId":owner,"reports":reports,"pending":self.pending_summary()}),
                    );
                    return Err(error);
                }
            };
            reports.push(self.insert_for(owner, artifact)?);
            self.pending
                .get_mut(&owner)
                .expect("Evidence owner")
                .remove(0);
            if self.pending[&owner].is_empty() {
                self.pending.remove(&owner);
            }
        }
        self.release(owner);
        Ok(json!({"captureId":owner,"reports":reports,"finalized":true}))
    }
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub(crate) fn has_pending_capture(&self, owner: Uuid) -> bool {
        self.pending.contains_key(&owner)
    }
    pub(crate) fn pending_summary(&self) -> Value {
        json!(self.pending.iter().map(|(id,items)|json!({"captureId":id,"formats":items.iter().map(|(f,_,_)|f.name).collect::<Vec<_>>(),"bytes":items.iter().map(|(_,b,_)|b.len()).sum::<usize>()})).collect::<Vec<_>>())
    }
    pub(crate) fn capacity_summary(&self) -> Value {
        json!({"items":self.items.len(),"bytes":self.items.values().map(|item|item.bytes).sum::<usize>(),"reservedItems":self.reservations.values().map(|(count,_)|count).sum::<usize>(),"reservedBytes":self.reservations.values().map(|(_,bytes)|bytes).sum::<usize>(),"maxItems":32,"maxBytes":128*1024*1024})
    }
    pub(crate) fn finalize_all(&mut self, directory: &Path) -> Result<(), Error> {
        for owner in self.pending.keys().copied().collect::<Vec<_>>() {
            self.finalize(directory, owner)?;
        }
        Ok(())
    }
    pub(crate) fn insert_for(&mut self, owner: Uuid, artifact: Artifact) -> Result<Value, Error> {
        let (count, bytes) = self
            .reservations
            .get_mut(&owner)
            .ok_or("EVIDENCE_RESERVATION_REQUIRED")?;
        if *count == 0 || *bytes < artifact.bytes {
            return Err("EVIDENCE_BOUNDS".into());
        }
        *count -= 1;
        *bytes -= artifact.bytes;
        if *count == 0 {
            self.reservations.remove(&owner);
        }
        let summary = artifact.summary.clone();
        self.items.insert(artifact.id, artifact);
        Ok(summary)
    }
    pub(crate) fn get(&self, id: Uuid) -> Result<Artifact, Error> {
        self.items
            .get(&id)
            .cloned()
            .ok_or("UNKNOWN_EVIDENCE".into())
    }
    pub(crate) fn forget(&mut self, id: Uuid) {
        self.items.remove(&id);
    }
    pub(crate) fn list(&self) -> Value {
        json!(
            self.items
                .values()
                .map(|artifact| artifact.summary.clone())
                .collect::<Vec<_>>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn staged_evidence_survives_a_failed_write_and_retries_without_recollection() {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("blocked");
        std::fs::write(&blocked, b"Acme").unwrap();
        let owner = Uuid::new_v4();
        let mut store = Store::default();
        store.reserve(owner, 1, 1024).unwrap();
        store
            .stage(
                owner,
                Format {
                    prefix: "audit",
                    extension: "json",
                    mime: "application/json",
                    name: "fixture-json",
                    max_bytes: 1024,
                },
                b"{\"Acme\":42}".to_vec(),
                json!({"private":true}),
            )
            .unwrap();
        assert_eq!(
            store.finalize(&blocked, owner).unwrap_err().code,
            "EVIDENCE_IO"
        );
        store.release(owner);
        assert_eq!(store.pending_summary()[0]["captureId"], json!(owner));
        assert!(store.capacity_for(32, 0).is_err());
        let value = store.finalize(root.path(), owner).unwrap();
        assert_eq!(value["reports"][0]["captureId"], json!(owner));
        assert_eq!(
            std::fs::read_to_string(value["reports"][0]["attachment"]["path"].as_str().unwrap())
                .unwrap(),
            "{\"Acme\":42}"
        );
        assert_eq!(
            store.finalize(root.path(), owner).unwrap_err().code,
            "UNKNOWN_CAPTURE"
        );
        assert!(store.pending_summary().as_array().unwrap().is_empty());
        assert!(store.capacity_for(31, 0).is_ok());
    }
}
