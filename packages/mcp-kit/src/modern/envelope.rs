//! Borrowed complete-result serialization with one merged metadata object.
use super::{CachePolicy, CompletePayload, RequestContext, SERVER_KEY};
use crate::{Interrupted, json::EncodeError, rpc};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::{Map, Value};

struct Metadata<'a> {
    custom: Option<&'a Map<String, Value>>,
    server: &'a Value,
}
impl Serialize for Metadata<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let Some(custom) = self.custom {
            for (key, value) in custom {
                map.serialize_entry(key, value)?;
            }
        }
        map.serialize_entry(SERVER_KEY, self.server)?;
        map.end()
    }
}
struct Envelope<'a, T: ?Sized> {
    context: &'a RequestContext,
    result: &'a T,
    cache: Option<CachePolicy>,
}
impl<T: CompletePayload + ?Sized> Serialize for Envelope<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("resultType", "complete")?;
        self.result.write_fields(&mut map)?;
        map.serialize_entry(
            "_meta",
            &Metadata {
                custom: self.result.metadata_map(),
                server: &self.context.server,
            },
        )?;
        if let Some(policy) = self
            .context
            .cache
            .map(|policy| self.cache.unwrap_or(policy))
        {
            let policy =
                if self.context.request_state.is_some() || self.context.input_responses.is_some() {
                    CachePolicy::default()
                } else {
                    policy
                };
            map.serialize_entry("ttlMs", &policy.ttl_ms)?;
            map.serialize_entry("cacheScope", &policy.cache_scope)?;
        }
        map.end()
    }
}
pub(super) fn encode(
    context: &RequestContext,
    id: &Value,
    result: &(impl CompletePayload + ?Sized),
    cache: Option<CachePolicy>,
    limit: usize,
    check: &dyn Fn() -> Result<(), Interrupted>,
) -> Result<Vec<u8>, EncodeError> {
    if result
        .metadata_map()
        .is_some_and(|map| !crate::metadata::result_allowed(map))
    {
        return Err(EncodeError::Invalid);
    }
    rpc::encode_success(
        id,
        &Envelope {
            context,
            result,
            cache,
        },
        limit,
        check,
    )
}
