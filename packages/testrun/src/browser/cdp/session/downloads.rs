//! Browser download events, bounded collection and cancellation in owned paths.
use super::*;
use crate::browser::{activity::Event as Activity, transfer::DOWNLOAD_BYTES};

impl Worker {
    pub(super) async fn configure_downloads(
        &mut self,
        id: Uuid,
        context: Reference,
        accept: bool,
        deadline: Instant,
    ) -> Result<(), Error> {
        if !accept {
            return Ok(());
        }
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let path = self
            .journal
            .directory()
            .join(format!("incoming-{}", context.id));
        crate::artifacts::directory(&path).map_err(|error| Error::caused("DOWNLOAD_IO", error))?;
        self.transfers
            .download_contexts
            .insert(context, path.clone());
        self.call(id,None,"Browser.setDownloadBehavior",json!({"behavior":"allowAndName","browserContextId":native,"downloadPath":path,"eventsEnabled":true}),deadline).await?;
        Ok(())
    }
    pub(super) fn download_event(&mut self, event: &Event) -> Result<(), Error> {
        match event.method.as_str() {
            "Browser.downloadWillBegin" => {
                let guid = event.params["guid"].as_str().ok_or("BACKEND_PROTOCOL")?;
                Uuid::parse_str(guid).map_err(|_| Error::from("BACKEND_PROTOCOL"))?;
                let native = event.params["frameId"].as_str().ok_or("BACKEND_PROTOCOL")?;
                let frame = self
                    .index
                    .list(Kind::Frame)
                    .into_iter()
                    .map(|(reference, _)| reference)
                    .find(|frame| {
                        self.index
                            .native(*frame, Kind::Frame)
                            .is_ok_and(|frame| frame == native)
                    })
                    .ok_or("FOREIGN_REFERENCE")?;
                let page = self.index.owning_page(frame).map_err(resource_error)?;
                let context = self
                    .index
                    .list(Kind::Page)
                    .into_iter()
                    .find(|(reference, _)| *reference == page)
                    .and_then(|(_, parent)| parent)
                    .ok_or("INVALID_SCOPE")?;
                if !self.transfers.download_contexts.contains_key(&context) {
                    return Err("DOWNLOADS_DISABLED".into());
                }
                if self.transfers.downloads.len() >= crate::browser::transfer::DOWNLOADS {
                    self.transfers
                        .downloads
                        .retain(|_, download| download.summary["state"] != "deleted");
                }
                if self.transfers.downloads.len() >= crate::browser::transfer::DOWNLOADS {
                    if self.transfers.pending_cancels.len() >= 32 {
                        return Err("DOWNLOAD_CAPACITY".into());
                    }
                    self.transfers
                        .pending_cancels
                        .push_back((guid.into(), context));
                    if self.transfers.refused.len() >= 16 {
                        self.transfers.refused.pop_front();
                    }
                    self.transfers.refused.push_back(
                        json!({"kind":"download","page":page,"code":"DOWNLOAD_CAPACITY"}),
                    );
                    return Ok(());
                }
                let reference = self.transfer_reference();
                let cancel = false;
                self.transfers.downloads.insert(reference,transfers::Download{guid:guid.into(),context,summary:json!({"reference":reference,"page":page,"url":crate::browser::diagnostics::url(event.params["url"].as_str().unwrap_or_default()),"suggestedFilename":event.params["suggestedFilename"].as_str().unwrap_or_default().chars().take(128).collect::<String>(),"state":"pending","bytes":0}),collect:false,cancel,expires:Instant::now()+Duration::from_secs(120),cancel_attempts:0});
                self.record_activity(page, Activity::Download, reference);
            }
            "Browser.downloadProgress" => {
                let guid = event.params["guid"].as_str().ok_or("BACKEND_PROTOCOL")?;
                if let Some(download) = self
                    .transfers
                    .downloads
                    .values_mut()
                    .find(|download| download.guid == guid)
                {
                    let bytes = event.params["receivedBytes"]
                        .as_f64()
                        .ok_or("BACKEND_PROTOCOL")?;
                    download.summary["bytes"] = json!(bytes);
                    if bytes > DOWNLOAD_BYTES as f64 {
                        download.cancel = true;
                        download.summary["code"] = json!("DOWNLOAD_BOUNDS");
                    }
                    match event.params["state"].as_str() {
                        Some("completed") => download.collect = true,
                        Some("canceled") => {
                            download.summary["state"] = json!("partial_or_failed");
                            download.cancel = false;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) async fn poll_transfers(&mut self) {
        for download in self.transfers.downloads.values_mut() {
            if download.summary["state"] == "pending" && Instant::now() >= download.expires {
                download.cancel = true;
                download.summary["code"] = json!("DOWNLOAD_TIMEOUT");
            }
        }
        while let Some((guid, context)) = self.transfers.pending_cancels.pop_front() {
            let native = self.index.native(context, Kind::Context);
            if let (Ok(native), Some(connection)) = (native, &self.connection) {
                let _ = connection
                    .call(
                        None,
                        "Browser.cancelDownload",
                        json!({"guid":guid,"browserContextId":native}),
                        Lane::Control,
                        Instant::now() + Duration::from_secs(2),
                    )
                    .await;
            }
        }
        let refs = self
            .transfers
            .downloads
            .iter()
            .filter(|(_, download)| download.collect || download.cancel)
            .map(|(reference, _)| *reference)
            .collect::<Vec<_>>();
        for reference in refs {
            let download = self
                .transfers
                .downloads
                .get_mut(&reference)
                .expect("Pending download");
            let cancel = download.cancel;
            let collect = download.collect;
            download.cancel = false;
            download.collect = false;
            if cancel {
                let result = self
                    .cancel_download(reference, Instant::now() + Duration::from_secs(3))
                    .await;
                if result.is_err() {
                    let download = self
                        .transfers
                        .downloads
                        .get_mut(&reference)
                        .expect("Download");
                    download.summary["state"] = json!("cancel_failed");
                    download.cancel_attempts += 1;
                    download.cancel = download.cancel_attempts < 3;
                    if !download.cancel {
                        self.lose("DOWNLOAD_CLEANUP_PENDING");
                    }
                }
            } else if collect {
                let result = self.collect_native_download(reference).await;
                if let Err(error) = result {
                    let download = self
                        .transfers
                        .downloads
                        .get_mut(&reference)
                        .expect("Download");
                    download.summary["state"] = json!("partial_or_failed");
                    download.summary["code"] = json!(error.code);
                }
            }
        }
        self.publish();
    }
    async fn collect_native_download(&mut self, reference: Reference) -> Result<(), Error> {
        let download = self
            .transfers
            .downloads
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        let path = self
            .transfers
            .download_contexts
            .get(&download.context)
            .ok_or("DOWNLOAD_IO")?
            .join(&download.guid);
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|error| Error::caused("DOWNLOAD_IO", error))?;
        if !metadata.is_file() || metadata.len() > DOWNLOAD_BYTES as u64 {
            return Err("DOWNLOAD_BOUNDS".into());
        }
        let source = path.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(source)
                .map_err(|error| Error::caused("DOWNLOAD_IO", error))?
                .take(DOWNLOAD_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| Error::caused("DOWNLOAD_IO", error))?;
            Ok::<_, Error>(bytes)
        })
        .await
        .map_err(|_| Error::from("DOWNLOAD_IO"))??;
        if bytes.len() > DOWNLOAD_BYTES {
            return Err("DOWNLOAD_BOUNDS".into());
        }
        let output = self
            .journal
            .directory()
            .join(format!("download-{}.bin", reference.id));
        crate::artifacts::write(&output, &bytes)
            .map_err(|error| Error::caused("DOWNLOAD_IO", error))?;
        let download = self
            .transfers
            .downloads
            .get_mut(&reference)
            .expect("Download");
        download.summary["state"] = json!("complete");
        download.summary["artifact"] = json!({"path":output,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"mimeType":"application/octet-stream"});
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| Error::caused("DOWNLOAD_IO", error))?;
        Ok(())
    }
    pub(super) fn download_status(&self, reference: Reference) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        Ok(
            json!({"download":self.transfers.downloads.get(&reference).ok_or("STALE_REFERENCE")?.summary}),
        )
    }
    pub(super) async fn cancel_download(
        &mut self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let download = self
            .transfers
            .downloads
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        let native = self
            .index
            .native(download.context, Kind::Context)
            .map_err(resource_error)?;
        self.connection
            .as_ref()
            .ok_or("BACKEND_LOST")?
            .call(
                None,
                "Browser.cancelDownload",
                json!({"guid":download.guid,"browserContextId":native}),
                Lane::Control,
                deadline,
            )
            .await
            .map_err(protocol_error)?;
        self.transfers
            .downloads
            .get_mut(&reference)
            .expect("Download")
            .summary["state"] = json!("cancel_requested");
        self.download_status(reference)
    }
    pub(super) async fn delete_download(&mut self, reference: Reference) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let download = self
            .transfers
            .downloads
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        if download.summary["state"] == "deleted" {
            return Ok(json!({"deleted":true}));
        }
        if download.summary["state"] != "complete"
            && download.summary["state"] != "partial_or_failed"
        {
            return Err("DOWNLOAD_PENDING".into());
        }
        if let Some(path) = download.summary["artifact"]["path"].as_str() {
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| Error::caused("DOWNLOAD_IO", error))?;
        }
        let download = self
            .transfers
            .downloads
            .get_mut(&reference)
            .expect("Download");
        download.summary["state"] = json!("deleted");
        download
            .summary
            .as_object_mut()
            .expect("Download object")
            .remove("artifact");
        Ok(json!({"deleted":true}))
    }
}
