//! Selected request inspection; body artifacts never enter ordinary history.
use super::*;
use crate::browser::network::{self, BodySide};
use base64::{Engine, engine::general_purpose::STANDARD};

impl Host {
    pub(super) async fn network_read(
        &self,
        context: Reference,
        request_id: Uuid,
        options: network::Read,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        self.state
            .lock()
            .expect("Browser owner")
            .resource(context, Some(Kind::Context))?;
        let (wire, has_response, complete, body_withheld) = {
            let diagnostics = self.diagnostics.lock().expect("Diagnostics");
            let (wire, has_response, complete) =
                diagnostics.resolve_request(context, request_id)?;
            let withheld = diagnostics.request_body_withheld(&wire);
            (wire, has_response, complete, withheld)
        };
        if body_withheld && matches!(options.body, Some(BodySide::Request)) {
            return Err(network::body_withheld());
        }
        let connection = self.connection()?;
        let request = connection
            .initializer(&wire)
            .await?
            .ok_or("NETWORK_REQUEST_EXPIRED")?;
        let response = if has_response {
            Some(Self::network_response(&connection, &wire, deadline).await?)
        } else {
            None
        };
        let mut result = json!({"context":context,"requestId":request_id,"url":crate::browser::diagnostics::url(request["url"].as_str().unwrap_or("")),"method":request["method"],"resourceType":request["resourceType"],"complete":complete,"status":response.as_ref().map(|(_,value)|value["status"].clone()),"headersIncluded":options.headers,"bodyIncluded":options.body.is_some(),"source":"playwright-owned-request"});
        self.network_request_relations(&connection, &request, request_id, &mut result)
            .await?;
        result["statusText"] = response
            .as_ref()
            .map_or(Value::Null, |(_, value)| value["statusText"].clone());
        result["fromServiceWorker"] = response
            .as_ref()
            .map_or(Value::Null, |(_, value)| value["fromServiceWorker"].clone());
        if options.response_details {
            result["responseDetails"] = self
                .network_response_details(&connection, &wire, response.as_ref(), complete, deadline)
                .await?;
        }
        self.network_read_headers(
            &connection,
            &wire,
            response.as_ref(),
            &options,
            deadline,
            &mut result,
        )
        .await?;
        result["rawHeadersIncluded"] = json!(options.raw_headers);
        if let Some(side) = options.body {
            self.network_evidence
                .lock()
                .expect("Network evidence")
                .capacity()?;
            let encoded = match side {
                BodySide::Request => self.network_request_body(
                    context,
                    request_id,
                    options.max_bytes,
                    &request,
                    &mut result,
                )?,
                BodySide::Response => {
                    if !complete {
                        return Err("BODY_NOT_FINISHED".into());
                    }
                    let (response, initializer) = response.as_ref().ok_or("BODY_UNAVAILABLE")?;
                    // This pinned driver materializes the whole response. Refuse
                    // unknown or compressed sizes before asking it to allocate.
                    network::bounded_identity_length(&initializer["headers"], options.max_bytes)?;
                    connection
                        .retained(response, "body", json!({}), deadline, false)
                        .await?
                        .ok_or("MISSING_RESULT")?["binary"]
                        .as_str()
                        .ok_or("BODY_UNAVAILABLE")?
                        .to_owned()
                }
            };
            if encoded.len() > options.max_bytes.div_ceil(3) * 4 {
                return Err("BODY_BOUNDS".into());
            }
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| Error::from("BACKEND_PROTOCOL"))?;
            let artifact = network::write_body(
                self.journal.directory(),
                context,
                request_id,
                side,
                &bytes,
                options.max_bytes,
            )?;
            result["body"] = self
                .network_evidence
                .lock()
                .expect("Network evidence")
                .insert(artifact)?;
        }
        Ok(result)
    }
    async fn network_response(
        connection: &Connection,
        wire: &crate::browser::objects::ObjectRef,
        deadline: Instant,
    ) -> Result<(crate::browser::objects::ObjectRef, Value), Error> {
        let value = connection
            .retained(wire, "response", json!({}), deadline, false)
            .await?
            .ok_or("MISSING_RESULT")?;
        let response = connection
            .resolve(
                value["response"]["guid"]
                    .as_str()
                    .ok_or("BODY_UNAVAILABLE")?,
                "Response",
            )
            .await?;
        let initializer = connection
            .initializer(&response)
            .await?
            .ok_or("NETWORK_REQUEST_EXPIRED")?;
        Ok((response, initializer))
    }
    fn network_request_body(
        &self,
        context: Reference,
        request_id: Uuid,
        max_bytes: usize,
        request: &Value,
        result: &mut Value,
    ) -> Result<String, Error> {
        Ok({
            let inputs = self.request_inputs.lock().expect("Request input owner");
            match inputs.get(context.connection_id, request_id) {
                Ok(snapshot) => {
                    let body = snapshot.body().unwrap_or(&[]);
                    if body.len() > max_bytes {
                        return Err("BODY_BOUNDS".into());
                    }
                    result["requestBodyFidelity"] = json!(snapshot.body_fidelity());
                    STANDARD.encode(body)
                }
                Err(_) if inputs.enabled(context) => return Err("BODY_UNAVAILABLE".into()),
                Err(_) => {
                    result["requestBodyFidelity"] = json!("sdk-wire-initializer");
                    request["postData"].as_str().unwrap_or("").to_owned()
                }
            }
        })
    }
    async fn network_request_relations(
        &self,
        connection: &Connection,
        request: &Value,
        request_id: Uuid,
        result: &mut Value,
    ) -> Result<(), Error> {
        // SDK Request relations: navigation, owning frame, service worker and
        // the redirect chain in both directions.
        let frame = match request["frame"]["guid"].as_str() {
            Some(guid) => {
                let wire = connection.resolve(guid, "Frame").await.ok();
                wire.and_then(|wire| self.state.lock().expect("Browser owner").by_wire(&wire))
                    .map(|frame| frame.reference)
            }
            None => None,
        };
        let redirected_from = self.redirected_request(request).await?;
        let redirected_to = self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .redirected_to(request_id);
        result["navigation"] = request["isNavigationRequest"].clone();
        result["frame"] = json!(frame);
        result["serviceWorker"] = json!(request.get("serviceWorker").is_some());
        result["redirectedFrom"] = json!(redirected_from);
        result["redirectedTo"] = json!(redirected_to);
        Ok(())
    }

    async fn network_response_details(
        &self,
        connection: &Connection,
        wire: &crate::browser::objects::ObjectRef,
        response: Option<&(crate::browser::objects::ObjectRef, Value)>,
        complete: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let pending = response.is_some()
            && !complete
            && self.state.lock().expect("Browser owner").engine
                == crate::browser::contract::Engine::Webkit;
        let end = self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .request_end(wire);
        let (address, security, timing) = if let Some((response, initializer)) = response {
            let call = async |method: &str| -> Result<Value, Error> {
                Ok(connection
                    .retained(response, method, json!({}), deadline, false)
                    .await?
                    .ok_or("MISSING_RESULT")?["value"]
                    .clone())
            };
            let (address, security) = if pending {
                (Value::Null, Value::Null)
            } else {
                tokio::try_join!(call("serverAddr"), call("securityDetails"))?
            };
            (address, security, initializer["timing"].clone())
        } else {
            (Value::Null, Value::Null, Value::Null)
        };
        let mut details = network::response::details(&timing, end, &address, &security);
        details["responseAvailable"] = json!(response.is_some());
        details["connectionDetailsPending"] = json!(pending);
        details["source"] = json!("pinned-playwright-response-and-request-events");
        // SDK Response.httpVersion() and Request.sizes(); the pinned driver
        // resolves both only when the request finishes, so earlier reads
        // report null instead of waiting for completion.
        if let Some((response, initializer)) = response {
            let call = async |method: &str| -> Result<Value, Error> {
                Ok(connection
                    .retained(response, method, json!({}), deadline, false)
                    .await?
                    .ok_or("MISSING_RESULT")?)
            };
            details["statusText"] = initializer["statusText"].clone();
            let (version, sizes) = if complete {
                (
                    call("httpVersion").await?["value"].clone(),
                    call("sizes").await?["sizes"].clone(),
                )
            } else {
                (Value::Null, Value::Null)
            };
            details["httpVersion"] = version;
            details["sizes"] = sizes;
            details["sizesPending"] = json!(!complete);
        }
        Ok(details)
    }

    async fn network_read_headers(
        &self,
        connection: &Connection,
        wire: &crate::browser::objects::ObjectRef,
        response: Option<&(crate::browser::objects::ObjectRef, Value)>,
        options: &network::Read,
        deadline: Instant,
        result: &mut Value,
    ) -> Result<(), Error> {
        if options.headers || options.raw_headers {
            let headers = connection
                .retained(wire, "rawRequestHeaders", json!({}), deadline, false)
                .await?
                .ok_or("MISSING_RESULT")?;
            if options.headers {
                result["requestHeaders"] = network::headers(&headers["headers"]);
            }
            if options.raw_headers {
                result["rawRequestHeaders"] = network::raw_headers(&headers["headers"]);
            }
            if let Some((response, _)) = response {
                let headers = connection
                    .retained(response, "rawResponseHeaders", json!({}), deadline, false)
                    .await?
                    .ok_or("MISSING_RESULT")?;
                if options.headers {
                    result["responseHeaders"] = network::headers(&headers["headers"]);
                }
                if options.raw_headers {
                    result["rawResponseHeaders"] = network::raw_headers(&headers["headers"]);
                }
            }
        }
        Ok(())
    }
    pub(super) fn network_evidence_remove(&self, id: Uuid) -> Result<Value, Error> {
        let artifact = self
            .network_evidence
            .lock()
            .expect("Network evidence")
            .get(id)?;
        network::remove_body(&artifact)?;
        self.network_evidence
            .lock()
            .expect("Network evidence")
            .forget(id);
        Ok(json!({"evidenceId":id,"removed":true}))
    }
}
