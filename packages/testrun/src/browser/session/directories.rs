//! Directory fixture registration and verified local-driver attachment parameters.
use super::*;
use crate::browser::directory::{self, Entry};

impl Host {
    pub(super) async fn directory_register(
        &self,
        name: String,
        entries: Vec<Entry>,
    ) -> Result<Value, Error> {
        directory::validate(&name, &entries)?;
        let at_capacity = {
            let transfers = self.transfers.lock().expect("Transfers");
            transfers.directories.len() >= 16
                || transfers.directories.len() + transfers.fixtures.len() >= 64
        };
        if at_capacity {
            return Err("FIXTURE_CAPACITY".into());
        }
        let mut inputs = Vec::new();
        for entry in entries {
            let (summary, bytes) = self.fixture_bytes(entry.fixture).await?;
            inputs.push((entry, summary, bytes));
        }
        let reference = Reference {
            connection_id: self.state.lock().expect("Browser owner").connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let directory =
            directory::create(self.journal.directory().into(), reference, name, inputs).await?;
        let summary = directory.summary.clone();
        self.transfers
            .lock()
            .expect("Transfers")
            .directories
            .insert(reference, directory);
        Ok(json!({"fixture":summary}))
    }
    pub(super) async fn attachment_parameters(
        &self,
        fixtures: Vec<Reference>,
    ) -> Result<(Value, Vec<Value>), Error> {
        let directories = self
            .transfers
            .lock()
            .expect("Transfers")
            .directories
            .clone();
        if fixtures
            .iter()
            .any(|reference| directories.contains_key(reference))
        {
            if self.attached.load(Ordering::SeqCst) {
                return Err("ATTACHMENT_DIRECTORY_UPLOAD_UNSUPPORTED".into());
            }
            if fixtures.len() != 1 {
                return Err("DIRECTORY_FIXTURE_REQUIRED".into());
            }
            let directory = directories
                .get(&fixtures[0])
                .ok_or("STALE_REFERENCE")?
                .clone();
            for source in &directory.sources {
                self.fixture_bytes(*source).await?;
            }
            directory::verify(directory.clone()).await?;
            return Ok((
                json!({"localDirectory":directory.path}),
                vec![directory.summary],
            ));
        }
        let (payloads, summaries) =
            crate::browser::transfer::payloads(fixtures, |reference| self.fixture_bytes(reference))
                .await?;
        let mut params = json!({});
        params["payloads"] = Value::Array(payloads);
        Ok((params, summaries))
    }
}
