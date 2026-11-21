//! Terminal outcome bounds and replay are independent of every grant/handoff.
use super::*;
const PROJECT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BINDING: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn malformed_optional_receipts_are_preserved_without_authority() {
    #[derive(Serialize, Deserialize)]
    struct Optional {
        #[serde(deserialize_with = "Registry::deserialize_optional")]
        receipts: Registry,
    }
    let opaque = json!({"records":[{"unknown":"historical outcome"}]});
    let mut value: Optional = serde_json::from_value(json!({"receipts":opaque})).unwrap();
    value.receipts.validate().unwrap();
    assert!(
        value
            .receipts
            .inspect(&context(), "read-request-acme", 20)
            .is_none()
    );
    assert_eq!(
        serde_json::to_value(&value).unwrap()["receipts"]["unavailable"],
        opaque
    );
    value
        .receipts
        .capture(capture(claim("read-request-new"), Status::Revoked, 20))
        .unwrap();
    let roundtrip: Optional =
        serde_json::from_value(serde_json::to_value(&value).unwrap()).unwrap();
    assert_eq!(roundtrip.receipts.unavailable, Some(opaque));
    assert!(
        roundtrip
            .receipts
            .inspect(&context(), "read-request-new", 21)
            .is_some()
    );
    let null: Optional = serde_json::from_value(json!({"receipts":null})).unwrap();
    let null: Optional = serde_json::from_value(serde_json::to_value(null).unwrap()).unwrap();
    assert_eq!(null.receipts.unavailable, Some(Value::Null));
}

#[test]
fn delayed_native_harvest_evicts_by_original_terminal_time_and_keeps_other_kinds() {
    let mut registry = Registry::default();
    registry
        .capture(capture(claim("read-request-kept"), Status::Revoked, 20))
        .unwrap();
    for index in 0..PER_KIND {
        let id = format!("hook-request-{index}");
        let native = Claim {
            kind: Kind::Hook,
            agent: Some("claude"),
            consumer: "native-hook",
            ..claim(&id)
        };
        registry
            .capture(capture(native, Status::Revoked, 100 + index as u64))
            .unwrap();
    }
    let old = Claim {
        kind: Kind::Hook,
        agent: Some("claude"),
        consumer: "native-hook",
        ..claim("hook-request-delayed")
    };
    registry
        .capture(capture(old, Status::Consumed, 20))
        .unwrap();
    assert!(registry.replay_consumer(old, 300).is_none());
    assert!(
        registry
            .inspect(&context(), "read-request-kept", 300)
            .is_some()
    );
    assert_eq!(
        registry
            .records
            .iter()
            .filter(|r| r.kind == Kind::Hook)
            .count(),
        PER_KIND
    );
    let native = Claim {
        kind: Kind::Hook,
        agent: Some("claude"),
        consumer: "native-hook",
        ..claim("hook-request-0")
    };
    for changed in [
        Claim {
            binding: PROJECT,
            ..native
        },
        Claim {
            session: Some("other"),
            ..native
        },
    ] {
        assert_eq!(
            registry.replay_consumer(changed, 300).unwrap()["code"],
            "approval-stale"
        );
    }
    registry.validate().unwrap();
}

#[test]
fn all_consumer_kinds_can_retain_their_complete_independent_capacity_after_reload() {
    let mut registry = Registry::default();
    for kind in [Kind::Read, Kind::Execution, Kind::Hook] {
        for index in 0..PER_KIND {
            let id = format!("{}{index}", kind.prefix());
            let claim = Claim {
                kind,
                agent: Some("claude"),
                ..claim(&id)
            };
            registry
                .capture(capture(claim, Status::Revoked, 20))
                .unwrap();
        }
    }
    registry.validate().unwrap();
    let registry: Registry =
        serde_json::from_value(serde_json::to_value(registry).unwrap()).unwrap();
    registry.validate().unwrap();
    assert_eq!(registry.records.len(), 3 * PER_KIND);
    for kind in [Kind::Read, Kind::Execution, Kind::Hook] {
        let id = format!("{}0", kind.prefix());
        assert!(registry.inspect(&context(), &id, 21).is_some());
    }
}

fn claim(id: &str) -> Claim<'_> {
    Claim {
        kind: Kind::Read,
        id,
        project: PROJECT,
        agent: Some("claude-sdk"),
        session: Some("acme-session"),
        consumer: "acme-factory",
        binding: BINDING,
    }
}
fn context() -> Context {
    let mut context = Context::new(PROJECT.into(), "/acme".into(), "mcp");
    context.agent = Some("claude".into());
    context.session_id = Some("acme-session".into());
    context
}
fn capture(claim: Claim<'_>, status: Status, at: u64) -> Capture<'_> {
    Capture {
        claim,
        decision: if status == Status::Consumed {
            Decision::Approve
        } else {
            Decision::Deny
        },
        status,
        grant: (status == Status::Consumed).then_some(Granted {
            id: "approval-private",
            issued: 10,
            expires: 300_010,
        }),
        reason: None,
        at,
    }
}

#[test]
fn replay_reports_terminal_outcomes_without_reusable_approval_material() {
    for status in [Status::Consumed, Status::Revoked] {
        let mut registry = Registry::default();
        let claim = claim("read-request-acme");
        registry.capture(capture(claim, status, 20)).unwrap();
        registry.validate().unwrap();
        let value = registry.replay_consumer(claim, 21).unwrap();
        assert_eq!(value["allowed"], false);
        assert_eq!(value["approval"]["approval_id"], Value::Null);
        assert_eq!(value["approval"]["status"], status.name());
        let operator = registry
            .replay_operator(OperatorReplay {
                context: &context(),
                id: claim.id,
                decision: if status == Status::Consumed {
                    Decision::Approve
                } else {
                    Decision::Deny
                },
                ttl_seconds: (status == Status::Consumed).then_some(300),
                reason: None,
                at: 21,
            })
            .unwrap()
            .unwrap();
        assert_eq!(operator["status"], status.name());
        assert_eq!(operator["allowed"], false);
        assert!(operator.get("approval_id").is_none());
        assert!(
            !serde_json::to_string(&registry)
                .unwrap()
                .contains("approval-private")
        );
    }
}

#[test]
fn contexts_parameters_and_claims_cannot_substitute_for_the_terminal_request() {
    let mut registry = Registry::default();
    let original = claim("read-request-acme");
    registry
        .capture(capture(original, Status::Consumed, 20))
        .unwrap();
    for changed in [
        Claim {
            consumer: "another-factory",
            ..original
        },
        Claim {
            session: Some("another-session"),
            ..original
        },
        Claim {
            binding: PROJECT,
            ..original
        },
        Claim {
            agent: Some("another-agent"),
            ..original
        },
    ] {
        let value = registry.replay_consumer(changed, 21).unwrap();
        assert_eq!(value["code"], "approval-stale");
        assert_eq!(value["allowed"], false);
    }
    assert!(
        registry
            .replay_consumer(
                Claim {
                    project: BINDING,
                    ..original
                },
                21
            )
            .is_none()
    );
    let mut foreign = context();
    foreign.session_id = Some("another-session".into());
    assert!(matches!(
        registry.replay_operator(OperatorReplay {
            context: &foreign,
            id: original.id,
            decision: Decision::Approve,
            ttl_seconds: Some(300),
            reason: None,
            at: 21
        }),
        Err(Error::Missing)
    ));
    for (decision, ttl, reason) in [
        (Decision::Deny, None, None),
        (Decision::Approve, Some(301), None),
        (Decision::Approve, Some(300), Some("changed reason")),
    ] {
        assert!(
            registry
                .replay_operator(OperatorReplay {
                    context: &context(),
                    id: original.id,
                    decision,
                    ttl_seconds: ttl,
                    reason,
                    at: 21
                })
                .is_err()
        );
    }
}

#[test]
fn retry_does_not_renew_retention_and_expired_or_foreign_epochs_disappear() {
    let mut registry = Registry::default();
    let claim = claim("read-request-acme");
    registry
        .capture(capture(claim, Status::Revoked, 20))
        .unwrap();
    let until = registry.records[0].until;
    registry
        .capture(capture(claim, Status::Revoked, 200))
        .unwrap();
    assert_eq!(registry.records[0].until, until);
    assert!(registry.replay_consumer(claim, 19).is_none());
    assert!(registry.replay_consumer(claim, until).is_none());
    registry.compact(PROJECT, until);
    assert_eq!(registry.records, [] as [Receipt; 0]);
    registry
        .capture(capture(
            self::claim("read-request-new"),
            Status::Revoked,
            until,
        ))
        .unwrap();
    registry.compact(BINDING, until);
    assert_eq!(registry.records, [] as [Receipt; 0]);
}

#[test]
fn retained_receipts_are_bounded_and_cannot_modify_a_recorded_outcome() {
    let mut registry = Registry::default();
    for index in 0..=PER_KIND {
        let id = format!("read-request-{index}");
        registry
            .capture(capture(claim(&id), Status::Revoked, 20))
            .unwrap();
    }
    assert_eq!(registry.records.len(), PER_KIND);
    assert!(
        registry
            .replay_consumer(claim("read-request-0"), 21)
            .is_none()
    );
    registry.validate().unwrap();
    assert!(
        registry
            .capture(capture(claim("read-request-1"), Status::Consumed, 21))
            .is_err()
    );
    registry.records[0].status = Status::Consumed;
    assert!(registry.validate().is_err());
}

#[test]
fn terminal_groups_replay_without_grants_and_mixed_or_unknown_sessions_refuse() {
    let mut registry = Registry::default();
    for id in ["read-request-first", "read-request-second"] {
        registry
            .capture(capture(claim(id), Status::Consumed, 20))
            .unwrap();
    }
    let group = registry
        .replay_approval_group(
            &context(),
            &["read-request-first", "read-request-second"],
            21,
        )
        .unwrap()
        .unwrap();
    assert_eq!(group.len(), 2);
    assert!(
        group
            .iter()
            .all(|row| row["allowed"] == false && row.get("approval_id").is_none())
    );
    assert!(matches!(
        registry.replay_approval_group(
            &context(),
            &["read-request-first", "read-request-live"],
            21
        ),
        Err(Error::Grouped { .. })
    ));
    let mut operator = context();
    operator.surface = "cli".into();
    registry
        .capture(capture(
            Claim {
                session: None,
                ..claim("read-request-unknown")
            },
            Status::Consumed,
            20,
        ))
        .unwrap();
    assert!(matches!(
        registry.replay_approval_group(
            &operator,
            &["read-request-first", "read-request-unknown"],
            21
        ),
        Err(Error::Grouped { .. })
    ));
}
