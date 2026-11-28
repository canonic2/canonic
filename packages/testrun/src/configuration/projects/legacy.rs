//! Read-only normalization of historical flat build profiles.
use super::*;
pub(super) fn normalize(value: &Value) -> Value {
    let mut project = json!({"root":".","kind":"native","profiles":{},"tasks":{},"checks":{},"services":{},"applications":{}});
    for (index, (name, profile)) in value["builds"]
        .as_object()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let mut build = profile.clone();
        let map = build.as_object_mut().unwrap();
        map.remove("platform");
        map.remove("target_kind");
        map.remove("development");
        project["profiles"][name] = json!({"platform":profile["platform"],"target_kind":profile["target_kind"],"build":build});
        if let Some(development) = profile.get("development") {
            let id = format!("p{index}");
            project["applications"][&id] = json!({});
            project["applications"][&id][profile["platform"].as_str().unwrap()] =
                json!({"app_id":development["app_id"]});
            if let Some(variant) = development.get("variant").filter(|v| !v.is_null()) {
                project["applications"][&id][profile["platform"].as_str().unwrap()]["options"] =
                    json!({"legacyVariant":variant});
            }
            project["profiles"][name]["application"] = json!(id);
            let command = |mut task: Value| {
                for field in ["env", "inherit_env"] {
                    if let Some(value) = profile.get(field) {
                        task[field] = value.clone();
                    }
                }
                task
            };
            for stage in ["preflight", "prepare"] {
                if let Some(task) = development.get(stage).filter(|value| !value.is_null()) {
                    let task_id = format!("{id}-{stage}");
                    project["tasks"][&task_id] = command(task.clone());
                    project["profiles"][name][stage] = json!([task_id]);
                }
            }
            for (check_index, (alias, task)) in development["checks"]
                .as_object()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let check_id = format!("{id}-check-{check_index}");
                project["checks"][&check_id] = command(task.clone());
                if project["profiles"][name].get("checks").is_none() {
                    project["profiles"][name]["checks"] = json!({});
                }
                project["profiles"][name]["checks"][alias] = json!(check_id);
            }
            if let Some(expo) = development.get("expo").filter(|value| !value.is_null()) {
                let service_id = format!("{id}-expo");
                let mut service = json!({"adapter":"expo","url":expo["url"],"execution":{"kind":"external"},"expo":{"slug":expo["slug"]}});
                for key in ["connection_url", "connection_mode"] {
                    if let Some(value) = expo.get(key).filter(|v| !v.is_null()) {
                        service["expo"][key] = value.clone();
                    }
                }
                if let Some(owner) = expo.get("owner").filter(|v| !v.is_null()) {
                    service["execution"]["reference"] = owner.clone();
                }
                if let Some(task) = development.get("server").filter(|v| !v.is_null()) {
                    service["execution"] = json!({"kind":"command","run":command(task.clone())});
                }
                project["services"][&service_id] = service;
                project["profiles"][name]["services"] = json!([service_id]);
            } else if let Some(task) = development.get("server").filter(|v| !v.is_null()) {
                let service_id = format!("{id}-server");
                project["services"][&service_id] = json!({"adapter":"custom","execution":{"kind":"command","run":command(task.clone())}});
                project["profiles"][name]["services"] = json!([service_id]);
            }
        }
    }
    json!({"version":2,"projects":{"legacy":project}})
}
