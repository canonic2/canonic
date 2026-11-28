//! Finished evidence integrity is checked once per physical metadata revision.
//! The cache holds validation stamps, never bytes, results or cursor state.
use super::*;

impl Owners {
    pub(super) fn verify_output(
        &self,
        dir: &Path,
        collector: &str,
        expected: Option<&str>,
        size: u64,
    ) -> Result<()> {
        let Some(expected) = expected else {
            return Ok(());
        };
        validate_id(collector, "collector-")?;
        let path = dir.join(format!("{collector}.bin"));
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| Error::new("EVIDENCE_IO", "Collector output unavailable"))?;
        if !metadata.is_file() || metadata.len() != size {
            return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
        }
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            )
        };
        #[cfg(not(unix))]
        let identity = metadata.created().ok();
        let stamp = format!(
            "{expected}:{}:{:?}:{identity:?}",
            metadata.len(),
            metadata.modified().ok()
        );
        if self
            .verified_outputs
            .lock()
            .expect("evidence validation")
            .get(&path)
            == Some(&stamp)
        {
            return Ok(());
        }
        if digest(&read_data(dir, collector)?) != expected {
            return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
        }
        let mut verified = self.verified_outputs.lock().expect("evidence validation");
        if verified.len() >= 4096 {
            verified.clear();
        }
        verified.insert(path, stamp);
        Ok(())
    }
}
