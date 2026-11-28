//! Context credentials answer owned page/frame/service-worker challenges privately.
use super::*;

pub(super) struct Attempt {
    network: Option<String>,
    attempted: bool,
}

impl Worker {
    fn credential_callback_state(&self, context: Reference) -> Value {
        let now = Instant::now();
        let mut manual = 0;
        let mut automatic = 0;
        let mut responding = 0;
        let mut retry_deferred = 0;
        let mut expired = 0;
        let mut failures = BTreeMap::<&'static str, usize>::new();
        for route in self
            .interactions
            .routes
            .values()
            .filter(|route| self.route_context(route) == Some(context))
        {
            if !route.auto_pass {
                manual += 1;
                continue;
            }
            automatic += 1;
            responding += usize::from(route.responding);
            retry_deferred += usize::from(route.retry_at > now);
            expired += usize::from(route.expires <= now);
            if let Some(code) = route.last_failure {
                *failures.entry(code).or_default() += 1;
            }
        }
        json!({"context":context,"manualCallbacks":manual,"automaticContinuations":automatic,
            "automaticResponding":responding,"automaticRetryDeferred":retry_deferred,"automaticExpired":expired,
            "automaticFailureCodes":failures,"automaticServiceActive":self.har_servicing,"defaultsChanged":false,"replayed":false})
    }
    fn credential_callback_refusal(&self, context: Reference) -> Error {
        let mut error = Error::from("ROUTE_CALLBACKS_PENDING");
        error.partial = Some(self.credential_callback_state(context));
        error
    }
    pub(super) fn automatic_authentication(&self, context: Reference) -> bool {
        self.http_contexts
            .require_context_credentials(context)
            .is_err()
            || self
                .context_layout
                .get(&context)
                .and_then(|options| options.http_credentials.as_ref())
                .is_some_and(|value| value.len() > 0)
    }

    pub(super) fn authentication_finished(&mut self, event: &Event) {
        if matches!(
            event.method.as_str(),
            "Network.loadingFinished" | "Network.loadingFailed"
        ) && let (Some(session), Some(network)) = (
            event.session_id.as_deref(),
            event.params["requestId"].as_str(),
        ) {
            self.fetch.authentication.retain(|(owner, _), attempt| {
                owner != session || attempt.network.as_deref() != Some(network)
            });
        }
    }

    pub(super) fn authentication_request(
        &mut self,
        session: &str,
        native: &str,
        network: Option<&str>,
    ) -> Result<(), Error> {
        let key = (session.to_owned(), native.to_owned());
        if !self.fetch.authentication.contains_key(&key) && self.fetch.authentication.len() >= 512 {
            return Err("AUTHENTICATION_REQUEST_CAPACITY".into());
        }
        let attempt = self.fetch.authentication.entry(key).or_insert(Attempt {
            network: None,
            attempted: false,
        });
        if attempt.network.is_none() {
            attempt.network = network.map(str::to_owned);
        }
        Ok(())
    }

    pub(super) fn authentication_response(
        &mut self,
        context: Reference,
        session: &str,
        native: &str,
        url: &str,
    ) -> Result<Value, Error> {
        self.authentication_request(session, native, None)?;
        let uncertain = self
            .http_contexts
            .require_context_credentials(context)
            .is_err()
            || self.fetch.authentication_lost;
        let credentials = self
            .context_layout
            .get(&context)
            .and_then(|options| options.http_credentials.as_ref())
            .and_then(|credentials| {
                url::Url::parse(url)
                    .ok()
                    .and_then(|url| credentials.matching(&url))
            })
            .cloned();
        let attempt = self
            .fetch
            .authentication
            .get_mut(&(session.to_owned(), native.to_owned()))
            .expect("Owned authentication request");
        // Chromium's pinned SDK selects by request origin, independent of the
        // challenge's scheme/source. `send: always` only affects the HTTP client.
        if uncertain || attempt.attempted {
            Ok(json!({"response":"CancelAuth"}))
        } else if let Some(credentials) = credentials {
            attempt.attempted = true;
            Ok(
                json!({"response":"ProvideCredentials","username":credentials.username,"password":credentials.password}),
            )
        } else {
            Ok(json!({"response":"Default"}))
        }
    }

    async fn settle_context_automatic_routes(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        loop {
            self.observation_ready(id, deadline)?;
            // Caller-owned callbacks remain an immediate refusal. Do not progress
            // them, alter defaults or dispatch automatic replies around that fence.
            if self
                .interactions
                .routes
                .values()
                .any(|route| !route.auto_pass && self.route_context(route) == Some(context))
            {
                return Err(self.credential_callback_refusal(context));
            }
            let next = self
                .interactions
                .routes
                .iter()
                .find(|(_, route)| {
                    route.auto_pass
                        && self.route_context(route) == Some(context)
                        && !route.responding
                        && route.expires > Instant::now()
                        && route.retry_at <= Instant::now()
                })
                .map(|(reference, route)| {
                    (
                        *reference,
                        route.unmatched_response(),
                        deadline.min(route.expires),
                    )
                });
            let Some((reference, response, expires)) = next else {
                return Ok(());
            };
            self.service_automatic_route(reference, response, expires)
                .await.map_err(|mut error| {
                    error.partial=Some(json!({"callbackState":self.credential_callback_state(context),"cause":error.partial}));
                    error
                })?;
        }
    }
    pub(super) async fn configure_context_credentials(
        &mut self,
        id: Uuid,
        context: Reference,
        credentials: Option<crate::browser::http::Credentials>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        if let Some(value) = &credentials {
            value.validate()?;
        }
        if Instant::now() >= deadline {
            return Err("NOT_DISPATCHED".into());
        }
        if self.fetch.authentication_lost {
            return Err("AUTHENTICATION_OBSERVATION_LOST".into());
        }
        if self.context_fetch_pending(context) {
            return Err("INTERCEPTION_OUTCOME_UNKNOWN".into());
        }
        self.settle_context_automatic_routes(id, context, deadline)
            .await?;
        if self
            .interactions
            .routes
            .values()
            .any(|route| self.route_context(route) == Some(context))
        {
            return Err(self.credential_callback_refusal(context));
        }
        let layout = self
            .context_layout
            .get_mut(&context)
            .ok_or("INVALID_SCOPE")?;
        self.http_contexts
            .context_credentials_initial(context, layout.http_credentials.as_ref());
        self.http_contexts.context_credentials_pending(context);
        layout.http_credentials = credentials.clone();
        // Keep HTTP clients fenced until every selected attachment acknowledges.
        // The desired native configuration remains owned after a partial failure.
        let result = self.refresh_context_fetch(id, context, deadline).await;
        if let Err(mut error) = result {
            error.partial = Some(
                json!({"context":context,"configurationComplete":false,"stateMayHaveChanged":true,"httpDefaults":"refused-until-acknowledged-reconfiguration","replayed":false,"cause":error.partial}),
            );
            return Err(error);
        }
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.http_contexts
            .context_credentials_applied(context, credentials.as_ref());
        // Clearing credentials may release the final authentication-only Fetch
        // owner. Recompute once with the acknowledged empty default.
        self.refresh_context_fetch(id, context, deadline).await.map_err(|mut error| {
            self.http_contexts.context_credentials_pending(context);
            error.partial = Some(json!({"context":context,"configurationComplete":false,"stateMayHaveChanged":true,"replayed":false,"cause":error.partial}));
            error
        })?;
        Ok(
            json!({"context":context,"scopeEnded":false,"requested":{"kind":"http_credentials","credentialCount":credentials.as_ref().map_or(0, |value| value.len())},"dispatchCompleted":true,"effectsObserved":false,"futureAttachments":true,"scope":"context-pages-child-frames-and-service-workers","serviceWorkers":true,"workers":false,"backend":"chrome_cdp","replayed":false}),
        )
    }
}
