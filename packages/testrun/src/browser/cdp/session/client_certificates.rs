//! Native context creation retains private relays through unknown allocation.
use super::*;
use crate::browser::{client_certificate_tunnel::Tunnel, http::client_certificates::Cached};

#[derive(Default)]
pub(super) struct Tunnels {
    entries: BTreeMap<Uuid, Entry>,
}
struct Entry {
    context: Option<Reference>,
    tunnel: Tunnel,
    cached: Cached,
}
impl Tunnels {
    async fn attach(
        &mut self,
        options: &mut ContextOptions,
        mut prepared: crate::browser::http::Options,
        deadline: Instant,
    ) -> Result<Option<(Uuid, contract::ContextProxy)>, Error> {
        if prepared
            .client_certificates
            .as_ref()
            .is_none_or(Vec::is_empty)
        {
            return Ok(None);
        }
        let cached = prepared
            .prepared_client_certificates
            .clone()
            .ok_or("HTTP_CLIENT_CERTIFICATE_PREPARATION_REQUIRED")?;
        let identities = crate::browser::http::client_certificates::take_prepared(&mut prepared)?;
        let tunnel = Tunnel::start(identities, options.proxy.clone(), deadline).await?;
        options.client_certificates = prepared.client_certificates;
        let proxy = tunnel.settings();
        let id = Uuid::new_v4();
        self.entries.insert(
            id,
            Entry {
                context: None,
                tunnel,
                cached,
            },
        );
        Ok(Some((id, proxy)))
    }
    pub(super) fn active(&self, context: Reference) -> bool {
        self.entries
            .values()
            .any(|entry| entry.context == Some(context))
    }
    pub(super) fn bind(&mut self, id: Uuid, context: Reference) {
        self.entries
            .get_mut(&id)
            .expect("Retained context tunnel")
            .context = Some(context);
    }
    pub(super) fn cached(&self, context: Reference) -> Option<Cached> {
        self.entries
            .values()
            .find(|entry| entry.context == Some(context))
            .map(|entry| entry.cached.clone())
    }
    pub(super) async fn close(
        &mut self,
        context: Option<Reference>,
        deadline: Instant,
    ) -> Result<(), Error> {
        let selected = self
            .entries
            .iter()
            .filter(|(_, entry)| context.is_none() || entry.context == context)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let mut failure = None;
        for id in selected {
            let entry = self.entries.get_mut(&id).expect("Retained private tunnel");
            match entry.tunnel.close(deadline).await {
                Ok(()) => {
                    self.entries.remove(&id);
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
impl Worker {
    pub(super) async fn context_create_configured(
        &mut self,
        id: Uuid,
        mut options: ContextOptions,
        explicit_proxy: bool,
        deadline: Instant,
    ) -> Result<(Reference, ContextOptions), Error> {
        if self.attached {
            contract::validate_attachment_context(contract::Engine::Chrome, &options)?;
        }
        let preparation = crate::browser::http::Options {
            client_certificates: options.client_certificates.clone(),
            ..Default::default()
        };
        let prepared = self
            .await_http_preparation(
                id,
                crate::browser::http::client_certificates::prepare_options(preparation, deadline),
                deadline,
            )
            .await?;
        let private = self
            .certificate_tunnels
            .attach(&mut options, prepared, deadline)
            .await?;
        let proxy = private
            .as_ref()
            .map(|(_, proxy)| proxy)
            .or_else(|| options.proxy.as_ref().filter(|_| explicit_proxy));
        let context = self.context_create_with_proxy_bound(id, proxy, private.as_ref().map(|(id, _)| *id), deadline).await.map_err(|mut error| {
            if private.is_some() {
                let context = private.as_ref().and_then(|(id, _)| self.certificate_tunnels.entries.get(id)).and_then(|entry| entry.context);
                error.partial = Some(json!({"context":context,"clientCertificateTunnelRetained":true,"cleanup":if context.is_some(){"owned-context-or-connection"}else{"owned-connection"},"replayed":false,"cause":error.partial}));
            }
            error
        })?;
        Ok((context, options))
    }
}
