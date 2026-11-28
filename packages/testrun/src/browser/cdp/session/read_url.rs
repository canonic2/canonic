//! Direct URL reading through the owned frame's network loader and inert HTML parsing.
use super::*;
use base64::Engine as _;

impl Worker {
    pub(super) async fn read_url(
        &mut self,
        id: Uuid,
        page: Reference,
        url: String,
        max_bytes: usize,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(id, page, None, deadline).await?;
        let native = self
            .index
            .native(scope.frame, Kind::Frame)
            .map_err(resource_error)?;
        let reply=self.call(id,Some(&scope.session),"Network.loadNetworkResource",json!({"frameId":native,"url":url,"options":{"disableCache":true,"includeCredentials":true}}),deadline).await?;
        let resource = &reply["resource"];
        if resource["success"] != true {
            return Err("READ_URL_FAILED".into());
        }
        let stream = resource["stream"]
            .as_str()
            .ok_or("READ_URL_FAILED")?
            .to_owned();
        let bytes = self.read_url_stream(id, &scope, &stream, deadline).await;
        let cleanup = self
            .connection
            .as_ref()
            .ok_or("BACKEND_LOST")?
            .call(
                Some(&scope.session),
                "IO.close",
                json!({"handle":stream}),
                Lane::Control,
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .map_err(protocol_error);
        let bytes = bytes?;
        cleanup?;
        let html = String::from_utf8_lossy(&bytes);
        let expression = format!(
            "({})({},{},{})",
            include_str!("../../reading/content.js"),
            json!(html),
            json!({"url":url,"maxBytes":max_bytes}),
            crate::browser::reading::ARIA_BOUND
        );
        let result = self
            .page_expression(id, &scope, expression, deadline)
            .await?;
        Ok(
            json!({"page":page,"source":"direct-url-html","scriptsExecuted":false,"status":resource["httpStatusCode"],"result":result}),
        )
    }
    async fn read_url_stream(
        &mut self,
        id: Uuid,
        scope: &page::Scope,
        stream: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        loop {
            let chunk = self
                .call_read(
                    id,
                    Some(&scope.session),
                    "IO.read",
                    json!({"handle":stream,"size":65536}),
                    deadline,
                )
                .await?;
            let text = chunk["data"].as_str().ok_or("BACKEND_PROTOCOL")?;
            let data = if chunk["base64Encoded"] == true {
                base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .map_err(|_| Error::from("BACKEND_PROTOCOL"))?
            } else {
                text.as_bytes().to_vec()
            };
            if bytes.len() + data.len() > 262144 {
                return Err("READ_URL_BOUNDS".into());
            }
            bytes.extend(data);
            if chunk["eof"] == true {
                return Ok(bytes);
            }
        }
    }
}
