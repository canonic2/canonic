//! Conservative instance-aware work admission. References never execute here.
use super::{ArgumentErrorKind, MAX_DEPTH, MAX_WORK, SchemaError, bounds};
use crate::control::Interrupted;
use referencing::{Draft, Resolver, ResourceRef};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Default)]
struct Node {
    same: Vec<usize>,
    properties: Vec<(String, usize)>,
    values: Vec<usize>,
    items: Vec<usize>,
    prefix: Vec<(usize, usize)>,
    names: Vec<usize>,
    cost: usize,
    comparison: usize,
    unique: bool,
    name_tests: usize,
}
impl Node {
    fn new(schema: &Value) -> Self {
        let mut node = Self {
            cost: 1 + schema.as_object().map_or(0, serde_json::Map::len),
            unique: schema["uniqueItems"] == true,
            name_tests: schema["patternProperties"]
                .as_object()
                .map_or(0, serde_json::Map::len),
            ..Self::default()
        };
        for key in ["const", "enum"] {
            if let Some(value) = schema.get(key) {
                node.comparison = node.comparison.saturating_add(
                    serde_json::to_vec(value).map_or(MAX_WORK, |bytes| bytes.len()),
                );
            }
        }
        for key in ["required", "dependentRequired", "type"] {
            if let Some(value) = schema.get(key) {
                node.cost += serde_json::to_vec(value).map_or(MAX_WORK, |bytes| bytes.len());
            }
        }
        node
    }
}
#[derive(Debug)]
pub(super) struct Plan {
    nodes: Vec<Node>,
}

impl Plan {
    pub(super) fn new<'a>(root: &'a Value, resolver: Resolver<'a>) -> Result<Self, SchemaError> {
        let anchors = bounds::anchors(root, resolver.clone())?;
        let mut plan = Self { nodes: vec![] };
        let mut known = HashMap::new();
        let mut pending = vec![];
        let mut edges: usize = 0;
        register(root, resolver, &mut known, &mut pending, &mut plan.nodes);
        while let Some((id, schema, resolver)) = pending.pop() {
            if plan.nodes.len() > super::MAX_SCHEMA_NODES {
                return Err(SchemaError::new("exceeds the schema graph node bound"));
            }
            let mut node = Node::new(schema);
            for (path, child) in bounds::locations(schema) {
                let selected = resolver
                    .in_subresource(ResourceRef::new(child, Draft::Draft202012))
                    .map_err(|error| SchemaError::new(error.to_string()))?;
                let child = register(child, selected, &mut known, &mut pending, &mut plan.nodes);
                if let Some(name) = path.strip_prefix("/properties/") {
                    node.properties
                        .push((name.replace("~1", "/").replace("~0", "~"), child));
                } else if path.starts_with("/patternProperties/")
                    || matches!(
                        path.as_str(),
                        "/additionalProperties" | "/unevaluatedProperties"
                    )
                {
                    // All values conservatively cover every pattern/evaluated-set outcome.
                    node.values.push(child);
                } else if matches!(path.as_str(), "/items" | "/contains" | "/unevaluatedItems") {
                    node.items.push(child);
                } else if let Some(index) = path.strip_prefix("/prefixItems/") {
                    node.prefix
                        .push((index.parse().expect("Schema array index"), child));
                } else if path == "/propertyNames" {
                    node.names.push(child);
                } else if !path.starts_with("/$defs/")
                    && !path.starts_with("/definitions/")
                    && path != "/contentSchema"
                {
                    node.same.push(child);
                }
            }
            for key in ["$ref", "$dynamicRef"] {
                if let Some(reference) = schema[key].as_str() {
                    let target = resolver
                        .lookup(reference)
                        .map_err(|error| SchemaError::new(error.to_string()))?;
                    let (target, selected, _) = target.into_inner();
                    // Pointers can turn arbitrary annotation data into a schema.
                    bounds::declarations(target)?;
                    let child =
                        register(target, selected, &mut known, &mut pending, &mut plan.nodes);
                    node.same.push(child);
                    if key == "$dynamicRef" {
                        let name = reference.rsplit_once('#').map_or("", |(_, name)| name);
                        for (anchor, selected) in &anchors {
                            if anchor["$dynamicAnchor"].as_str() == Some(name) {
                                let child = register(
                                    anchor,
                                    selected.clone(),
                                    &mut known,
                                    &mut pending,
                                    &mut plan.nodes,
                                );
                                node.same.push(child);
                            }
                        }
                    }
                }
            }
            let local_edges = node.same.len()
                + node.properties.len()
                + node.values.len()
                + node.items.len()
                + node.prefix.len()
                + node.names.len();
            node.cost = node.cost.saturating_add(local_edges);
            edges = edges.saturating_add(local_edges);
            if edges > super::MAX_SCHEMA_NODES {
                return Err(SchemaError::new("exceeds the schema graph edge bound"));
            }
            plan.nodes[id] = node;
        }
        // A cycle must cross an instance-child edge. Also bound repeated same-value
        // reference expansion, including unused definitions, before compilation.
        let mut visits = vec![None; plan.nodes.len()];
        let mut active = vec![false; plan.nodes.len()];
        for id in 0..plan.nodes.len() {
            plan.expansion(id, 0, &mut visits, &mut active)?;
        }
        Ok(plan)
    }

    fn expansion(
        &self,
        id: usize,
        depth: usize,
        visits: &mut [Option<(usize, usize)>],
        active: &mut [bool],
    ) -> Result<(usize, usize), SchemaError> {
        if active[id] || depth > MAX_DEPTH {
            return Err(SchemaError::new(
                "contains a non-descending reference cycle or excessive schema depth",
            ));
        }
        if let Some((count, height)) = visits[id] {
            if depth.saturating_add(height) > MAX_DEPTH {
                return Err(SchemaError::new(
                    "exceeds the same-value schema depth bound",
                ));
            }
            return Ok((count, height));
        }
        active[id] = true;
        let mut count: usize = 1;
        let mut height = 0;
        for child in &self.nodes[id].same {
            let (child_count, child_height) = self.expansion(*child, depth + 1, visits, active)?;
            count = count.saturating_add(child_count);
            height = height.max(child_height + 1);
            if count > 4096 {
                return Err(SchemaError::new(
                    "exceeds the same-value reference expansion bound",
                ));
            }
        }
        active[id] = false;
        visits[id] = Some((count, height));
        Ok((count, height))
    }

    pub(super) fn check(
        &self,
        value: &Value,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<(), ArgumentErrorKind> {
        let mut pending = vec![(0, Instance::Value(value), 0)];
        let mut work: usize = 0;
        while let Some((id, value, depth)) = pending.pop() {
            check().map_err(|_| ArgumentErrorKind::Interrupted)?;
            if depth > 256 {
                return Err(ArgumentErrorKind::Limit);
            }
            let node = &self.nodes[id];
            work = work
                .saturating_add(node.cost)
                .saturating_add(value.immediate());
            if node.comparison > 0 || node.unique {
                let bytes = value.bytes(check)?;
                work = work.saturating_add(bytes.saturating_mul(node.comparison));
                if node.unique
                    && let Instance::Value(Value::Array(items)) = value
                {
                    work = work.saturating_add(
                        bytes.saturating_mul(items.len().saturating_mul(items.len())),
                    );
                }
            }
            if work > MAX_WORK {
                return Err(ArgumentErrorKind::Limit);
            }
            // Do not deduplicate: repeated applicators really can repeat engine work.
            for child in &node.same {
                enqueue(&mut pending, *child, value, depth + 1)?;
            }
            if let Instance::Value(Value::Object(properties)) = value {
                work = work.saturating_add(node.name_tests.saturating_mul(value.immediate()));
                if work > MAX_WORK {
                    return Err(ArgumentErrorKind::Limit);
                }
                for (name, child) in &node.properties {
                    if let Some(value) = properties.get(name) {
                        enqueue(&mut pending, *child, Instance::Value(value), depth + 1)?;
                    }
                }
                for child in &node.values {
                    for value in properties.values() {
                        enqueue(&mut pending, *child, Instance::Value(value), depth + 1)?;
                    }
                }
                for child in &node.names {
                    for name in properties.keys() {
                        enqueue(&mut pending, *child, Instance::Name(name), depth + 1)?;
                    }
                }
            }
            if let Instance::Value(Value::Array(items)) = value {
                for child in &node.items {
                    for value in items {
                        enqueue(&mut pending, *child, Instance::Value(value), depth + 1)?;
                    }
                }
                for (index, child) in &node.prefix {
                    if let Some(value) = items.get(*index) {
                        enqueue(&mut pending, *child, Instance::Value(value), depth + 1)?;
                    }
                }
            }
            // Bound the admission walk's own queue as well as executed visits.
            if pending.len() > super::MAX_SCHEMA_NODES {
                return Err(ArgumentErrorKind::Limit);
            }
        }
        Ok(())
    }
}

fn enqueue<'a>(
    pending: &mut Vec<(usize, Instance<'a>, usize)>,
    id: usize,
    value: Instance<'a>,
    depth: usize,
) -> Result<(), ArgumentErrorKind> {
    if pending.len() >= super::MAX_SCHEMA_NODES {
        return Err(ArgumentErrorKind::Limit);
    }
    pending.push((id, value, depth));
    Ok(())
}

fn register<'a>(
    schema: &'a Value,
    resolver: Resolver<'a>,
    known: &mut HashMap<usize, usize>,
    pending: &mut Vec<(usize, &'a Value, Resolver<'a>)>,
    nodes: &mut Vec<Node>,
) -> usize {
    let address = std::ptr::from_ref(schema) as usize;
    if let Some(id) = known.get(&address) {
        return *id;
    }
    let id = nodes.len();
    known.insert(address, id);
    nodes.push(Node::default());
    pending.push((id, schema, resolver));
    id
}

#[derive(Clone, Copy)]
enum Instance<'a> {
    Value(&'a Value),
    Name(&'a str),
}
impl Instance<'_> {
    fn immediate(self) -> usize {
        match self {
            Self::Name(name) => name.len() + 1,
            Self::Value(Value::String(name)) => name.len() + 1,
            Self::Value(Value::Array(items)) => items.len() + 1,
            Self::Value(Value::Object(properties)) => {
                properties.keys().map(|key| key.len() + 1).sum::<usize>() + 1
            }
            Self::Value(_) => 1,
        }
    }
    fn bytes(
        self,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<usize, ArgumentErrorKind> {
        match self {
            Self::Name(name) => Ok(name.len() + 2),
            // The raw instance was already admitted. Measure with the same encoder,
            // without retaining a second serialized copy for every comparison visit.
            Self::Value(value) => crate::json::measure(value, super::MAX_VALUE_BYTES, check)
                .map_err(|error| match error {
                    crate::json::EncodeError::Interrupted => ArgumentErrorKind::Interrupted,
                    _ => ArgumentErrorKind::Limit,
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    #[test]
    fn comparison_measurement_preserves_encoded_sizes_and_interrupts_during_writes() {
        for value in [
            Value::Null,
            json!({"\"\\\n\u{0000}":[true,false,-1,1.25,u64::MAX,"é🦀"]}),
            json!("a".repeat(256 * 1024)),
        ] {
            assert_eq!(
                Instance::Value(&value).bytes(&|| Ok(())).unwrap(),
                serde_json::to_vec(&value).unwrap().len()
            );
        }
        let calls = Cell::new(0);
        let value = json!(["acme", "\n".repeat(1024)]);
        assert_eq!(
            Instance::Value(&value).bytes(&|| {
                calls.set(calls.get() + 1);
                if calls.get() > 8 {
                    Err(Interrupted)
                } else {
                    Ok(())
                }
            }),
            Err(ArgumentErrorKind::Interrupted)
        );
    }
}
