//! Composition of browser workflows with the manager's pool. Each browser
//! connection is a `browser:<connectionId>` target in the pool, beside the
//! devices and other browsers, usable by every client.
use super::*;

impl Caller {
    pub(super) async fn browser_call(&mut self, name: &str, mut args: Value) -> Result<Value> {
        crate::browser::surface::validate(name, &args)?;
        if matches!(name, "browser_list" | "browser_doctor") {
            return crate::browser::runtime::inspect(name, args);
        }
        let target = args
            .as_object_mut()
            .and_then(|args| args.remove("targetId"))
            .as_ref()
            .and_then(Value::as_str)
            .map(crate::browser::runtime::connection)
            .transpose()?;
        let (Some(service), Some(id)) = (self.service.as_ref(), self.pool.as_deref()) else {
            if let Some(target) = target {
                args["targetId"] = json!(crate::browser::runtime::target(target));
            }
            return crate::browser::runtime::offline(
                self.pool_records()?,
                self.service.is_some(),
                &args,
            )
            .await;
        };
        let (generation, directory) = service.browser_identity()?;
        if name == "browser_connect" {
            let options: crate::browser::contract::ConnectOptions =
                serde_json::from_value(args).map_err(|e| Error::invalid(e.to_string()))?;
            options
                .validate()
                .map_err(crate::browser::runtime::launch_options_error)?;
            // Each connect starts another browser, a target of its own.
            let connection = uuid::Uuid::new_v4();
            let target = crate::browser::runtime::target(connection);
            service.claim(&target)?;
            let result = service
                .browsers
                .connect(id, generation, &directory, connection, options)
                .await;
            if result.is_err() {
                service.release_target(&target);
            }
            return service.with_browser_record(id, &directory, result);
        }
        let result = service.browsers.call(id, name, args, target).await;
        if name == "browser_status" {
            return result;
        }
        service.with_browser_record(id, &directory, result)
    }
}
