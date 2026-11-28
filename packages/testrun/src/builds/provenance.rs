//! Per-field and environment provenance without retaining resolved secret values.
use super::*;
pub(super) fn complete(request: &Request, profile: &Profile, source: &mut Value) {
    use crate::configuration::provenance::{escape, fields};
    let mut origins = Env::new();
    fields(
        &serde_json::to_value(profile).unwrap(),
        "",
        "default",
        &mut origins,
        false,
    );
    if let Some(inline) = &request.inline {
        fields(inline, "", "inline", &mut origins, true);
    }
    if let Some(name) = &request.profile
        && let Some(values) = source["provenance"].as_object()
    {
        let prefix = if request.root.is_some() && source["sourceVersion"] == 2 {
            format!(
                "/projects/{}/profiles/{name}/build",
                request.project.display()
            )
        } else {
            format!("/builds/{name}")
        };
        for (pointer, label) in values {
            if let Some(pointer) = pointer.strip_prefix(&prefix)
                && let Some(label) = label.as_str()
            {
                origins.insert(pointer.into(), label.into());
            }
        }
        if request.root.is_some() && source["sourceVersion"] == 2 {
            let outer = format!("/projects/{}/profiles/{name}", request.project.display());
            for field in ["platform", "target_kind"] {
                if let Some(label) = values
                    .get(&format!("{outer}/{field}"))
                    .and_then(Value::as_str)
                {
                    origins.insert(format!("/{field}"), label.into());
                }
            }
            if let Some(application) = profile
                .development
                .as_ref()
                .and_then(|development| development.variant.as_deref())
            {
                let pointer = format!(
                    "/projects/{}/applications/{application}/{}/app_id",
                    request.project.display(),
                    profile.platform
                );
                if let Some(label) = values.get(&pointer).and_then(Value::as_str) {
                    origins.insert("/development/app_id".into(), label.into());
                }
            }
        }
    }
    override_origins(request, profile, &mut origins);
    let mut build = Env::new();
    let mut launch = Env::new();
    for key in request.baseline.keys() {
        build.insert(key.clone(), "client.baseline".into());
    }
    for key in &profile.inherit_env {
        build.insert(key.clone(), format!("client.bindings.{key}"));
    }
    for key in profile.env.keys() {
        build.insert(
            key.clone(),
            origins
                .get(&format!("/env/{}", escape(key)))
                .cloned()
                .unwrap_or_else(|| "profile.env".into()),
        );
    }
    for key in request.env.keys() {
        build.insert(key.clone(), "invocation.env".into());
    }
    if request.launch {
        for key in &profile.launch.inherit_env {
            launch.insert(key.clone(), format!("client.bindings.{key}"));
        }
        for key in profile.launch.env.keys() {
            launch.insert(
                key.clone(),
                origins
                    .get(&format!("/launch/env/{}", escape(key)))
                    .cloned()
                    .unwrap_or_else(|| "profile.launch.env".into()),
            );
        }
    }
    source["effectiveProvenance"] = json!(origins);
    source["environmentProvenance"] = json!({"build":build,"launch":launch});
}

fn override_origins(request: &Request, profile: &Profile, origins: &mut Env) {
    use crate::configuration::provenance::escape;
    let overrides = &request.overrides;
    for (field, present) in [
        (
            format!(
                "/{}/configuration",
                if profile.builder == "xcode" {
                    "xcode"
                } else {
                    "react-native"
                }
            ),
            overrides.configuration.is_some(),
        ),
        ("/react-native/variant".into(), overrides.variant.is_some()),
        ("/flutter/flavor".into(), overrides.flavor.is_some()),
        (
            "/flutter/entry_point".into(),
            overrides.entry_point.is_some(),
        ),
        ("/flutter/mode".into(), overrides.mode.is_some()),
        ("/timeout_ms".into(), overrides.timeout_ms.is_some()),
        ("/launch/args".into(), overrides.launch_args.is_some()),
    ] {
        if present {
            origins.insert(field, "invocation.overrides".into());
        }
    }
    for key in overrides.launch_env.keys() {
        origins.insert(
            format!("/launch/env/{}", escape(key)),
            "invocation.overrides.launch_env".into(),
        );
    }
}
