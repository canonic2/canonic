//! Prompt routing shared by legacy and modern request admission.

use crate::{
    Inbound, Session,
    rpc::{self, RpcError},
};
use serde_json::Value;

impl Session {
    pub(super) fn prompt_request(&mut self, id: Value, method: &str, params: &Value) -> Inbound {
        let failure = |error| Inbound::Reply(rpc::failure(id.clone(), &error));
        let Some(catalog) = &self.options.prompts else {
            return failure(RpcError::new(rpc::METHOD_NOT_FOUND, "Prompts disabled"));
        };
        match method {
            "prompts/list" => match catalog.listing(params) {
                Ok(result) => Inbound::Reply(rpc::success(id, result)),
                Err(error) => failure(error),
            },
            "prompts/get" => match catalog.call(params) {
                Ok(call) => {
                    self.outstanding.insert(id.to_string());
                    Inbound::Prompt { id, call }
                }
                Err(error) => failure(error),
            },
            _ => failure(RpcError::new(rpc::METHOD_NOT_FOUND, "Method not found")),
        }
    }
}
