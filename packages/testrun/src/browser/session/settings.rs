//! Owned environment mutations retain native outcomes and report observation separately.
use super::*;
use crate::browser::settings::{ContextSetting, MediaOptions};

fn setting_error(error: connection::Error, mut partial: Value) -> Error {
    let uncertain = matches!(
        &error,
        connection::Error::Remote(_)
            | connection::Error::Timeout {
                dispatch: connection::Dispatch::PossiblyDispatched
            }
            | connection::Error::Lost {
                dispatch: connection::Dispatch::PossiblyDispatched
            }
    );
    partial["stateMayHaveChanged"] = json!(uncertain);
    partial["nativeDispatch"] = json!(if uncertain {
        "possibly_dispatched"
    } else {
        "not_dispatched"
    });
    if uncertain {
        Error {
            partial: Some(partial),
            ..Error::caused("OUTCOME_UNKNOWN", error)
        }
    } else {
        Error {
            partial: Some(partial),
            ..Error::from(error)
        }
    }
}

impl Host {
    pub(super) async fn page_set_extra_http_headers(
        &self,
        page: Reference,
        headers: Vec<crate::browser::http::Header>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        crate::browser::http::validate_headers(&headers)?;
        let wire = self.wire(page, Kind::Page)?;
        self.connection()?.retained(&wire, "setExtraHTTPHeaders", json!({"headers":headers}), deadline, false)
            .await.map_err(|error| setting_error(error, json!({"page":page,"headerCount":headers.len(),"effectsObserved":false,"replayed":false,"restoration":"explicit-reconfiguration-or-close-owned-page"})))?;
        let scope_ended = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(page, Some(Kind::Page))
            .is_err();
        Ok(
            json!({"page":page,"headerCount":headers.len(),"dispatchCompleted":true,"effectsObserved":false,"scopeEnded":scope_ended,"backend":"playwright","replayed":false}),
        )
    }
    pub(super) async fn context_clock(
        &self,
        context: Reference,
        action: crate::browser::clock::Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let wire = self.wire(context, Kind::Context)?;
        let (method, params) = action.wire()?;
        self.connection()?.retained(&wire,method,params,deadline,false).await.map_err(|error|setting_error(error,json!({"context":context,"action":action,"effectsObserved":false,"restoration":"close_owned_context"})))?;
        Ok(
            json!({"context":context,"action":action,"dispatchCompleted":true,"effectsObserved":false,"scope":"context-pages-frames-and-future-documents","hostDeadlinesUnchanged":true,"restoration":"close_owned_context"}),
        )
    }
    /// The pinned `credentialsGet` records, private keys included; they stay
    /// here and leave only as public views.
    async fn driver_credentials(
        &self,
        wire: &crate::browser::objects::ObjectRef,
        filter: Value,
        deadline: Instant,
    ) -> Result<Vec<crate::browser::credentials::Credential>, Error> {
        let reply = self
            .connection()?
            .retained(wire, "credentialsGet", filter, deadline, false)
            .await?
            .ok_or("BACKEND_PROTOCOL")?;
        serde_json::from_value(reply["credentials"].clone())
            .map_err(|_| Error::from("BACKEND_PROTOCOL"))
    }
    /// Pinned `credentials.create` for a validated `Create` action; failures
    /// of the call itself pass through `failure`.
    async fn driver_create(
        &self,
        wire: &crate::browser::objects::ObjectRef,
        action: &crate::browser::credentials::Action,
        id: Option<&str>,
        failure: impl FnOnce(crate::browser::connection::Error) -> Error,
        deadline: Instant,
    ) -> Result<crate::browser::credentials::Credential, Error> {
        let existing = self.driver_credentials(wire, json!({}), deadline).await?;
        if existing.len() >= crate::browser::credentials::MAX_CREDENTIALS {
            return Err("CREDENTIAL_CAPACITY".into());
        }
        // The pinned registry silently replaces an ID; a seeded key never
        // overwrites one the page or a snapshot already holds.
        if id.is_some_and(|id| existing.iter().any(|credential| credential.id == id)) {
            return Err("DUPLICATE_CREDENTIAL_ID".into());
        }
        // The driver takes the action's own optional fields.
        let mut params = json!(action);
        if let Some(fields) = params.as_object_mut() {
            fields.remove("kind");
        }
        let reply = self
            .connection()?
            .retained(wire, "credentialsCreate", params, deadline, false)
            .await
            .map_err(failure)?
            .ok_or("BACKEND_PROTOCOL")?;
        serde_json::from_value(reply["credential"].clone())
            .map_err(|_| Error::from("BACKEND_PROTOCOL"))
    }
    /// Pinned `BrowserContext.credentials*`. Results carry public views only.
    pub(super) async fn context_credentials(
        &self,
        context: Reference,
        action: crate::browser::credentials::Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        use crate::browser::credentials::{self, Action};
        action.validate()?;
        let wire = self.wire(context, Kind::Context)?;
        let failure = |error| {
            setting_error(
                error,
                json!({"context":context,"action":action,"effectsObserved":false,"privateKeysReturned":false,"replayed":false,"restoration":"close_owned_context"}),
            )
        };
        let installed = || {
            self.state
                .lock()
                .expect("Browser owner")
                .authenticators
                .contains(&context)
        };
        match &action {
            Action::Install {} => {
                self.connection()?
                    .retained(&wire, "credentialsInstall", json!({}), deadline, false)
                    .await
                    .map_err(failure)?;
                let mut state = self.state.lock().expect("Browser owner");
                if state.resource(context, Some(Kind::Context)).is_ok() {
                    state.authenticators.insert(context);
                }
                Ok(credentials::outcome(context, &action, None, true, None))
            }
            Action::Create { id, .. } => {
                let credential = self
                    .driver_create(&wire, &action, id.as_deref(), failure, deadline)
                    .await?;
                Ok(credentials::outcome(
                    context,
                    &action,
                    Some(&credential),
                    installed(),
                    None,
                ))
            }
            Action::List {
                rp_id,
                id,
                cursor,
                limit,
            } => {
                let all = self.driver_credentials(&wire, json!({}), deadline).await?;
                credentials::page(
                    context,
                    all,
                    rp_id.as_deref(),
                    id.as_deref(),
                    cursor.clone(),
                    *limit,
                    installed(),
                )
            }
            Action::Delete { id } => {
                let existed = !self
                    .driver_credentials(&wire, json!({"id":id}), deadline)
                    .await?
                    .is_empty();
                self.connection()?
                    .retained(
                        &wire,
                        "credentialsDelete",
                        json!({"id":id}),
                        deadline,
                        false,
                    )
                    .await
                    .map_err(failure)?;
                Ok(credentials::outcome(
                    context,
                    &action,
                    None,
                    installed(),
                    Some(existed),
                ))
            }
        }
    }
    pub(super) async fn context_configure(
        &self,
        context: Reference,
        setting: ContextSetting,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let wire = self.wire(context, Kind::Context)?;
        let engine = self.state.lock().expect("Browser owner").engine;
        // All permissions are checked before Playwright can store a grant or
        // update one of several pages in an owned context.
        let (method, params) = setting.wire(engine)?;
        let previous_headers = if matches!(&setting, ContextSetting::ExtraHttpHeaders { .. }) {
            let initial = self
                .state
                .lock()
                .expect("Browser owner")
                .context_options
                .get(&context)
                .and_then(|options| options.extra_http_headers.clone())
                .unwrap_or_default();
            let mut owner = self.http_contexts.lock().expect("HTTP owner");
            owner.context_headers_initial(context, &initial);
            Some(owner.context_headers_pending(context))
        } else {
            None
        };
        let previous_credentials = if matches!(&setting, ContextSetting::HttpCredentials { .. }) {
            let initial = self
                .state
                .lock()
                .expect("Browser owner")
                .context_options
                .get(&context)
                .and_then(|options| options.http_credentials.clone());
            let mut owner = self.http_contexts.lock().expect("HTTP owner");
            owner.context_credentials_initial(context, initial.as_ref());
            Some(owner.context_credentials_pending(context))
        } else {
            None
        };
        self.connection()?.retained(&wire,method,params,deadline,false).await.map_err(|error| {
            // A rejected pinned setExtraHTTPHeaders has restored its previous option,
            // which the SDK's own request context then follows.
            let headers_restored = super::input::not_dispatched(&error)
                || matches!(&error, connection::Error::Remote(_));
            if headers_restored
                && let Some(Some(headers)) = &previous_headers
            {
                let state = self.state.lock().expect("Browser owner");
                let mut owner = self.http_contexts.lock().expect("HTTP owner");
                if state.resource(context, Some(Kind::Context)).is_ok() {
                    owner.context_headers_applied(context, headers);
                } else {
                    owner.context_headers_closed(context);
                }
            }
            if super::input::not_dispatched(&error)
                && let Some(Some(credentials)) = &previous_credentials
            {
                let state=self.state.lock().expect("Browser owner");
                let mut owner=self.http_contexts.lock().expect("HTTP owner");
                if state.resource(context,Some(Kind::Context)).is_ok() {owner.context_credentials_applied(context,Some(credentials));} else {owner.context_credentials_closed(context);}
            }
            setting_error(error,json!({"context":context,"requested":setting.summary(),"stateMayHaveChanged":true,"restoration":"close_owned_context","effectsObserved":false}))
        })?;
        let scope_ended = {
            let mut state = self.state.lock().expect("Browser owner");
            let ended = state.resource(context, Some(Kind::Context)).is_err();
            if let ContextSetting::HttpCredentials { credentials } = &setting {
                if ended {
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .context_credentials_closed(context);
                } else {
                    if let Some(options) = state.context_options.get_mut(&context) {
                        options.http_credentials = credentials.clone();
                    }
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .context_credentials_applied(context, credentials.as_ref());
                }
            }
            if let ContextSetting::ExtraHttpHeaders { headers } = &setting {
                if ended {
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .context_headers_closed(context);
                } else {
                    if let Some(options) = state.context_options.get_mut(&context) {
                        options.extra_http_headers = Some(headers.clone());
                    }
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .context_headers_applied(context, headers);
                }
            }
            ended
        };
        Ok(
            json!({"context":context,"requested":setting.summary(),"backend":"playwright","dispatchCompleted":true,"effectsObserved":false,"scopeEnded":scope_ended,"restoration":"close_owned_context"}),
        )
    }
    pub(super) async fn page_emulate_media(
        &self,
        page: Reference,
        options: MediaOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        let scope = self.page_scope(page, None, deadline).await?;
        let wire = self.wire(page, Kind::Page)?;
        let params =
            serde_json::to_value(&options).map_err(|_| Error::from("INVALID_MEDIA_OPTIONS"))?;
        self.inputs.lock().expect("Browser input").invalidate(page);
        self.suspend_previews(page);
        let applied = self
            .connection()?
            .retained(&wire, "emulateMedia", params, deadline, false)
            .await;
        // Reconfigure even after a method error: WebKit can have changed some
        // state before another native update fails. This never repeats the setter.
        let preview_recovery = match self.restart_previews(page, deadline).await {
            Ok(()) => json!({"succeeded":true}),
            Err(error) => json!({"succeeded":false,"code":error.code}),
        };
        applied.map_err(|error|setting_error(error,json!({"page":page,"requested":options,"stateMayHaveChanged":true,"effectsObserved":false,"previewRecovery":preview_recovery,"restoration":"reset_media_overrides_or_close_owned_context"})))?;
        let observed = async {
            if self.frame_current(&scope.wire, deadline).await? != scope.reference {
                return Err(Error::from("DOCUMENT_CHANGED"));
            }
            let observed = self
                .evaluate(
                    &scope.wire,
                    &format!("() => {}", crate::browser::settings::MEDIA_OBSERVATION),
                    Value::Null,
                    deadline,
                )
                .await?;
            if self.frame_current(&scope.wire, deadline).await? != scope.reference {
                return Err(Error::from("DOCUMENT_CHANGED"));
            }
            Ok::<Value, Error>(observed)
        }
        .await;
        match observed {
            Ok(observed) => Ok(
                json!({"page":page,"frame":scope.reference,"requested":options,"observed":observed,"dispatchCompleted":true,"source":"playwright-media-emulation","previewRecovery":preview_recovery,"restoration":"reset_media_overrides_or_close_owned_context"}),
            ),
            Err(mut error) => {
                error.partial = Some(
                    json!({"page":page,"requested":options,"dispatchCompleted":true,"effectsObserved":false,"previewRecovery":preview_recovery}),
                );
                Err(error)
            }
        }
    }
}
