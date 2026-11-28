//! Verified upload payloads, intercepted choosers and private download collection.
use super::*;
use crate::browser::{
    objects::ObjectRef,
    page::Locator,
    transfer::{self, Chooser, Download, Fixture, Registration},
};
use base64::Engine as _;
use std::{fs, path::PathBuf};

impl Host {
    fn transfer_reference(&self) -> Reference {
        Reference {
            connection_id: self.state.lock().expect("Browser owner").connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    fn transfer_scope(&self, reference: Reference) -> Result<(), Error> {
        if reference.connection_id != self.state.lock().expect("Browser owner").connection {
            return Err("FOREIGN_REFERENCE".into());
        }
        Ok(())
    }
    pub(super) async fn fixture_register(&self, input: Registration) -> Result<Value, Error> {
        input.validate()?;
        let at_capacity = {
            let transfers = self.transfers.lock().expect("Transfers");
            transfers.fixtures.len() + transfers.directories.len() >= 64
        };
        if at_capacity {
            return Err("FIXTURE_CAPACITY".into());
        }
        let reference = self.transfer_reference();
        let fixture = Fixture::store(self.journal.directory(), reference, input, true).await?;
        let receipt = json!({"fixture":fixture.summary});
        self.transfers
            .lock()
            .expect("Transfers")
            .fixtures
            .insert(reference, fixture);
        Ok(receipt)
    }
    pub(super) async fn fixture_bytes(
        &self,
        reference: Reference,
    ) -> Result<(Value, Vec<u8>), Error> {
        self.transfer_scope(reference)?;
        let fixture = self
            .transfers
            .lock()
            .expect("Transfers")
            .fixtures
            .get(&reference)
            .cloned()
            .ok_or("STALE_REFERENCE")?;
        fixture.read().await
    }
    pub(super) async fn fixture_verify(&self, reference: Reference) -> Result<Value, Error> {
        let directory = self
            .transfers
            .lock()
            .expect("Transfers")
            .directories
            .get(&reference)
            .cloned();
        if let Some(directory) = directory {
            for source in &directory.sources {
                self.fixture_bytes(*source).await?;
            }
            crate::browser::directory::verify(directory.clone()).await?;
            return Ok(json!({"fixture":directory.summary,"verified":true}));
        }
        let (summary, _) = self.fixture_bytes(reference).await?;
        Ok(json!({"fixture":summary,"verified":true}))
    }
    pub(super) async fn upload(
        &self,
        page: Reference,
        frame: Option<Reference>,
        locator: Locator,
        fixtures: Vec<Reference>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(page, frame, deadline).await?;
        let (mut params, summaries) = self.attachment_parameters(fixtures).await?;
        params["selector"] = json!(locator.scoped_selector(scope.reference)?);
        params["strict"] = json!(true);
        self.connection()?
            .retained(&scope.wire, "setInputFiles", params, deadline, false)
            .await?;
        Ok(
            json!({"page":page,"frame":self.frame_current(&scope.wire,deadline).await?,"fixtures":summaries,"attached":true,"serverUploadVerified":false,"source":"playwright-set-input-files"}),
        )
    }
    pub(super) async fn chooser_event(
        self: &Arc<Self>,
        page: Reference,
        params: &Value,
    ) -> Result<(), Error> {
        let connection = self.connection()?;
        let element = connection
            .resolve(
                params["element"]["guid"]
                    .as_str()
                    .ok_or("MISSING_CHOOSER")?,
                "ElementHandle",
            )
            .await?;
        let owner = connection
            .retained(
                &element,
                "ownerFrame",
                json!({}),
                Instant::now() + Duration::from_secs(5),
                false,
            )
            .await?
            .ok_or("MISSING_RESULT")?;
        let frame = connection
            .resolve(
                owner["frame"]["guid"].as_str().ok_or("MISSING_FRAME")?,
                "Frame",
            )
            .await?;
        let reference = {
            let mut state = self.state.lock().expect("Browser owner");
            let frame = state.by_wire(&frame).ok_or("STALE_REFERENCE")?.reference;
            if state.ancestor(frame, Kind::Page) == Some(page) {
                Some(state.issue(Kind::Handle, element.clone(), Some(frame))?)
            } else {
                None
            }
        };
        // A chooser from another page's frame is refused on its own: its
        // element is released and the refusal recorded.
        let Some(reference) = reference else {
            let released = self.dispose_element(&element).await.is_ok();
            self.transfers.lock().expect("Transfers").refuse(
                json!({"kind":"chooser","page":page,"code":"FOREIGN_FRAME","released":released}),
            );
            return Ok(());
        };
        self.transfers.lock().expect("Transfers").choosers.insert(
            reference,
            Chooser {
                page,
                multiple: params["isMultiple"].as_bool().unwrap_or(false),
            },
        );
        self.record_activity(
            page,
            crate::browser::activity::Event::FileChooser,
            reference,
        )?;
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            // A resolve in progress finishes first; if it fails, the chooser
            // still expires.
            loop {
                let Some(host) = weak.upgrade() else {
                    return;
                };
                let changed = host.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if !host
                    .transfers
                    .lock()
                    .expect("Transfers")
                    .choosers
                    .contains_key(&reference)
                {
                    return;
                }
                if let Some(_claim) = Responding::claim(&host, reference) {
                    let _ = host.expire_chooser(reference).await;
                    return;
                }
                // Bounded, so a session dropped meanwhile is not kept alive.
                let _ = tokio::time::timeout(Duration::from_secs(1), changed).await;
            }
        });
        Ok(())
    }
    async fn expire_chooser(&self, reference: Reference) -> Result<(), Error> {
        let resource = self
            .state
            .lock()
            .expect("Browser owner")
            .resources
            .get(&reference.id)
            .cloned();
        if let Some(resource) = resource {
            self.dispose_element(&resource.wire).await?;
            self.state.lock().expect("Browser owner").retire(reference);
        }
        self.transfers
            .lock()
            .expect("Transfers")
            .choosers
            .remove(&reference);
        Ok(())
    }
    pub(super) async fn chooser_resolve(
        &self,
        reference: Reference,
        fixtures: Vec<Reference>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let wire = self.wire(reference, Kind::Handle)?;
        let multiple = self
            .transfers
            .lock()
            .expect("Transfers")
            .choosers
            .get(&reference)
            .ok_or("STALE_REFERENCE")?
            .multiple;
        if !multiple && fixtures.len() > 1 {
            return Err("SINGLE_FILE_INPUT".into());
        }
        let (params, summaries) = self.attachment_parameters(fixtures).await?;
        // Held until this resolve ends however it ends, so a failure leaves
        // the chooser to a retry or its expiry.
        let Some(_claim) = Responding::claim(self, reference) else {
            return Err("CALLBACK_ALREADY_RESOLVING".into());
        };
        self.connection()?
            .retained(&wire, "setInputFiles", params, deadline, true)
            .await?;
        self.dispose_element(&wire).await?;
        self.state.lock().expect("Browser owner").retire(reference);
        self.transfers
            .lock()
            .expect("Transfers")
            .choosers
            .remove(&reference);
        Ok(
            json!({"chooser":reference,"fixtures":summaries,"attached":true,"serverUploadVerified":false}),
        )
    }
    pub(super) async fn download_event(
        self: &Arc<Self>,
        page: Reference,
        params: &Value,
    ) -> Result<(), Error> {
        let connection = self.connection()?;
        let wire = connection
            .resolve(
                params["artifact"]["guid"]
                    .as_str()
                    .ok_or("MISSING_DOWNLOAD")?,
                "Artifact",
            )
            .await?;
        let initial = connection
            .initializer(&wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let incoming = PathBuf::from(initial["absolutePath"].as_str().ok_or("MISSING_DOWNLOAD")?);
        let root = self.journal.directory().join("incoming");
        let known = fs::canonicalize(&root).map_err(|e| Error::caused("DOWNLOAD_IO", e))?;
        // One unacceptable download is refused on its own: it is cancelled
        // and the refusal recorded, and the session goes on.
        let refusal = if incoming.parent().and_then(|p| fs::canonicalize(p).ok()) != Some(known) {
            Some("FOREIGN_DOWNLOAD_PATH")
        } else if !self.transfers.lock().expect("Transfers").download_room() {
            Some("DOWNLOAD_CAPACITY")
        } else {
            None
        };
        if let Some(code) = refusal {
            let cancelled = connection
                .retained(
                    &wire,
                    "cancel",
                    json!({}),
                    Instant::now() + Duration::from_secs(5),
                    true,
                )
                .await
                .is_ok();
            self.transfers.lock().expect("Transfers").refuse(json!({"kind":"download","page":page,"url":params["url"],"code":code,"cancelled":cancelled}));
            return Ok(());
        }
        let reference = self.transfer_reference();
        let summary = json!({"reference":reference,"page":page,"url":params["url"],"suggestedFilename":params["suggestedFilename"],"state":"pending","bytes":0});
        self.transfers.lock().expect("Transfers").downloads.insert(
            reference,
            Download {
                wire,
                incoming,
                summary,
            },
        );
        self.record_activity(page, crate::browser::activity::Event::Download, reference)?;
        let host = self.clone();
        let task = tokio::spawn(async move {
            host.collect_download(reference).await;
        });
        self.transfer_tasks.lock().await.push(task);
        Ok(())
    }
    async fn collect_download(self: Arc<Self>, reference: Reference) {
        let (wire, incoming) = {
            let transfers = self.transfers.lock().expect("Transfers");
            let d = &transfers.downloads[&reference];
            (d.wire.clone(), d.incoming.clone())
        };
        let collected = self.download_bytes(&wire, &incoming).await;
        if collected.is_err()
            && let Ok(connection) = self.connection()
        {
            let _ = connection
                .retained(
                    &wire,
                    "cancel",
                    json!({}),
                    Instant::now() + Duration::from_secs(5),
                    true,
                )
                .await;
        }
        let result = match collected {
            Ok(bytes) => {
                let path = self
                    .journal
                    .directory()
                    .join(format!("download-{}.bin", reference.id));
                let output = path.clone();
                let digest = format!("{:x}", Sha256::digest(&bytes));
                let count = bytes.len();
                tokio::task::spawn_blocking(move || {
                    crate::artifacts::write(&output, &bytes)?;
                    crate::artifacts::sync_directory(output.parent().unwrap())
                        .map_err(|e| crate::error::Error::new("DOWNLOAD_IO", e.to_string()))?;
                    Ok(())
                })
                .await
                .map_err(|_| Error::from("DOWNLOAD_IO"))
                .and_then(|r: crate::error::Result<()>| r.map_err(|e| Error::caused("DOWNLOAD_IO", e)))
                .map(|_| json!({"path":path,"bytes":count,"sha256":digest,"mimeType":"application/octet-stream"}))
            }
            Err(error) => Err(error),
        };
        let mut transfers = self.transfers.lock().expect("Transfers");
        if let Some(download) = transfers.downloads.get_mut(&reference) {
            match result {
                Ok(artifact) => {
                    download.summary["state"] = json!("complete");
                    download.summary["artifact"] = artifact;
                }
                Err(error) => {
                    if download.summary["state"] != "deleted" {
                        download.summary["state"] = json!("partial_or_failed");
                        download.summary["code"] = json!(error.code);
                    }
                }
            }
        }
        self.changed.notify_waiters();
    }
    async fn download_bytes(&self, wire: &ObjectRef, incoming: &Path) -> Result<Vec<u8>, Error> {
        let connection = self.connection()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        let stream = connection.retained(wire, "saveAsStream", json!({}), deadline, false);
        tokio::pin!(stream);
        let mut oversized = false;
        let reply = loop {
            tokio::select! {
                reply=&mut stream=>break reply,
                ()=tokio::time::sleep(Duration::from_millis(50))=>{
                    if !oversized&&fs::symlink_metadata(incoming).is_ok_and(|m|!m.is_file()||m.len()>transfer::DOWNLOAD_BYTES as u64){oversized=true;let _=connection.retained(wire,"cancel",json!({}),Instant::now()+Duration::from_secs(5),true).await;}
                }
            }
        };
        let reply = match reply {
            Ok(reply) => reply,
            Err(_) if oversized => return Err("DOWNLOAD_BOUNDS".into()),
            Err(error) => return Err(error.into()),
        };
        let stream = super::workflows::channel(&connection, reply, "stream", "Stream").await?;
        if oversized {
            connection
                .retained(
                    &stream,
                    "close",
                    json!({}),
                    Instant::now() + Duration::from_secs(5),
                    true,
                )
                .await?;
            return Err("DOWNLOAD_BOUNDS".into());
        }
        let mut bytes = Vec::new();
        let result = async {
            loop {
                let reply = connection
                    .retained(&stream, "read", json!({"size":65536}), deadline, false)
                    .await?
                    .ok_or("MISSING_RESULT")?;
                let chunk = base64::engine::general_purpose::STANDARD
                    .decode(reply["binary"].as_str().ok_or("MISSING_RESULT")?)
                    .map_err(|_| Error::from("DOWNLOAD_PROTOCOL"))?;
                if chunk.is_empty() {
                    break;
                }
                if chunk.len() > 65536 || bytes.len() + chunk.len() > transfer::DOWNLOAD_BYTES {
                    return Err(Error::from("DOWNLOAD_BOUNDS"));
                }
                bytes.extend(chunk);
                if let Some(download) = self
                    .transfers
                    .lock()
                    .expect("Transfers")
                    .downloads
                    .values_mut()
                    .find(|d| d.wire == *wire)
                {
                    download.summary["bytes"] = json!(bytes.len());
                }
            }
            Ok::<_, Error>(bytes)
        }
        .await;
        connection
            .retained(
                &stream,
                "close",
                json!({}),
                Instant::now() + Duration::from_secs(5),
                true,
            )
            .await?;
        result
    }
    pub(super) fn download_status(&self, reference: Reference) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let transfers = self.transfers.lock().expect("Transfers");
        Ok(
            json!({"download":transfers.downloads.get(&reference).ok_or("STALE_REFERENCE")?.summary}),
        )
    }
    pub(super) async fn download_cancel(
        &self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let wire = self
            .transfers
            .lock()
            .expect("Transfers")
            .downloads
            .get(&reference)
            .ok_or("STALE_REFERENCE")?
            .wire
            .clone();
        self.connection()?
            .retained(&wire, "cancel", json!({}), deadline, true)
            .await?;
        if let Some(download) = self
            .transfers
            .lock()
            .expect("Transfers")
            .downloads
            .get_mut(&reference)
            && download.summary["state"] != "complete"
        {
            download.summary["state"] = json!("cancel_requested");
        }
        self.download_status(reference)
    }
    pub(super) async fn download_delete(
        &self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let (wire, path) = {
            let transfers = self.transfers.lock().expect("Transfers");
            let d = transfers
                .downloads
                .get(&reference)
                .ok_or("STALE_REFERENCE")?;
            if d.summary["state"] == "deleted" {
                return Ok(json!({"deleted":true}));
            }
            if d.summary["state"] != "complete" && d.summary["state"] != "partial_or_failed" {
                return Err("DOWNLOAD_PENDING".into());
            }
            (
                d.wire.clone(),
                d.summary["artifact"]["path"].as_str().map(PathBuf::from),
            )
        };
        self.connection()?
            .retained(&wire, "delete", json!({}), deadline, true)
            .await?;
        if let Some(path) = path {
            tokio::fs::remove_file(path)
                .await
                .map_err(|e| Error::caused("DOWNLOAD_IO", e))?;
        }
        if let Some(d) = self
            .transfers
            .lock()
            .expect("Transfers")
            .downloads
            .get_mut(&reference)
        {
            d.summary["state"] = json!("deleted");
            d.summary.as_object_mut().unwrap().remove("artifact");
        }
        Ok(json!({"deleted":true}))
    }
}

/// Exclusive use of one chooser by a resolve or its expiry, released however
/// that work ends.
struct Responding<'a> {
    host: &'a Host,
    reference: Reference,
}
impl<'a> Responding<'a> {
    fn claim(host: &'a Host, reference: Reference) -> Option<Self> {
        host.responding
            .lock()
            .expect("Responses")
            .insert(reference)
            .then_some(Self { host, reference })
    }
}
impl Drop for Responding<'_> {
    fn drop(&mut self) {
        self.host
            .responding
            .lock()
            .expect("Responses")
            .remove(&self.reference);
        self.host.changed.notify_waiters();
    }
}
