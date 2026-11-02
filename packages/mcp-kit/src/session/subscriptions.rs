//! Connection-owned subscription streams, catalog replacement and finite lifetime.
use super::{Arc, ConfigurationError, Inbound, RpcError, Session, Value, json, rpc};
use crate::subscription::{Change, Filter, SUBSCRIPTION_KEY};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(super) struct Stream {
    id: Value,
    filter: Filter,
    deadline: Instant,
}

impl Session {
    pub(super) fn listen(&mut self, id: Value, method: &str, params: &Value) -> Inbound {
        let fail = |error| Inbound::Reply(rpc::failure(id.clone(), &error));
        if self.closed {
            return fail(RpcError::new(rpc::SERVER_ERROR, "Session is closed"));
        }
        let Some(policy) = &self.options.subscriptions else {
            return fail(RpcError::new(
                rpc::METHOD_NOT_FOUND,
                "Subscriptions disabled",
            ));
        };
        if method != "subscriptions/listen" {
            return fail(RpcError::new(rpc::METHOD_NOT_FOUND, "Method not found"));
        }
        let mut filter = match Filter::parse(params) {
            Ok(filter) => filter,
            Err(error) => return fail(error),
        };
        if self.subscriptions.len() >= policy.max_streams {
            return fail(RpcError::new(
                rpc::SERVER_ERROR,
                "Subscription capacity exceeded",
            ));
        }
        filter.prompts_list_changed &= self.options.prompts.is_some();
        filter.resources_list_changed &= self.options.resources.is_some();
        filter
            .resource_subscriptions
            .retain(|uri| policy.resource_uris.contains(uri));
        let acknowledged = rpc::notification(
            "notifications/subscriptions/acknowledged",
            json!({
                "_meta":{SUBSCRIPTION_KEY:id},"notifications":filter
            }),
        );
        self.outstanding.insert(id.to_string());
        self.subscriptions.insert(
            id.to_string(),
            Stream {
                id: id.clone(),
                filter,
                deadline: Instant::now() + policy.timeout,
            },
        );
        Inbound::Subscription { id, acknowledged }
    }

    /// Replaces prepared catalog state or publishes a consumer resource update.
    /// Send returned frames in order, after each stream's acknowledgement.
    ///
    /// # Errors
    /// Returns a configuration error without applying the change when disabled,
    /// closed, targeting a disabled catalog, or conflicting with bundled Skills.
    pub fn publish(&mut self, change: Change) -> Result<Vec<Value>, ConfigurationError> {
        if self.closed || self.options.subscriptions.is_none() {
            return Err(ConfigurationError(
                "subscription publication is disabled or closed".into(),
            ));
        }
        let (method, uri) = match change {
            Change::Tools(catalog) => {
                self.catalog = catalog;
                ("notifications/tools/list_changed", None)
            }
            Change::Prompts(catalog) => {
                if self.options.prompts.is_none() {
                    return Err(ConfigurationError("prompt catalog is disabled".into()));
                }
                self.options.prompts = Some(catalog);
                ("notifications/prompts/list_changed", None)
            }
            Change::Resources(catalog) => {
                if self.options.resources.is_none() {
                    return Err(ConfigurationError("resource catalog is disabled".into()));
                }
                let catalog = match &self.options.skills {
                    Some(skills) => Arc::new(
                        skills
                            .compose(Some(&catalog))
                            .map_err(|error| ConfigurationError(error.to_string()))?,
                    ),
                    None => catalog,
                };
                self.options.resources = Some(catalog);
                ("notifications/resources/list_changed", None)
            }
            Change::ResourceUpdated(uri) => {
                crate::resource::validate_uri(&uri)
                    .map_err(|error| ConfigurationError(error.to_string()))?;
                ("notifications/resources/updated", Some(uri))
            }
        };
        let mut frames = self.expire_subscriptions(Instant::now());
        frames.extend(self.subscriptions.values().filter_map(|stream| {
            let matches = match method {
                "notifications/tools/list_changed" => stream.filter.tools_list_changed,
                "notifications/prompts/list_changed" => stream.filter.prompts_list_changed,
                "notifications/resources/list_changed" => stream.filter.resources_list_changed,
                _ => uri
                    .as_ref()
                    .is_some_and(|uri| stream.filter.resource_subscriptions.contains(uri)),
            };
            matches.then(|| {
                let mut params = json!({"_meta":{SUBSCRIPTION_KEY:stream.id}});
                if let Some(uri) = &uri {
                    params["uri"] = json!(uri);
                }
                rpc::notification(method, params)
            })
        }));
        Ok(frames)
    }

    /// Completes one stream gracefully and releases its ID and captured context.
    #[must_use]
    pub fn end_subscription(&mut self, id: &Value) -> Option<Value> {
        self.subscriptions.remove(&id.to_string())?;
        let mut result = self.request_context(id).map_or_else(
            || json!({"resultType":"complete","_meta":{}}),
            |context| context.complete_value(json!({})),
        );
        result["_meta"][SUBSCRIPTION_KEY] = id.clone();
        self.finish(id);
        Some(rpc::success(id.clone(), result))
    }

    /// Completes every stream during graceful shutdown.
    #[must_use]
    pub fn end_subscriptions(&mut self) -> Vec<Value> {
        let ids: Vec<_> = self
            .subscriptions
            .values()
            .map(|stream| stream.id.clone())
            .collect();
        ids.iter()
            .filter_map(|id| self.end_subscription(id))
            .collect()
    }

    /// Completes streams at or beyond their deadline. Independent runtimes must poll this.
    #[must_use]
    pub fn expire_subscriptions(&mut self, now: Instant) -> Vec<Value> {
        let ids: Vec<_> = self
            .subscriptions
            .values()
            .filter(|stream| stream.deadline <= now)
            .map(|stream| stream.id.clone())
            .collect();
        ids.iter()
            .filter_map(|id| self.end_subscription(id))
            .collect()
    }

    /// Time until the nearest stream deadline, for an independent runtime's wakeup.
    #[must_use]
    pub fn subscription_wait(&self) -> Option<Duration> {
        self.subscriptions
            .values()
            .map(|stream| stream.deadline.saturating_duration_since(Instant::now()))
            .min()
    }
}
