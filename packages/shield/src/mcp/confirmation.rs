//! Connection-owned, bounded, expiring modern confirmation state. Echoed state
//! selects retained arguments; it never supplies authorization by itself.
use canonic_mcp_kit::{Decision, RequestContext, RpcError, ToolCall, rpc};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Tools whose call waits for the person's answer to Shield's form.
pub(super) const CONFIRMED_TOOLS: &[&str] = &[
    "project_enroll",
    "policy_apply",
    "request_approve",
    "request_deny",
    "session_grant_approve",
    "protection_set",
    "enable",
    "disable",
    "registry_add",
    "registry_remove",
    "registry_default",
    "policy_repair",
    "history_configure",
    "policy_reject",
    "mcp_setup",
    "cache_prepare",
];

#[derive(Default)]
pub(super) struct Confirmations {
    pending: BTreeMap<String, Pending>,
}

struct Pending {
    guard: Option<super::PolicyWait>,
    tool: String,
    arguments: serde_json::Value,
    expires: Instant,
    review_binding: Option<u64>,
}

impl Confirmations {
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    pub(super) fn discard(&mut self, request: &RequestContext) -> Option<u64> {
        self.pending
            .remove(request.request_state.as_ref()?)?
            .review_binding
    }
    /// Cleanup binding of a continuation before resume consumes it, including invalid retries.
    pub(super) fn review_binding(&self, request: &RequestContext) -> Option<u64> {
        self.pending
            .get(request.request_state.as_ref()?)?
            .review_binding
    }

    pub(super) fn begin(
        &mut self,
        request: &RequestContext,
        call: &ToolCall,
        message: &str,
        review_binding: Option<u64>,
        guard: Option<super::PolicyWait>,
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
                guard,
                tool: call.name.clone(),
                arguments: call.arguments.clone(),
                expires: Instant::now() + Duration::from_mins(5),
                review_binding,
            },
        );
        Ok(result)
    }

    pub(super) fn resume(
        &mut self,
        request: &RequestContext,
        call: &ToolCall,
    ) -> Result<(Decision, Option<u64>), RpcError> {
        let invalid = || {
            RpcError::new(
                rpc::INVALID_PARAMS,
                "Invalid, expired or consumed confirmation; inspect the proposal and receipt before retrying",
            )
        };
        let token = request.request_state.as_ref().ok_or_else(invalid)?;
        // Consume on any retry, including decline, changed arguments or missing
        // capability. Connection destruction revokes every remaining token.
        let mut pending = self.pending.remove(token).ok_or_else(invalid)?;
        if pending.expires <= Instant::now()
            || !CONFIRMED_TOOLS.contains(&call.name.as_str())
            || pending.tool != call.name
            || pending.arguments != call.arguments
        {
            return Err(invalid());
        }
        let decision = request.confirmation_decision()?.ok_or_else(invalid)?;
        if let Some(guard) = &mut pending.guard {
            guard.disarm();
        }
        Ok((decision, pending.review_binding))
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
            confirmations
                .begin(&request, &call, "Review", None, None)
                .unwrap();
        }
        assert!(
            confirmations
                .begin(&request, &call, "Review", None, None)
                .is_err()
        );
        let token = confirmations.pending.keys().next().unwrap().clone();
        confirmations.pending.get_mut(&token).unwrap().expires = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("fixture clock can represent an expired continuation");
        assert!(confirmations.resume(&context(Some(&token)), &call).is_err());
        assert!(confirmations.resume(&context(Some(&token)), &call).is_err());
        confirmations
            .begin(&request, &call, "Review", None, None)
            .unwrap();
        let token = confirmations.pending.keys().next().unwrap().clone();
        let count = confirmations.pending.len();
        confirmations.discard(&context(Some(&token)));
        assert_eq!(confirmations.pending.len(), count - 1);
        assert!(confirmations.resume(&context(Some(&token)), &call).is_err());
    }
}
