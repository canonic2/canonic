//! Private Playwright storage-state snapshots and explicit destructive restore contracts.
use super::contract::{Engine, Reference};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
pub(crate) mod cache;
pub(crate) mod cookies;
mod files;
pub(crate) mod native;
pub(crate) mod web;
pub(crate) use files::read_private;

/// Whether any origin of pinned state carries OPFS entries.
pub(crate) fn opfs_included(state: &Value) -> bool {
    state["origins"]
        .as_array()
        .is_some_and(|origins| origins.iter().any(|origin| origin.get("opfs").is_some()))
}

/// Context cookie and page `WebStorage` operations, alike on every engine,
/// and native Chrome Cache Storage.
pub(crate) fn capabilities(available: bool, chrome: bool) -> Value {
    let mut operations = vec![
        "context_cookies_read",
        "context_cookies_add",
        "context_cookies_clear",
        "page_web_storage_read",
        "page_web_storage_update",
    ];
    if chrome {
        operations.extend(["context_cache_storage_read", "context_cache_storage_delete"]);
    }
    json!({"available":available,"operations":operations,"cacheStorage":{"available":available && chrome,"scope":"origin-in-owned-context-through-a-live-page","maxEntriesPerCache":cache::MAX_ENTRIES,"maxBodyBytes":cache::MAX_BODY_BYTES,"reason":(!chrome).then_some("no-pinned-driver-api")},"cookies":{"scope":"owned-context","readOrder":"name-domain-path-partition","urlSelection":"pinned-domain-suffix-path-prefix-secure-localhost","clearCriteria":["exact-string","ecmascript-regexp"],"maxAddCookies":256},"webStorage":{"kinds":["local","session"],"scope":"page-main-frame-utility-world","readOrder":"item-name","maxNameBytes":web::MAX_NAME_BYTES,"maxValueBytes":web::MAX_VALUE_BYTES},"secretBearingReads":true,"fullSdkParity":false})
}

/// One keyset page of `rows`, sorted here by their unique keys, through the
/// shared page builder. Returns the projected items and the next cursor.
pub(crate) fn keyset<T>(
    name: &str,
    query: &Value,
    mut rows: Vec<(Vec<u8>, T)>,
    mut view: impl FnMut(T) -> Value,
    cursor: Option<&str>,
    limit: Option<u64>,
    read_tool: &str,
) -> Result<(Vec<Value>, Option<String>), super::session::Error> {
    use canonic_mcp_kit::page::{Builder, Limits, Position, Request};
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let request =
        Request::from_cli(cursor, limit, Limits::STANDARD).map_err(|error| error.code())?;
    let scope = crate::pagination::scope(name, query)
        .map_err(|_| super::session::Error::from("INTERNAL"))?;
    let after = match request
        .cursor
        .as_deref()
        .map(|cursor| scope.decode(cursor))
        .transpose()
        .map_err(|error| error.code())?
    {
        Some(Position::Keyset(key)) => Some(key),
        Some(Position::Offset(_)) => return Err("invalid_cursor".into()),
        None => None,
    };
    let mut builder = Builder::new(
        &scope,
        request.limit,
        Limits::STANDARD,
        crate::protocol::MAX_REPLY,
        read_tool,
    )
    .map_err(|error| error.code())?;
    let mut rows = rows
        .into_iter()
        .filter(|(key, _)| after.as_ref().is_none_or(|after| key > after))
        .peekable();
    // A row the byte budget refuses stays unread, so the page keeps a cursor.
    let mut more = false;
    loop {
        if builder.is_full() {
            more = rows.peek().is_some();
            break;
        }
        let Some((key, row)) = rows.next() else {
            break;
        };
        if !builder
            .push(view(row), &Position::Keyset(key))
            .map_err(|error| error.code())?
        {
            more = true;
            break;
        }
    }
    let page = builder.finish(more);
    Ok((page.items, page.next_cursor))
}

pub(crate) const MAX_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_SNAPSHOTS: usize = 16;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default, rename = "indexedDB")]
    pub indexed_db: bool,
    #[serde(default)]
    pub opfs: bool,
    /// Pinned `storageState({credentials})`: the context's virtual WebAuthn
    /// credentials, private keys included, in the private snapshot.
    #[serde(default)]
    pub credentials: bool,
    /// Pinned `storageState({path})`; the frontend binds relative paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}
impl Options {
    pub(crate) fn validate(&self, engine: Engine) -> Result<(), &'static str> {
        if self.opfs && engine == Engine::Webkit {
            return Err("OPFS_UNSUPPORTED");
        }
        if self.path.as_deref().is_some_and(|path| {
            path.len() > 4096 || path.contains('\0') || !std::path::Path::new(path).is_absolute()
        }) {
            return Err("HTTP_STORAGE_STATE_PATH_INVALID");
        }
        Ok(())
    }
}

/// Writes an exported snapshot's state to the caller's `path`. The private
/// snapshot stays retained when the copy fails.
pub(crate) async fn copy_export(
    mut result: Value,
    path: Option<String>,
    state: &Value,
    deadline: tokio::time::Instant,
) -> Result<Value, super::session::Error> {
    let Some(path) = path else {
        return Ok(result);
    };
    let bytes = serde_json::to_vec(state)
        .map_err(|_| super::session::Error::from("INVALID_STORAGE_STATE"))?;
    let work = tokio::task::spawn_blocking(move || super::http::storage_state_write(&path, &bytes));
    let written = match tokio::time::timeout_at(deadline, work).await {
        Ok(Ok(written)) => written,
        Ok(Err(error)) => Err(super::session::Error::caused(
            "HTTP_STORAGE_STATE_EXPORT_IO",
            error,
        )),
        Err(_) => Err("HTTP_DEADLINE".into()),
    };
    match written {
        Ok(written) => {
            result["export"] = written;
            Ok(result)
        }
        Err(mut error) => {
            error.partial = Some(
                json!({"snapshot":result["snapshot"],"stateRetained":true,"exportComplete":false,"replayed":false}),
            );
            Err(error)
        }
    }
}

pub(crate) const PLAYWRIGHT_FORMAT: &str = "playwright-storage-state-v1";

/// The number of virtual credentials pinned state restores. The pinned server
/// installs an authenticator only for a nonempty array.
pub(crate) fn credential_count(state: &Value) -> usize {
    state["credentials"].as_array().map_or(0, Vec::len)
}

/// Restoring credentials replaces the context's real authenticators, so it
/// needs the explicit `installAuthenticator` request.
pub(crate) fn require_installation(state: &Value, install: bool) -> Result<(), &'static str> {
    if credential_count(state) > 0 && !install {
        return Err("AUTHENTICATOR_INSTALL_REQUIRED");
    }
    Ok(())
}

/// Creation `storageState` has no explicit installation request; credential
/// restore goes through `storage_apply` with `installAuthenticator`.
pub(crate) fn refuse_creation_credentials(state: &Value) -> Result<(), &'static str> {
    if credential_count(state) > 0 {
        return Err("VIRTUAL_CREDENTIALS_REQUIRE_APPLY");
    }
    Ok(())
}

/// How `storage_apply` treats state the snapshot does not name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApplyMode {
    /// Pinned `setStorageState`: cache and cookies cleared, every visited and
    /// snapshot origin cleared and restored.
    Replace,
    /// Native Chrome only: the snapshot's items upsert; nothing else changes.
    Merge,
}

pub(crate) struct Snapshot {
    pub reference: Reference,
    pub state: Value,
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: usize,
    pub published: bool,
    /// `PLAYWRIGHT_FORMAT` on Firefox/WebKit, `native::FORMAT` on Chrome.
    pub format: &'static str,
}
impl Snapshot {
    pub fn summary(&self) -> Value {
        let mut summary = json!({"reference":self.reference,"format":self.format,"bytes":self.bytes,"sha256":self.sha256,"cookieCount":self.state["cookies"].as_array().map_or(0, Vec::len),"originCount":self.state["origins"].as_array().map_or(0, Vec::len),"credentialCount":credential_count(&self.state),"artifact":self.published.then(||json!({"path":self.path,"mimeType":"application/json"})),"published":self.published,"secretBearing":true,"retention":"private-evidence-until-explicit-storage-forget","sessionStorageIncluded":false});
        if self.format == PLAYWRIGHT_FORMAT {
            summary["driverVersion"] = json!(super::DRIVER_VERSION);
        } else {
            summary["included"] = self.state["included"].clone();
            summary["backend"] = self.state["backend"].clone();
        }
        summary
    }
}
#[derive(Default)]
pub(crate) struct Store(pub BTreeMap<Reference, Snapshot>);
impl Store {
    pub fn summary(&self) -> Value {
        json!({"snapshots":self.0.values().map(Snapshot::summary).collect::<Vec<_>>(),"maxSnapshots":MAX_SNAPSHOTS,"maxBytesPerSnapshot":MAX_BYTES})
    }
    pub(crate) fn capacity(&self) -> Result<(), super::session::Error> {
        if self.0.len() >= MAX_SNAPSHOTS {
            return Err("STORAGE_CAPACITY".into());
        }
        Ok(())
    }
    /// Retains validated `state` as a private artifact in `directory`.
    pub(crate) fn retain(
        &mut self,
        connection: uuid::Uuid,
        directory: &std::path::Path,
        state: Value,
        format: &'static str,
    ) -> Result<Value, super::session::Error> {
        use sha2::Digest;
        self.capacity()?;
        let reference = Reference {
            connection_id: connection,
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let bytes = serde_json::to_vec(&state)
            .map_err(|_| super::session::Error::from("INVALID_STORAGE_STATE"))?;
        let path = directory.join(format!("auth-state-{}.json", reference.id));
        // Retain allocation before publication: even a failed atomic write can
        // leave owned cleanup, and must not lose the snapshot's reference.
        self.0.insert(
            reference,
            Snapshot {
                reference,
                state,
                path: path.clone(),
                sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
                bytes: bytes.len(),
                published: false,
                format,
            },
        );
        if let Err(error) = crate::artifacts::write(&path, &bytes) {
            let mut error = super::session::Error::caused("STORAGE_PUBLICATION_FAILED", error);
            error.partial = Some(
                json!({"snapshot":reference,"publicationComplete":false,"nextAction":"browser_status"}),
            );
            return Err(error);
        }
        let snapshot = self.0.get_mut(&reference).ok_or("STALE_REFERENCE")?;
        snapshot.published = true;
        Ok(json!({"snapshot":snapshot.summary()}))
    }
    /// Deletes only an unchanged owned artifact and its reference.
    pub(crate) fn forget(&mut self, reference: Reference) -> Result<Value, super::session::Error> {
        use sha2::Digest;
        let snapshot = self.0.get(&reference).ok_or("STALE_REFERENCE")?;
        match std::fs::symlink_metadata(&snapshot.path) {
            Ok(_) => {
                let bytes = read_private(&snapshot.path)?;
                if format!("{:x}", sha2::Sha256::digest(&bytes)) != snapshot.sha256 {
                    return Err("STORAGE_ARTIFACT_CHANGED".into());
                }
                std::fs::remove_file(&snapshot.path)
                    .map_err(|_| super::session::Error::from("STORAGE_CLEANUP_PENDING"))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("STORAGE_CLEANUP_PENDING".into()),
        }
        self.0.remove(&reference);
        Ok(json!({"snapshot":reference,"forgotten":true,"applicationStateChanged":false}))
    }
}

/// The pinned wire schema validates the complete opaque storage formats,
/// including IndexedDB/OPFS codecs, before an owned context can be changed.
pub(crate) fn validate(state: &Value) -> Result<(), &'static str> {
    validate_with(state, false)
}
/// Chromium cookies also carry the pinned `_crHasCrossSiteAncestor` partition
/// flag, which native Chrome accepts and Firefox/WebKit refuse.
pub(crate) fn validate_with(state: &Value, chromium: bool) -> Result<(), &'static str> {
    super::schema::encoded_len(state, MAX_BYTES).map_err(|_| "STORAGE_BOUNDS")?;
    super::schema::validate(
        "BrowserContextSetStorageStateParams",
        Some(&json!({"storageState":state})),
        &super::objects::ObjectTable::default(),
    )
    .map_err(|_| "INVALID_STORAGE_STATE")?;
    if let Some(credentials) = state.get("credentials") {
        super::credentials::validate_all(credentials)?;
    }
    if let Some(origins) = state["origins"].as_array() {
        for origin in origins {
            super::settings::validate_origin(
                origin["origin"].as_str().ok_or("INVALID_STORAGE_ORIGIN")?,
            )?;
        }
    }
    if let Some(cookies) = state["cookies"].as_array() {
        for cookie in cookies {
            let domain = cookie["domain"]
                .as_str()
                .filter(|domain| !domain.is_empty())
                .ok_or("INVALID_COOKIE_SCOPE")?;
            let domain = domain.strip_prefix('.').unwrap_or(domain);
            super::settings::validate_origin(&format!("https://{domain}"))?;
            if cookie.get("url").is_some()
                || !chromium && cookie.get("_crHasCrossSiteAncestor").is_some()
                || cookie
                    .get("_crHasCrossSiteAncestor")
                    .is_some_and(|ancestor| {
                        !ancestor.is_boolean() || cookie.get("partitionKey").is_none()
                    })
                || !cookie["path"].as_str().is_some_and(|path| {
                    path.starts_with('/') && !path.chars().any(char::is_control)
                })
                || cookie["expires"]
                    .as_f64()
                    .is_some_and(|expires| !(-1.0..=253_402_300_799.0).contains(&expires))
            {
                return Err("INVALID_COOKIE_SCOPE");
            }
        }
    }
    Ok(())
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    vec![
        object(
            json!({"operation":{"const":"storage_register"},"path":{"type":"string","minLength":1,"description":"Absolute private regular state JSON file; content is copied privately and never returned"}}),
            &["operation", "path"],
        ),
        object(
            json!({"operation":{"const":"storage_export"},"context":reference,"options":object(json!({"indexedDB":{"type":"boolean","default":false},"opfs":{"type":"boolean","default":false},"credentials":{"type":"boolean","default":false,"description":"Include virtual credentials, private keys included"},"path":{"type":"string","minLength":1,"maxLength":4096}}), &[])}),
            &["operation", "context"],
        ),
        object(
            json!({"operation":{"const":"storage_apply"},"context":reference,"snapshot":reference,"mode":{"enum":["replace","merge"],"description":"replace as setStorageState; merge (Chrome) upserts only snapshot items"},"installAuthenticator":{"type":"boolean","default":false,"description":"Required for snapshots with virtual credentials; replaces the context's real authenticators"}}),
            &["operation", "context", "snapshot", "mode"],
        ),
        object(
            json!({"operation":{"const":"storage_forget"},"snapshot":reference}),
            &["operation", "snapshot"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_state_formats_are_validated_without_permitting_implicit_scope_or_credentials() {
        assert!(
            serde_json::from_value::<Options>(json!({"indexedDB":true}))
                .unwrap()
                .indexed_db
        );
        assert!(serde_json::from_value::<Options>(json!({"indexedDb":true})).is_err());
        let state = json!({"cookies":[{"name":"acme","value":"private","domain":"example.com","path":"/","expires":-1,"httpOnly":true,"secure":true,"sameSite":"Lax"}],"origins":[{"origin":"https://example.com","localStorage":[{"name":"account","value":"private"}]}]});
        validate(&state).unwrap();
        let mut invalid = state.clone();
        invalid["origins"][0]["origin"] = json!("https://name:private@example.com/path");
        assert!(validate(&invalid).is_err());
        let mut invalid = state;
        invalid["cookies"][0]["path"] = json!("relative");
        assert!(validate(&invalid).is_err());
        assert_eq!(
            Options {
                opfs: true,
                ..Default::default()
            }
            .validate(Engine::Webkit),
            Err("OPFS_UNSUPPORTED")
        );
    }
}
