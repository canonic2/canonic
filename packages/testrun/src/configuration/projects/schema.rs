//! Version-two structural validation and adapter-independent project schemas.
use super::super::schema::{env, object, strings, text};
use super::*;
fn id() -> Value {
    json!({"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)?$"})
}
fn map(item: Value, max: usize) -> Value {
    json!({"type":"object","maxProperties":max,"additionalProperties":item})
}
fn extension() -> Value {
    object(
        json!({"adapter":id(),"options":{"type":"object"}}),
        &["adapter"],
    )
}
pub(crate) fn command(local: bool) -> Value {
    object(
        json!({"command":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"string","maxLength":16384}},"cwd":text(),"env":env(local),"inherit_env":strings(),"timeout_ms":{"type":"integer","minimum":1000,"maximum":3600000},"effects":strings(),"resources":strings(),"expected_output":text()}),
        if local { &[] } else { &["command"] },
    )
}
pub(crate) fn service(local: bool) -> Value {
    let mut execution = object(
        json!({"kind":{"enum":["command","external","box"]},"run":command(local),"reference":text(),"workspace":text(),"service":text()}),
        if local { &[] } else { &["kind"] },
    );
    if !local {
        execution["anyOf"] = json!([
            {"properties":{"kind":{"const":"command"}},"required":["run"]},
            {"properties":{"kind":{"const":"external"}}},
            {"properties":{"kind":{"const":"box"}},"required":["workspace","service"]}
        ]);
    }
    object(
        json!({"adapter":id(),"options":{"type":"object"},"execution":execution,"url":text(),"address":object(json!({"host":text(),"port":{"type":"integer","minimum":1,"maximum":65535}}),if local {&[]} else {&["host","port"]}),"expo":object(json!({"slug":text(),"application":text(),"connection_url":text(),"connection_mode":{"enum":["lan","tunnel","forwarded"]}}),if local {&[]} else {&["slug"]}),"metro":object(json!({"application":text(),"connection_url":text(),"connection_mode":{"enum":["lan","tunnel","forwarded"]}}),&[]),"readiness":object(json!({"path":text(),"status":{"type":"integer","minimum":100,"maximum":599},"identity":object(json!({"pointer":{"type":"string","maxLength":4096},"equals":{"type":["string","number","boolean","null"]}}),&["pointer","equals"])}),&[]),"depends_on":strings(),"lifetime_ms":{"type":"integer","minimum":1000,"maximum":3600000}}),
        if local {
            &[]
        } else {
            &["adapter", "execution"]
        },
    )
}
fn build(local: bool) -> Value {
    let mut schema = super::super::schema::profile(local);
    schema.as_object_mut().unwrap().remove("anyOf");
    let properties = schema["properties"].as_object_mut().unwrap();
    for key in ["platform", "target_kind", "development"] {
        properties.remove(key);
    }
    properties.insert("builder".into(), id());
    properties.insert("resources".into(), strings());
    properties.insert("options".into(), json!({"type":"object"}));
    let artifact = properties.get_mut("artifact").unwrap();
    artifact.as_object_mut().unwrap().remove("maxProperties");
    artifact["properties"]["kind"] = id();
    artifact["properties"]["validator"] = extension();
    if !local {
        artifact["anyOf"] = json!([{"required":["path"]},{"required":["glob"]}]);
    }
    schema["required"] = if local { json!([]) } else { json!(["builder"]) };
    schema
}
fn profile(local: bool) -> Value {
    object(
        json!({"platform":id(),"target_kind":id(),"driver":extension(),"application":text(),"build":build(local),"prepare":strings(),"preflight":strings(),"checks":map(text(),128),"services":{"type":"array","maxItems":32,"items":text()},"open":object(json!({"kind":id(),"service":text(),"path":text(),"url":text(),"engine":id(),"mode":{"enum":["headless","headed"]},"context":{"type":"object"},"options":{"type":"object"}}),if local {&[]} else {&["kind"]})}),
        if local {
            &[]
        } else {
            &["platform", "target_kind"]
        },
    )
}
pub(crate) fn project(local: bool) -> Value {
    let identity = object(
        json!({"app_id":text(),"version":text(),"build":text(),"executable":text(),"package":text(),"url":text(),"service":text(),"adapter":id(),"options":{"type":"object"}}),
        &[],
    );
    let mut kind = id();
    kind["examples"] = json!(super::CORE_KINDS);
    object(
        json!({"root":text(),"kind":kind,"options":{"type":"object"},"applications":map(map(identity,128),128),"tasks":map(command(local),128),"checks":map(command(local),128),"services":map(service(local),128),"profiles":map(profile(local),128)}),
        if local { &[] } else { &["root", "kind"] },
    )
}
pub(crate) fn document(local: bool) -> Value {
    object(
        json!({"version":{"const":2},"projects":map(project(local),128),"services":map(service(local),128),"resources":map(object(json!({"path":text()}),if local {&[]} else {&["path"]}),128),"compatibility":object(json!({"v1":object(json!({"project":text(),"profiles":map(text(),128)}),&["project","profiles"])}),&[])}),
        &["version"],
    )
}
pub(crate) fn validate_root(value: &Value, local: bool) -> Result<()> {
    let mut schema = document(local);
    schema["properties"]["projects"] = map(json!({"type":"object"}), 128);
    canonic_mcp_kit::schema::validate(&schema, value)
        .map_err(|e| super::super::provenance::argument(e, value))?;
    let mut profiles = 0;
    for group in ["services", "resources"] {
        for name in value[group]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, _)| name)
        {
            if !super::name(name) {
                return Err(Error::new("CONFIG_INVALID", "Invalid shared object name")
                    .details(json!({"pointer":format!("/{group}/{name}")})));
            }
        }
    }
    for resource in value["resources"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, resource)| resource)
    {
        if let Some(path) = resource["path"].as_str() {
            super::super::path(path)?;
        }
    }
    let mut services = value["services"].as_object().map_or(0, |v| v.len());
    for (name, project) in value["projects"].as_object().into_iter().flatten() {
        if !super::name(name) {
            return Err(Error::new("CONFIG_INVALID", "Invalid project name")
                .details(json!({"pointer":format!("/projects/{name}")})));
        }
        profiles += project["profiles"].as_object().map_or(0, |v| v.len());
        services += project["services"].as_object().map_or(0, |v| v.len());
    }
    if profiles > 1024 || services > 256 {
        return Err(Error::new(
            "CONFIG_INVALID",
            "Configuration exceeds total profile/service bounds",
        ));
    }
    Ok(())
}
