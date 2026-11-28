//! Pure uniqueness, reference freshness and condition rules over bounded observations.
use crate::{
    error::{Error, Result},
    ui::Query,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// An element is either a current query or a snapshot-qualified element id.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Target {
    Query {
        query: Query,
    },
    Reference {
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
        #[serde(rename = "elementId")]
        element_ref: String,
    },
}
pub(crate) fn validate_pointer(args: &Value) -> Result<()> {
    if args.get("point").is_some() == args.get("element").is_some() {
        return Err(Error::invalid("Supply exactly one of point or element"));
    }
    if let Some(target) = args.get("element") {
        parse_target(target)?;
    }
    Ok(())
}
pub(crate) fn parse_target(value: &Value) -> Result<Target> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::invalid("element must be an object"))?;
    let fields = if object.contains_key("query") {
        &["query"][..]
    } else {
        &["snapshotId", "elementId"][..]
    };
    if object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(Error::invalid("Supply query, or snapshotId and elementId"));
    }
    let target: Target =
        serde_json::from_value(value.clone()).map_err(|_| Error::invalid("Invalid element"))?;
    if let Target::Query { query } = &target {
        query.validate()?;
    }
    Ok(target)
}

/// Partial scope is not traversal truncation: uniqueness can be proved inside
/// an untruncated frontmost-app scope, but absence on a partial tree stays unknown.
pub(crate) fn unique(query: &Query, snapshot: &Value) -> Result<Value> {
    query.validate()?;
    let found = query.find(&snapshot["root"], 8)?;
    let matches = found["matches"].as_array().unwrap();
    let code = if matches.len() > 1 {
        "AMBIGUOUS_ELEMENT"
    } else if matches.is_empty() {
        if snapshot["quality"] == "complete"
            && snapshot["truncated"] != true
            && found["truncated"] != true
        {
            "ELEMENT_NOT_FOUND"
        } else {
            "INCONCLUSIVE"
        }
    } else if snapshot["truncated"] == true || found["truncated"] == true {
        "UNIQUENESS_UNPROVEN"
    } else {
        return Ok(matches[0].clone());
    };
    Err(Error::new(code, "Narrow the query within the exposed scope or read a fresh reference")
        .details(json!({"choices":matches,"quality":snapshot["quality"],"coverage":snapshot["coverage"],"truncated":snapshot["truncated"],"queryTruncated":found["truncated"]})))
}

fn node<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    if root["elementId"] == reference {
        return Some(root);
    }
    root["children"]
        .as_array()?
        .iter()
        .find_map(|child| node(child, reference))
}

/// Positional refs never become durable IDs. Conservatively reject any tree or
/// frontmost process change, even when a nearby node looks identical.
pub(crate) fn revalidate(old: &Value, fresh: &Value, reference: &str) -> Result<Value> {
    let previous = node(&old["root"], reference).ok_or_else(|| {
        Error::new(
            "ELEMENT_NOT_FOUND",
            "Reference was not present in its snapshot",
        )
    })?;
    let current = node(&fresh["root"], reference).ok_or_else(|| {
        Error::new(
            if fresh["quality"] == "complete" {
                "ELEMENT_NOT_FOUND"
            } else {
                "STALE_ELEMENT"
            },
            "Reference is missing from the fresh exposed tree",
        )
    })?;
    if old["root"] != fresh["root"]
        || old["pid"] != fresh["pid"]
        || old["foregroundApp"] != fresh["foregroundApp"]
        || previous != current
    {
        return Err(Error::new(
            "STALE_ELEMENT",
            "The exposed tree or foreground process changed; read a fresh target",
        ));
    }
    let mut current = current.clone();
    current.as_object_mut().unwrap().remove("children");
    Ok(current)
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Condition {
    Exists {
        query: Query,
    },
    Gone {
        query: Query,
    },
    Value {
        query: Query,
        value: crate::ui::TextMatch,
    },
    State {
        query: Query,
        state: String,
        equals: bool,
    },
    Foreground {
        #[serde(rename = "appId")]
        bundle_id: String,
    },
}

pub(crate) fn parse_wait(platform: &str, args: &Value) -> Result<(Vec<Condition>, u64)> {
    let conditions = Vec::<Condition>::deserialize(&args["conditions"])
        .map_err(|_| Error::invalid("Arguments do not match the tool schema"))?;
    if conditions.is_empty() || conditions.len() > 8 {
        return Err(Error::invalid("Supply 1..8 conditions"));
    }
    for condition in &conditions {
        condition.validate()?;
        if platform == "simulator"
            && matches!(condition, Condition::State { state, .. } if !["enabled", "focused"].contains(&state.as_str()))
        {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Simulator state predicates support enabled and focused only",
            ));
        }
    }
    if platform == "simulator"
        && args["allowPartial"] != true
        && conditions
            .iter()
            .any(|condition| !matches!(condition, Condition::Foreground { .. }))
    {
        return Err(Error::new(
            "PARTIAL_UI_TREE",
            "Explicit allowPartial is required for simulator UI conditions",
        ));
    }
    let ms = args
        .get("timeoutMs")
        .map_or(Some(10000), Value::as_u64)
        .filter(|ms| *ms <= 30000)
        .ok_or_else(|| Error::invalid("timeoutMs must be 0..30000"))?;
    Ok((conditions, ms))
}

impl Condition {
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Exists { query } | Self::Gone { query } => query.validate(),
            Self::Value { query, value } => {
                query.validate()?;
                value.matches("").map(|_| ())
            }
            Self::State { query, state, .. } => {
                query.validate()?;
                if ![
                    "enabled",
                    "focused",
                    "selected",
                    "checked",
                    "clickable",
                    "scrollable",
                ]
                .contains(&state.as_str())
                {
                    return Err(Error::invalid("Unsupported state predicate"));
                }
                Ok(())
            }
            Self::Foreground { bundle_id } => {
                if bundle_id.is_empty()
                    || bundle_id.len() > 256
                    || !bundle_id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                {
                    return Err(Error::invalid("Invalid foreground app identifier"));
                }
                Ok(())
            }
        }
    }
    pub(crate) fn evaluate(&self, snapshot: &Value) -> Result<(bool, Value)> {
        let (Self::Exists { query } | Self::Gone { query }) = self else {
            let (query, value, state) = match self {
                Self::Value { query, value } => (query, Some(value), None),
                Self::State {
                    query,
                    state,
                    equals,
                } => (query, None, Some((state, equals))),
                Self::Foreground { .. } => {
                    return Err(Error::invalid("Foreground needs platform evidence"));
                }
                _ => unreachable!(),
            };
            return match unique(query, snapshot) {
                Ok(element) => {
                    let satisfied = if let Some(value) = value {
                        // A cut value satisfies only when its kept start proves it.
                        element["secure"] != true
                            && element["value"]
                                .as_str()
                                .map(|text| {
                                    value.matches_observed(text, element["valueTruncated"] == true)
                                })
                                .transpose()?
                                .flatten()
                                .unwrap_or(false)
                    } else {
                        let (state, equals) = state.unwrap();
                        element["states"][state].as_bool() == Some(*equals)
                    };
                    Ok((
                        satisfied,
                        json!({"element":element,"quality":snapshot["quality"],"coverage":snapshot["coverage"]}),
                    ))
                }
                Err(error) => Ok((false, json!({"resolution":error}))),
            };
        };
        let found = query.find(&snapshot["root"], 8)?;
        let exists = !found["matches"].as_array().unwrap().is_empty();
        let complete = snapshot["quality"] == "complete"
            && snapshot["truncated"] != true
            && found["truncated"] != true;
        Ok((
            if matches!(self, Self::Gone { .. }) {
                !exists && complete
            } else {
                exists
            },
            json!({"matches":found["matches"],"quality":snapshot["quality"],"coverage":snapshot["coverage"],"truncated":snapshot["truncated"],"queryTruncated":found["truncated"],"absenceConclusive":complete}),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query() -> Query {
        serde_json::from_value(json!({"identifier":"acme.home"})).unwrap()
    }
    fn snapshot() -> Value {
        json!({"quality":"partial","coverage":"frontmost_app","truncated":false,"pid":123,"root":{"elementId":"root","children":[{"elementId":"one","identifier":"acme.home","label":"Home","states":{"enabled":true}}]}})
    }
    #[test]
    fn uniqueness_and_absence_preserve_scope_and_traversal_limits() {
        let mut tree = snapshot();
        assert_eq!(unique(&query(), &tree).unwrap()["label"], "Home");
        tree["truncated"] = json!(true);
        assert_eq!(
            unique(&query(), &tree).unwrap_err().code,
            "UNIQUENESS_UNPROVEN"
        );
        let duplicate = tree["root"]["children"][0].clone();
        tree["root"]["children"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let ambiguous = unique(&query(), &tree).unwrap_err();
        assert_eq!(ambiguous.code, "AMBIGUOUS_ELEMENT");
        assert_eq!(
            ambiguous.details.unwrap()["choices"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        tree["root"]["children"] = json!([]);
        assert_eq!(unique(&query(), &tree).unwrap_err().code, "INCONCLUSIVE");
        let gone = Condition::Gone { query: query() };
        assert!(!gone.evaluate(&tree).unwrap().0);
        tree["quality"] = json!("complete");
        assert!(!gone.evaluate(&tree).unwrap().0);
        assert_eq!(unique(&query(), &tree).unwrap_err().code, "INCONCLUSIVE");
        tree["truncated"] = json!(false);
        assert!(gone.evaluate(&tree).unwrap().0);
        assert_eq!(
            unique(&query(), &tree).unwrap_err().code,
            "ELEMENT_NOT_FOUND"
        );
    }
    #[test]
    fn reference_requires_unchanged_fresh_tree_and_process() {
        let old = snapshot();
        assert_eq!(revalidate(&old, &old, "one").unwrap()["label"], "Home");
        let mut fresh = old.clone();
        fresh["root"]["children"][0]["label"] = json!("Other");
        assert_eq!(
            revalidate(&old, &fresh, "one").unwrap_err().code,
            "STALE_ELEMENT"
        );
        fresh = old.clone();
        fresh["pid"] = json!(124);
        assert_eq!(
            revalidate(&old, &fresh, "one").unwrap_err().code,
            "STALE_ELEMENT"
        );
        fresh["root"]["children"] = json!([]);
        assert_eq!(
            revalidate(&old, &fresh, "one").unwrap_err().code,
            "STALE_ELEMENT"
        );
        fresh["quality"] = json!("complete");
        assert_eq!(
            revalidate(&old, &fresh, "one").unwrap_err().code,
            "ELEMENT_NOT_FOUND"
        );
    }
    #[test]
    fn unknown_state_is_not_false_and_secure_values_cannot_match() {
        let mut tree = snapshot();
        let state = Condition::State {
            query: query(),
            state: "selected".into(),
            equals: false,
        };
        assert!(!state.evaluate(&tree).unwrap().0);
        tree["root"]["children"][0]["secure"] = json!(true);
        tree["root"]["children"][0]["value"] = json!("secret");
        crate::ui::normalize_roles(&mut tree["root"]);
        assert!(!tree.to_string().contains("secret"));
        let value: Query =
            serde_json::from_value(json!({"value":{"text":"","mode":"substring"}})).unwrap();
        let result = value.find(&tree["root"]["children"][0], 8).unwrap();
        assert_eq!(result["matches"], json!([]));
    }
}
