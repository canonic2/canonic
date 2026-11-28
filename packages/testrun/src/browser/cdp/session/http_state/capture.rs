//! Pinned Chromium cookie projection and utility-world storage serialization.
use super::*;
use crate::browser::http::StorageStateOptions as Options;

fn expression(origin: &str, options: &Options) -> String {
    format!(
        "(async()=>{{if(location.origin!=={origin})throw Error('Storage origin changed');const module={{exports:{{}}}};\n{source}\nconst state=await new (module.exports.StorageScript())('chromium').collect({options});if(location.origin!=={origin})throw Error('Storage origin changed');return state;}})()",
        origin = json!(origin),
        source = include_str!("../../../reading/storage.js"),
        options = json!({"indexedDB":options.indexed_db,"opfs":options.opfs})
    )
}
fn cookies(cookies: &Value) -> Result<Value, Error> {
    cookies
        .as_array()
        .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?
        .iter()
        .map(|cookie| {
            if cookie["partitionKeyOpaque"] == true {
                return Err("HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE".into());
            }
            let object = cookie.as_object().ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
            let mut projected = serde_json::Map::new();
            for key in [
                "name", "value", "domain", "path", "expires", "httpOnly", "secure",
            ] {
                projected.insert(
                    key.into(),
                    object
                        .get(key)
                        .cloned()
                        .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?,
                );
            }
            projected.insert(
                "sameSite".into(),
                object.get("sameSite").cloned().unwrap_or(json!("Lax")),
            );
            if let Some(partition) = object.get("partitionKey") {
                let site = partition["topLevelSite"]
                    .as_str()
                    .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
                let ancestor = partition["hasCrossSiteAncestor"]
                    .as_bool()
                    .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
                projected.insert("partitionKey".into(), json!(site));
                projected.insert("_crHasCrossSiteAncestor".into(), json!(ancestor));
            }
            Ok(Value::Object(projected))
        })
        .collect::<Result<Vec<_>, Error>>()
        .map(Value::Array)
}
fn append(origins: &mut Vec<Value>, origin: &str, mut state: Value) -> Result<(), Error> {
    let object = state.as_object_mut().ok_or("INVALID_STORAGE_STATE")?;
    if ["localStorage", "indexedDB", "opfs"].iter().any(|key| {
        object
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
    }) {
        object.insert("origin".into(), json!(origin));
        origins.push(state);
    }
    Ok(())
}
impl Worker {
    pub(in super::super) async fn native_http_storage_state(
        &mut self,
        id: Uuid,
        client: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Some(context) = self.http_contexts.associated_context(client)? else {
            return self
                .http_contexts
                .storage_state(client, options, self.journal.directory());
        };
        if self.http_state.capturing || self.http_state.collection.is_some() {
            return Err("HTTP_STORAGE_STATE_PENDING".into());
        }
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.drain()?;
        self.http_state.origins.plan(context).map_err(Error::from)?;
        let capture = Uuid::new_v4();
        self.http_contexts.reserve_storage_state(capture)?;
        self.http_state.capturing = true;
        let result: Result<(), Error> = async {
            let state = self
                .collect_http_state(id, context, capture, &options, deadline)
                .await?;
            self.index
                .native(context, Kind::Context)
                .map_err(resource_error)?;
            self.http_contexts
                .stage_reserved_storage_state(client, options, state, capture)?;
            if let Some(collection) = self.http_state.collection.as_mut() {
                collection.complete = true;
            }
            Ok(())
        }
        .await;
        let cleanup = self.cleanup_storage_page(deadline).await;
        self.http_state.capturing = false;
        if result.is_err() {
            self.http_contexts.release_storage_state(capture);
        }
        if cleanup.is_err() {
            return Err(self.storage_cleanup_pending());
        }
        result?;
        self.http_contexts
            .finalize_evidence(self.journal.directory(), capture)
    }
    pub(in super::super) async fn native_http_state_finalize(
        &mut self,
        capture: Uuid,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if let Some(collection) = &self.http_state.collection
            && collection.capture == capture
        {
            if self.http_state.capturing || !collection.complete {
                return Err(self.storage_cleanup_pending());
            }
            self.http_state.capturing = true;
            let result = self.cleanup_storage_page(deadline).await;
            self.http_state.capturing = false;
            result?;
        }
        self.http_contexts
            .finalize_evidence(self.journal.directory(), capture)
    }
    pub(super) async fn collect_http_state(
        &mut self,
        id: Uuid,
        context: Reference,
        capture: Uuid,
        options: &Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let read = self
            .call_read(
                id,
                None,
                "Storage.getCookies",
                json!({"browserContextId":native}),
                deadline,
            )
            .await?;
        let cookies = cookies(&read["cookies"])?;
        let plan = self.http_state.origins.plan(context).map_err(Error::from)?;
        let mut remaining = plan.origins;
        let mut origins = Vec::new();
        // Match the SDK's current-page order, then its insertion-ordered fallback.
        // A failing live page leaves that origin available to another live page.
        for (page, origin) in plan.pages {
            if !remaining.contains(&origin) {
                continue;
            }
            let result = async {
                let scope = self.page_storage_scope(id, page, None, deadline).await?;
                self.page_expression(id, &scope, expression(&origin, options), deadline)
                    .await
            }
            .await;
            if let Ok(state) = result {
                append(&mut origins, &origin, state)?;
                remaining.retain(|pending| pending != &origin);
            }
        }
        if !remaining.is_empty() {
            self.controller_current(id, true)?;
            if Instant::now() >= deadline {
                return Err("CONDITION_TIMEOUT".into());
            }
            let (_, session) = self.storage_page(id, context, capture, deadline).await?;
            for origin in remaining {
                let state = self
                    .storage_origin_evaluate(
                        id,
                        &session,
                        &origin,
                        expression(&origin, options),
                        false,
                        deadline,
                    )
                    .await
                    .map_err(|error| {
                        if error.code == "STORAGE_SCRIPT_FAILED" {
                            Error::from("HTTP_STORAGE_COLLECTION_FAILED")
                        } else {
                            error
                        }
                    })?;
                append(&mut origins, &origin, state)?;
            }
        }
        Ok(json!({"cookies":cookies,"origins":origins}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_http_state_cookie_projection_preserves_pinned_defaults_and_partition_identity() {
        let source = json!({"name":"Acme","value":"private","domain":"example.com","path":"/","expires":-1,"httpOnly":true,"secure":true,"size":11,"priority":"Medium","session":true,"sourcePort":443});
        let projected = cookies(&json!([source])).unwrap();
        assert_eq!(
            projected[0],
            json!({"name":"Acme","value":"private","domain":"example.com","path":"/","expires":-1,"httpOnly":true,"secure":true,"sameSite":"Lax"})
        );
        let mut source = projected[0].clone();
        source["partitionKey"] =
            json!({"topLevelSite":"https://example.com","hasCrossSiteAncestor":false});
        let projected = cookies(&json!([source.clone()])).unwrap();
        assert_eq!(projected[0]["partitionKey"], "https://example.com");
        assert_eq!(projected[0]["_crHasCrossSiteAncestor"], false);
        source["partitionKeyOpaque"] = json!(true);
        assert_eq!(
            cookies(&json!([source])).unwrap_err().code,
            "HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE"
        );
        assert!(cookies(&json!([{"name":"incomplete"}])).is_err());
    }
    #[test]
    fn native_http_state_omits_empty_origins_and_keeps_collection_order() {
        let mut origins = vec![];
        append(
            &mut origins,
            "https://empty.example.com",
            json!({"localStorage":[],"indexedDB":[],"opfs":[]}),
        )
        .unwrap();
        append(
            &mut origins,
            "https://b.example.com",
            json!({"localStorage":[{"name":"Acme","value":"private"}]}),
        )
        .unwrap();
        append(
            &mut origins,
            "https://a.example.com",
            json!({"localStorage":[],"opfs":[{"type":"directory","path":["Acme"]}]}),
        )
        .unwrap();
        assert_eq!(
            origins
                .iter()
                .map(|state| state["origin"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["https://b.example.com", "https://a.example.com"]
        );
    }
}
