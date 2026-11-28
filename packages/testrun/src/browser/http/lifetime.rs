//! Public HTTP-client disposal and response-body lifetime are separate from evidence.
use super::*;
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"http_context_dispose"},"httpContext":reference,"options":DisposeOptions::schema()},"required":["operation","httpContext"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"http_context_read"},"httpContext":reference,"includeReason":{"type":"boolean","default":false},"maxBytes":{"type":"integer","minimum":1,"maximum":65536,"default":65536}},"required":["operation","httpContext"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"http_response_body_read"},"requestId":{"type":"string","format":"uuid"},"format":{"enum":["base64","text","json","json_value"]},"maxBytes":{"type":"integer","minimum":1,"maximum":65536,"default":65536}},"required":["operation","requestId","format"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"http_response_dispose"},"requestId":{"type":"string","format":"uuid"}},"required":["operation","requestId"],"additionalProperties":false}),
    ]
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisposeOptions {
    #[serde(default)]
    pub reason: Option<String>,
}
impl DisposeOptions {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self
            .reason
            .as_ref()
            .is_some_and(|reason| reason.len() > 65536)
        {
            return Err("HTTP_DISPOSE_REASON_BOUNDS".into());
        }
        Ok(())
    }
    pub(crate) fn schema() -> Value {
        json!({"type":"object","properties":{"reason":{"type":["string","null"],"maxLength":65536}},"additionalProperties":false})
    }
}
pub(super) struct Disposed {
    pub(super) initial_reason: Option<String>,
    pub(super) fetch_reason: Option<String>,
}
pub(super) struct BodyLifetime {
    context: Reference,
    disposed: bool,
}
impl Client {
    pub(super) fn require_open(&self, activity: &Activity) -> Result<(), Error> {
        if activity.disposed.is_some() {
            let mut error = Error::from("HTTP_CONTEXT_DISPOSED");
            error.partial =
                Some(json!({"httpContext":self.reference,"disposed":true,"replayed":false}));
            Err(error)
        } else {
            Ok(())
        }
    }
}
impl Contexts {
    pub(crate) fn dispose(
        &mut self,
        reference: Reference,
        options: DisposeOptions,
    ) -> Result<Value, Error> {
        options.validate()?;
        let client = self.lookup(reference)?;
        let repeated = client.dispose_leases(options.reason, false);
        self.dispose_bodies(reference);
        // Associated disposal leaves already-dispatched SDK work alive. The
        // isolated owner destroys only its exact request sockets.
        if client.browser.is_none() {
            for (id, job) in &self.jobs {
                if job.client == reference {
                    job.disposed.store(true, Ordering::Release);
                    self.cancel_request(*id);
                }
            }
        }
        Ok(
            json!({"httpContext":reference,"disposed":true,"repeated":repeated,
            "active":client.is_active(),"activeRequests":client.request_count(),
            "inflightPolicy":if client.browser.is_some() {"continue-dispatched"} else {"cancel-owned"},
            "bodyEvidenceRemoved":false,"replayed":false}),
        )
    }
    pub(crate) fn context_read(
        &self,
        reference: Reference,
        include_reason: bool,
        max_bytes: usize,
    ) -> Result<Value, Error> {
        if !(1..=65536).contains(&max_bytes) {
            return Err("HTTP_RESPONSE_RESULT_BOUNDS".into());
        }
        let client = self.lookup(reference)?;
        let activity = client.activity.lock().expect("HTTP activity");
        let mut value = json!({"httpContext":reference,"disposed":activity.disposed.is_some(),
            "browserAssociated":client.browser.is_some(),"private":true,"replayed":false});
        if include_reason {
            value["initialReason"] = json!(
                activity
                    .disposed
                    .as_ref()
                    .and_then(|state| state.initial_reason.as_ref())
            );
            value["fetchReason"] = json!(
                activity
                    .disposed
                    .as_ref()
                    .and_then(|state| state.fetch_reason.as_ref())
            );
        }
        crate::browser::schema::encoded_len(&value, max_bytes)
            .map_err(|_| Error::from("HTTP_RESPONSE_RESULT_BOUNDS"))?;
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn disposed_request(&self, id: Uuid) -> bool {
        self.jobs
            .get(&id)
            .is_some_and(|job| job.disposed.load(Ordering::Acquire))
    }
    pub(crate) fn disposal_watch(&self, id: Uuid) -> Result<Arc<AtomicBool>, Error> {
        self.jobs
            .get(&id)
            .map(|job| job.disposed.clone())
            .ok_or("UNKNOWN_HTTP_RESPONSE".into())
    }
    pub(crate) fn browser_context_closed(&mut self, context: Reference) {
        let clients = self
            .items
            .values()
            .filter(|client| client.options.browser_context == Some(context))
            .map(|client| client.reference)
            .collect::<Vec<_>>();
        for reference in clients {
            self.close_client_work(reference);
        }
    }
    pub(crate) fn shutdown_work(&mut self) {
        let clients = self.items.keys().copied().collect::<Vec<_>>();
        for reference in clients {
            self.close_client_work(reference);
        }
    }
    fn close_client_work(&mut self, reference: Reference) {
        self.items[&reference].dispose_leases(None, true);
        self.dispose_bodies(reference);
        for (id, job) in &self.jobs {
            if job.client == reference {
                job.disposed.store(true, Ordering::Release);
                self.cancel_request(*id);
            }
        }
    }
    pub(super) fn dispose_bodies(&mut self, reference: Reference) {
        for body in self.body_lifetimes.values_mut() {
            if body.context == reference {
                body.disposed = true;
            }
        }
    }
    pub(super) fn retain_body_lifetime(&mut self, id: Uuid, context: Reference) {
        let disposed = self.items.get(&context).is_none_or(|client| {
            client
                .activity
                .lock()
                .expect("HTTP activity")
                .disposed
                .is_some()
        });
        self.body_lifetimes
            .insert(id, BodyLifetime { context, disposed });
    }
    pub(crate) fn response_body_read(
        &self,
        id: Uuid,
        format: BodyFormat,
        max_bytes: usize,
    ) -> Result<Value, Error> {
        let lifetime = self
            .body_lifetimes
            .get(&id)
            .ok_or("UNKNOWN_HTTP_RESPONSE")?;
        if lifetime.disposed {
            return Err("HTTP_RESPONSE_DISPOSED".into());
        }
        let artifact = self
            .evidence
            .get(*self.captures.get(&id).ok_or("UNKNOWN_HTTP_RESPONSE")?)?;
        let mut value = read_response(&artifact, format, max_bytes)?;
        value["requestId"] = json!(id);
        crate::browser::schema::encoded_len(&value, max_bytes)
            .map_err(|_| Error::from("HTTP_RESPONSE_RESULT_BOUNDS"))?;
        Ok(value)
    }
    pub(crate) fn response_dispose(&mut self, id: Uuid) -> Result<Value, Error> {
        let context = self
            .body_lifetimes
            .get(&id)
            .ok_or("UNKNOWN_HTTP_RESPONSE")?
            .context;
        self.get(context)?;
        self.body_lifetimes
            .get_mut(&id)
            .expect("Owned response body")
            .disposed = true;
        Ok(json!({"requestId":id,"disposed":true,"bodyEvidenceRemoved":false,"replayed":false}))
    }
}
