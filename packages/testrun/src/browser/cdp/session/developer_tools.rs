//! Main-world tool discovery, validation and invocation through fenced handles.
use super::*;
use crate::browser::{developer_tools as tools, evaluation::EvaluationValue};
impl Worker {
    pub(super) async fn tool_evaluate(
        &mut self,
        id: Uuid,
        scope: Reference,
        expression: String,
        argument: Value,
        retain: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.execute_evaluation(
            id,
            Request::Evaluate {
                scope,
                expression,
                is_function: Some(true),
                argument: EvaluationValue::Json { value: argument },
                return_handle: retain,
            },
            deadline,
        )
        .await
    }
    pub(super) async fn developer_tools_operation(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::DeveloperToolResultRead { handle } => {
                self.developer_result_read(id, handle, deadline).await
            }
            Request::DeveloperToolsDiscover { scope } => {
                self.discover_developer_tools(id, scope, deadline).await
            }
            Request::DeveloperToolCall {
                registry,
                tool_id,
                parameters,
                return_handle,
            } => {
                self.developer_tools.tool(registry, &tool_id)?;
                let input = json!({"toolId":tool_id,"parameters":parameters});
                let checked = self
                    .tool_evaluate(
                        id,
                        registry,
                        "(registry,input)=>registry.validate(input.toolId,input.parameters)".into(),
                        input.clone(),
                        false,
                        deadline,
                    )
                    .await?;
                let paths = tools::validation(&checked)?;
                let argument = self.developer_tools.argument(
                    registry,
                    &tool_id,
                    input["parameters"].clone(),
                    paths,
                )?;
                let mut result = self
                    .execute_evaluation(
                        id,
                        Request::Evaluate {
                            scope: registry,
                            expression:
                                "(registry,input)=>registry.invoke(input.toolId,input.parameters,input.elementPaths)"
                                    .into(),
                            is_function: Some(true),
                            argument,
                            return_handle,
                        },
                        deadline,
                    )
                    .await
                    .map_err(|error| tools::invocation_error(error, registry, &tool_id))?;
                result["registry"] = json!(registry);
                result["toolId"] = json!(tool_id);
                Ok(result)
            }
            Request::DeveloperToolsRelease { registry } => {
                self.release_developer_tools(id, registry, deadline).await
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn discover_developer_tools(
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
        for registry in self.developer_tools.references() {
            if !self.evaluation_handle_live(registry) {
                self.developer_tools.forget(registry);
            }
        }
        self.developer_tools.reserve()?;
        let expression = format!("async()=>({})({})", tools::DISCOVER, tools::VALIDATOR);
        let allocated = self
            .tool_evaluate(id, scope, expression, Value::Null, true, deadline)
            .await
            .and_then(|allocated| {
                serde_json::from_value(allocated["handle"].clone())
                    .map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))
            });
        let registry = match allocated {
            Ok(registry) => registry,
            Err(error) => {
                self.developer_tools.allocation_failed();
                return Err(error);
            }
        };
        self.developer_tools.begin(registry, scope);
        let result = async {
            let metadata = self
                .tool_evaluate(
                    id,
                    registry,
                    "registry=>registry.describe()".into(),
                    Value::Null,
                    false,
                    deadline,
                )
                .await?;
            self.developer_tools.finish(registry, &metadata)
        }
        .await;
        result.map_err(|error| tools::retained(error, registry))
    }
    pub(super) async fn release_developer_tools(
        &mut self,
        id: Uuid,
        registry: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.developer_tools.released(registry) {
            return Ok(json!({"registry":registry,"released":true}));
        }
        if !self.developer_tools.contains(registry) {
            return Err("UNKNOWN_DEVELOPER_TOOLS".into());
        }
        if self.evaluation_handle_live(registry) {
            let result = async {
                self.tool_evaluate(
                    id,
                    registry,
                    "registry=>registry.close()".into(),
                    Value::Null,
                    false,
                    deadline,
                )
                .await?;
                self.execute_evaluation(id, Request::HandleDispose { handle: registry }, deadline)
                    .await?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                return Err(tools::retained(error, registry));
            }
        }
        self.developer_tools.forget(registry);
        Ok(json!({"registry":registry,"released":true}))
    }
}
