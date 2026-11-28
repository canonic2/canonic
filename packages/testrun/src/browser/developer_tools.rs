//! Bounded page-tool registries and JSON inputs; callbacks stay in their document.
use super::{contract::Reference, session::Error};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub(crate) const DISCOVER: &str = include_str!("reading/developer-tools.js");
pub(crate) const VALIDATOR: &str = include_str!("reading/developer-validator.js");
pub(crate) fn result_expression() -> String {
    format!(
        "(value,frame)=>({})(value,frame,{}, {})",
        include_str!("reading/developer-result.js"),
        super::reading::REFERENCES,
        include_str!("reading/evaluation.js")
    )
}
pub(crate) fn result_projection(
    mut evaluated: Value,
    handle: Reference,
    page: Reference,
    frame: Reference,
) -> Result<Value, Error> {
    let result = (|| {
        let text = evaluated["value"]["value"]
            .as_str()
            .filter(|text| text.len() <= 60000)
            .ok_or("DEVELOPER_TOOL_RESULT_BOUNDS")?;
        let projected: Value =
            serde_json::from_str(text).map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))?;
        super::schema::encoded_len(&projected["value"], 32768)
            .map_err(|_| Error::from("DEVELOPER_TOOL_RESULT_BOUNDS"))?;
        let entries = projected["annotations"]
            .as_array()
            .filter(|entries| entries.len() <= 32)
            .ok_or("DEVELOPER_TOOL_PROTOCOL")?;
        if projected["complete"] != true
            || projected["projection"] != "own-enumerable-data-properties"
        {
            return Err("DEVELOPER_TOOL_PROTOCOL".into());
        }
        let mut annotations = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let locator: super::page::Locator = serde_json::from_value(entry["locator"].clone())
                .map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))?;
            if !matches!(&locator,super::page::Locator::Element { frame:owner,.. } if *owner==frame)
                || entry["index"].as_u64() != Some(index as u64)
            {
                return Err("DEVELOPER_TOOL_PROTOCOL".into());
            }
            let path: Vec<String> = serde_json::from_value(entry["path"].clone())
                .map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))?;
            if path.len() > 32
                || path.iter().any(|key| key.len() > 4096)
                || path.iter().map(String::len).sum::<usize>() > 32768
            {
                return Err("DEVELOPER_TOOL_RESULT_BOUNDS".into());
            }
            annotations.push(json!({"index":index,"path":path,"locator":locator}));
        }
        evaluated["value"] = json!(super::evaluation::result(&projected["value"])?);
        evaluated["annotations"] = json!(annotations);
        evaluated["projection"] = json!("own-enumerable-data-properties");
        evaluated["symbolsIncluded"] = json!(false);
        evaluated["complete"] = json!(true);
        evaluated["handle"] = json!(handle);
        evaluated["page"] = json!(page);
        evaluated["frame"] = json!(frame);
        evaluated["privateRead"] = json!(true);
        evaluated["originalToolReplayed"] = json!(false);
        evaluated["projectionCleanupPending"] = json!(false);
        evaluated["source"] = json!("owned-handle-dom-result-projection");
        super::schema::encoded_len(&evaluated, 65536)
            .map_err(|_| Error::from("DEVELOPER_TOOL_RESULT_BOUNDS"))?;
        Ok(evaluated)
    })();
    result.map_err(|mut error:Error|{error.partial=Some(json!({"handle":handle,"frame":frame,"dispatchCompleted":true,"potentiallyMutating":true,"originalToolReplayed":false}));error})
}
struct Registry {
    scope: Reference,
    tools: BTreeSet<String>,
    metadata: Option<Value>,
}
#[derive(Default)]
pub(crate) struct Registries {
    items: BTreeMap<Reference, Registry>,
    closed: VecDeque<Reference>,
    allocating: usize,
}
impl Registries {
    pub(crate) fn reserve(&mut self) -> Result<(), Error> {
        if self.items.len() + self.allocating >= 4 {
            Err("DEVELOPER_TOOL_CAPACITY".into())
        } else {
            self.allocating += 1;
            Ok(())
        }
    }
    pub(crate) fn allocation_failed(&mut self) {
        self.allocating = self.allocating.saturating_sub(1);
    }
    pub(crate) fn begin(&mut self, reference: Reference, scope: Reference) {
        self.allocation_failed();
        self.items.insert(
            reference,
            Registry {
                scope,
                tools: BTreeSet::new(),
                metadata: None,
            },
        );
    }
    pub(crate) fn references(&self) -> Vec<Reference> {
        self.items.keys().copied().collect()
    }
    pub(crate) fn contains(&self, reference: Reference) -> bool {
        self.items.contains_key(&reference)
    }
    pub(crate) fn released(&self, reference: Reference) -> bool {
        self.closed.contains(&reference)
    }
    pub(crate) fn forget(&mut self, reference: Reference) {
        self.items.remove(&reference);
        if self.closed.len() == 128 {
            self.closed.pop_front();
        }
        self.closed.push_back(reference);
    }
    pub(crate) fn finish(&mut self, reference: Reference, value: &Value) -> Result<Value, Error> {
        let text = value["value"]["value"]
            .as_str()
            .ok_or("DEVELOPER_TOOL_PROTOCOL")?;
        if text.len() > 131072 {
            return Err("DEVELOPER_TOOL_METADATA_BOUNDS".into());
        }
        let metadata: Value =
            serde_json::from_str(text).map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))?;
        let groups = metadata["groups"]
            .as_array()
            .filter(|groups| groups.len() <= 16)
            .ok_or("DEVELOPER_TOOL_PROTOCOL")?;
        let mut tools = BTreeSet::new();
        for group in groups {
            for tool in group["tools"].as_array().ok_or("DEVELOPER_TOOL_PROTOCOL")? {
                let id = tool["toolId"]
                    .as_str()
                    .filter(|id| id.len() <= 32 && !id.is_empty())
                    .ok_or("DEVELOPER_TOOL_PROTOCOL")?;
                if !tools.insert(id.to_owned()) || tools.len() > 32 {
                    return Err("DEVELOPER_TOOL_PROTOCOL".into());
                }
            }
        }
        let registry = self
            .items
            .get_mut(&reference)
            .ok_or("UNKNOWN_DEVELOPER_TOOLS")?;
        registry.tools = tools;
        registry.metadata = Some(metadata.clone());
        Ok(
            json!({"registry":reference,"scope":registry.scope,"metadata":metadata,"potentiallyMutating":true,"dispatchCompleted":true}),
        )
    }
    pub(crate) fn tool(&self, reference: Reference, tool: &str) -> Result<(), Error> {
        let registry = self
            .items
            .get(&reference)
            .ok_or("UNKNOWN_DEVELOPER_TOOLS")?;
        if registry.metadata.is_none() {
            return Err("DEVELOPER_TOOLS_PENDING".into());
        }
        if !registry.tools.contains(tool) {
            return Err("UNKNOWN_DEVELOPER_TOOL".into());
        }
        Ok(())
    }
    pub(crate) fn argument(
        &self,
        reference: Reference,
        tool: &str,
        parameters: Value,
        paths: Vec<Vec<String>>,
    ) -> Result<super::evaluation::EvaluationValue, Error> {
        use super::evaluation::{Entry, EvaluationValue};
        self.tool(reference, tool)?;
        validate_parameters(&parameters)?;
        if paths.len() > 32
            || paths.iter().any(|path| path.len() > 32)
            || super::schema::encoded_len(&json!(paths), 4096).is_err()
        {
            return Err("DEVELOPER_TOOL_ELEMENT_INVALID".into());
        }
        for path in &paths {
            let mut value = &parameters;
            for key in path {
                value = if let Some(items) = value.as_array() {
                    let index = key
                        .parse::<usize>()
                        .map_err(|_| Error::from("DEVELOPER_TOOL_ELEMENT_INVALID"))?;
                    if index.to_string() != *key {
                        return Err("DEVELOPER_TOOL_ELEMENT_INVALID".into());
                    }
                    items.get(index)
                } else {
                    value.as_object().and_then(|items| items.get(key))
                }
                .ok_or("DEVELOPER_TOOL_ELEMENT_INVALID")?;
            }
            if !value
                .as_object()
                .is_some_and(|object| object.len() == 1 && object.contains_key("elementHandle"))
            {
                return Err("DEVELOPER_TOOL_ELEMENT_INVALID".into());
            }
        }
        fn convert(
            value: &Value,
            path: &mut Vec<String>,
            paths: &[Vec<String>],
            next: &mut u32,
        ) -> Result<EvaluationValue, Error> {
            if paths.contains(path) {
                return Ok(EvaluationValue::Handle {
                    handle: serde_json::from_value(value["elementHandle"].clone())
                        .map_err(|_| Error::from("DEVELOPER_TOOL_ELEMENT_INVALID"))?,
                });
            }
            if !paths.iter().any(|candidate| candidate.starts_with(path)) {
                return Ok(EvaluationValue::Json {
                    value: value.clone(),
                });
            }
            let id = *next;
            *next += 1;
            if let Some(items) = value.as_array() {
                let mut converted = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    path.push(index.to_string());
                    converted.push(convert(item, path, paths, next)?);
                    path.pop();
                }
                Ok(EvaluationValue::Array {
                    id,
                    items: converted,
                })
            } else {
                let mut entries = Vec::new();
                for (key, item) in value.as_object().ok_or("DEVELOPER_TOOL_ELEMENT_INVALID")? {
                    path.push(key.clone());
                    let value = convert(item, path, paths, next)?;
                    path.pop();
                    entries.push(Entry {
                        key: key.clone(),
                        value,
                    });
                }
                Ok(EvaluationValue::Object { id, entries })
            }
        }
        let converted = convert(&parameters, &mut Vec::new(), &paths, &mut 2)?;
        Ok(EvaluationValue::Object {
            id: 1,
            entries: vec![
                Entry {
                    key: "toolId".into(),
                    value: EvaluationValue::Json { value: json!(tool) },
                },
                Entry {
                    key: "parameters".into(),
                    value: converted,
                },
                Entry {
                    key: "elementPaths".into(),
                    value: EvaluationValue::Json {
                        value: json!(paths),
                    },
                },
            ],
        })
    }
}
pub(crate) fn validate_parameters(value: &Value) -> Result<(), &'static str> {
    if !value.is_object() || super::schema::encoded_len(value, 65536).is_err() {
        return Err("DEVELOPER_TOOL_ARGUMENTS_BOUNDS");
    }
    fn walk(value: &Value, depth: usize, budget: &mut usize) -> Result<(), &'static str> {
        *budget += 1;
        if depth > 32 || *budget > 2048 {
            return Err("DEVELOPER_TOOL_ARGUMENTS_BOUNDS");
        }
        if let Some(items) = value.as_array() {
            for item in items {
                walk(item, depth + 1, budget)?;
            }
        }
        if let Some(items) = value.as_object() {
            for item in items.values() {
                walk(item, depth + 1, budget)?;
            }
        }
        Ok(())
    }
    walk(value, 0, &mut 0)
}
pub(crate) fn validation(value: &Value) -> Result<Vec<Vec<String>>, Error> {
    let text = value["value"]["value"]
        .as_str()
        .ok_or("DEVELOPER_TOOL_PROTOCOL")?;
    if text.len() > 8192 {
        return Err("DEVELOPER_TOOL_PROTOCOL".into());
    }
    let result: Value =
        serde_json::from_str(text).map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))?;
    if result["valid"] == true {
        return serde_json::from_value(result["elementPaths"].clone())
            .map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"));
    }
    let mut error = Error::from(if result["unsupportedSchema"] == true {
        "DEVELOPER_TOOL_SCHEMA_UNSUPPORTED"
    } else {
        "DEVELOPER_TOOL_ARGUMENTS_INVALID"
    });
    error.partial =
        Some(json!({"validation":result,"potentiallyMutating":true,"toolDispatched":false}));
    Err(error)
}
pub(crate) fn retained(mut error: Error, registry: Reference) -> Error {
    let cause = error.partial.take();
    error.partial = Some(
        json!({"registry":registry,"nextAction":"developer_tools_release","cause":cause,"potentiallyMutating":true}),
    );
    error
}
pub(crate) fn invocation_error(mut error: Error, registry: Reference, tool: &str) -> Error {
    let mut partial = error.partial.take().unwrap_or_else(|| json!({}));
    if !partial.is_object() {
        partial = json!({"cause":partial});
    }
    partial["registry"] = json!(registry);
    partial["toolId"] = json!(tool);
    partial["validationCompleted"] = json!(true);
    partial["potentiallyMutating"] = json!(true);
    error.partial = Some(partial);
    error
}
pub(crate) fn capabilities(available: bool) -> Value {
    json!({"available":available,"operations":["developer_tools_discover","developer_tool_call","developer_tool_result_read","developer_tools_release"],"source":"application-devtoolstooldiscovery","schemaValidator":"AJV-8.20.0-browser-helper","maxRegistries":4,"maxToolsPerRegistry":32,"maxMetadataBytes":131072,"maxParameterBytes":65536,"resultHandles":true,"domParameters":"schema-declared-nested-HTMLElement-owned-handles","maxElementParameters":32,"maxElementPathBytes":4096,"maxElementSchemaBranches":128,"domAnnotations":available,"domAnnotationScope":"owned-document-element-locators","maxResultAnnotations":32,"maxProjectedValueBytes":32768,"webMcpDebugTools":false,"nodeRequiredForChrome":false})
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"developer_tools_discover"},"scope":reference},"required":["operation","scope"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"developer_tool_call"},"registry":reference,"toolId":{"type":"string","minLength":1,"maxLength":32},"parameters":{"type":"object","default":{}},"returnHandle":{"type":"boolean","default":true}},"required":["operation","registry","toolId"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"developer_tools_release"},"registry":reference},"required":["operation","registry"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"developer_tool_result_read"},"handle":reference},"required":["operation","handle"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn result_annotations_keep_exact_frames_and_completed_read_facts() {
        let frame = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let handle = Reference {
            id: uuid::Uuid::new_v4(),
            ..frame
        };
        let page = Reference {
            id: uuid::Uuid::new_v4(),
            ..frame
        };
        let projected = json!({"value":{"n":17},"annotations":[{"index":0,"path":["node"],"locator":{"kind":"element","frame":frame,"id":uuid::Uuid::new_v4()}}],"complete":true,"projection":"own-enumerable-data-properties"});
        let evaluated = |projected: &Value| json!({"value":{"kind":"json","value":projected.to_string()},"dispatchCompleted":true,"potentiallyMutating":true});
        let result = result_projection(evaluated(&projected), handle, page, frame).unwrap();
        assert_eq!(result["value"], json!({"kind":"json","value":17}));
        assert_eq!(result["annotations"][0]["locator"]["frame"], json!(frame));
        assert_eq!(result["originalToolReplayed"], false);
        let mut forged = projected;
        forged["annotations"][0]["locator"]["frame"] = json!(page);
        let error = result_projection(evaluated(&forged), handle, page, frame).unwrap_err();
        assert_eq!(error.code, "DEVELOPER_TOOL_PROTOCOL");
        assert_eq!(error.partial.unwrap()["dispatchCompleted"], true);
    }
    #[test]
    fn registry_admission_counts_pending_discovery_and_keeps_exact_tool_ownership() {
        let mut owner = Registries::default();
        for _ in 0..4 {
            owner.reserve().unwrap();
        }
        assert_eq!(owner.reserve().unwrap_err().code, "DEVELOPER_TOOL_CAPACITY");
        owner.allocation_failed();
        owner.reserve().unwrap();
        let reference = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        owner.begin(reference, reference);
        let description=json!({"groups":[{"name":"Acme","tools":[{"toolId":"t0","name":"count","inputSchema":{"type":"object"}}]}]}).to_string();
        owner
            .finish(
                reference,
                &json!({"value":{"kind":"json","value":description}}),
            )
            .unwrap();
        owner.tool(reference, "t0").unwrap();
        assert_eq!(
            owner.tool(reference, "unknown").unwrap_err().code,
            "UNKNOWN_DEVELOPER_TOOL"
        );
        assert_eq!(owner.reserve().unwrap_err().code, "DEVELOPER_TOOL_CAPACITY");
        owner.forget(reference);
        assert!(owner.released(reference));
        assert_eq!(
            owner.tool(reference, "t0").unwrap_err().code,
            "UNKNOWN_DEVELOPER_TOOLS"
        );
        owner.reserve().unwrap();
    }
    #[test]
    fn element_parameters_resolve_only_validated_paths_in_nested_values() {
        let reference = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let mut owner = Registries::default();
        owner.reserve().unwrap();
        owner.begin(reference, reference);
        let description =
            json!({"groups":[{"tools":[{"toolId":"t0","elementParameters":["element"]}]}]})
                .to_string();
        owner
            .finish(reference, &json!({"value":{"value":description}}))
            .unwrap();
        let argument = owner
            .argument(
                reference,
                "t0",
                json!({"nested":{"nodes":[{"elementHandle":reference}]},"plain":{"elementHandle":reference}}),
                vec![vec!["nested".into(),"nodes".into(),"0".into()]],
            )
            .unwrap();
        let mut resolved = 0;
        let encoded = super::super::evaluation::argument(&argument, |handle| {
            assert_eq!(handle, reference);
            resolved += 1;
            Ok(json!({"objectId":"Acme"}))
        })
        .unwrap();
        assert_eq!(resolved, 1);
        assert_eq!(encoded["handles"].as_array().unwrap().len(), 1);
        assert_eq!(
            owner
                .argument(
                    reference,
                    "t0",
                    json!({"element":{"elementHandle":"invalid"}}),
                    vec![vec!["element".into()]],
                )
                .unwrap_err()
                .code,
            "DEVELOPER_TOOL_ELEMENT_INVALID"
        );
    }
    #[test]
    fn tool_parameters_bound_nested_data_before_page_execution() {
        assert!(validate_parameters(&json!({"amount":2})).is_ok());
        assert!(validate_parameters(&json!([])).is_err());
        let mut value = json!({});
        for _ in 0..34 {
            value = json!({"next":value});
        }
        assert!(validate_parameters(&value).is_err());
        assert!(validate_parameters(&json!({"text":"x".repeat(65536)})).is_err());
    }
}
