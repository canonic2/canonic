//! Owned callback matching preserves callbacks from other integrations.
use super::*;

fn group(command: &str) -> Value {
    json!({"matcher":"Bash","timeout":30,"hooks":[{"type":"command",
        "command":command,"statusMessage":"Shield project enforcement"}]})
}

fn owned(command: &str) -> HookGroups {
    BTreeMap::from([("PreToolUse".into(), group(command))])
}

fn source(group: &Value) -> String {
    json!({"env":{"ACME_SETTING":"preserved"},"hooks":{"PreToolUse":[group]}}).to_string()
}

#[test]
fn explicit_hook_repair_preserves_empty_user_groups_and_removes_retired_callbacks() {
    let empty = json!({"matcher":"Acme","hooks":[]});
    let foreign = json!({"hooks":[{"type":"command","command":"acme-hook"}]});
    let source = json!({"hooks":{
        "PreToolUse":[empty, group("shield __internal native-hook stale")],
        "RetiredEvent":[empty, group("shield __internal native-hook retired"), foreign],
        "ForeignEvent":{"host_specific":"preserved"}
    }})
    .to_string();
    let desired = owned("shield __internal native-hook current");
    let repaired = parse(Some(&repair(Some(&source), &desired).unwrap())).unwrap();
    assert_eq!(
        repaired["hooks"]["PreToolUse"],
        json!([empty, desired["PreToolUse"]])
    );
    assert_eq!(repaired["hooks"]["RetiredEvent"], json!([empty, foreign]));
    assert_eq!(
        repaired["hooks"]["ForeignEvent"],
        json!({"host_specific":"preserved"})
    );
}

#[test]
fn owned_callbacks_capture_and_update_without_claiming_foreign_callbacks() {
    let previous = owned("shield __internal native-hook old");
    let desired = owned("shield __internal native-hook new");
    let foreign = json!({"type":"command","command":"acme-hook","timeout":7});
    let mut shared = previous["PreToolUse"].clone();
    shared["hooks"] = json!([foreign, previous["PreToolUse"]["hooks"][0], foreign]);
    let actual = source(&shared);
    assert_eq!(capture(Some(&actual), &previous).unwrap(), previous);
    let mut diagnostics = Vec::new();
    diagnose(Some(&actual), &previous, &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty());
    let updated = merge(Some(&actual), &previous, &desired).unwrap();
    let document = parse(Some(&updated)).unwrap();
    assert_eq!(document["env"], json!({"ACME_SETTING":"preserved"}));
    assert_eq!(document["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(
        document["hooks"]["PreToolUse"][0]["hooks"],
        json!([foreign, desired["PreToolUse"]["hooks"][0], foreign])
    );
    assert_eq!(capture(Some(&updated), &desired).unwrap(), desired);
    let mut diagnostics = Vec::new();
    diagnose(Some(&updated), &desired, &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty());
}

#[test]
fn owned_callback_removal_preserves_the_foreign_callback_header_and_order() {
    let previous = owned("shield __internal native-hook old");
    let first = json!({"type":"command","command":"acme-first"});
    let last = json!({"type":"command","command":"acme-last"});
    let mut shared = previous["PreToolUse"].clone();
    shared["hooks"] = json!([first, previous["PreToolUse"]["hooks"][0], last]);
    let removed = merge(Some(&source(&shared)), &previous, &HookGroups::new()).unwrap();
    let actual = parse(Some(&removed)).unwrap();
    let mut expected = shared;
    expected["hooks"] = json!([first, last]);
    assert_eq!(actual["hooks"]["PreToolUse"], json!([expected]));
    assert_eq!(actual["env"], json!({"ACME_SETTING":"preserved"}));
}

#[test]
fn owned_callback_scope_changes_do_not_move_foreign_callbacks_to_the_new_scope() {
    let previous = owned("shield __internal native-hook old");
    let mut desired = owned("shield __internal native-hook new");
    desired.get_mut("PreToolUse").unwrap()["matcher"] = json!("Read");
    let foreign = json!({"type":"command","command":"acme-hook"});
    let mut shared = previous["PreToolUse"].clone();
    shared["hooks"]
        .as_array_mut()
        .unwrap()
        .push(foreign.clone());
    let updated = merge(Some(&source(&shared)), &previous, &desired).unwrap();
    let actual = parse(Some(&updated)).unwrap();
    assert_eq!(actual["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
    assert_eq!(actual["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(actual["hooks"]["PreToolUse"][0]["hooks"], json!([foreign]));
    assert_eq!(actual["hooks"]["PreToolUse"][1], desired["PreToolUse"]);
}

#[test]
fn owned_callbacks_reject_changed_headers_or_ambiguous_shield_callbacks() {
    let previous = owned("shield __internal native-hook old");
    for mutation in [
        "matcher",
        "extra-header",
        "missing-header",
        "changed-callback",
        "duplicate-callback",
        "extra-shield",
        "second-group",
    ] {
        let mut actual = parse(Some(&source(&previous["PreToolUse"]))).unwrap();
        let current = &mut actual["hooks"]["PreToolUse"][0];
        match mutation {
            "matcher" => current["matcher"] = json!("Read"),
            "extra-header" => current["extra"] = json!(true),
            "missing-header" => {
                current.as_object_mut().unwrap().remove("timeout");
            }
            "changed-callback" => current["hooks"][0]["command"] = json!("acme-changed"),
            "duplicate-callback" => {
                let duplicate = current["hooks"][0].clone();
                current["hooks"].as_array_mut().unwrap().push(duplicate);
            }
            "extra-shield" => current["hooks"]
                .as_array_mut()
                .unwrap()
                .push(group("another __internal native-hook")["hooks"][0].clone()),
            "second-group" => actual["hooks"]["PreToolUse"]
                .as_array_mut()
                .unwrap()
                .push(previous["PreToolUse"].clone()),
            _ => unreachable!(),
        }
        let actual = actual.to_string();
        assert!(capture(Some(&actual), &previous).is_err(), "{mutation}");
        assert!(
            merge(Some(&actual), &previous, &previous).is_err(),
            "{mutation}"
        );
        let mut diagnostics = Vec::new();
        diagnose(Some(&actual), &previous, &mut diagnostics).unwrap();
        assert!(!diagnostics.is_empty(), "{mutation}");
    }
}
