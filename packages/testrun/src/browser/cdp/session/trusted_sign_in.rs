//! Trusted sign-in and sign-in detection through the owned native pages.
use super::*;
use crate::browser::authentication::{self, Pages};

/// Journey steps run as this operation's own page work.
struct WorkerPages<'a> {
    worker: &'a mut Worker,
    id: Uuid,
}
impl Pages for WorkerPages<'_> {
    fn run(
        &mut self,
        request: Request,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + '_>> {
        Box::pin(self.worker.execute_one(self.id, request, deadline))
    }
    fn secret_policy(&mut self, page: Reference) -> Result<(), Error> {
        let context = self.context(page)?;
        let reason = if self.worker.trace.is_some() {
            Some("trace")
        } else if self.worker.har_archives.capturing(context) {
            Some("har_capture")
        } else {
            (self.worker.routing_rules.manual(page) || self.worker.routing_rules.manual(context))
                .then_some("request_callbacks")
        };
        authentication::secret_policy(page, reason)
    }
    fn withhold_request_bodies(&mut self, page: Reference) -> Result<Reference, Error> {
        let context = self.context(page)?;
        self.worker
            .diagnostics
            .history
            .withhold_bodies(context, true);
        Ok(context)
    }
    fn release_request_bodies(
        &mut self,
        context: Reference,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        // Requests the browser reported during entry are recorded first.
        let _ = self.worker.drain();
        self.worker
            .diagnostics
            .history
            .withhold_bodies(context, false);
        Box::pin(async {})
    }
}
impl WorkerPages<'_> {
    fn context(&self, page: Reference) -> Result<Reference, Error> {
        Ok(self
            .worker
            .index
            .list(Kind::Page)
            .into_iter()
            .find_map(|(candidate, context)| (candidate == page).then_some(context))
            .flatten()
            .ok_or("STALE_REFERENCE")?)
    }
}

impl Worker {
    /// `auth_*` requests as this operation's own page work.
    pub(super) async fn execute_authentication(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        authentication::execute(&mut WorkerPages { worker: self, id }, request, deadline).await
    }
}
