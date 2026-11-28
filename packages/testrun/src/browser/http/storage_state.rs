//! Isolated HTTP state is initialized before allocation and exported as private evidence.
use super::*;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Input {
    File(String),
    Inline(Value),
}
impl std::fmt::Debug for Input {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpStorageState")
            .field(
                "source",
                &match self {
                    Self::File(_) => "private-file",
                    Self::Inline(_) => "inline",
                },
            )
            .finish()
    }
}
impl Input {
    pub(super) fn validate(&self) -> Result<(), Error> {
        match self {
            Self::File(path) => validate_path(path),
            Self::Inline(value) => validate(value),
        }
    }
    pub(super) fn schema() -> Value {
        let pair = json!({"type":"object","properties":{"name":{"type":"string"},"value":{"type":"string"}},"required":["name","value"],"additionalProperties":false});
        let cookie = json!({"type":"object","properties":{"name":{"type":"string"},"value":{"type":"string"},"domain":{"type":"string"},"path":{"type":"string"},"expires":{"type":"number"},"httpOnly":{"type":"boolean"},"secure":{"type":"boolean"},"sameSite":{"enum":["Strict","Lax","None"]},"partitionKey":{"type":"string"},"_crHasCrossSiteAncestor":{"type":"boolean"}},"required":["name","value","domain","path","expires","httpOnly","secure","sameSite"],"additionalProperties":false});
        let origin = json!({"type":"object","properties":{"origin":{"type":"string"},"localStorage":{"type":"array","items":pair},"indexedDB":{"type":"array","items":{"type":"object"}},"opfs":{"type":"array","items":{"type":"object"}}},"required":["origin","localStorage"],"additionalProperties":false});
        json!({"anyOf":[{"type":"string","minLength":1,"maxLength":4096},{"type":"object","properties":{"cookies":{"type":"array","items":cookie},"origins":{"type":"array","items":origin}},"additionalProperties":false},{"type":"null"}],"description":"Isolated HTTP context only: inline pinned APIRequest storage state or a caller-relative JSON file path. Copied before client allocation. At most 1 MiB state and 256 live cookies/64 KiB retained cookie data; unsafe cookie header values refuse. IndexedDB/OPFS records are validated, never executed. Browser-associated initialization refuses."})
    }
}
/// Browser-context `storageState`: inline state passes through; a bound file is
/// read with the HTTP rules. The caller validates the state for its engine.
pub(crate) async fn load(input: Input, deadline: Instant) -> Result<Value, Error> {
    let path = match input {
        Input::Inline(value) => return Ok(value),
        Input::File(path) => path,
    };
    validate_path(&path)?;
    let permit = super::files::reader(deadline).await?;
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let bytes = read_file(Path::new(&path))?;
        serde_json::from_slice(&bytes).map_err(|_| Error::from("INVALID_STORAGE_STATE"))
    });
    tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| Error::from("HTTP_DEADLINE"))?
        .map_err(|_| Error::from("HTTP_STORAGE_STATE_FILE_IO"))?
}
/// Pinned `storageState({path})`: missing directories are created owner-only
/// and the file is replaced atomically with an owner-only copy.
pub(crate) fn write_owner_only(path: &str, bytes: &[u8]) -> Result<Value, Error> {
    let target = Path::new(path);
    let parent = target.parent().ok_or("HTTP_STORAGE_STATE_PATH_INVALID")?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(parent)
        .map_err(|error| Error::caused("HTTP_STORAGE_STATE_EXPORT_IO", error))?;
    let temporary = parent.join(format!(".{}.partial", Uuid::new_v4()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        crate::artifacts::sync(&file)?;
        std::fs::rename(&temporary, target)
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(Error::caused("HTTP_STORAGE_STATE_EXPORT_IO", error));
    }
    Ok(json!({"path":path,"bytes":bytes.len(),"replacesExisting":true}))
}
/// Paths arrive absolute: the frontend binds relative ones to its working directory.
fn validate_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') || !Path::new(path).is_absolute()
    {
        return Err("HTTP_STORAGE_STATE_PATH_INVALID".into());
    }
    Ok(())
}
/// Pinned `newContext({storageState})` reads the file with `readFile`: links are
/// followed and permissions are not checked. Special files refuse without blocking.
fn read_file(path: &Path) -> Result<Vec<u8>, Error> {
    let max = super::super::storage::MAX_BYTES;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = options
        .open(path)
        .map_err(|error| Error::caused("HTTP_STORAGE_STATE_FILE_IO", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| Error::caused("HTTP_STORAGE_STATE_FILE_IO", error))?;
    if !metadata.is_file() {
        return Err("HTTP_STORAGE_STATE_FILE_NOT_REGULAR".into());
    }
    if metadata.len() > max as u64 {
        return Err("STORAGE_BOUNDS".into());
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::caused("HTTP_STORAGE_STATE_FILE_IO", error))?;
    if bytes.len() > max {
        return Err("STORAGE_BOUNDS".into());
    }
    Ok(bytes)
}
/// Pinned `storageState({path})` creates missing directories and replaces the
/// file. The copy is owner-only because the state carries credentials; created
/// directories are owner-only too. The evidence stays published when this fails.
pub(crate) async fn copy_to_path(
    artifact: super::super::artifacts::Artifact,
    mut receipt: Value,
    path: Option<String>,
    deadline: Instant,
) -> Result<Value, Error> {
    let Some(path) = path else {
        return Ok(receipt);
    };
    let evidence = artifact.id;
    let work = tokio::task::spawn_blocking(move || -> Result<Value, Error> {
        write_owner_only(&path, &artifact.read()?)
    });
    let outcome = match tokio::time::timeout_at(deadline, work).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => Err(Error::caused("HTTP_STORAGE_STATE_EXPORT_IO", error)),
        Err(_) => Err("HTTP_DEADLINE".into()),
    };
    match outcome {
        Ok(file) => {
            receipt["file"] = file;
            Ok(receipt)
        }
        Err(mut error) => {
            error.partial = Some(
                json!({"evidenceId":evidence,"evidencePublished":true,"fileWritten":"unknown-or-failed","replayed":false}),
            );
            Err(error)
        }
    }
}
fn validate(value: &Value) -> Result<(), Error> {
    super::super::schema::encoded_len(value, super::super::storage::MAX_BYTES)
        .map_err(|_| Error::from("STORAGE_BOUNDS"))?;
    super::super::schema::validate(
        "PlaywrightNewRequestParams",
        Some(&json!({"storageState":value})),
        &super::super::objects::ObjectTable::default(),
    )
    .map_err(|_| Error::from("INVALID_STORAGE_STATE"))?;
    Ok(())
}
pub(crate) async fn prepare(
    mut options: super::Options,
    deadline: Instant,
) -> Result<super::Options, Error> {
    if Instant::now() >= deadline {
        return Err("HTTP_DEADLINE".into());
    }
    options.validate()?;
    let path = match &options.storage_state {
        Some(Input::File(path)) => path.clone(),
        _ => return Ok(options),
    };
    options.storage_state = None;
    let permit = super::files::reader(deadline).await?;
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if Instant::now() >= deadline {
            return Err(Error::from("HTTP_DEADLINE"));
        }
        let bytes = read_file(Path::new(&path))?;
        let value =
            serde_json::from_slice(&bytes).map_err(|_| Error::from("INVALID_STORAGE_STATE"))?;
        validate(&value)?;
        if Instant::now() >= deadline {
            return Err(Error::from("HTTP_DEADLINE"));
        }
        options.storage_state = Some(Input::Inline(value));
        Ok(options)
    });
    tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| Error::from("HTTP_DEADLINE"))?
        .map_err(|error| Error::caused("STORAGE_IO", error))?
}
pub(super) fn initialize(input: Option<Input>) -> Result<(Cookies, Vec<Value>), Error> {
    let state = match input {
        None => json!({}),
        Some(Input::Inline(value)) => value,
        Some(Input::File(_)) => return Err("HTTP_STORAGE_STATE_UNPREPARED".into()),
    };
    validate(&state)?;
    let cookies = Cookies::initialize(state["cookies"].as_array().map_or(&[][..], Vec::as_slice))?;
    let origins = state["origins"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|origin| {
            let mut normalized = json!({"indexedDB":[],"opfs":[]});
            normalized
                .as_object_mut()
                .expect("Origin defaults")
                .extend(origin.as_object().expect("Validated origin").clone());
            normalized
        })
        .collect();
    Ok((cookies, origins))
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default, rename = "indexedDB")]
    pub indexed_db: bool,
    #[serde(default)]
    pub opfs: bool,
    /// Pinned `storageState({path})`; relative paths bind to the caller first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}
impl Options {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.path.as_deref().map_or(Ok(()), validate_path)
    }
}
impl Contexts {
    pub(crate) fn storage_state(
        &mut self,
        context: Reference,
        options: Options,
        directory: &Path,
    ) -> Result<Value, Error> {
        let client = self.get(context)?;
        if client.browser.is_some() {
            return Err("HTTP_BROWSER_STORAGE_STATE_UNAVAILABLE".into());
        }
        let cookies = client.cookies.lock().expect("HTTP cookies").snapshot();
        let origins = client
            .storage_origins
            .iter()
            .map(|origin| {
                let mut origin = origin.clone();
                if !options.indexed_db {
                    origin["indexedDB"] = json!([]);
                }
                if !options.opfs {
                    origin
                        .as_object_mut()
                        .expect("Stored origin")
                        .remove("opfs");
                }
                origin
            })
            .collect::<Vec<_>>();
        let state = json!({"cookies":cookies,"origins":origins});
        self.publish_storage_state(context, options, state, directory)
    }
    /// The artifact a successful state receipt published, for a caller copy.
    pub(crate) fn published_storage_state(
        &self,
        receipt: &Value,
    ) -> Result<super::super::artifacts::Artifact, Error> {
        let id = serde_json::from_value(receipt["reports"][0]["evidenceId"].clone())
            .map_err(|_| Error::from("MISSING_RESULT"))?;
        self.evidence_get(id)
    }
    pub(crate) fn publish_storage_state(
        &mut self,
        context: Reference,
        options: Options,
        state: Value,
        directory: &Path,
    ) -> Result<Value, Error> {
        let (bytes, metadata) = self.storage_state_bytes(context, &options, &state)?;
        let capture = Uuid::new_v4();
        self.evidence.reserve(capture, 1, bytes.len())?;
        self.stage_storage_state_bytes(capture, bytes, metadata)?;
        self.finalize_evidence(directory, capture)
    }
    pub(crate) fn reserve_storage_state(&mut self, capture: Uuid) -> Result<(), Error> {
        self.evidence
            .reserve(capture, 1, super::super::storage::MAX_BYTES)
    }
    pub(crate) fn release_storage_state(&mut self, capture: Uuid) {
        self.evidence.release(capture);
    }
    pub(crate) fn stage_reserved_storage_state(
        &mut self,
        context: Reference,
        options: Options,
        state: Value,
        capture: Uuid,
    ) -> Result<(), Error> {
        let (bytes, metadata) = self.storage_state_bytes(context, &options, &state)?;
        self.stage_storage_state_bytes(capture, bytes, metadata)
    }
    fn storage_state_bytes(
        &self,
        context: Reference,
        options: &Options,
        state: &Value,
    ) -> Result<(Vec<u8>, Value), Error> {
        let associated = self.get(context)?.options.browser_context.is_some();
        super::super::schema::encoded_len(state, super::super::storage::MAX_BYTES)
            .map_err(|_| Error::from("STORAGE_BOUNDS"))?;
        super::super::schema::validate(
            "APIRequestContextStorageStateResult",
            Some(state),
            &super::super::objects::ObjectTable::default(),
        )
        .map_err(|_| Error::from("INVALID_STORAGE_STATE"))?;
        let cookie_count = state["cookies"]
            .as_array()
            .expect("Validated cookies")
            .len();
        let origin_count = state["origins"]
            .as_array()
            .expect("Validated origins")
            .len();
        let bytes = serde_json::to_vec(state).map_err(|_| Error::from("INVALID_STORAGE_STATE"))?;
        Ok((
            bytes,
            json!({"httpContext":context,"cookieCount":cookie_count,"originCount":origin_count,"indexedDB":options.indexed_db,"opfs":options.opfs,"browserAssociated":associated,"secretBearing":true,"privacy":"private-explicit-http-storage-state","replayed":false}),
        ))
    }
    fn stage_storage_state_bytes(
        &mut self,
        capture: Uuid,
        bytes: Vec<u8>,
        metadata: Value,
    ) -> Result<(), Error> {
        let format = super::super::artifacts::Format {
            prefix: "http-state",
            extension: "json",
            mime: "application/json",
            name: "playwright-storage-state-v1",
            max_bytes: super::super::storage::MAX_BYTES,
        };
        if let Err(error) = self.evidence.stage(capture, format, bytes, metadata) {
            self.evidence.release(capture);
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn finalize_evidence(
        &mut self,
        directory: &Path,
        capture: Uuid,
    ) -> Result<Value, Error> {
        if !self.evidence.has_pending_capture(capture) {
            return Err("UNKNOWN_CAPTURE".into());
        }
        private_evidence_directory(directory, "http-state").and_then(|directory|self.evidence.finalize(&directory, capture)).map_err(|mut error| {
            error.partial = Some(json!({"captureId":capture,"stateRetained":true,"nextAction":"http_evidence_finalize","replayed":false,"cause":error.partial}));
            error
        })
    }
    pub(crate) fn finalize_all_evidence(&mut self, directory: &Path) -> Result<(), Error> {
        if !self.evidence.has_pending() {
            return Ok(());
        }
        private_evidence_directory(directory, "http-state").and_then(|directory|self.evidence.finalize_all(&directory)).map_err(|error| {
            let mut pending = Error::from("CLEANUP_PENDING");
            pending.partial = Some(json!({"stage":"http-storage-state-publication","cause":{"code":error.code,"partial":error.partial},"pendingArtifacts":self.evidence.pending_summary(),"replayed":false}));
            pending
        })
    }
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"http_context_storage_state"},"httpContext":reference,"options":{"type":"object","properties":{"indexedDB":{"type":"boolean","default":false},"opfs":{"type":"boolean","default":false},"path":{"type":"string","minLength":1,"maxLength":4096,"description":"Also write a copy here: caller-relative, parents created, replaced, owner-only."}},"additionalProperties":false}},"required":["operation","httpContext"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"http_evidence_finalize"},"captureId":{"type":"string","format":"uuid"}},"required":["operation","captureId"],"additionalProperties":false}),
    ]
}
