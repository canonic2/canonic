//! Private verified fixtures and CDP file attachment; no caller host paths.
use super::*;
use crate::browser::{
    page::Locator,
    transfer::{self, Fixture, Registration},
};

#[derive(Default)]
pub(super) struct Transfers {
    pub directories: BTreeMap<Reference, crate::browser::directory::Directory>,
    pub fixtures: BTreeMap<Reference, Fixture>,
    pub downloads: BTreeMap<Reference, Download>,
    pub download_contexts: BTreeMap<Reference, std::path::PathBuf>,
    pub choosers: BTreeMap<Reference, Chooser>,
    pub refused: std::collections::VecDeque<Value>,
    pub pending_cancels: std::collections::VecDeque<(String, Reference)>,
    pub responding: BTreeSet<Reference>,
}
pub(super) struct Chooser {
    pub page: Reference,
    pub frame: Reference,
    pub session: String,
    pub node: i64,
    pub multiple: bool,
    pub expires: Instant,
}
pub(super) struct Download {
    pub guid: String,
    pub context: Reference,
    pub summary: Value,
    pub collect: bool,
    pub cancel: bool,
    pub expires: Instant,
    pub cancel_attempts: u8,
}
impl Worker {
    pub(super) fn transfer_reference(&self) -> Reference {
        Reference {
            connection_id: self.shared.state.lock().expect("Chrome state").connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    pub(super) fn transfer_scope(&self, reference: Reference) -> Result<(), Error> {
        if reference.connection_id != self.shared.state.lock().expect("Chrome state").connection {
            return Err("FOREIGN_REFERENCE".into());
        }
        Ok(())
    }
    pub(super) async fn fixture_register(&mut self, input: Registration) -> Result<Value, Error> {
        input.validate()?;
        if self.transfers.fixtures.len() + self.transfers.directories.len() >= 64 {
            return Err("FIXTURE_CAPACITY".into());
        }
        let reference = self.transfer_reference();
        let fixture = Fixture::store(self.journal.directory(), reference, input, false).await?;
        let receipt = json!({"fixture":fixture.summary});
        self.transfers.fixtures.insert(reference, fixture);
        Ok(receipt)
    }
    pub(super) async fn fixture_bytes(
        &self,
        reference: Reference,
    ) -> Result<(Value, Vec<u8>), Error> {
        self.transfer_scope(reference)?;
        let fixture = self
            .transfers
            .fixtures
            .get(&reference)
            .cloned()
            .ok_or("STALE_REFERENCE")?;
        fixture.read().await
    }
    pub(super) async fn page_upload(
        &mut self,
        id: Uuid,
        page: Reference,
        frame: Option<Reference>,
        locator: Locator,
        fixtures: Vec<Reference>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(id, page, frame, deadline).await?;
        if let Some((path, summary)) = self.directory_parameters(&fixtures).await? {
            return self
                .upload_directory(id, page, &scope, locator, path, summary, deadline)
                .await;
        }
        let (payloads, summaries) =
            transfer::payloads(fixtures, |reference| self.fixture_bytes(reference)).await?;
        self.page_expression(
            id,
            &scope,
            format!(
                "(()=>{{{};return true;}})()",
                crate::browser::reading::selector("chromium")
            ),
            deadline,
        )
        .await?;
        let expression = format!(
            "(()=>{{const injected={};const elements=injected.querySelectorAll(injected.parseSelector({}),document);if(elements.length!==1)return {{error:elements.length?'AMBIGUOUS_LOCATOR':'ELEMENT_NOT_FOUND'}};const element=elements[0];if(element.tagName!=='INPUT'||element.type!=='file')return {{error:'NOT_FILE_INPUT'}};if(element.webkitdirectory&&{}>0)return {{error:'DIRECTORY_INPUT_REQUIRED'}};if(!element.multiple&&{}>1)return {{error:'SINGLE_FILE_INPUT'}};const result=injected.setInputFiles(element,{});return result?{{error:'FILE_ATTACH_FAILED'}}:{{attached:true}};}})()",
            "globalThis[Symbol.for('canonic.testrun.selector.v3')]",
            json!(locator.scoped_selector(scope.frame)?),
            payloads.len(),
            payloads.len(),
            Value::Array(payloads)
        );
        let mut params =
            json!({"uniqueContextId":scope.unique,"returnByValue":true,"awaitPromise":true});
        params["expression"] = Value::String(expression);
        let result = self
            .call_fixture_payload(id, &scope.session, params, deadline)
            .await?;
        if result.get("exceptionDetails").is_some() {
            return Err(page::execution_error(&result));
        }
        page::query_result(result["result"]["value"].clone())?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        Ok(
            json!({"page":page,"frame":scope.frame,"fixtures":summaries,"attached":true,"serverUploadVerified":false,"source":"chrome-cdp-file-payload"}),
        )
    }
}
