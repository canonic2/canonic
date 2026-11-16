//! Restoring local handler contexts without creating another MCP session.
use super::{Call, ProgressSink, invalid};
use crate::{
    Catalog, Control, RequestContext, RpcError, SessionOptions,
    stdio::{Context, Request},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    protocol: Option<Protocol>,
    grant: Option<Grant>,
    apps: bool,
    id: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Protocol {
    version: String,
    capabilities: Value,
    metadata: Value,
    request_state: Option<String>,
    client_info: Option<Value>,
    input_responses: Option<Value>,
    tasks: bool,
    cache: Option<Cache>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Cache {
    ttl_ms: u64,
    cache_scope: String,
}

impl Cache {
    fn restore(self) -> Result<crate::CachePolicy, RpcError> {
        let scope = match self.cache_scope.as_str() {
            "private" => crate::CacheScope::Private,
            "public" => crate::CacheScope::Public,
            _ => return Err(invalid()),
        };
        crate::CachePolicy::new(self.ttl_ms, scope)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Grant {
    audiences: Vec<String>,
    scopes: Vec<String>,
    subject: Option<String>,
    client_id: Option<String>,
    expires_at: Option<Expiry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expiry {
    seconds: u64,
    nanoseconds: u32,
}

/// Prepared worker declarations shared by all assignments on one implementation.
/// Catalog validators, bundled composition and modern server identity are
/// prepared once; restoring a call creates no MCP session or new validators.
pub struct Decoder {
    catalog: Arc<Catalog>,
    options: SessionOptions,
    server: Arc<Value>,
}

impl Decoder {
    /// Validate complete worker declarations before admitting assignments.
    /// # Errors
    /// Returns the same catalog/session composition errors as the core session.
    pub fn new(
        catalog: Arc<Catalog>,
        mut options: SessionOptions,
    ) -> Result<Self, crate::ConfigurationError> {
        crate::session::configure(&mut options)?;
        crate::apps::validate_configuration(&catalog, options.apps.as_deref())?;
        let server = Arc::new(
            options
                .modern
                .as_ref()
                .map_or(Value::Null, |modern| modern.server.identity()),
        );
        Ok(Self {
            catalog,
            options,
            server,
        })
    }
}

impl Grant {
    fn restore(self) -> Result<Arc<crate::http::Grant>, RpcError> {
        let mut grant = crate::http::Grant::new(self.audiences).with_scopes(self.scopes);
        if let Some(subject) = self.subject {
            grant = grant.with_subject(subject);
        }
        if let Some(client) = self.client_id {
            grant = grant.with_client_id(client);
        }
        if let Some(expiry) = self.expires_at {
            if expiry.nanoseconds >= 1_000_000_000 {
                return Err(invalid());
            }
            grant = grant.with_expiry(
                UNIX_EPOCH
                    .checked_add(Duration::new(expiry.seconds, expiry.nanoseconds))
                    .ok_or_else(invalid)?,
            );
        }
        if !grant.within_bounds() {
            return Err(invalid());
        }
        Ok(Arc::new(grant))
    }
}

impl Protocol {
    fn restore(
        self,
        work: &super::Work,
        options: &SessionOptions,
        schema: Option<crate::ResultSchema>,
        server: Arc<Value>,
    ) -> Result<Arc<RequestContext>, RpcError> {
        use crate::modern::{CAPABILITIES_KEY, CLIENT_KEY, PROTOCOL_VERSION, VERSION_KEY};
        if self.version != PROTOCOL_VERSION
            || self.metadata[VERSION_KEY] != self.version
            || self.metadata[CAPABILITIES_KEY] != self.capabilities
            || &self.metadata[CLIENT_KEY] != self.client_info.as_ref().unwrap_or(&Value::Null)
            || (self.tasks && options.tasks.is_none())
        {
            return Err(invalid());
        }
        let method = match work.kind.as_str() {
            "tool" => "tools/call",
            "prompt" => "prompts/get",
            "completion" => "completion/complete",
            "method" | "notification" => work.params["method"].as_str().ok_or_else(invalid)?,
            method => method,
        };
        let mut params = match work.kind.as_str() {
            "method" | "notification" => work.params["params"].clone(),
            _ => work.params.clone(),
        };
        params["_meta"] = self.metadata;
        if let Some(state) = self.request_state {
            params["requestState"] = json!(state);
        }
        if let Some(responses) = self.input_responses {
            params["inputResponses"] = responses;
        }
        let modern = options.modern.as_ref().ok_or_else(invalid)?;
        let cache = self.cache.map(Cache::restore).transpose()?;
        let context = RequestContext::parse_prepared(
            &params,
            modern,
            method,
            if self.tasks {
                options.tasks.as_ref()
            } else {
                None
            },
            schema,
            server,
            cache,
        )?;
        if context.worker_cache() != cache {
            return Err(invalid());
        }
        Ok(context)
    }
}

impl Call {
    /// Rebuild typed work and its handler context on an authenticated worker.
    /// `control` must retain the adapter's original monotonic deadline and be
    /// cancelled by its source-assignment pump. This function never starts or
    /// renews a timeout. Account for transport/queue time before constructing it.
    /// The frontend still owns reply encoding and the original result contract.
    /// Remote resource holds/publishers refuse until the consumer implements an
    /// explicit frontend resource protocol; they never silently return `None`.
    ///
    /// # Errors
    /// Refuses foreign assignments, malformed metadata, unsupported revisions,
    /// invalid grants, mismatched catalogs and a control extending the wire budget.
    pub fn restore(
        &self,
        assignment: &super::Assignment,
        decoder: &Decoder,
        control: Arc<Control>,
        progress: Option<ProgressSink>,
    ) -> Result<(Request, Context), RpcError> {
        let catalog = &decoder.catalog;
        let options = &decoder.options;
        self.check(&assignment.frontend, &assignment.worker)?;
        if self.assignment != *assignment
            || control.remaining() > Duration::from_millis(self.remaining_ms)
        {
            return Err(invalid());
        }
        let request = self.work.restore(catalog, options)?;
        let metadata: Metadata =
            serde_json::from_value(self.metadata.clone()).map_err(|_| invalid())?;
        if metadata
            .id
            .as_ref()
            .is_some_and(|id| !crate::rpc::valid_id(id))
        {
            return Err(invalid());
        }
        let schema = match &request {
            Request::Tool(call) => catalog.result_schema(&call.name),
            _ => None,
        };
        let protocol = metadata
            .protocol
            .map(|protocol| {
                protocol.restore(&self.work, options, schema.clone(), decoder.server.clone())
            })
            .transpose()?;
        // Apps is derived from request-local metadata on modern calls. Legacy
        // support was negotiated by the retained frontend and is carried explicitly.
        if let Some(protocol) = &protocol
            && protocol.supports_apps() != metadata.apps
        {
            return Err(invalid());
        }
        let grant = metadata.grant.map(Grant::restore).transpose()?;
        let has_progress = match &request {
            Request::Tool(call) => call.progress_token.is_some(),
            Request::Prompt(call) => call.progress_token.is_some(),
            Request::Completion(call) => call.progress_token.is_some(),
            _ => false,
        };
        Ok((
            request,
            Context::from_worker(
                control,
                protocol,
                grant,
                metadata.id,
                schema,
                metadata.apps,
                progress.filter(|_| has_progress),
            ),
        ))
    }
}

#[cfg(test)]
mod tests;
