//! Native context capture uses owned Network sessions and retains local publication.
use super::*;
impl Worker {
    #[expect(
        clippy::too_many_arguments,
        reason = "Keep native request identity, capture scope, options and deadline explicit across setup"
    )]
    pub(super) async fn har_capture_start(
        &mut self,
        id: Uuid,
        context: Reference,
        page: Option<Reference>,
        options: (
            crate::browser::contract::HarCaptureContent,
            crate::browser::contract::HarCaptureMode,
            Option<crate::browser::har::Export>,
        ),
        replaces: Option<Uuid>,
        filter: Option<crate::browser::routing::Matcher>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.diagnostic_query_scope(context, page)?;
        self.prune_har_routes();
        if replaces.is_some_and(|id| self.routing_rules.used(id)) {
            return Err("HAR_ARCHIVE_IN_USE".into());
        }
        let summary = self
            .har_archives
            .capture_start(context, page, options, replaces, filter)?;
        let sessions = self
            .index
            .targets()
            .into_iter()
            .filter_map(|(_, _, _, session)| session)
            .filter(|session| {
                self.index
                    .diagnostic_scope(session)
                    .is_some_and(|(owner, _, _)| owner == context)
            })
            .collect::<Vec<_>>();
        for session in sessions {
            if let Err(mut error) = self.configure_diagnostics(id, &session, deadline).await {
                self.har_archives.capture_context_gap(context, 1);
                error.partial = Some(
                    json!({"capture":summary,"setupPending":true,"nextAction":"har_capture_stop"}),
                );
                return Err(error);
            }
        }
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        Ok(summary)
    }
    pub(super) async fn har_capture_stop(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.har_capture_stop_inner(id,context,deadline).await.map_err(|mut error|{
            if error.partial.is_none() && let Ok(capture)=self.har_archives.capture_status(context){error.partial=Some(json!({"capture":capture,"cleanupPending":true,"nextAction":"retry-stop-or-disconnect"}));}error
        })
    }
    async fn har_capture_stop_inner(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        let pending = self.har_archives.capture_stop(context)?;
        for request in pending {
            match self
                .har_capture_payload(id, context, request, deadline)
                .await
            {
                Ok(bytes) => self
                    .har_archives
                    .capture_body(context, request, Some(&bytes), ""),
                Err(error) => self
                    .har_archives
                    .capture_body(context, request, None, error.code),
            }
        }
        for page in self.har_archives.capture_pages(context) {
            // A closed or unreadable page keeps the pinned initial page values.
            let Ok(scope) = self.page_scope(id, page, None, deadline).await else {
                continue;
            };
            if let Ok(details) = self
                .page_expression(
                    id,
                    &scope,
                    format!("({})()", crate::browser::har::PAGE_DETAILS),
                    deadline,
                )
                .await
            {
                self.har_archives
                    .capture_page_details(context, page, &details);
            }
        }
        let sessions = self
            .index
            .targets()
            .into_iter()
            .filter_map(|(_, _, _, session)| session)
            .filter(|session| {
                self.index
                    .diagnostic_scope(session)
                    .is_some_and(|(owner, _, _)| owner == context)
            })
            .collect::<Vec<_>>();
        for session in sessions {
            self.configure_diagnostics(id, &session, deadline).await?;
        }
        self.har_archives
            .capture_publish(self.journal.directory(), context)
    }
}
