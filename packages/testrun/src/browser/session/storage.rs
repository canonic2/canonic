//! Private file acquisition, native state capture and non-replayed owned-context restore.
use super::*;
use crate::browser::storage::{self, Options};
use std::path::Path;

/// Creation `storageState`, loaded and validated for the driver's `newContext`,
/// which applies it before any page.
pub(super) async fn initial_storage_state(
    input: Option<crate::browser::http::StorageStateInput>,
    engine: crate::browser::contract::Engine,
    deadline: Instant,
) -> Result<Option<crate::browser::http::StorageStateInput>, Error> {
    let Some(input) = input else {
        return Ok(None);
    };
    let state = crate::browser::http::storage_state_load(input, deadline).await?;
    storage::validate(&state)?;
    storage::refuse_creation_credentials(&state)?;
    Options {
        opfs: storage::opfs_included(&state),
        ..Default::default()
    }
    .validate(engine)?;
    Ok(Some(crate::browser::http::StorageStateInput::Inline(state)))
}

impl Host {
    fn invalidate_storage_pages(&self, context: Reference) {
        let pages = {
            let state = self.state.lock().expect("Browser owner");
            state
                .resources
                .values()
                .filter(|resource| {
                    resource.kind == Kind::Page
                        && state.ancestor(resource.reference, Kind::Context) == Some(context)
                })
                .map(|resource| resource.reference)
                .collect::<Vec<_>>()
        };
        let mut inputs = self.inputs.lock().expect("Browser input");
        for page in pages {
            inputs.invalidate(page);
        }
    }
    fn storage_capacity(&self) -> Result<(), Error> {
        self.storage.lock().expect("Browser storage").capacity()
    }

    fn retain_storage(&self, state: Value) -> Result<Value, Error> {
        storage::validate(&state)?;
        let connection = self.state.lock().expect("Browser owner").connection;
        self.storage.lock().expect("Browser storage").retain(
            connection,
            self.journal.directory(),
            state,
            storage::PLAYWRIGHT_FORMAT,
        )
    }

    pub(super) fn storage_register(&self, path: String) -> Result<Value, Error> {
        self.storage_capacity()?;
        let bytes = storage::read_private(Path::new(&path))?;
        let state =
            serde_json::from_slice(&bytes).map_err(|_| Error::from("INVALID_STORAGE_STATE"))?;
        self.retain_storage(state)
    }

    pub(super) async fn storage_export(
        &self,
        context: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.storage_capacity()?;
        options.validate(self.state.lock().expect("Browser owner").engine)?;
        let wire = self.wire(context, Kind::Context)?;
        let mut state = self
            .connection()?
            .retained(
                &wire,
                "storageState",
                json!({"indexedDB":options.indexed_db,"opfs":options.opfs,"credentials":options.credentials}),
                deadline,
                false,
            )
            .await?
            .ok_or("MISSING_RESULT")?;
        if !options.credentials
            && let Some(credentials) = state.get("credentials")
        {
            if !credentials.as_array().is_some_and(Vec::is_empty) {
                return Err("UNREQUESTED_CREDENTIAL_STATE".into());
            }
            // An unrequested empty field would uninstall a context's virtual
            // authenticator when the snapshot is later restored.
            state
                .as_object_mut()
                .ok_or("INVALID_STORAGE_STATE")?
                .remove("credentials");
        }
        let copy = options.path.is_some().then(|| state.clone());
        let mut result = self.retain_storage(state)?;
        result["context"] = json!(context);
        result["included"] = json!({"cookies":true,"localStorage":true,"indexedDB":options.indexed_db,"opfs":options.opfs,"sessionStorage":false,"virtualCredentials":options.credentials});
        match copy {
            Some(state) => storage::copy_export(result, options.path, &state, deadline).await,
            None => Ok(result),
        }
    }

    pub(super) async fn storage_apply(
        &self,
        context: Reference,
        snapshot: Reference,
        mode: storage::ApplyMode,
        install_authenticator: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if mode == storage::ApplyMode::Merge {
            return Err("STORAGE_MERGE_UNSUPPORTED".into());
        }
        let state = self
            .storage
            .lock()
            .expect("Browser storage")
            .0
            .get(&snapshot)
            .ok_or("STALE_REFERENCE")?
            .state
            .clone();
        storage::validate(&state)?;
        storage::require_installation(&state, install_authenticator)?;
        let credentials = storage::credential_count(&state);
        Options {
            opfs: storage::opfs_included(&state),
            ..Default::default()
        }
        .validate(self.state.lock().expect("Browser owner").engine)?;
        let wire = self.wire(context, Kind::Context)?;
        self.invalidate_storage_pages(context);
        let applied = self
            .connection()?
            .retained(
                &wire,
                "setStorageState",
                json!({"storageState":state}),
                deadline,
                false,
            )
            .await;
        self.invalidate_storage_pages(context);
        applied.map_err(|error| {
            let unknown = !super::input::not_dispatched(&error);
            let mut result = if unknown { Error::caused("OUTCOME_UNKNOWN", error) } else { Error::from(error) };
            result.partial = Some(json!({"context":context,"snapshot":snapshot,"mode":"replace","stateMayHaveChanged":unknown,"nextAction":"inspect_owned_context_or_close_it","replayed":false}));
            result
        })?;
        // Pinned `setStorageState` replaces the authenticator's credentials and
        // installs it, or uninstalls it when the snapshot has none.
        {
            let mut state = self.state.lock().expect("Browser owner");
            if state.resource(context, Some(Kind::Context)).is_ok() {
                if credentials > 0 {
                    state.authenticators.insert(context);
                } else {
                    state.authenticators.remove(&context);
                }
            }
        }
        Ok(
            json!({"context":context,"snapshot":snapshot,"dispatchCompleted":true,"mode":"replace","effectsObserved":false,"credentials":{"restored":credentials,"authenticatorInstalled":credentials>0},"restoration":"close_owned_context_or_apply_explicit_snapshot"}),
        )
    }

    async fn context_cookies(&self, context: Reference, deadline: Instant) -> Result<Value, Error> {
        let wire = self.wire(context, Kind::Context)?;
        let reply = self
            .connection()?
            .retained(&wire, "cookies", json!({"urls":[]}), deadline, false)
            .await?
            .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
        Ok(reply["cookies"].clone())
    }
    pub(super) async fn context_cookies_read(
        &self,
        context: Reference,
        urls: crate::browser::http::CookieUrls,
        cursor: Option<String>,
        limit: Option<u64>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        urls.parse()?;
        let cookies = self.context_cookies(context, deadline).await?;
        storage::cookies::page(context, &cookies, &urls, cursor, limit)
    }
    pub(super) async fn context_cookies_add(
        &self,
        context: Reference,
        cookies: Vec<Value>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let cookies = storage::cookies::added(cookies)?;
        let wire = self.wire(context, Kind::Context)?;
        self.connection()?
            .retained(
                &wire,
                "addCookies",
                json!({"cookies":cookies}),
                deadline,
                false,
            )
            .await?;
        Ok(storage::cookies::add_result(context, cookies.len()))
    }
    /// Unfiltered clears use the driver's `clearCookies`; a filter expires the
    /// cookies it selects through `addCookies`, as the pinned server does.
    pub(super) async fn context_cookies_clear(
        &self,
        operation: Uuid,
        context: Reference,
        options: crate::browser::http::CookieFilter,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        if options.is_empty() {
            let wire = self.wire(context, Kind::Context)?;
            self.connection()?
                .retained(&wire, "clearCookies", json!({}), deadline, false)
                .await?;
            return Ok(storage::cookies::clear_result(context, None));
        }
        let values = self
            .context_cookies(context, deadline)
            .await?
            .as_array()
            .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?
            .clone();
        let selected = {
            let work = storage::cookies::expired(values, options, deadline);
            tokio::pin!(work);
            loop {
                self.admissible(operation, deadline)?;
                tokio::select! {
                    result=&mut work=>break result?,
                    _=tokio::time::sleep(Duration::from_millis(25))=>{}
                }
            }
        };
        self.admissible(operation, deadline)?;
        let count = selected.len();
        if !selected.is_empty() {
            // The exact context object is revalidated after matching.
            let wire = self.wire(context, Kind::Context)?;
            self.connection()?
                .retained(
                    &wire,
                    "addCookies",
                    json!({"cookies":selected}),
                    deadline,
                    false,
                )
                .await?;
        }
        Ok(storage::cookies::clear_result(context, Some(count)))
    }
    pub(super) async fn page_web_storage_read(
        &self,
        page: Reference,
        kind: storage::web::Kind,
        name: Option<String>,
        cursor: Option<String>,
        limit: Option<u64>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        storage::web::validate_read(name.as_deref(), cursor.as_deref(), limit)?;
        let wire = self.wire(page, Kind::Page)?;
        let connection = self.connection()?;
        if let Some(name) = name {
            let reply = connection
                .retained(
                    &wire,
                    "webStorageGetItem",
                    json!({"kind":kind.wire(),"name":name}),
                    deadline,
                    false,
                )
                .await?
                .unwrap_or_default();
            return storage::web::item(page, kind, &name, &reply["value"]);
        }
        let reply = connection
            .retained(
                &wire,
                "webStorageItems",
                json!({"kind":kind.wire()}),
                deadline,
                false,
            )
            .await?
            .ok_or("BACKEND_PROTOCOL")?;
        storage::web::page(page, kind, &reply["items"], cursor, limit)
    }
    pub(super) async fn page_web_storage_update(
        &self,
        page: Reference,
        kind: storage::web::Kind,
        update: storage::web::Update,
        deadline: Instant,
    ) -> Result<Value, Error> {
        update.validate()?;
        let wire = self.wire(page, Kind::Page)?;
        let (method, params) = update.driver(kind);
        self.connection()?
            .retained(&wire, method, params, deadline, false)
            .await?;
        Ok(update.result(page, kind))
    }

    pub(super) fn storage_forget(&self, reference: Reference) -> Result<Value, Error> {
        self.storage
            .lock()
            .expect("Browser storage")
            .forget(reference)
    }
}
