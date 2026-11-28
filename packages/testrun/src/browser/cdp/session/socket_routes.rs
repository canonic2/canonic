//! Native WebSocket routing with exact owned main-world execution.
use super::*;
use crate::browser::socket_routes::{self, Backend};
struct Native<'a> {
    worker: &'a mut Worker,
    id: Uuid,
}
impl Worker {
    pub(super) async fn socket_send(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        let catalog = self.evaluation.socket_routes.clone();
        socket_routes::command(&mut Native { worker: self, id }, catalog, request, deadline).await
    }
    pub(super) async fn socket_route(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        self.reconcile(id, deadline).await?;
        let catalog = self.evaluation.socket_routes.clone();
        socket_routes::execute(&mut Native { worker: self, id }, catalog, request, deadline).await
    }
}
impl Backend for Native<'_> {
    fn send(
        &mut self,
        scope: Reference,
        frame: Reference,
        source: String,
        deadline: Instant,
    ) -> futures_util::future::BoxFuture<'_, Result<Value, Error>> {
        Box::pin(async move {
            let page = self.scope_is_page(scope)?;
            self.worker
                .index
                .native(frame, Kind::Frame)
                .map_err(resource_error)?;
            let (_, world) = self
                .worker
                .index
                .execution_world(frame)
                .map_err(resource_error)?;
            let route = self
                .worker
                .index
                .route(world, Some(Kind::World))
                .map_err(resource_error)?;
            let (context, owner, _) = self
                .worker
                .index
                .diagnostic_scope(&route.session_id)
                .ok_or("INVALID_SCOPE")?;
            if if page {
                owner != Some(scope)
            } else {
                context != scope
            } {
                return Err("INVALID_SCOPE".into());
            }
            self.worker
                .execute_evaluation(
                    self.id,
                    Request::Evaluate {
                        scope: frame,
                        expression: source,
                        is_function: Some(false),
                        argument: Default::default(),
                        return_handle: false,
                    },
                    deadline,
                )
                .await
        })
    }
    fn scope_is_page(&self, scope: Reference) -> Result<bool, Error> {
        if self.worker.index.retained(scope, Kind::Page) {
            self.worker
                .index
                .native(scope, Kind::Page)
                .map_err(resource_error)?;
            Ok(true)
        } else {
            self.worker
                .index
                .native(scope, Kind::Context)
                .map_err(resource_error)?;
            Ok(false)
        }
    }
    async fn binding(&mut self, request: Request, deadline: Instant) -> Result<Value, Error> {
        self.worker
            .binding_operation(self.id, request, deadline)
            .await
    }
    async fn script(&mut self, request: Request, deadline: Instant) -> Result<Value, Error> {
        self.worker.init_script(self.id, request, deadline).await
    }
    async fn documents(
        &mut self,
        scope: Reference,
        source: String,
        deadline: Instant,
    ) -> Result<(), Error> {
        let page_scope = self.scope_is_page(scope)?;
        let pages = if page_scope {
            vec![scope]
        } else {
            self.worker
                .index
                .list(Kind::Page)
                .into_iter()
                .filter(|(_, parent)| *parent == Some(scope))
                .map(|(page, _)| page)
                .collect()
        };
        let worlds = pages
            .into_iter()
            .flat_map(|page| self.worker.index.page_worlds(page))
            .collect::<Vec<_>>();
        for world in worlds {
            let Ok(route) = self.worker.index.route(world, Some(Kind::World)) else {
                continue;
            };
            let result=self.worker.call(self.id,Some(&route.session_id),"Runtime.evaluate",json!({"expression":source,"uniqueContextId":route.execution_unique_id,"returnByValue":true,"awaitPromise":false}),deadline).await;
            match result {
                Ok(value) if value.get("exceptionDetails").is_some() => {
                    return Err("WEBSOCKET_HELPER_FAILED".into());
                }
                Err(error) => {
                    self.scope_is_page(scope)?;
                    if self.worker.index.route(world, Some(Kind::World)).is_ok() {
                        return Err(error);
                    }
                }
                _ => {}
            }
        }
        self.scope_is_page(scope)?;
        Ok(())
    }
}
