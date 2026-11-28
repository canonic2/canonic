//! React hook installation and document-bound native framework queries.
use super::*;
use crate::browser::framework;
impl Worker {
    async fn prepare_framework(
        &mut self,
        id: Uuid,
        page: Reference,
        kind: Kind,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.index.native(page, kind).map_err(resource_error)?;
        let mut result = self
            .register_init_script(
                id,
                page,
                std::borrow::Cow::Borrowed(framework::HOOK),
                deadline,
            )
            .await?;
        result
            .as_object_mut()
            .expect("Script registration")
            .remove("registered");
        result["prepared"] = json!(true);
        result["nextAction"] = json!(if kind == Kind::Context {
            "page_create"
        } else {
            "page_navigate"
        });
        result["scope"] = json!(if kind == Kind::Context {
            "context-and-pages-and-frames"
        } else {
            "page-and-child-frames"
        });
        Ok(result)
    }
    async fn open_framework(
        &mut self,
        id: Uuid,
        scope: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        self.index
            .native(scope, Kind::Page)
            .or_else(|_| self.index.native(scope, Kind::Frame))
            .map_err(resource_error)?;
        for registry in self.frameworks.references() {
            if !self.evaluation_handle_live(registry) {
                self.frameworks.forget(registry);
            }
        }
        self.frameworks.reserve()?;
        let allocated = self
            .tool_evaluate(
                id,
                scope,
                format!("()=>({})()", framework::HELPER),
                Value::Null,
                true,
                deadline,
            )
            .await
            .and_then(|value| {
                serde_json::from_value(value["handle"].clone())
                    .map_err(|_| Error::from("FRAMEWORK_PROTOCOL"))
            });
        let registry = match allocated {
            Ok(registry) => registry,
            Err(error) => {
                self.frameworks.failed();
                return Err(error);
            }
        };
        self.frameworks.insert(registry);
        let described = self
            .tool_evaluate(
                id,
                registry,
                "registry=>registry.describe()".into(),
                Value::Null,
                false,
                deadline,
            )
            .await
            .and_then(framework::result)
            .map_err(|error| framework::retained(error, registry))?;
        Ok(
            json!({"registry":registry,"scope":scope,"report":described["report"],"potentiallyMutating":true,"dispatchCompleted":true}),
        )
    }
    pub(super) async fn framework_operation(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::FrameworkPrepare { page } => {
                self.prepare_framework(id, page, Kind::Page, deadline).await
            }
            Request::ContextFrameworkPrepare { context } => {
                self.prepare_framework(id, context, Kind::Context, deadline)
                    .await
            }
            Request::FrameworkOpen { scope } => self.open_framework(id, scope, deadline).await,
            Request::FrameworkQuery { registry, query } => {
                self.frameworks.require(registry)?;
                let result = self
                    .tool_evaluate(
                        id,
                        registry,
                        "(registry,query)=>registry.query(query)".into(),
                        json!(query),
                        false,
                        deadline,
                    )
                    .await
                    .and_then(framework::result)
                    .map_err(|error| framework::retained(error, registry))?;
                Ok(
                    json!({"registry":registry,"report":result["report"],"untrustedApplicationContent":true,"potentiallyMutating":true,"dispatchCompleted":true}),
                )
            }
            Request::FrameworkRelease { registry } => {
                if self.frameworks.closed(registry) {
                    return Ok(json!({"registry":registry,"released":true}));
                }
                self.frameworks.require(registry)?;
                if self.evaluation_handle_live(registry) {
                    self.tool_evaluate(
                        id,
                        registry,
                        "registry=>registry.close()".into(),
                        Value::Null,
                        false,
                        deadline,
                    )
                    .await
                    .map_err(|error| framework::retained(error, registry))?;
                    self.execute_evaluation(
                        id,
                        Request::HandleDispose { handle: registry },
                        deadline,
                    )
                    .await
                    .map_err(|error| framework::retained(error, registry))?;
                }
                self.frameworks.forget(registry);
                Ok(json!({"registry":registry,"released":true,"hookInstallationPreserved":true}))
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
}
