//! Selected computed CSS properties through a strict document-bound locator.
use super::*;
use crate::browser::{page::Locator, values};
impl Host {
    pub(super) async fn page_styles(
        &self,
        page: Reference,
        frame: Option<Reference>,
        locator: Locator,
        properties: Vec<String>,
        max_bytes: usize,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(page, frame, deadline).await?;
        let result=self.connection()?.retained(&scope.wire,"evalOnSelector",json!({"selector":locator.scoped_selector(scope.reference)?,"strict":true,"expression":crate::browser::reading::DOM,"isFunction":true,"arg":values::argument(&json!({"read":"styles","properties":properties,"maxBytes":max_bytes}))?}),deadline,false).await?.ok_or("MISSING_RESULT")?;
        let text = values::result(&result["value"])?;
        let result: Value = serde_json::from_str(text.as_str().ok_or("NON_JSON_RESULT")?)
            .map_err(|_| Error::from("NON_JSON_RESULT"))?;
        if self.frame_current(&scope.wire, deadline).await? != scope.reference {
            return Err("DOCUMENT_CHANGED".into());
        }
        Ok(
            json!({"page":page,"frame":scope.reference,"read":"styles","source":"playwright-frame","result":result}),
        )
    }
}
