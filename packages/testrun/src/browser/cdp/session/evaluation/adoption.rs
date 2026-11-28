//! Cross-frame arguments borrow exact source handles and own temporary target wrappers.
use super::super::execution_groups::{Key, Purpose};
use super::*;

pub(super) struct Arguments {
    pub(super) encoded: Value,
    sources: BTreeMap<Reference, Handle>,
    slots: Vec<(usize, Reference)>,
    pub(super) cleanup: Vec<Key>,
}
impl Arguments {
    pub(super) fn needs_adoption(&self) -> bool {
        !self.sources.is_empty()
    }
}

impl Worker {
    pub(super) fn evaluation_arguments(
        &self,
        argument: &EvaluationValue,
        document: Reference,
        world: Reference,
    ) -> Result<Arguments, Error> {
        let mut sources = BTreeMap::new();
        let mut slots = Vec::new();
        let mut index = 0;
        let encoded = evaluation::argument(argument, |reference| {
            let handle = self
                .evaluation
                .handles
                .get(&reference)
                .ok_or("STALE_REFERENCE")?;
            self.index
                .route(handle.world, Some(Kind::World))
                .map_err(|_| "STALE_REFERENCE")?;
            let slot = index;
            index += 1;
            if handle.world == world {
                return Ok(json!({"objectId":handle.object}));
            }
            let page = self
                .index
                .owning_page(document)
                .map_err(|_| "FOREIGN_HANDLE")?;
            if self.index.native(document, Kind::Frame).is_err()
                || self.index.native(handle.scope, Kind::Frame).is_err()
                || self.index.owning_page(handle.scope).ok() != Some(page)
            {
                return Err("FOREIGN_HANDLE");
            }
            sources.entry(reference).or_insert_with(|| handle.clone());
            slots.push((slot, reference));
            Ok(json!({}))
        })?;
        Ok(Arguments {
            encoded,
            sources,
            slots,
            cleanup: Vec::new(),
        })
    }

    pub(super) fn begin_argument_adoption(
        &mut self,
        world: Reference,
        session: &str,
    ) -> Result<Key, Error> {
        let context = self
            .index
            .diagnostic_scope(session)
            .map(|(context, _, _)| context)
            .ok_or("INVALID_SCOPE")?;
        self.index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        self.evaluation
            .groups
            .begin(context, session.into(), Purpose::ArgumentAdoption)
    }

    fn argument_worlds_live(&self, source: &Handle, world: Reference) -> Result<(), Error> {
        let source_live = self.evaluation_handle_live(source.reference);
        let target_live = self.index.route(world, Some(Kind::World)).is_ok();
        if !source_live || !target_live {
            let mut error = Error::from("STALE_REFERENCE");
            error.partial =
                Some(json!({"sourceWorldLive":source_live,"targetWorldLive":target_live}));
            return Err(error);
        }
        Ok(())
    }

    pub(super) async fn adopt_evaluation_arguments(
        &mut self,
        id: Uuid,
        world: Reference,
        key: &Key,
        arguments: &mut Arguments,
        deadline: Instant,
    ) -> Result<(), Error> {
        let target = self
            .index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        let unique = target.execution_unique_id.ok_or("INVALID_SCOPE")?;
        let numeric = target.execution_numeric_id.ok_or("INVALID_SCOPE")?;
        let mut adopted = BTreeMap::new();
        for (reference, source) in &arguments.sources {
            self.argument_worlds_live(source, world)?;
            let source_key = self.begin_argument_adoption(source.world, &source.session)?;
            let mut stage = "source-extraction";
            let result: Result<Value,Error> = async {
            // Only the Testrun-owned wrapper's data slot is read. Application
            // objects are not enumerated and their getters are not invoked.
            let extracted = self.execution_group_call(id,&source_key,"Runtime.callFunctionOn",json!({"functionDeclaration":"function(){return this.value;}","objectId":source.object,"returnByValue":false,"awaitPromise":false,"objectGroup":source_key.1}),deadline).await?;
            stage = "source-extraction-fence";
            self.argument_worlds_live(source,world)?;
            if extracted.get("exceptionDetails").is_some() {return Err("HANDLE_ADOPTION_UNAVAILABLE".into());}
            let value = &extracted["result"];
            let argument = if let Some(object) = value["objectId"].as_str() {
                if value["subtype"]!="node" {return Err("FOREIGN_HANDLE".into());}
                // The pinned CDP adoption uses the source renderer session's
                // numeric context namespace. Cross-renderer nodes stay refused.
                if source.session!=key.0 {return Err("HANDLE_ADOPTION_UNAVAILABLE".into());}
                stage = "describe-node";
                let node = self.execution_group_call(id,&source_key,"DOM.describeNode",json!({"objectId":object,"depth":0}),deadline).await?;
                self.argument_worlds_live(source,world)?;
                let backend = node["node"]["backendNodeId"].as_u64().ok_or("BACKEND_PROTOCOL")?;
                stage = "resolve-node";
                let resolved = self.execution_group_call(id,key,"DOM.resolveNode",json!({"backendNodeId":backend,"executionContextId":numeric,"objectGroup":key.1}),deadline).await?;
                self.argument_worlds_live(source,world)?;
                let object = resolved["object"]["objectId"].as_str().ok_or("HANDLE_ADOPTION_UNAVAILABLE")?;
                json!({"objectId":object})
            } else { primitive_argument(value)? };
            stage = "target-wrapper";
            let wrapper = self.execution_group_call(id,key,"Runtime.callFunctionOn",json!({"functionDeclaration":"function(value){return {value};}","uniqueContextId":unique,"arguments":[argument],"returnByValue":false,"awaitPromise":false,"objectGroup":key.1}),deadline).await?;
            self.argument_worlds_live(source,world)?;
            if wrapper.get("exceptionDetails").is_some() {return Err("HANDLE_ADOPTION_UNAVAILABLE".into());}
            let object = wrapper["result"]["objectId"].as_str().ok_or("BACKEND_PROTOCOL")?;
            Ok(json!({"objectId":object}))
            }.await;
            self.release_execution_group(
                &source_key,
                deadline.min(Instant::now() + Duration::from_secs(2)),
            )
            .await;
            arguments.cleanup.push(source_key);
            adopted.insert(
                *reference,
                result.map_err(|mut error| {
                    let cause = error.partial.take();
                    error.partial = Some(json!({"adoptionStage":stage,"cause":cause}));
                    error
                })?,
            );
        }
        // Every source must still be live at the caller dispatch boundary.
        // After adoption, the caller owns its argument value in the target
        // world; source navigation cannot replay or cancel that expression.
        for source in arguments.sources.values() {
            self.argument_worlds_live(source, world)?;
        }
        for (slot, reference) in &arguments.slots {
            arguments.encoded["handles"][*slot] = adopted[reference].clone();
        }
        Ok(())
    }
}

fn primitive_argument(value: &Value) -> Result<Value, Error> {
    if value["type"] == "undefined" {
        return Ok(json!({}));
    }
    if value["type"] == "object" && value["subtype"] == "null" {
        return Ok(json!({"value":null}));
    }
    if !matches!(
        value["type"].as_str(),
        Some("string" | "boolean" | "number" | "bigint")
    ) {
        return Err("FOREIGN_HANDLE".into());
    }
    if let Some(raw) = value.get("unserializableValue") {
        let raw = raw.as_str().ok_or("BACKEND_PROTOCOL")?;
        let valid = if value["type"] == "number" {
            matches!(raw, "NaN" | "Infinity" | "-Infinity" | "-0")
        } else if value["type"] == "bigint" {
            raw.strip_suffix('n')
                .map(|digits| digits.strip_prefix('-').unwrap_or(digits))
                .is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                })
        } else {
            false
        };
        if !valid {
            return Err("BACKEND_PROTOCOL".into());
        }
        return Ok(json!({"unserializableValue":raw}));
    }
    let primitive = value.get("value").ok_or("BACKEND_PROTOCOL")?;
    if !match value["type"].as_str() {
        Some("string") => primitive.is_string(),
        Some("boolean") => primitive.is_boolean(),
        Some("number") => primitive.is_number(),
        _ => false,
    } {
        return Err("BACKEND_PROTOCOL".into());
    }
    Ok(json!({"value":primitive}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evaluation_adoption_preserves_primitive_protocol_arguments_and_refuses_malformed_values() {
        for (remote, expected) in [
            (json!({"type":"undefined"}), json!({})),
            (
                json!({"type":"object","subtype":"null","value":null}),
                json!({"value":null}),
            ),
            (
                json!({"type":"string","value":"Acme café"}),
                json!({"value":"Acme café"}),
            ),
            (
                json!({"type":"boolean","value":false}),
                json!({"value":false}),
            ),
            (json!({"type":"number","value":42}), json!({"value":42})),
            (
                json!({"type":"number","unserializableValue":"-0"}),
                json!({"unserializableValue":"-0"}),
            ),
            (
                json!({"type":"number","unserializableValue":"NaN"}),
                json!({"unserializableValue":"NaN"}),
            ),
            (
                json!({"type":"bigint","unserializableValue":"-12345678901234567890n"}),
                json!({"unserializableValue":"-12345678901234567890n"}),
            ),
        ] {
            assert_eq!(primitive_argument(&remote).unwrap(), expected);
        }
        for remote in [
            json!({"type":"function"}),
            json!({"type":"symbol"}),
            json!({"type":"object","value":{}}),
            json!({"type":"number","unserializableValue":"42+fetch('/acme')"}),
            json!({"type":"string","value":false}),
            json!({"type":"boolean","unserializableValue":"NaN"}),
            json!({"type":"bigint","unserializableValue":"n"}),
        ] {
            assert!(primitive_argument(&remote).is_err());
        }
    }
}
