//! Bounded subscription policy, explicit filters and consumer-owned change events.

use crate::{Catalog, PromptCatalog, ResourceCatalog, RpcError, json, resource, rpc};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
    time::Duration,
};

/// Maximum resource URIs in a filter or connection allowlist.
pub const MAX_RESOURCE_SUBSCRIPTIONS: usize = 64;
/// Maximum encoded filter or allowlist size, in bytes.
pub const MAX_FILTER_BYTES: usize = 64 * 1024;
/// Reserved metadata key identifying a subscription stream.
pub const SUBSCRIPTION_KEY: &str = "io.modelcontextprotocol/subscriptionId";

/// Explicit modern subscription policy for one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionOptions {
    /// Simultaneous streams, within 1..=64; defaults to 16.
    pub max_streams: usize,
    /// Lifetime from admission, at least one millisecond and at most 24 hours.
    pub timeout: Duration,
    /// Exact resource addresses eligible for updates, independently of read authority.
    pub resource_uris: Vec<String>,
}

impl Default for SubscriptionOptions {
    fn default() -> Self {
        Self {
            max_streams: 16,
            timeout: Duration::from_mins(5),
            resource_uris: Vec::new(),
        }
    }
}

impl SubscriptionOptions {
    pub(crate) fn validate(&self) -> Result<(), crate::ConfigurationError> {
        let mut seen = HashSet::new();
        if !(1..=64).contains(&self.max_streams)
            || self.timeout.as_millis() == 0
            || self.timeout > crate::control::MAX_TIMEOUT
            || self.resource_uris.len() > MAX_RESOURCE_SUBSCRIPTIONS
            || json::encode(&self.resource_uris, MAX_FILTER_BYTES).is_err()
            || self
                .resource_uris
                .iter()
                .any(|uri| resource::validate_uri(uri).is_err() || !seen.insert(uri))
        {
            return Err(crate::ConfigurationError(
                "invalid subscription bounds or resource allowlist".into(),
            ));
        }
        Ok(())
    }
}

/// A consumer change; catalogs are prepared once before publication.
#[derive(Debug, Clone)]
pub enum Change {
    /// Replace the tool catalog and notify interested modern streams.
    Tools(Arc<Catalog>),
    /// Replace an already enabled prompt catalog.
    Prompts(Arc<PromptCatalog>),
    /// Replace an already enabled resource catalog, preserving bundled Skills.
    Resources(Arc<ResourceCatalog>),
    /// Notify interested streams after consumer-owned bytes at this URI change.
    ResourceUpdated(String),
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Filter {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tools_list_changed: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub prompts_list_changed: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub resources_list_changed: bool,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub resource_subscriptions: BTreeSet<String>,
}

impl Filter {
    pub(crate) fn parse(params: &Value) -> Result<Self, RpcError> {
        let invalid = || rpc::RpcError::new(rpc::INVALID_PARAMS, "Invalid subscription filter");
        if !rpc::only(params, &["notifications", "_meta"]) {
            return Err(invalid());
        }
        let value = &params["notifications"];
        if !rpc::only(
            value,
            &[
                "toolsListChanged",
                "promptsListChanged",
                "resourcesListChanged",
                "resourceSubscriptions",
            ],
        ) || json::measure(value, MAX_FILTER_BYTES, &|| Ok(())).is_err()
        {
            return Err(invalid());
        }
        let flag = |key| match value.get(key) {
            None => Ok(false),
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(invalid()),
        };
        let mut resource_subscriptions = BTreeSet::new();
        if let Some(uris) = value.get("resourceSubscriptions") {
            let uris = uris
                .as_array()
                .filter(|uris| uris.len() <= MAX_RESOURCE_SUBSCRIPTIONS)
                .ok_or_else(invalid)?;
            for uri in uris {
                let uri = uri.as_str().ok_or_else(invalid)?;
                resource::validate_uri(uri).map_err(|_| invalid())?;
                resource_subscriptions.insert(uri.to_owned());
            }
        }
        Ok(Self {
            tools_list_changed: flag("toolsListChanged")?,
            prompts_list_changed: flag("promptsListChanged")?,
            resources_list_changed: flag("resourcesListChanged")?,
            resource_subscriptions,
        })
    }
}
