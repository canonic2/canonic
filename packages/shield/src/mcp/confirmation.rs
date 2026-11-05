//! Connection-owned, bounded, expiring modern confirmation state. Echoed state
//! selects retained arguments; it never supplies authorization by itself.
use canonic_mcp_kit::{Decision, RequestContext, RpcError, ToolCall, rpc};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Tools whose call waits for the person's answer to Shield's form.
pub(super) const CONFIRMED_TOOLS: &[&str] = &["policy_apply", "request_approve"];

#[derive(Default)]
pub(super) struct Confirmations {
    pending: BTreeMap<String, Pending>,
}

struct Pending {
    arguments: serde_json::Value,
    expires: Instant,
}

impl Confirmations {
    pub(super) fn begin(
        &mut self,
        request: &RequestContext,
        call: &ToolCall,
        message: &str,
    ) -> Result<canonic_mcp_kit::modern::InputRequiredResult, RpcError> {
        self.pending
            .retain(|_, pending| pending.expires > Instant::now());
        if self.pending.len() >= 8 {
            return Err(RpcError::new(
                rpc::INVALID_PARAMS,
                "Too many pending confirmations",
            ));
        }
        let mut bytes = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| RpcError::new(rpc::INTERNAL_ERROR, "Confirmation entropy unavailable"))?;
        let token = crate::policy::fingerprint(&bytes);
        let result = request.confirmation(message, &token)?;
        self.pending.insert(
            token,
            Pending {
                arguments: call.arguments.clone(),
                expires: Instant::now() + Duration::from_mins(5),
            },
        );
        Ok(result)
    }

    pub(super) fn resume(
        &mut self,
        request: &RequestContext,
        call: &ToolCall,
    ) -> Result<Decision, RpcError> {
        let invalid = || {
            RpcError::new(
                rpc::INVALID_PARAMS,
                "Invalid, expired or consumed confirmation; inspect the proposal and receipt before retrying",
            )
        };
        let token = request.request_state.as_ref().ok_or_else(invalid)?;
        // Consume on any retry, including decline, changed arguments or missing
        // capability. Connection destruction revokes every remaining token.
        let pending = self.pending.remove(token).ok_or_else(invalid)?;
        if pending.expires <= Instant::now()
            || !CONFIRMED_TOOLS.contains(&call.name.as_str())
            || pending.arguments != call.arguments
        {
            return Err(invalid());
        }
        request.confirmation_decision()?.ok_or_else(invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_mcp_kit::{Catalog, ModernOptions, ServerInfo, Session, SessionOptions, Tool};
    use serde_json::json;
    use std::sync::Arc;

    fn context(state: Option<&str>) -> Arc<RequestContext> {
        let catalog = Catalog::new(vec![Tool::new(
            "policy_apply",
            "Apply",
            json!({"type":"object"}),
        )])
        .unwrap();
        let mut session = Session::try_new(
            Arc::new(catalog),
            SessionOptions {
                modern: Some(ModernOptions::new(ServerInfo::new("shield", "test"))),
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let mut params = json!({"name":"policy_apply","arguments":{},"_meta":{
            "io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{"elicitation":{"form":{}}}
        }});
        if let Some(state) = state {
            params["requestState"] = json!(state);
            params["inputResponses"] = json!({"confirmation":{"action":"accept"}});
        }
        session
            .receive_value(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":params}));
        session.request_context(&json!(1)).unwrap()
    }

    #[test]
    fn expired_and_exhausted_continuations_refuse_without_reusing_state() {
        let call = ToolCall {
            name: "policy_apply".into(),
            arguments: json!({}),
            progress_token: None,
        };
        let mut confirmations = Confirmations::default();
        let request = context(None);
        for _ in 0..8 {
            confirmations.begin(&request, &call, "Review").unwrap();
        }
        assert!(confirmations.begin(&request, &call, "Review").is_err());
        let token = confirmations.pending.keys().next().unwrap().clone();
        confirmations.pending.get_mut(&token).unwrap().expires = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("fixture clock can represent an expired continuation");
        assert!(confirmations.resume(&context(Some(&token)), &call).is_err());
        assert!(confirmations.resume(&context(Some(&token)), &call).is_err());
        confirmations.begin(&request, &call, "Review").unwrap();
    }
}
