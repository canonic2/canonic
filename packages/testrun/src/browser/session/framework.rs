//! Driver React hook preparation and document-bound renderer queries.
use super::*;
use crate::browser::framework;
impl Host {
    async fn prepare_framework(
        &self,
        page: Reference,
        kind: Kind,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        self.fence_init_scripts();
        let wire = self.wire(page, kind)?;
        let field = if kind == Kind::Context {
            "context"
        } else {
            "page"
        };
        let script_id = self
            .init_scripts
            .lock()
            .expect("Init scripts")
            .begin(page)?;
        let mut registered = false;
        let result:Result<Value,Error>=async{
            let connection=self.connection()?;
            let reply=connection.retained(&wire,"addInitScript",json!({"source":framework::HOOK}),deadline,false).await?.ok_or("MISSING_RESULT")?;
            let guid=reply["disposable"]["guid"].as_str().ok_or("BACKEND_PROTOCOL")?;
            let disposable=connection.resolve(guid,"Disposable").await?;
            self.init_scripts.lock().expect("Init scripts").complete(script_id,disposable)?;
            registered=true;
            connection.event_barrier(deadline).await?;
            self.wire(page,kind)?;
            Ok(json!({(field):page,"scriptId":script_id,"prepared":true,"currentDocumentExecuted":false,"nextAction":if kind==Kind::Context{"page_create"}else{"page_navigate"},"scope":if kind==Kind::Context{"context-and-pages-and-frames"}else{"page-and-child-frames"}}))
        }.await;
        result.map_err(|mut error|{
            let uncertain=registered||matches!(error.code,"OUTCOME_UNKNOWN"|"BACKEND_PROTOCOL"|"MISSING_RESULT");
            if !uncertain||error.code=="STALE_REFERENCE"{self.init_scripts.lock().expect("Init scripts").forget(script_id);}
            error.partial=Some(json!({(field):page,"scriptId":script_id,"registered":registered,"scopeVerified":false,"registrationUncertain":!registered&&uncertain}));
            error
        })
    }
    async fn open_framework(&self, scope: Reference, deadline: Instant) -> Result<Value, Error> {
        let kind = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(scope, None)?
            .kind;
        if !matches!(kind, Kind::Page | Kind::Frame) {
            return Err("INVALID_SCOPE".into());
        }
        let references = self.frameworks.lock().expect("Frameworks").references();
        for registry in references {
            if self
                .state
                .lock()
                .expect("Browser owner")
                .resource(registry, Some(Kind::Handle))
                .is_err()
            {
                self.frameworks.lock().expect("Frameworks").forget(registry);
            }
        }
        self.frameworks.lock().expect("Frameworks").reserve()?;
        let allocated = self
            .tool_evaluate(
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
                self.frameworks.lock().expect("Frameworks").failed();
                return Err(error);
            }
        };
        self.frameworks.lock().expect("Frameworks").insert(registry);
        let result = self
            .tool_evaluate(
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
            json!({"registry":registry,"scope":scope,"report":result["report"],"potentiallyMutating":true,"dispatchCompleted":true}),
        )
    }
    pub(super) async fn framework_operation(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::FrameworkPrepare { page } => {
                self.prepare_framework(page, Kind::Page, deadline).await
            }
            Request::ContextFrameworkPrepare { context } => {
                self.prepare_framework(context, Kind::Context, deadline)
                    .await
            }
            Request::FrameworkOpen { scope } => self.open_framework(scope, deadline).await,
            Request::FrameworkQuery { registry, query } => {
                self.frameworks
                    .lock()
                    .expect("Frameworks")
                    .require(registry)?;
                let result = self
                    .tool_evaluate(
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
                {
                    let owner = self.frameworks.lock().expect("Frameworks");
                    if owner.closed(registry) {
                        return Ok(json!({"registry":registry,"released":true}));
                    }
                    owner.require(registry)?;
                }
                let live = self
                    .state
                    .lock()
                    .expect("Browser owner")
                    .resource(registry, Some(Kind::Handle))
                    .is_ok();
                if live {
                    self.tool_evaluate(
                        registry,
                        "registry=>registry.close()".into(),
                        Value::Null,
                        false,
                        deadline,
                    )
                    .await
                    .map_err(|error| framework::retained(error, registry))?;
                    self.close_resource(registry, Kind::Handle, "dispose", deadline)
                        .await
                        .map_err(|error| framework::retained(error, registry))?;
                }
                self.frameworks.lock().expect("Frameworks").forget(registry);
                Ok(json!({"registry":registry,"released":true,"hookInstallationPreserved":true}))
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
}
