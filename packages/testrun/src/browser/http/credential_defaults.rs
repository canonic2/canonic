//! A context owns mutable credentials; each admitted request owns one snapshot.
use super::*;
pub(crate) type Shared = Arc<Mutex<Option<Credentials>>>;
impl Options {
    pub(crate) fn effective_credentials(&self) -> Result<Option<Credentials>, Error> {
        match &self.inherited_credentials {
            Some(shared) => shared
                .lock()
                .expect("Context HTTP credentials")
                .clone()
                .map(Some)
                .ok_or_else(|| "HTTP_CONTEXT_CREDENTIALS_UNCERTAIN".into()),
            None => Ok(self.http_credentials.clone()),
        }
    }
}
impl Contexts {
    pub(crate) fn require_context_credentials(&self, context: Reference) -> Result<(), Error> {
        if self
            .browser_credentials
            .get(&context)
            .is_some_and(|shared| shared.lock().expect("Context HTTP credentials").is_none())
        {
            return Err("HTTP_CONTEXT_CREDENTIALS_UNCERTAIN".into());
        }
        Ok(())
    }
    pub(crate) fn context_credentials_initial(
        &mut self,
        context: Reference,
        initial: Option<&Credentials>,
    ) {
        self.browser_credentials.entry(context).or_insert_with(|| {
            Arc::new(Mutex::new(Some(
                initial
                    .cloned()
                    .unwrap_or_else(|| Credentials::List(vec![])),
            )))
        });
    }
    pub(crate) fn inherit_credentials(
        &mut self,
        options: &mut Options,
        context: Reference,
        initial: Option<&Credentials>,
    ) {
        self.context_credentials_initial(context, initial);
        options.inherited_credentials = Some(
            self.browser_credentials
                .get(&context)
                .expect("Owned context credentials")
                .clone(),
        );
        // The shared owner is authoritative; do not retain an unused initial
        // secret copy in each inherited client's immutable options.
        options.http_credentials = None;
    }
    pub(crate) fn context_credentials_pending(
        &mut self,
        context: Reference,
    ) -> Option<Credentials> {
        self.browser_credentials
            .entry(context)
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .lock()
            .expect("Context HTTP credentials")
            .take()
    }
    pub(crate) fn context_credentials_applied(
        &mut self,
        context: Reference,
        credentials: Option<&Credentials>,
    ) {
        *self
            .browser_credentials
            .entry(context)
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .lock()
            .expect("Context HTTP credentials") = Some(
            credentials
                .cloned()
                .unwrap_or_else(|| Credentials::List(vec![])),
        );
    }
    pub(crate) fn context_credentials_closed(&mut self, context: Reference) {
        if let Some(shared) = self.browser_credentials.remove(&context) {
            *shared.lock().expect("Context HTTP credentials") = None;
        }
    }
    pub(crate) fn context_defaults_closed(&mut self, context: Reference) {
        self.browser_context_closed(context);
        self.context_headers_closed(context);
        self.context_credentials_closed(context);
    }
}
