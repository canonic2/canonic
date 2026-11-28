//! Live browser-context defaults shared by associated clients, fenced on uncertainty.
use super::*;

pub(crate) type Shared = Arc<Mutex<Option<Vec<Header>>>>;

impl Contexts {
    pub(crate) fn context_headers_initial(&mut self, context: Reference, initial: &[Header]) {
        self.browser_headers
            .entry(context)
            .or_insert_with(|| Arc::new(Mutex::new(Some(initial.to_vec()))));
    }
    pub(crate) fn inherit_headers(
        &mut self,
        options: &mut Options,
        context: Reference,
        initial: &[Header],
    ) {
        if options.headers.is_none() {
            options.inherited_headers = Some(
                self.browser_headers
                    .entry(context)
                    .or_insert_with(|| Arc::new(Mutex::new(Some(initial.to_vec()))))
                    .clone(),
            );
        }
    }

    pub(crate) fn context_headers_pending(&mut self, context: Reference) -> Option<Vec<Header>> {
        self.browser_headers
            .entry(context)
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .lock()
            .expect("Context HTTP headers")
            .take()
    }

    pub(crate) fn context_headers_applied(&mut self, context: Reference, headers: &[Header]) {
        *self
            .browser_headers
            .entry(context)
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .lock()
            .expect("Context HTTP headers") = Some(headers.to_vec());
    }

    pub(crate) fn context_headers_closed(&mut self, context: Reference) {
        if let Some(headers) = self.browser_headers.remove(&context) {
            *headers.lock().expect("Context HTTP headers") = None;
        }
    }
}
