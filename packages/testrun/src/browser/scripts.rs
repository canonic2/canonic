//! Scoped init-script registrations; source text is never retained in status.
use super::contract::Reference;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) const MAX_SOURCE: usize = 65536;
pub(crate) fn validate(source: &str) -> Result<(), &'static str> {
    if source.is_empty() || source.len() > MAX_SOURCE {
        Err("INIT_SCRIPT_BOUNDS")
    } else {
        Ok(())
    }
}

struct Script<T> {
    page: Reference,
    native: Option<T>,
    removing: bool,
}
pub(crate) struct Scripts<T> {
    items: BTreeMap<Uuid, Script<T>>,
}
impl<T> Default for Scripts<T> {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
        }
    }
}
impl<T: Clone> Scripts<T> {
    pub(crate) fn begin(&mut self, page: Reference) -> Result<Uuid, &'static str> {
        if self.items.len() >= 64 {
            return Err("INIT_SCRIPT_CAPACITY");
        }
        let id = Uuid::new_v4();
        self.items.insert(
            id,
            Script {
                page,
                native: None,
                removing: false,
            },
        );
        Ok(id)
    }
    pub(crate) fn complete(&mut self, id: Uuid, native: T) -> Result<(), &'static str> {
        self.items.get_mut(&id).ok_or("UNKNOWN_INIT_SCRIPT")?.native = Some(native);
        Ok(())
    }
    pub(crate) fn native(&self, page: Reference, id: Uuid) -> Result<T, &'static str> {
        let script = self
            .items
            .get(&id)
            .filter(|s| s.page == page)
            .ok_or("UNKNOWN_INIT_SCRIPT")?;
        script.native.clone().ok_or("INIT_SCRIPT_PENDING")
    }
    pub(crate) fn forget(&mut self, id: Uuid) {
        self.items.remove(&id);
    }
    pub(crate) fn begin_remove(&mut self, scope: Reference, id: Uuid) -> Result<(), &'static str> {
        let item = self
            .items
            .get_mut(&id)
            .filter(|item| item.page == scope)
            .ok_or("UNKNOWN_INIT_SCRIPT")?;
        if item.removing {
            return Err("INIT_SCRIPT_CLEANUP_PENDING");
        }
        if item.native.is_none() {
            return Err("INIT_SCRIPT_PENDING");
        }
        item.removing = true;
        Ok(())
    }
    pub(crate) fn retry_remove(&mut self, id: Uuid) {
        if let Some(item) = self.items.get_mut(&id) {
            item.removing = false;
        }
    }
    pub(crate) fn retain(&mut self, mut live: impl FnMut(Reference) -> bool) {
        self.items.retain(|_, script| live(script.page));
    }
    pub(crate) fn list(&self, page: Reference) -> Value {
        json!({"page":page,"scripts":self.items.iter().filter(|(_,s)|s.page==page).map(|(id,s)|json!({"scriptId":id,"state":if s.removing{"removing"}else if s.native.is_some(){"registered"}else{"pending"}})).collect::<Vec<_>>(),"currentDocumentExecuted":false,"executionOrder":"unspecified"})
    }
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"page_init_script_add"},"page":reference,"source":{"type":"string","minLength":1,"maxLength":MAX_SOURCE}},"required":["operation","page","source"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"page_init_script_list"},"page":reference},"required":["operation","page"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"page_init_script_remove"},"page":reference,"scriptId":{"type":"string","format":"uuid"}},"required":["operation","page","scriptId"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"context_init_script_add"},"context":reference,"source":{"type":"string","minLength":1,"maxLength":MAX_SOURCE}},"required":["operation","context","source"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"context_init_script_list"},"context":reference},"required":["operation","context"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"context_init_script_remove"},"context":reference,"scriptId":{"type":"string","format":"uuid"}},"required":["operation","context","scriptId"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    fn pending_scripts_keep_capacity_and_never_cross_page_authority() {
        let owner = page();
        let other = page();
        let mut scripts = Scripts::<String>::default();
        let id = scripts.begin(owner).unwrap();
        assert_eq!(scripts.native(owner, id), Err("INIT_SCRIPT_PENDING"));
        assert_eq!(scripts.native(other, id), Err("UNKNOWN_INIT_SCRIPT"));
        scripts.complete(id, "native-private-id".into()).unwrap();
        scripts.begin_remove(owner, id).unwrap();
        assert_eq!(
            scripts.begin_remove(owner, id),
            Err("INIT_SCRIPT_CLEANUP_PENDING")
        );
        assert_eq!(scripts.list(owner)["scripts"][0]["state"], "removing");
        scripts.retry_remove(id);
        assert_eq!(scripts.list(owner)["scripts"][0]["state"], "registered");
        assert!(
            !scripts
                .list(owner)
                .to_string()
                .contains("native-private-id")
        );
        for _ in 1..64 {
            scripts.begin(owner).unwrap();
        }
        assert_eq!(scripts.begin(owner), Err("INIT_SCRIPT_CAPACITY"));
        scripts.forget(id);
        assert!(scripts.begin(owner).is_ok());
        scripts.retain(|p| p != owner);
        assert!(
            scripts.list(owner)["scripts"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
