//! Connection-owned, bounded modern confirmation state. A question stays
//! answerable until it is answered, cancelled or its connection ends; age never
//! revokes it. Echoed state selects retained arguments; it never supplies
//! authorization by itself.
use canonic_mcp_kit::{Decision, RequestContext, RpcError, ToolCall, rpc};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::BTreeMap;

/// Why a confirmation could not start.
#[derive(Debug)]
pub(super) enum Refusal {
    /// Eight questions are open; the review stays pending for a later attempt.
    Full(RpcError),
    /// The host cannot present this confirmation.
    Unavailable,
}

#[derive(Default)]
pub(super) struct Confirmations {
    pending: BTreeMap<String, Pending>,
}

struct Pending {
    guard: Option<super::PolicyWait>,
    tool: String,
    arguments: serde_json::Value,
    review_binding: Option<u64>,
    action: Option<ToolCall>,
}

impl Confirmations {
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
        mut guard: Option<super::PolicyWait>,
        action: Option<ToolCall>,
    ) -> Result<canonic_mcp_kit::modern::InputRequiredResult, Refusal> {
        if self.pending.len() >= 8 {
            self.release_closed_reviews();
        }
        if self.pending.len() >= 8 {
            // A full queue leaves the new review pending for a later attempt.
            if let Some(guard) = &mut guard {
                guard.disarm();
            }
            return Err(Refusal::Full(RpcError::new(
                rpc::INVALID_PARAMS,
                "Too many pending confirmations; answer or cancel an open one, then retry",
            )));
        }
        let mut bytes = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| Refusal::Unavailable)?;
        let token = crate::policy::fingerprint(&bytes);
        let result = request
            .confirmation(message, &token)
            .map_err(|_| Refusal::Unavailable)?;
        self.pending.insert(
            token,
            Pending {
                guard,
                tool: call.name.clone(),
                arguments: call.arguments.clone(),
                review_binding,
                action,
            },
        );
        Ok(result)
    }

    /// Free the slots of `policy_apply` questions whose review has closed, as
    /// when a newer proposal superseded it and the host dropped the old form.
    fn release_closed_reviews(&mut self) {
        self.pending.retain(|_, pending| match &mut pending.guard {
            Some(guard)
                if guard.armed
                    && !crate::review::proposal_lifecycle::still_pending(
                        &guard.context,
                        &guard.arguments,
                    ) =>
            {
                guard.disarm();
                false
            }
            _ => true,
        });
    }

    pub(super) fn resume(
        &mut self,
        request: &RequestContext,
        call: &ToolCall,
    ) -> Result<(Decision, Option<u64>, Option<ToolCall>), RpcError> {
        let invalid = || {
            RpcError::new(
                rpc::INVALID_PARAMS,
                "Invalid or consumed confirmation; inspect the proposal and receipt before retrying",
            )
        };
        let token = request.request_state.as_ref().ok_or_else(invalid)?;
        // Consume on any retry, including decline, changed arguments or missing
        // capability. Connection destruction revokes every remaining token.
        let mut pending = self.pending.remove(token).ok_or_else(invalid)?;
        if pending.tool != call.name || pending.arguments != call.arguments {
            return Err(invalid());
        }
        let decision = request.confirmation_decision()?.ok_or_else(invalid)?;
        if let Some(guard) = &mut pending.guard {
            guard.disarm();
        }
        Ok((decision, pending.review_binding, pending.action))
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
    fn continuations_are_single_use_bounded_and_freed_by_answer_or_cancel() {
        let call = ToolCall {
            name: "policy_apply".into(),
            arguments: json!({}),
            progress_token: None,
        };
        let mut confirmations = Confirmations::default();
        let request = context(None);
        for _ in 0..8 {
            confirmations
                .begin(&request, &call, "Review", None, None, None)
                .unwrap();
        }
        assert!(
            confirmations
                .begin(&request, &call, "Review", None, None, None)
                .is_err()
        );
        // An answer is accepted however long the question was open, once.
        let token = confirmations.pending.keys().next().unwrap().clone();
        let (decision, _, _) = confirmations.resume(&context(Some(&token)), &call).unwrap();
        assert_eq!(decision, Decision::Accepted);
        assert!(confirmations.resume(&context(Some(&token)), &call).is_err());
        confirmations
            .begin(&request, &call, "Review", None, None, None)
            .unwrap();
        let token = confirmations.pending.keys().next().unwrap().clone();
        let count = confirmations.pending.len();
        confirmations.discard(&context(Some(&token)));
        assert_eq!(confirmations.pending.len(), count - 1);
        assert!(confirmations.resume(&context(Some(&token)), &call).is_err());
    }
}
