//! Driver WebSocket routing through the shared managed binding/script owners.
use super::*;
use crate::browser::socket_routes::{self, Backend};
impl Host {
    pub(super) async fn socket_route(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        socket_routes::execute(&mut &*self, self.socket_routes.clone(), request, deadline).await
    }
}
impl Backend for &Host {
    fn send(
        &mut self,
        scope: Reference,
        frame: Reference,
        source: String,
        deadline: Instant,
    ) -> futures_util::future::BoxFuture<'_, Result<Value, Error>> {
        Box::pin(async move {
            {
                let state = self.state.lock().expect("Browser owner");
                let kind = state.resource(scope, None)?.kind;
                state.resource(frame, Some(Kind::Frame))?;
                if !matches!(kind, Kind::Page | Kind::Context)
                    || state.ancestor(frame, kind) != Some(scope)
                {
                    return Err("INVALID_SCOPE".into());
                }
            }
            self.evaluate_scoped(
                frame,
                source,
                Some(false),
                Default::default(),
                false,
                deadline,
            )
            .await
        })
    }
    fn scope_is_page(&self, scope: Reference) -> Result<bool, Error> {
        let state = self.state.lock().expect("Browser owner");
        match state.resource(scope, None)?.kind {
            Kind::Page => Ok(true),
            Kind::Context => Ok(false),
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn binding(&mut self, request: Request, deadline: Instant) -> Result<Value, Error> {
        self.binding_operation(request, deadline).await
    }
    async fn script(&mut self, request: Request, deadline: Instant) -> Result<Value, Error> {
        self.init_script(request, deadline).await
    }
    async fn documents(
        &mut self,
        scope: Reference,
        source: String,
        deadline: Instant,
    ) -> Result<(), Error> {
        let frames = {
            let state = self.state.lock().expect("Browser owner");
            state.resource(scope, None)?;
            state
                .subtree(scope)
                .into_iter()
                .filter(|r| r.kind == Kind::Frame)
                .map(|r| r.reference)
                .collect::<Vec<_>>()
        };
        for frame in frames {
            let result = self
                .evaluate_scoped(
                    frame,
                    source.clone(),
                    Some(false),
                    Default::default(),
                    false,
                    deadline,
                )
                .await;
            if let Err(error) = result {
                self.scope_is_page(scope)?;
                let gone = self
                    .state
                    .lock()
                    .expect("Browser owner")
                    .resource(frame, Some(Kind::Frame))
                    .is_err();
                if !gone {
                    return Err(error);
                }
            }
        }
        self.scope_is_page(scope)?;
        Ok(())
    }
}
