//! Bounded native heap capture and private DevTools snapshot inspection.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
mod import;
pub(crate) use import::{read as read_import, validate as validate_import, validate_snapshot};
pub(crate) const MAX_BYTES: usize = 32 * 1024 * 1024;
pub(crate) fn default_bytes() -> usize {
    16 * 1024 * 1024
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteRange {
    #[serde(default)]
    pub min: Option<u64>,
    #[serde(default)]
    pub max: Option<u64>,
}
impl ByteRange {
    fn validate(self) -> Result<(), &'static str> {
        if self.min.is_some_and(|value| value > 9007199254740991)
            || self.max.is_some_and(|value| value > 9007199254740991)
            || self.min.zip(self.max).is_some_and(|(min, max)| min > max)
        {
            Err("HEAP_QUERY_BOUNDS")
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    RetainedSize,
    SelfSize,
    Id,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EdgeSort {
    #[default]
    RetainedSize,
    SelfSize,
    Name,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Retention {
    ObjectsRetainedByDetachedDomNodes,
    ObjectsRetainedByConsole,
    ObjectsRetainedByEventHandlers,
    ObjectsRetainedByContexts,
    SharedNativeContext,
    NoNativeContext,
    AttributedToSpecificNativeContext,
}
pub(crate) fn default_name_length() -> usize {
    100
}
pub(crate) fn name_length(value: usize) -> Result<(), &'static str> {
    if (1..=1024).contains(&value) {
        Ok(())
    } else {
        Err("HEAP_QUERY_BOUNDS")
    }
}
fn exclude_primitives() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    Summary {},
    Objects {
        #[serde(default, rename = "className")]
        class_name: Option<String>,
        #[serde(default, rename = "propertyName")]
        property_name: Option<String>,
        #[serde(default, rename = "nodeType")]
        node_type: Option<String>,
        #[serde(default, rename = "retainedSize")]
        retained_size: Option<ByteRange>,
        #[serde(default, rename = "selfSize")]
        self_size: Option<ByteRange>,
        #[serde(default, rename = "sortBy")]
        sort_by: Option<Sort>,
        #[serde(default, rename = "isDetached")]
        is_detached: Option<bool>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
    Object {
        #[serde(rename = "nodeId")]
        node_id: u64,
    },
    Edges {
        #[serde(rename = "nodeId")]
        node_id: u64,
        #[serde(default, rename = "sortBy")]
        sort_by: EdgeSort,
        #[serde(default, rename = "minRetainedSize")]
        min_retained_size: Option<u64>,
        #[serde(default = "exclude_primitives", rename = "excludePrimitives")]
        exclude_primitives: bool,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
    Retainers {
        #[serde(rename = "nodeId")]
        node_id: u64,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
    RetainingPaths {
        #[serde(rename = "nodeId")]
        node_id: u64,
        #[serde(default, rename = "maxDepth")]
        max_depth: Option<usize>,
        #[serde(default, rename = "maxNodes")]
        max_nodes: Option<usize>,
        #[serde(default, rename = "maxSiblings")]
        max_siblings: Option<usize>,
    },
    Dominators {
        #[serde(rename = "nodeId")]
        node_id: u64,
    },
    Classes {
        #[serde(default, rename = "filterName")]
        filter_name: Option<Retention>,
        #[serde(default, rename = "objectId")]
        object_id: Option<u64>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
    ClassNodes {
        #[serde(default, rename = "filterName")]
        filter_name: Option<Retention>,
        #[serde(default, rename = "objectId")]
        object_id: Option<u64>,
        #[serde(rename = "classId")]
        class_id: u64,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
    DuplicateStrings {
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
    Contexts {
        #[serde(default, rename = "retainedSize")]
        retained_size: Option<ByteRange>,
        #[serde(default, rename = "scopeInfoNodeId")]
        scope_info_node_id: Option<u64>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
}
fn limit() -> usize {
    default_limit()
}
pub(crate) fn default_limit() -> usize {
    50
}
impl Query {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Self::Edges {
            min_retained_size: Some(size),
            ..
        } = self
            && *size > 9007199254740991
        {
            return Err("HEAP_QUERY_BOUNDS");
        }
        if let Self::Classes {
            filter_name,
            object_id,
            ..
        }
        | Self::ClassNodes {
            filter_name,
            object_id,
            ..
        } = self
            && (object_id.is_some_and(|id| id > 9007199254740991)
                || (*filter_name == Some(Retention::AttributedToSpecificNativeContext)
                    && object_id.is_none()))
        {
            return Err("HEAP_QUERY_BOUNDS");
        }
        if let Self::Contexts {
            retained_size,
            scope_info_node_id,
            ..
        } = self
        {
            if scope_info_node_id.is_some_and(|id| id > 9007199254740991) {
                return Err("HEAP_QUERY_BOUNDS");
            }
            if let Some(range) = retained_size {
                range.validate()?;
            }
        }
        match self {
            Self::ClassNodes { class_id, .. } if *class_id == 0 || *class_id > 100000 => {
                return Err("HEAP_QUERY_BOUNDS");
            }
            Self::Objects {
                class_name,
                property_name,
                node_type,
                ..
            } if [class_name, property_name, node_type]
                .iter()
                .any(|name| name.as_ref().is_some_and(|name| name.len() > 1024)) =>
            {
                return Err("HEAP_QUERY_BOUNDS");
            }
            Self::Object { node_id }
            | Self::Edges { node_id, .. }
            | Self::Retainers { node_id, .. }
            | Self::RetainingPaths { node_id, .. }
            | Self::Dominators { node_id }
                if *node_id > 9007199254740991 =>
            {
                return Err("HEAP_QUERY_BOUNDS");
            }
            _ => {}
        }
        if let Self::Objects {
            retained_size,
            self_size,
            ..
        } = self
        {
            for range in [retained_size, self_size].into_iter().flatten() {
                range.validate()?;
            }
        }
        if let Self::RetainingPaths {
            max_depth,
            max_nodes,
            max_siblings,
            ..
        } = self
            && (max_depth.is_some_and(|value| !(1..=32).contains(&value))
                || max_nodes.is_some_and(|value| !(1..=5000).contains(&value))
                || max_siblings.is_some_and(|value| !(1..=100).contains(&value)))
        {
            return Err("HEAP_QUERY_BOUNDS");
        }
        match self {
            Self::Objects { offset, limit, .. }
            | Self::Edges { offset, limit, .. }
            | Self::Retainers { offset, limit, .. }
            | Self::Classes { offset, limit, .. }
            | Self::ClassNodes { offset, limit, .. }
            | Self::Contexts { offset, limit, .. }
            | Self::DuplicateStrings { offset, limit } => page(*offset, *limit),
            _ => Ok(()),
        }
    }
}
pub(crate) fn page(offset: usize, limit: usize) -> Result<(), &'static str> {
    if offset > 100000 || !(1..=100).contains(&limit) {
        Err("HEAP_QUERY_BOUNDS")
    } else {
        Ok(())
    }
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let node = json!({"type":"integer","minimum":0,"maximum":9007199254740991u64});
    let offset = json!({"type":"integer","minimum":0,"maximum":100000,"default":0});
    let limit = json!({"type":"integer","minimum":1,"maximum":100,"default":50});
    let mut queries = Vec::new();
    for kind in [
        "summary",
        "objects",
        "object",
        "edges",
        "retainers",
        "retaining_paths",
        "dominators",
        "classes",
        "class_nodes",
        "duplicate_strings",
        "contexts",
    ] {
        let mut properties = json!({"kind":{"const":kind}});
        let mut required = vec!["kind"];
        if matches!(
            kind,
            "object" | "edges" | "retainers" | "retaining_paths" | "dominators"
        ) {
            properties["nodeId"] = node.clone();
            required.push("nodeId");
        }
        if matches!(
            kind,
            "objects"
                | "edges"
                | "retainers"
                | "classes"
                | "class_nodes"
                | "duplicate_strings"
                | "contexts"
        ) {
            properties["offset"] = offset.clone();
            properties["limit"] = limit.clone();
        }
        if kind == "class_nodes" {
            properties["classId"] = json!({"type":"integer","minimum":1,"maximum":100000});
            required.push("classId");
        }
        if kind == "edges" {
            properties["sortBy"] =
                json!({"enum":["retainedSize","selfSize","name"],"default":"retainedSize"});
            properties["minRetainedSize"] =
                json!({"type":["integer","null"],"minimum":0,"maximum":9007199254740991u64});
            properties["excludePrimitives"] = json!({"type":"boolean","default":true});
        }
        if matches!(kind, "classes" | "class_nodes") {
            properties["filterName"] = json!({"enum":["objectsRetainedByDetachedDomNodes","objectsRetainedByConsole","objectsRetainedByEventHandlers","objectsRetainedByContexts","sharedNativeContext","noNativeContext","attributedToSpecificNativeContext",null]});
            properties["objectId"] =
                json!({"type":["integer","null"],"minimum":0,"maximum":9007199254740991u64});
        }
        if kind == "objects" {
            properties["className"] = json!({"type":["string","null"],"maxLength":1024});
            properties["isDetached"] = json!({"type":["boolean","null"]});
            for name in ["propertyName", "nodeType"] {
                properties[name] = json!({"type":["string","null"],"maxLength":1024});
            }
            let bound =
                json!({"type":["integer","null"],"minimum":0,"maximum":9007199254740991u64});
            let range = json!({"anyOf":[{"type":"object","properties":{"min":bound,"max":bound},"additionalProperties":false},{"type":"null"}]});
            properties["retainedSize"] = range.clone();
            properties["selfSize"] = range;
            properties["sortBy"] = json!({"enum":["retainedSize","selfSize","id",null]});
        }
        if kind == "contexts" {
            properties["scopeInfoNodeId"] =
                json!({"type":["integer","null"],"minimum":0,"maximum":9007199254740991u64});
            let bound =
                json!({"type":["integer","null"],"minimum":0,"maximum":9007199254740991u64});
            properties["retainedSize"] = json!({"anyOf":[{"type":"object","properties":{"min":bound,"max":bound},"additionalProperties":false},{"type":"null"}]});
        }
        if kind == "retaining_paths" {
            properties["maxDepth"] = json!({"type":["integer","null"],"minimum":1,"maximum":32});
            properties["maxNodes"] = json!({"type":["integer","null"],"minimum":1,"maximum":5000});
            properties["maxSiblings"] =
                json!({"type":["integer","null"],"minimum":1,"maximum":100});
        }
        queries.push(json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}));
    }
    let id = json!({"type":"string","format":"uuid"});
    vec![
        json!({"type":"object","properties":{"operation":{"const":"heap_import"},"developer":reference,"path":{"type":"string","minLength":1,"maxLength":4096,"description":"Absolute path to an owner-private regular UTF-8 heap snapshot file in a private directory"},"maxBytes":{"type":"integer","minimum":1024,"maximum":MAX_BYTES,"default":default_bytes()},"identityGroup":{"type":["string","null"],"format":"uuid","description":"Optional caller assertion that imported snapshots share object identity; never treated as a verified captured epoch"}},"required":["operation","developer","path"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"heap_stop"},"page":reference},"required":["operation","page"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"heap_capture"},"page":reference,"maxBytes":{"type":"integer","minimum":1024,"maximum":MAX_BYTES,"default":default_bytes()}},"required":["operation","page"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"heap_load"},"developer":reference,"evidenceId":id},"required":["operation","developer","evidenceId"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"heap_query"},"developer":reference,"snapshotId":id,"query":{"oneOf":queries},"maxNameLength":{"type":"integer","minimum":1,"maximum":1024,"default":default_name_length()}},"required":["operation","developer","snapshotId","query"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"heap_compare"},"developer":reference,"snapshotId":id,"baseSnapshotId":id,"classIndex":{"type":["integer","null"],"minimum":0,"maximum":100000},"maxNameLength":{"type":"integer","minimum":1,"maximum":1024,"default":default_name_length()},"offset":offset,"limit":limit},"required":["operation","developer","snapshotId","baseSnapshotId"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"heap_release"},"developer":reference,"snapshotId":id},"required":["operation","developer","snapshotId"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_ranges_and_search_limits_are_validated_before_model_work() {
        for value in [
            json!({"kind":"classes","filterName":"attributedToSpecificNativeContext"}),
            json!({"kind":"class_nodes","classId":1,"objectId":9007199254740992u64}),
            json!({"kind":"class_nodes","classId":0}),
            json!({"kind":"class_nodes","classId":100001}),
            json!({"kind":"contexts","retainedSize":{"min":2,"max":1}}),
            json!({"kind":"contexts","scopeInfoNodeId":9007199254740992u64}),
            json!({"kind":"contexts","limit":101}),
            json!({"kind":"objects","retainedSize":{"min":10,"max":9}}),
            json!({"kind":"objects","selfSize":{"max":9007199254740992u64}}),
            json!({"kind":"retaining_paths","nodeId":1,"maxDepth":0}),
            json!({"kind":"retaining_paths","nodeId":1,"maxNodes":5001}),
            json!({"kind":"edges","nodeId":1,"minRetainedSize":9007199254740992u64}),
            json!({"kind":"retaining_paths","nodeId":1,"maxSiblings":101}),
        ] {
            let query: Query = serde_json::from_value(value).unwrap();
            assert_eq!(query.validate(), Err("HEAP_QUERY_BOUNDS"));
        }
        let query:Query=serde_json::from_value(json!({"kind":"objects","propertyName":"Acme","nodeType":"object","selfSize":{"min":0,"max":100},"sortBy":"id"})).unwrap();
        query.validate().unwrap();
        assert_eq!(name_length(0), Err("HEAP_QUERY_BOUNDS"));
        assert_eq!(name_length(1025), Err("HEAP_QUERY_BOUNDS"));
        assert!(name_length(2).is_ok());
    }
}
