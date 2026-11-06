//! Operator session-grant inspection and terminal-only approval/revocation.
use super::{Context, Error, Value, json, terminal};
use crate::cli::public::SessionGrantCommand;
use crate::review::session_grants;

pub(super) fn run(
    context: &Context,
    command: &SessionGrantCommand,
    json_mode: bool,
) -> Result<Value, Error> {
    match command {
        SessionGrantCommand::Propose { input } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(input)?
                .take(1_048_577)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 1_048_576 {
                return Err(Error::Invalid("session grant input exceeds 1 MiB"));
            }
            let args = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Invalid("invalid session grant JSON"))?;
            session_grants::propose(context, &args)
        }
        SessionGrantCommand::List {
            session_ref,
            cursor,
            limit,
        } => {
            let mut args = json!({"limit":limit});
            if let Some(reference) = session_ref {
                args["session_ref"] = json!(reference);
            }
            if let Some(cursor) = cursor {
                args["cursor"] = json!(cursor);
            }
            session_grants::show(context, &args)
        }
        SessionGrantCommand::Show { id } => session_grants::show(context, &selection(id)),
        SessionGrantCommand::Approve { proposal_id } => {
            let mut tty = terminal::Terminal::required(json_mode)?;
            let row = session_grants::show(context, &json!({"proposal_id":proposal_id}))?;
            let args = &row["apply_arguments"];
            if let Some(message) = session_grants::confirmation(context, args)?
                && let Err(error) = tty.confirm(&json!({"session_grant_review":message}))
            {
                if matches!(error, Error::Cancelled) {
                    session_grants::decline(context, args)?;
                }
                return Err(error);
            }
            session_grants::apply(context, args, true)
        }
        SessionGrantCommand::Revoke { id } => {
            let mut tty = terminal::Terminal::required(json_mode)?;
            let args = selection(id);
            let row = session_grants::show(context, &args)?;
            tty.confirm(&json!({"session_grant_review":format!("Revoke this session grant?\n{}\nGrant: {}\nProposal: {}",row["review"].as_str().unwrap_or_default(),row["grant_id"],row["proposal_id"])}))?;
            session_grants::revoke(context, &args)
        }
    }
}
fn selection(id: &str) -> Value {
    if id.starts_with("session-grant-") {
        json!({"grant_id":id})
    } else {
        json!({"proposal_id":id})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_file_proposal_retains_native_target_without_issuing_authority() {
        let (root, mut context, reference) = crate::review::session_grants::native_fixture();
        context.surface = "cli".into();
        context.agent = None;
        context.session_id = None;
        let input = root.path().join("grant.json");
        std::fs::write(
            &input,
            serde_json::to_vec(&json!({
                "session_ref":reference,"idempotency_key":"operator-preparation",
                "scope":[{"conditions":{"domain":"argv","executable":"acme",
                    "arguments":{"kind":"exact","values":["read"]},
                    "cwd":{"within":{"root":"project"}}}}]
            }))
            .unwrap(),
        )
        .unwrap();
        let result = run(&context, &SessionGrantCommand::Propose { input }, true).unwrap();
        assert_eq!(result["target"]["agent"], "codex");
        assert_eq!(result["proposal_status"], "pending");
        assert!(result["grant_id"].is_null());
        assert!(result["apply_arguments"]["candidate_digest"].is_string());
    }
}
