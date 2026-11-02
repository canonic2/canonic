//! Completion admission uses the same immutable catalogs as discovery.
use crate::{
    CompleteRequest, Inbound, Session,
    rpc::{self, RpcError},
};
use serde_json::Value;

impl Session {
    pub(super) fn completion_request(
        &mut self,
        id: Value,
        method: &str,
        params: &Value,
    ) -> Inbound {
        let failure = |error| Inbound::Reply(rpc::failure(id.clone(), &error));
        if method != "completion/complete" || !self.options.completions {
            return failure(RpcError::new(rpc::METHOD_NOT_FOUND, "Method not found"));
        }
        match CompleteRequest::parse(
            params,
            self.options.prompts.as_deref(),
            self.options.resources.as_deref(),
        ) {
            Ok(request) => {
                self.outstanding.insert(id.to_string());
                Inbound::Completion { id, request }
            }
            Err(error) => failure(error),
        }
    }
}
