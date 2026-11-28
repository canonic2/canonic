//! Explicit application-to-agent bindings; arguments are disclosed only by read.
use super::contract::Reference;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Equal to the evaluation bound, so a routed 512 KiB socket message fits.
pub(crate) const MAX_ARGUMENT_BYTES: usize = super::evaluation::MAX_BYTES;
/// Owned replies carry values, never borrowed live browser objects.
pub(crate) fn value_argument(
    value: &super::evaluation::EvaluationValue,
) -> Result<Value, &'static str> {
    let mut argument = super::evaluation::argument(value, |_| Err("BINDING_HANDLE_UNSUPPORTED"))?;
    prefix_error_stacks(&mut argument["value"]);
    super::schema::encoded_len(&argument, super::evaluation::MAX_BYTES)
        .map_err(|_| "VALUE_BOUNDS")?;
    Ok(argument)
}
fn prefix_error_stacks(value: &mut Value) {
    if let Some(error) = value.get_mut("e") {
        let header = format!(
            "{}: {}",
            error["n"].as_str().expect("Encoded error name"),
            error["m"].as_str().expect("Encoded error message")
        );
        let stack = error["s"].as_str().expect("Encoded error stack");
        if !stack.starts_with(&header) {
            error["s"] = json!(format!("{header}\n{stack}"));
        }
    } else if let Some(items) = value.get_mut("a").and_then(Value::as_array_mut) {
        for item in items {
            prefix_error_stacks(item);
        }
    } else if let Some(entries) = value.get_mut("o").and_then(Value::as_array_mut) {
        for entry in entries {
            prefix_error_stacks(&mut entry["v"]);
        }
    }
}
pub(crate) const BRIDGE: &str = include_str!("reading/binding.js");
pub(crate) const SETTLE: &str = "function(raw,ticket,value,reject){const owner=globalThis[Symbol.for(raw)];return owner?owner.settle(ticket,value,reject):false;}";
pub(crate) const CLOSE: &str = "function(raw){const owner=globalThis[Symbol.for(raw)];if(owner)owner.close();return Reflect.deleteProperty(globalThis,raw);}";
pub(crate) fn validate(name: &str) -> Result<(), &'static str> {
    if name.is_empty()
        || name.len() > 128
        || name.chars().any(char::is_control)
        || name.starts_with("__testrun")
    {
        return Err("BINDING_NAME_INVALID");
    }
    Ok(())
}
#[derive(Clone)]
pub(crate) struct Registration<T> {
    pub scope: Reference,
    pub name: String,
    pub backend: Option<T>,
    pub removing: bool,
    removal_inflight: bool,
}
pub(crate) struct Registrations<T> {
    items: BTreeMap<Uuid, Registration<T>>,
}
impl<T> Default for Registrations<T> {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
        }
    }
}
impl<T: Clone> Registrations<T> {
    pub(crate) fn begin(&mut self, scope: Reference, name: String) -> Result<Uuid, &'static str> {
        validate(&name)?;
        if self.items.len() >= 64 {
            return Err("BINDING_CAPACITY");
        }
        if self.find(scope, &name).is_some() {
            return Err("BINDING_NAME_CONFLICT");
        }
        let id = Uuid::new_v4();
        self.items.insert(
            id,
            Registration {
                scope,
                name,
                backend: None,
                removing: false,
                removal_inflight: false,
            },
        );
        Ok(id)
    }
    pub(crate) fn find(&self, scope: Reference, name: &str) -> Option<Uuid> {
        self.items
            .iter()
            .find(|(_, r)| r.scope == scope && r.name == name)
            .map(|(id, _)| *id)
    }
    pub(crate) fn get(&self, scope: Reference, id: Uuid) -> Result<Registration<T>, &'static str> {
        self.items
            .get(&id)
            .filter(|r| r.scope == scope)
            .cloned()
            .ok_or("UNKNOWN_BINDING")
    }
    pub(crate) fn complete(&mut self, id: Uuid, value: T) -> Result<(), &'static str> {
        self.items.get_mut(&id).ok_or("UNKNOWN_BINDING")?.backend = Some(value);
        Ok(())
    }
    pub(crate) fn start_remove(&mut self, scope: Reference, id: Uuid) -> Result<(), &'static str> {
        let item = self
            .items
            .get_mut(&id)
            .filter(|r| r.scope == scope)
            .ok_or("UNKNOWN_BINDING")?;
        if item.removal_inflight {
            return Err("BINDING_REMOVAL_PENDING");
        }
        item.removing = true;
        item.removal_inflight = true;
        Ok(())
    }
    pub(crate) fn retry_remove(&mut self, id: Uuid) {
        if let Some(item) = self.items.get_mut(&id) {
            item.removal_inflight = false;
        }
    }
    pub(crate) fn forget(&mut self, id: Uuid) {
        self.items.remove(&id);
    }
    pub(crate) fn retain(&mut self, mut live: impl FnMut(Reference) -> bool) {
        self.items.retain(|_, r| live(r.scope));
    }
    pub(crate) fn list(&self, scope: Reference) -> Value {
        json!({"scope":scope,"bindings":self.items.iter().filter(|(_,r)|r.scope==scope).map(|(id,r)|json!({"bindingId":id,"name":r.name,"state":if r.removing{"removing"}else if r.backend.is_some(){"registered"}else{"pending"}})).collect::<Vec<_>>(),"argumentsIncluded":false})
    }
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"binding_add"},"scope":reference,"name":{"type":"string","minLength":1,"maxLength":128}},"required":["operation","scope","name"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"binding_list"},"scope":reference},"required":["operation","scope"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"binding_remove"},"scope":reference,"bindingId":{"type":"string","format":"uuid"}},"required":["operation","scope","bindingId"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"binding_read"},"callback":reference},"required":["operation","callback"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binding_typed_settlements_decode_as_explicit_value_forms() {
        for kind in ["binding_resolve_value", "binding_reject_value"] {
            let request = json!({"operation":"callback_resolve","callback":{"connectionId":Uuid::new_v4(),"id":Uuid::new_v4(),"generation":Uuid::new_v4()},"response":{"kind":kind,"value":{"kind":"bigint","value":"9007199254740993"}}});
            let decoded: super::super::contract::Request = serde_json::from_value(request.clone())
                .expect("Typed binding settlement must be an admitted explicit response form");
            assert_eq!(serde_json::to_value(decoded).unwrap(), request);
        }
    }
    #[test]
    fn binding_typed_settlements_preserve_graphs_and_refuse_borrowed_handles() {
        let cyclic=serde_json::from_value(json!({"kind":"object","id":1,"entries":[{"key":"self","value":{"kind":"reference","id":1}}]})).unwrap();
        assert_eq!(
            value_argument(&cyclic).unwrap(),
            json!({"value":{"id":1,"o":[{"k":"self","v":{"ref":1}}]},"handles":[]})
        );
        let invalid = serde_json::from_value(json!({"kind":"reference","id":1})).unwrap();
        assert_eq!(
            value_argument(&invalid).unwrap_err(),
            "INVALID_VALUE_REFERENCE"
        );
        let handle=serde_json::from_value(json!({"kind":"array","id":1,"items":[{"kind":"handle","handle":{"connectionId":Uuid::new_v4(),"id":Uuid::new_v4(),"generation":Uuid::new_v4()}}]})).unwrap();
        assert_eq!(
            value_argument(&handle).unwrap_err(),
            "BINDING_HANDLE_UNSUPPORTED"
        );
        for stack in [
            "Acme private stack",
            "TypeError: Acme failure\nAcme private stack",
        ] {
            let error=serde_json::from_value(json!({"kind":"array","id":1,"items":[{"kind":"error","name":"TypeError","message":"Acme failure","stack":stack}]})).unwrap();
            assert_eq!(
                value_argument(&error).unwrap()["value"]["a"][0]["e"]["s"],
                "TypeError: Acme failure\nAcme private stack"
            );
        }
    }
    #[test]
    fn registrations_preserve_pending_identity_and_reuse_capacity_after_scope_loss() {
        let scope = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let mut registry = Registrations::<()>::default();
        let id = registry.begin(scope, "Acme".into()).unwrap();
        assert_eq!(
            registry.begin(scope, "Acme".into()),
            Err("BINDING_NAME_CONFLICT")
        );
        assert_eq!(registry.list(scope)["bindings"][0]["state"], "pending");
        registry.complete(id, ()).unwrap();
        assert_eq!(registry.list(scope)["bindings"][0]["state"], "registered");
        registry.start_remove(scope, id).unwrap();
        assert_eq!(
            registry.start_remove(scope, id),
            Err("BINDING_REMOVAL_PENDING")
        );
        assert_eq!(registry.list(scope)["bindings"][0]["state"], "removing");
        registry.retry_remove(id);
        assert_eq!(registry.list(scope)["bindings"][0]["state"], "removing");
        registry.start_remove(scope, id).unwrap();
        registry.retain(|_| false);
        assert!(registry.get(scope, id).is_err());
        assert!(registry.begin(scope, "Acme".into()).is_ok());
        assert!(validate("Acme.name;fetch('/acme')").is_ok());
        for name in ["", "Acme\nname", "__testrun_private"] {
            assert!(validate(name).is_err());
        }
    }
}
