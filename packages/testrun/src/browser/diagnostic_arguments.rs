//! Opt-in console argument owners; backend handles never enter ordinary metadata.
use super::{contract::Reference, evaluation::PropertyQuery, session::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(crate) const MAX_GROUPS: usize = 32;
pub(crate) const MAX_ARGUMENTS: usize = 16;
pub(crate) const CODEC: &str = include_str!("reading/evaluation.js");
pub(crate) const READER: &str = include_str!("reading/diagnostic-arguments.js");

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Read {
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(default)]
    pub properties: Option<PropertyQuery>,
}
impl Read {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.index >= MAX_ARGUMENTS
            || self.path.len() > 16
            || self.path.iter().any(|key| key.len() > 1024)
            || self.path.iter().map(String::len).sum::<usize>() > 8192
        {
            return Err("DIAGNOSTIC_ARGUMENT_QUERY_BOUNDS");
        }
        if let Some(properties) = &self.properties {
            properties.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Live,
    Removing,
    Expired,
}
pub(crate) struct Group<T> {
    pub(crate) context: Reference,
    pub(crate) page: Option<Reference>,
    pub(crate) worker: Option<Reference>,
    pub(crate) count: usize,
    pub(crate) truncated: bool,
    pub(crate) phase: Phase,
    pub(crate) payload: T,
}
pub(crate) struct Groups<T> {
    enabled: BTreeSet<Reference>,
    pub(crate) items: BTreeMap<Uuid, Group<T>>,
}
impl<T> Default for Groups<T> {
    fn default() -> Self {
        Self {
            enabled: BTreeSet::new(),
            items: BTreeMap::new(),
        }
    }
}
impl<T> Groups<T> {
    pub(crate) fn enabled(&self, context: Reference) -> bool {
        self.enabled.contains(&context)
    }
    pub(crate) fn retain_contexts(&mut self, live: &BTreeSet<Reference>) {
        self.enabled.retain(|context| live.contains(context));
        for group in self
            .items
            .values_mut()
            .filter(|group| !live.contains(&group.context))
        {
            group.phase = Phase::Expired;
        }
    }
    pub(crate) fn configure(&mut self, context: Reference, enabled: bool) {
        if enabled {
            self.enabled.insert(context);
        } else {
            self.enabled.remove(&context);
            for group in self
                .items
                .values_mut()
                .filter(|group| group.context == context)
            {
                group.phase = Phase::Removing;
            }
        }
    }
    pub(crate) fn capacity(&self) -> Result<(), &'static str> {
        if self.items.len() >= MAX_GROUPS {
            Err("DIAGNOSTIC_ARGUMENT_CAPACITY")
        } else {
            Ok(())
        }
    }
    pub(crate) fn insert(&mut self, group: Group<T>) -> Result<Uuid, &'static str> {
        if !self.enabled(group.context) {
            return Err("DIAGNOSTIC_ARGUMENTS_DISABLED");
        }
        self.capacity()?;
        let id = Uuid::new_v4();
        self.items.insert(id, group);
        Ok(id)
    }
    pub(crate) fn get(&self, context: Reference, id: Uuid) -> Result<&Group<T>, Error> {
        let group = self
            .items
            .get(&id)
            .filter(|group| group.context == context)
            .ok_or("DIAGNOSTIC_ARGUMENTS_EXPIRED")?;
        if group.phase != Phase::Live {
            return Err("DIAGNOSTIC_ARGUMENTS_EXPIRED".into());
        }
        Ok(group)
    }
    pub(crate) fn removing(&mut self, context: Reference, id: Uuid) -> Result<(), Error> {
        let group = self
            .items
            .get_mut(&id)
            .filter(|group| group.context == context)
            .ok_or("DIAGNOSTIC_ARGUMENTS_EXPIRED")?;
        group.phase = Phase::Removing;
        Ok(())
    }
    pub(crate) fn list(&self, context: Reference) -> Value {
        json!({"context":context,"enabled":self.enabled(context),"cleanupPending":self.items.values().any(|group|group.context==context&&group.phase!=Phase::Live),"groups":self.items.iter().filter(|(_,group)|group.context==context).map(|(id,group)|json!({"groupId":id,"page":group.page,"worker":group.worker,"count":group.count,"truncated":group.truncated,"phase":group.phase})).collect::<Vec<_>>(),"maxGroups":MAX_GROUPS,"maxArgumentsPerGroup":MAX_ARGUMENTS})
    }
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties: Value, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let query = object(
        json!({"index":{"type":"integer","minimum":0,"maximum":15,"default":0},"path":{"type":"array","maxItems":16,"items":{"type":"string","maxLength":1024},"default":[]},"properties":{"anyOf":[super::evaluation::property_schema(),{"type":"null"}]}}),
        &[],
    );
    vec![
        object(
            json!({"operation":{"const":"diagnostics_arguments_set"},"context":reference,"enabled":{"type":"boolean"}}),
            &["operation", "context", "enabled"],
        ),
        object(
            json!({"operation":{"const":"diagnostics_arguments_list"},"context":reference}),
            &["operation", "context"],
        ),
        object(
            json!({"operation":{"const":"diagnostics_arguments_read"},"context":reference,"groupId":{"type":"string","format":"uuid"},"query":query}),
            &["operation", "context", "groupId"],
        ),
        object(
            json!({"operation":{"const":"diagnostics_arguments_remove"},"context":reference,"groupId":{"type":"string","format":"uuid"}}),
            &["operation", "context", "groupId"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    fn disabling_retains_cleanup_capacity_and_metadata_never_exposes_payloads() {
        let first = context();
        let second = context();
        let mut groups = Groups::default();
        groups.configure(first, true);
        groups.configure(second, true);
        let mut ids = Vec::new();
        for _ in 0..MAX_GROUPS {
            ids.push(
                groups
                    .insert(Group {
                        context: first,
                        page: None,
                        worker: None,
                        count: 1,
                        truncated: false,
                        phase: Phase::Live,
                        payload: "Acme-private-argument",
                    })
                    .unwrap(),
            );
        }
        assert!(
            !groups
                .list(first)
                .to_string()
                .contains("Acme-private-argument")
        );
        assert_eq!(
            groups.get(second, ids[0]).err().unwrap().code,
            "DIAGNOSTIC_ARGUMENTS_EXPIRED"
        );
        groups.configure(first, false);
        assert!(groups.capacity().is_err());
        assert!(!groups.enabled(first));
        assert!(groups.get(first, ids[0]).is_err());
        // The adapter retires a slot only after its cleanup witness.
        groups.items.remove(&ids[0]);
        assert!(groups.capacity().is_ok());
        assert!(groups.enabled(second));
    }
    #[test]
    fn argument_queries_bound_paths_indices_and_property_pages() {
        assert!(Read::default().validate().is_ok());
        assert!(
            Read {
                index: MAX_ARGUMENTS,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Read {
                path: vec!["é".repeat(513)],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Read {
                properties: Some(PropertyQuery {
                    offset: 10001,
                    limit: 1
                }),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
