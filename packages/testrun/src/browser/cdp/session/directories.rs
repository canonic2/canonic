//! Native directory fixture registration and verified path attachment.
use super::*;
use crate::browser::{
    directory::{self, Entry},
    page::Locator,
};

impl Worker {
    pub(super) async fn directory_register(
        &mut self,
        name: String,
        entries: Vec<Entry>,
    ) -> Result<Value, Error> {
        directory::validate(&name, &entries)?;
        if self.transfers.directories.len() >= 16
            || self.transfers.directories.len() + self.transfers.fixtures.len() >= 64
        {
            return Err("FIXTURE_CAPACITY".into());
        }
        let mut inputs = Vec::new();
        for entry in entries {
            let (summary, bytes) = self.fixture_bytes(entry.fixture).await?;
            inputs.push((entry, summary, bytes));
        }
        let reference = self.transfer_reference();
        let directory =
            directory::create(self.journal.directory().into(), reference, name, inputs).await?;
        let summary = directory.summary.clone();
        self.transfers.directories.insert(reference, directory);
        Ok(json!({"fixture":summary}))
    }
    pub(super) async fn directory_parameters(
        &self,
        fixtures: &[Reference],
    ) -> Result<Option<(std::path::PathBuf, Value)>, Error> {
        if !fixtures
            .iter()
            .any(|reference| self.transfers.directories.contains_key(reference))
        {
            return Ok(None);
        }
        if fixtures.len() != 1 {
            return Err("DIRECTORY_FIXTURE_REQUIRED".into());
        }
        let directory = self
            .transfers
            .directories
            .get(&fixtures[0])
            .ok_or("STALE_REFERENCE")?
            .clone();
        self.transfer_scope(fixtures[0])?;
        for source in &directory.sources {
            self.fixture_bytes(*source).await?;
        }
        directory::verify(directory.clone()).await?;
        Ok(Some((directory.path, directory.summary)))
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "One upload binds operation, exact page/frame locator, fixture and chooser policy to its deadline"
    )]
    pub(super) async fn upload_directory(
        &mut self,
        id: Uuid,
        page: Reference,
        scope: &page::Scope,
        locator: Locator,
        path: std::path::PathBuf,
        summary: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let group = format!("testrun-directory-{id}");
        let result=async{
            let expression=page::query_expression(json!({"selector":locator.scoped_selector(scope.frame)?,"read":"frame"}));
            let object=self.call_read(id,Some(&scope.session),"Runtime.evaluate",json!({"expression":expression,"uniqueContextId":scope.unique,"objectGroup":group,"returnByValue":false,"awaitPromise":true}),deadline).await?;
            if object.get("exceptionDetails").is_some(){return Err(page::execution_error(&object));}
            let object=object["result"]["objectId"].as_str().ok_or("ELEMENT_NOT_FOUND")?;
            let kind=self.call_read(id,Some(&scope.session),"Runtime.callFunctionOn",json!({"objectId":object,"functionDeclaration":"function(){return this.tagName==='INPUT'&&this.type==='file'&&this.webkitdirectory;}","returnByValue":true}),deadline).await?;
            if kind["result"]["value"]!=true{return Err("DIRECTORY_INPUT_REQUIRED".into());}
            self.call(id,Some(&scope.session),"DOM.setFileInputFiles",json!({"objectId":object,"files":[path]}),deadline).await?;
            Ok(json!({"page":page,"frame":scope.frame,"fixtures":[summary],"attached":true,"serverUploadVerified":false,"source":"chrome-cdp-directory"}))
        }.await;
        let cleanup = self
            .connection
            .as_ref()
            .ok_or("BACKEND_LOST")?
            .call(
                Some(&scope.session),
                "Runtime.releaseObjectGroup",
                json!({"objectGroup":group}),
                Lane::Control,
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .map_err(protocol_error);
        let value = result?;
        cleanup?;
        Ok(value)
    }
}
