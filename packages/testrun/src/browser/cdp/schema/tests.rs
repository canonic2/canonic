//! Pinned CDP validation behavior without browser or host I/O.

use super::*;
use serde_json::json;

#[test]
fn required_optional_enum_and_referenced_values_follow_the_pinned_protocol() {
    assert!(command("Browser.getVersion", &json!({})).is_ok());
    assert!(
        command(
            "Target.attachToTarget",
            &json!({"targetId":"acme","flatten":true})
        )
        .is_ok()
    );
    assert!(matches!(
        command("Target.attachToTarget", &json!({"flatten":true})),
        Err(Error::Shape)
    ));
    assert!(matches!(
        command(
            "Target.attachToTarget",
            &json!({"targetId":"acme","flatten":null})
        ),
        Err(Error::Shape)
    ));
    assert!(matches!(
        command(
            "Runtime.evaluate",
            &json!({"expression":"1","hostPath":"private"})
        ),
        Err(Error::Shape)
    ));
    assert!(matches!(
        command(
            "Emulation.setEmulatedMedia",
            &json!({"features":[{"name":"prefers-color-scheme","value":true}]})
        ),
        Err(Error::Shape)
    ));
    assert!(matches!(
        result("Runtime.evaluate", &json!({})),
        Err(Error::Shape)
    ));
    assert!(
        result(
            "Runtime.evaluate",
            &json!({"result":{"type":"number","value":1}})
        )
        .is_ok()
    );
    assert!(matches!(
        result(
            "Runtime.evaluate",
            &json!({"result":{"type":"acme-invalid"}})
        ),
        Err(Error::Shape)
    ));
}

#[test]
fn arbitrary_application_values_still_have_structural_bounds() {
    let mut value = json!(null);
    for _ in 0..65 {
        value = json!([value]);
    }
    assert!(matches!(
        command(
            "Runtime.callFunctionOn",
            &json!({"functionDeclaration":"x => x","arguments":[{"value":value}]})
        ),
        Err(Error::Bounds)
    ));
    assert!(event("Acme.optionalMetadata", &json!({"extension":true})).is_ok());
    assert!(matches!(
        event("Target.unmappedLifecycle", &json!({})),
        Err(Error::Method)
    ));
    assert!(matches!(
        event(
            "Runtime.executionContextDestroyed",
            &json!({"executionContextId":1})
        ),
        Err(Error::Shape)
    ));
}
