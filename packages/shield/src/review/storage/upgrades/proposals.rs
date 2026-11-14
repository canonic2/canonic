//! Historical proposal captures are evidence, never a source of new approval.
//! Missing old captures close live attempts; their exact absent/null form stays
//! in diagnostic reasons while the current representation uses empty objects.
use super::{Error, Value, json};

pub(super) fn prepare(target: &mut Value) -> Result<(), Error> {
    let rows = target["proposals"]
        .as_array_mut()
        .ok_or(Error::Invalid("invalid historical proposal collection"))?;
    for row in rows {
        let fields = row
            .as_object_mut()
            .ok_or(Error::Invalid("invalid historical proposal"))?;
        // A proposal stores either whole retained candidates or the policy
        // file texts it writes; the latter has no base candidate to capture.
        let retained = fields.get("candidate").is_some_and(Value::is_object);
        let texts = fields
            .get("files")
            .and_then(Value::as_object)
            .is_some_and(|files| !files.is_empty());
        if !(retained || texts) {
            return Err(Error::Invalid(
                "historical proposal candidate is unavailable",
            ));
        }
        let names: &[&str] = if retained {
            &["base_candidate", "base_metadata", "candidate_metadata"]
        } else {
            &["base_metadata", "candidate_metadata"]
        };
        let mut captured = serde_json::Map::new();
        let mut missing = Vec::new();
        for &name in names {
            match fields.get(name) {
                Some(value) if value.is_object() => {}
                Some(Value::Null) => {
                    captured.insert(name.into(), Value::Null);
                }
                None => {
                    missing.push(name);
                    captured.insert(name.into(), Value::Null);
                }
                Some(_) => return Err(Error::Invalid("invalid historical proposal capture")),
            }
        }
        mark_missing_receipt(fields)?;
        if captured.is_empty() {
            continue;
        }
        let status = fields
            .get("status")
            .and_then(Value::as_str)
            .ok_or(Error::Invalid("invalid historical proposal status"))?
            .to_owned();
        if ![
            "pending",
            "applied",
            "rejected",
            "withdrawn",
            "stale",
            "failed",
        ]
        .contains(&status.as_str())
        {
            return Err(Error::Invalid("invalid historical proposal status"));
        }
        let evidence = json!({"fields":captured,"missing":missing,"original_status":status});
        let reasons = fields
            .get_mut("reasons")
            .and_then(Value::as_object_mut)
            .ok_or(Error::Invalid("invalid historical proposal reasons"))?;
        if reasons.contains_key("historical_capture") {
            return Err(Error::Invalid(
                "historical proposal capture evidence conflicts",
            ));
        }
        reasons.insert("historical_capture".into(), evidence);
        for &name in names {
            if !fields.get(name).is_some_and(Value::is_object) {
                fields.insert(name.into(), json!({}));
            }
        }
        if status == "pending" {
            fields.insert("status".into(), json!("withdrawn"));
            fields.insert(
                "closed_reason".into(),
                json!("historical_capture_unavailable"),
            );
        }
    }
    Ok(())
}

/// Fill the defaults the historical codec explicitly supported, and mark an
/// applied proposal without a receipt. Neither absent operator identity nor an
/// absent receipt implies approval.
fn mark_missing_receipt(fields: &mut serde_json::Map<String, Value>) -> Result<(), Error> {
    fields.entry("receipt").or_insert(Value::Null);
    fields.entry("operator").or_insert(Value::Null);
    if fields.get("status").and_then(Value::as_str) != Some("applied")
        || !fields.get("receipt").is_some_and(Value::is_null)
    {
        return Ok(());
    }
    let reasons = fields
        .get_mut("reasons")
        .and_then(Value::as_object_mut)
        .ok_or(Error::Invalid("invalid historical proposal reasons"))?;
    if reasons
        .get("historical_receipt_unavailable")
        .is_some_and(|value| value != &Value::Bool(true))
    {
        return Err(Error::Invalid(
            "historical proposal receipt evidence conflicts",
        ));
    }
    reasons.insert("historical_receipt_unavailable".into(), Value::Bool(true));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint(status: &str) -> Value {
        json!({"proposals":[{
            "proposal_id":"historical-proposal","status":status,
            "candidate":{"config":"original candidate"},
            "base_candidate":{"config":"original base"},
            "base_metadata":{"sources":{}},"candidate_metadata":{"sources":{}},
            "receipt":if status == "applied" {json!({"activation_id":"original-activation","committed":true})} else {Value::Null},
            "operator":if status == "applied" {json!({"mechanism":"operator-cli"})} else {Value::Null},
            "signature":"original-input-signature","retry_key":"original-key","retry_owner":"original-owner",
            "reasons":{"agent_reason":"original intent"},"comparison":{"coverage":"original comparison"}
        }]})
    }

    #[test]
    fn historical_proposals_keep_complete_captures_and_supported_defaults() {
        for status in ["pending", "applied", "rejected", "withdrawn", "stale"] {
            let mut target = checkpoint(status);
            let original = target.clone();
            prepare(&mut target).unwrap();
            assert_eq!(target, original);
        }
        let mut target = checkpoint("pending");
        target["proposals"][0]
            .as_object_mut()
            .unwrap()
            .remove("receipt");
        target["proposals"][0]
            .as_object_mut()
            .unwrap()
            .remove("operator");
        prepare(&mut target).unwrap();
        assert_eq!(target["proposals"][0]["receipt"], Value::Null);
        assert_eq!(target["proposals"][0]["operator"], Value::Null);
        assert_eq!(target["proposals"][0]["status"], "pending");
    }

    #[test]
    fn historical_proposals_mark_missing_applied_receipts_without_fabricating_approval() {
        for missing_receipt in [false, true] {
            for incomplete_capture in [false, true] {
                let mut target = checkpoint("applied");
                target["proposals"][0]["receipt"] = Value::Null;
                if missing_receipt {
                    target["proposals"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("receipt");
                }
                if incomplete_capture {
                    target["proposals"][0]["base_metadata"] = Value::Null;
                }
                let original = target["proposals"][0].clone();
                prepare(&mut target).unwrap();
                let row = &target["proposals"][0];
                assert_eq!(row["status"], "applied");
                assert_eq!(row["receipt"], Value::Null);
                assert_eq!(row["reasons"]["historical_receipt_unavailable"], true);
                for name in [
                    "operator",
                    "candidate",
                    "signature",
                    "proposal_id",
                    "retry_key",
                    "retry_owner",
                ] {
                    assert_eq!(row[name], original[name]);
                }
                let once = target.clone();
                prepare(&mut target).unwrap();
                assert_eq!(target, once);
            }
        }
    }

    #[test]
    fn historical_proposals_close_only_incomplete_pending_captures_without_new_authority() {
        for status in ["pending", "applied", "rejected", "withdrawn", "stale"] {
            let mut target = checkpoint(status);
            target["proposals"][0]
                .as_object_mut()
                .unwrap()
                .remove("base_candidate");
            target["proposals"][0]["base_metadata"] = Value::Null;
            target["proposals"][0]["candidate_metadata"] = Value::Null;
            let original = target["proposals"][0].clone();
            prepare(&mut target).unwrap();
            let row = &target["proposals"][0];
            for name in [
                "proposal_id",
                "candidate",
                "receipt",
                "operator",
                "signature",
                "retry_key",
                "retry_owner",
                "comparison",
            ] {
                assert_eq!(row[name], original[name], "{status} {name}");
            }
            assert_eq!(
                row["reasons"]["agent_reason"],
                original["reasons"]["agent_reason"]
            );
            assert_eq!(
                row["reasons"]["historical_capture"],
                json!({
                    "fields":{"base_candidate":null,"base_metadata":null,"candidate_metadata":null},
                    "missing":["base_candidate"],"original_status":status
                })
            );
            for name in ["base_candidate", "base_metadata", "candidate_metadata"] {
                assert_eq!(row[name], json!({}));
            }
            assert_eq!(
                row["status"],
                if status == "pending" {
                    "withdrawn"
                } else {
                    status
                }
            );
            if status == "pending" {
                assert_eq!(row["closed_reason"], "historical_capture_unavailable");
            }
        }
    }

    #[test]
    fn historical_proposals_refuse_malformed_capture_or_conflicting_evidence() {
        for field in [
            "candidate",
            "base_candidate",
            "base_metadata",
            "candidate_metadata",
            "reasons",
        ] {
            let mut target = checkpoint("pending");
            target["proposals"][0]["base_metadata"] = Value::Null;
            target["proposals"][0][field] = json!(["invalid"]);
            assert!(prepare(&mut target).is_err(), "{field}");
        }
        let mut target = checkpoint("pending");
        target["proposals"][0]["base_metadata"] = Value::Null;
        target["proposals"][0]["reasons"]["historical_capture"] = json!({"existing":"evidence"});
        assert!(prepare(&mut target).is_err());
    }
}
