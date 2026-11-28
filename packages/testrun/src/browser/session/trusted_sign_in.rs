//! Trusted sign-in and sign-in detection through the owned Playwright pages.
use super::*;
use crate::browser::authentication::{self, Pages};

/// Journey steps run as this operation's own page work.
struct HostPages<'a> {
    host: &'a Arc<Host>,
    operation: Uuid,
}
impl Pages for HostPages<'_> {
    fn run(
        &mut self,
        request: Request,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + '_>> {
        Box::pin(self.host.execute(self.operation, request, deadline))
    }
    fn secret_policy(&mut self, page: Reference) -> Result<(), Error> {
        let context = self.context(page)?;
        let reason = if self.host.trace.lock().expect("Browser trace").is_some() {
            Some("trace")
        } else if self
            .host
            .har_archives
            .lock()
            .expect("HAR archives")
            .capturing(context)
        {
            Some("har_capture")
        } else {
            let rules = self.host.routing.rules.lock().expect("Routing rules");
            (rules.manual(page) || rules.manual(context)).then_some("request_callbacks")
        };
        authentication::secret_policy(page, reason)
    }
    fn withhold_request_bodies(&mut self, page: Reference) -> Result<Reference, Error> {
        let context = self.context(page)?;
        self.host
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .withhold_bodies(context, true);
        Ok(context)
    }
    fn release_request_bodies(
        &mut self,
        context: Reference,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            // Requests sent during entry are recorded before the window ends.
            if let Ok(connection) = self.host.connection() {
                let _ = connection
                    .event_barrier(Instant::now() + RELEASE_BARRIER)
                    .await;
            }
            self.host
                .diagnostics
                .lock()
                .expect("Diagnostics")
                .withhold_bodies(context, false);
        })
    }
}
impl HostPages<'_> {
    fn context(&self, page: Reference) -> Result<Reference, Error> {
        Ok(self
            .host
            .state
            .lock()
            .expect("Browser owner")
            .ancestor(page, Kind::Context)
            .ok_or("STALE_REFERENCE")?)
    }
}
/// The longest wait for recording requests sent during entry.
const RELEASE_BARRIER: std::time::Duration = std::time::Duration::from_secs(2);

impl Host {
    /// `auth_*` requests as this operation's own page work.
    pub(super) async fn execute_authentication(
        self: &Arc<Self>,
        operation: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let mut pages = HostPages {
            host: self,
            operation,
        };
        authentication::execute(&mut pages, request, deadline).await
    }
}
