//! Driver tools use the same page/frame handles and retain cleanup on failure.
use super::*;
use crate::browser::{developer_tools as tools, evaluation::EvaluationValue};
impl Host {
    pub(super) async fn tool_evaluate(
        &self,
        scope: Reference,
        expression: String,
        argument: Value,
        retain: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.evaluate_scoped(
            scope,
            expression,
            Some(true),
            EvaluationValue::Json { value: argument },
            retain,
            deadline,
        )
        .await
    }
    pub(super) async fn developer_tools_operation(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::DeveloperToolResultRead { handle } => {
                self.connection()?.event_barrier(deadline).await?;
                let (frame, page) = {
                    let state = self.state.lock().expect("Browser owner");
                    state.resource(handle, Some(Kind::Handle))?;
                    let frame = state.ancestor(handle, Kind::Frame).ok_or("INVALID_SCOPE")?;
                    state.resource(frame, Some(Kind::Frame))?;
                    let page = state.ancestor(frame, Kind::Page).ok_or("INVALID_SCOPE")?;
                    (frame, page)
                };
                let evaluated = self
                    .tool_evaluate(
                        handle,
                        tools::result_expression(),
                        json!(frame),
                        false,
                        deadline,
                    )
                    .await?;
                tools::result_projection(evaluated, handle, page, frame)
            }
            Request::DeveloperToolsDiscover { scope } => {
                self.discover_developer_tools(scope, deadline).await
            }
            Request::DeveloperToolCall {
                registry,
                tool_id,
                parameters,
                return_handle,
            } => {
                let credential = self.authority.credential(std::time::Instant::now());
                self.developer_tools
                    .lock()
                    .expect("Developer tools")
                    .tool(registry, &tool_id)?;
                let input = json!({"toolId":tool_id,"parameters":parameters});
                let checked = self
                    .tool_evaluate(
                        registry,
                        "(registry,input)=>registry.validate(input.toolId,input.parameters)".into(),
                        input.clone(),
                        false,
                        deadline,
                    )
                    .await?;
                let paths = tools::validation(&checked)?;
                let argument = self
                    .developer_tools
                    .lock()
                    .expect("Developer tools")
                    .argument(registry, &tool_id, input["parameters"].clone(), paths)?;
                if self.authority.credential(std::time::Instant::now()) != credential {
                    let mut error = Error::from("STALE_CONTROL_LEASE");
                    error.partial = Some(
                        json!({"registry":registry,"toolId":tool_id,"validationCompleted":true,"toolDispatched":false,"potentiallyMutating":true,"nextAction":"control_status"}),
                    );
                    return Err(error);
                }
                let mut result = self
                    .evaluate_scoped(
                        registry,
                        "(registry,input)=>registry.invoke(input.toolId,input.parameters,input.elementPaths)".into(),
                        Some(true),
                        argument,
                        return_handle,
                        deadline,
                    )
                    .await
                    .map_err(|error| tools::invocation_error(error, registry, &tool_id))?;
                result["registry"] = json!(registry);
                result["toolId"] = json!(tool_id);
                Ok(result)
            }
            Request::DeveloperToolsRelease { registry } => {
                self.release_developer_tools(registry, deadline).await
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn discover_developer_tools(
        &self,
        scope: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let kind = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(scope, None)?
            .kind;
        if !matches!(kind, Kind::Page | Kind::Frame) {
            return Err("INVALID_SCOPE".into());
        }
        let references = self
            .developer_tools
            .lock()
            .expect("Developer tools")
            .references();
        for registry in references {
            if self
                .state
                .lock()
                .expect("Browser owner")
                .resource(registry, Some(Kind::Handle))
                .is_err()
            {
                self.developer_tools
                    .lock()
                    .expect("Developer tools")
                    .forget(registry);
            }
        }
        self.developer_tools
            .lock()
            .expect("Developer tools")
            .reserve()?;
        let expression = format!("async()=>({})({})", tools::DISCOVER, tools::VALIDATOR);
        let allocated = self
            .tool_evaluate(scope, expression, Value::Null, true, deadline)
            .await
            .and_then(|allocated| {
                serde_json::from_value(allocated["handle"].clone())
                    .map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))
            });
        let registry = match allocated {
            Ok(registry) => registry,
            Err(error) => {
                self.developer_tools
                    .lock()
                    .expect("Developer tools")
                    .allocation_failed();
                return Err(error);
            }
        };
        self.developer_tools
            .lock()
            .expect("Developer tools")
            .begin(registry, scope);
        let result = async {
            let metadata = self
                .tool_evaluate(
                    registry,
                    "registry=>registry.describe()".into(),
                    Value::Null,
                    false,
                    deadline,
                )
                .await?;
            self.developer_tools
                .lock()
                .expect("Developer tools")
                .finish(registry, &metadata)
        }
        .await;
        result.map_err(|error| tools::retained(error, registry))
    }
    pub(super) async fn release_developer_tools(
        &self,
        registry: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        {
            let owner = self.developer_tools.lock().expect("Developer tools");
            if owner.released(registry) {
                return Ok(json!({"registry":registry,"released":true}));
            }
            if !owner.contains(registry) {
                return Err("UNKNOWN_DEVELOPER_TOOLS".into());
            }
        }
        let live = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(registry, Some(Kind::Handle))
            .is_ok();
        if live {
            let result = async {
                self.tool_evaluate(
                    registry,
                    "registry=>registry.close()".into(),
                    Value::Null,
                    false,
                    deadline,
                )
                .await?;
                self.close_resource(registry, Kind::Handle, "dispose", deadline)
                    .await?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                return Err(tools::retained(error, registry));
            }
        }
        self.developer_tools
            .lock()
            .expect("Developer tools")
            .forget(registry);
        Ok(json!({"registry":registry,"released":true}))
    }
}
