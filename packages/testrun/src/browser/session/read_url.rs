//! Direct HTML reads through the owned context's request channel, without navigation.
use super::*;
use base64::Engine as _;

impl Host {
    pub(super) async fn read_url(
        &self,
        page: Reference,
        url: String,
        max_bytes: usize,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(page, None, deadline).await?;
        let context = self
            .state
            .lock()
            .expect("Browser owner")
            .ancestor(page, Kind::Context)
            .ok_or("INVALID_SCOPE")?;
        let connection = self.connection()?;
        let wire = self.wire(context, Kind::Context)?;
        let init = connection
            .initializer(&wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let request = connection
            .resolve(
                init["requestContext"]["guid"]
                    .as_str()
                    .ok_or("MISSING_INITIALIZER")?,
                "APIRequestContext",
            )
            .await?;
        let result = connection
            .retained(
                &request,
                "fetch",
                json!({"url":url,"method":"GET","maxRedirects":10,"maxRetries":0}),
                deadline,
                false,
            )
            .await?
            .ok_or("MISSING_RESULT")?;
        let response = &result["response"];
        let uid = response["fetchUid"].as_str().ok_or("MISSING_RESULT")?;
        let body = connection
            .retained(
                &request,
                "fetchResponseBody",
                json!({"fetchUid":uid}),
                deadline,
                false,
            )
            .await;
        let cleanup = connection
            .retained(
                &request,
                "disposeAPIResponse",
                json!({"fetchUid":uid}),
                Instant::now() + Duration::from_secs(2),
                true,
            )
            .await;
        let body = body?.ok_or("MISSING_RESULT")?;
        cleanup?;
        let encoded = body["binary"].as_str().ok_or("MISSING_RESULT")?;
        if encoded.len() > 4 * 262144 / 3 + 4 {
            return Err("READ_URL_BOUNDS".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| Error::from("NON_JSON_RESULT"))?;
        if bytes.len() > 262144 {
            return Err("READ_URL_BOUNDS".into());
        }
        let expression = format!(
            "(options)=>({})(options.html,options,{})",
            include_str!("../reading/content.js"),
            crate::browser::reading::ARIA_BOUND
        );
        let result = self
            .evaluate(
                &scope.wire,
                &expression,
                json!({"html":String::from_utf8_lossy(&bytes),"url":url,"maxBytes":max_bytes}),
                deadline,
            )
            .await?;
        if self.frame_current(&scope.wire, deadline).await? != scope.reference {
            return Err("DOCUMENT_CHANGED".into());
        }
        Ok(
            json!({"page":page,"source":"direct-url-html","scriptsExecuted":false,"status":response["status"],"result":result}),
        )
    }
}
