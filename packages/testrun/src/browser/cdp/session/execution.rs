//! Caller execution shares ordinary admission without borrowing control slots.
use super::*;

pub(super) fn metadata(request: &Request) -> bool {
    matches!(
        request,
        Request::EventsRead { .. }
            | Request::EventsWait { .. }
            | Request::DiagnosticsRead { .. }
            | Request::DiagnosticsWait { .. }
    )
}

pub(super) fn independent(request: &Request) -> bool {
    fn caller(request: &Request) -> bool {
        matches!(
            request,
            Request::Evaluate { .. }
                | Request::SelectorEvaluate { .. }
                | Request::HandleRead { .. }
                | Request::HandleProperty { .. }
                | Request::HandleProperties { .. }
                | Request::HandleOwnerFrame { .. }
                | Request::HandleContentFrame { .. }
                | Request::FrameElement { .. }
                | Request::FrameRetain { .. }
                | Request::FrameOwnerRelease { .. }
        )
    }
    fn page_read(request: &Request) -> bool {
        // Geometry produces an input observation, so it keeps mutation order.
        matches!(request, Request::PageRead { read, .. }
            if !matches!(read, crate::browser::page::Read::Geometry))
    }
    caller(request)
        || page_read(request)
        || metadata(request)
        || matches!(request, Request::HttpFetch { .. } | Request::HttpFetchRequest { .. })
        || matches!(request, Request::ContextClose { .. } | Request::PageClose { .. } | Request::NetworkRoute { .. })
        || crate::browser::socket_routes::independent_batch(request)
        // Shared act wraps even a single callback in a batch. It must reach
        // the native call that is waiting for that callback, using normal
        // capacity and the original ordered batch/authority checks.
        || matches!(request, Request::Batch { requests }
            if !requests.is_empty() && requests.iter().all(|request| matches!(request,
                Request::CallbackResolve { .. } | Request::ContextClose { .. } | Request::PageClose { .. } | Request::NetworkRoute { .. })))
        || matches!(request, Request::Batch { requests }
            if !requests.is_empty()
                && (requests.iter().all(caller) || requests.iter().all(page_read)))
}

impl Worker {
    pub(super) fn perform_independent(
        &mut self,
        command: Command,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(self.perform(command))
    }
}
