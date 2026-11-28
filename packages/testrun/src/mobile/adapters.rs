//! Physical-runner and Android mutation adapters; domain validation remains in the parent.
use super::{Mutation, point, string};
use crate::{
    device::Runners,
    error::{Error, Result},
};
use serde_json::Value;

/// Observation-only checks run before the caller records native dispatch.
pub(crate) async fn preflight(mutation: &Mutation) -> Result<()> {
    if mutation.target.kind != "android" || mutation.operation != "type_text" {
        return Ok(());
    }
    let focused = crate::android::Android::default()
        .focused_editable(&mutation.target.platform_id)
        .await
        .map_err(|cause| {
            Error::new(
                "FOCUS_VERIFICATION_UNAVAILABLE",
                "Android focus inspection failed; no text input was dispatched",
            )
            .details(serde_json::json!({"stage":"focus_verification","inputDispatched":false,"cause":cause}))
        })?;
    if !focused {
        return Err(Error::new(
            "FOCUS_NOT_PROVEN",
            "Android does not report a focused editable field; no text input was dispatched",
        )
        .details(serde_json::json!({"stage":"focus_verification","inputDispatched":false})));
    }
    Ok(())
}

pub(crate) async fn perform(mutation: &Mutation, runners: &Runners) -> Result<Value> {
    let args = &mutation.arguments;
    let name = mutation.operation.as_str();
    let target = &mutation.target;
    let timeout = mutation.timeout;
    if target.kind == "device" {
        return physical(
            mutation,
            runners.runner(&target.platform_id),
            crate::app_lifecycle::device(name, args, &target.platform_id, timeout),
        )
        .await;
    }
    android(mutation).await
}

/// Inspection readiness must not select the backend of an app lifecycle command.
async fn physical(
    mutation: &Mutation,
    runner: Result<&crate::device::Runner>,
    lifecycle: impl std::future::Future<Output = Result<Value>>,
) -> Result<Value> {
    let args = &mutation.arguments;
    let name = mutation.operation.as_str();
    // CoreDevice does not wait for XCTest's animation quiescence. Keep
    // lifecycle dispatch independent of optional inspection setup.
    if matches!(name, "app_launch" | "app_restart" | "app_terminate") {
        return lifecycle.await;
    }
    let mut arguments = args.clone();
    if let Some(object) = arguments.as_object_mut() {
        for field in [
            "runId",
            "goalId",
            "sessionId",
            "leaseId",
            "commandId",
            "timeoutMs",
        ] {
            object.remove(field);
        }
    }
    // The owned runner names an element `elementRef` and an app `bundleId`.
    if let Some(element) = args.get("element") {
        arguments["snapshotId"] = element["snapshotId"].clone();
        arguments["elementRef"] = element["elementId"].clone();
        if let Some(object) = arguments.as_object_mut() {
            object.remove("element");
        }
    }
    if let Some(scope) = args.get("scope") {
        arguments["scope"] = super::runner_scope(scope)?;
    }
    runner?
        .call(
            name,
            arguments,
            Some(&mutation.command_id),
            mutation.timeout,
        )
        .await
        .map(|reply| reply.data)
}

async fn android(mutation: &Mutation) -> Result<Value> {
    let args = &mutation.arguments;
    let name = mutation.operation.as_str();
    let target = &mutation.target;
    let timeout = mutation.timeout;
    let adapter = crate::android::Android::default();
    let serial = &target.platform_id;
    let display: Option<crate::ui::Display> = args
        .get("display")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|_| Error::invalid("Invalid display reference"))?;
    match name {
        // Static admission refused launch arguments before this dispatch.
        "app_launch" | "app_restart" => {
            crate::app_lifecycle::android(name, args, serial, timeout).await
        }
        "app_terminate" => {
            adapter
                .terminate(serial, string(args, "appId")?, timeout)
                .await
        }
        "open_url" => adapter.open_url(serial, super::url(args)?, timeout).await,
        "tap" => {
            adapter
                .tap(serial, point(args, "point")?, display.as_ref(), timeout)
                .await
        }
        "long_press" => {
            let p = point(args, "point")?;
            adapter
                .swipe(
                    serial,
                    [p, p],
                    args["durationMs"].as_u64().unwrap_or(500),
                    display.as_ref(),
                    timeout,
                )
                .await
        }
        "swipe" => {
            adapter
                .swipe(
                    serial,
                    [point(args, "from")?, point(args, "to")?],
                    args["durationMs"].as_u64().unwrap_or(300),
                    display.as_ref(),
                    timeout,
                )
                .await
        }
        // Chords were admitted only after this session probed keycombination.
        "key" => {
            let modifiers: Vec<String> = args
                .get("modifiers")
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()
                .map_err(|_| Error::invalid("Invalid key modifiers"))?
                .unwrap_or_default();
            adapter
                .key(serial, string(args, "key")?, &modifiers, timeout)
                .await
        }
        "type_text" if args["replace"] == true => {
            adapter
                .replace_text(
                    serial,
                    &args["textProof"],
                    args["text"].as_str().unwrap_or(""),
                    timeout,
                )
                .await
        }
        "type_text" => {
            adapter
                .type_text(serial, string(args, "text")?, timeout)
                .await
        }
        _ => Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Operation is unavailable for this Android target",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::{Target, TargetRef};
    use serde_json::json;
    use std::time::Duration;

    #[tokio::test]
    async fn physical_lifecycle_uses_coredevice_with_or_without_a_runner_and_never_falls_back() {
        let runner = crate::device::Runner::fixture();
        for operation in ["app_launch", "app_restart", "app_terminate"] {
            for connected in [false, true] {
                for failed in [false, true] {
                    let mutation = Mutation {
                        operation: operation.into(),
                        arguments: json!({"appId":"com.example.acme"}),
                        target: Target {
                            identity: TargetRef {
                                target_id: "device:Acme".into(),
                                generation: "generation".into(),
                            },
                            name: "Acme".into(),
                            kind: "device".into(),
                            state: "Connected".into(),
                            runtime: "27".into(),
                            metadata: Default::default(),
                            capabilities: Default::default(),
                            platform_id: "Acme".into(),
                            usb_id: None,
                            epoch: "epoch".into(),
                        },
                        command_id: "once".into(),
                        timeout: Duration::from_millis(50),
                    };
                    let inspection = if connected {
                        Ok(&runner)
                    } else {
                        Err(Error::new("RUNNER_SETUP_REQUIRED", "No inspection runner"))
                    };
                    let mut calls = 0;
                    let result = physical(&mutation, inspection, async {
                        calls += 1;
                        if failed {
                            Err(Error::new("TIMEOUT", "Fixture launch deadline expired"))
                        } else {
                            Ok(json!({"launch":{"backend":"coredevice"}}))
                        }
                    })
                    .await;
                    assert_eq!(calls, 1);
                    if failed {
                        assert_eq!(result.unwrap_err().code, "TIMEOUT");
                    } else {
                        assert_eq!(result.unwrap()["launch"]["backend"], "coredevice");
                    }
                }
            }
        }
    }
}
