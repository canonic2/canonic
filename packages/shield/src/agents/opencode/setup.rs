//! Portable owned plugin bytes and the V1 MCP entry; other plugins stay owned
//! by the host/user. Runtime V2 configuration is not interpreted as V1.
use crate::{agents::setup::HookGroups, review::Error};
use serde_json::{Value, json};

pub(super) fn plugin() -> String {
    let transport = include_str!("../../../adapters/src/modules/process-transport/index.mjs")
        .replace(
            "export async function transport",
            "async function transport",
        );
    let json = include_str!("../../../adapters/src/modules/enforcement-response/json.mjs")
        .replace("export function parseJSON", "function parseJSON");
    let plugin = include_str!("../../../adapters/src/modules/opencode/index.mjs")
        .replace(
            "import { transport } from '../process-transport/index.mjs';\n",
            "",
        )
        .replace(
            "import { parseJSON } from '../enforcement-response/json.mjs';\n",
            "",
        );
    format!("// Shield owned OpenCode V1 plugin; protocol 1.\n{transport}\n{json}\n{plugin}")
}

fn source(groups: &HookGroups) -> Result<Option<&str>, Error> {
    let mut source = None;
    for group in groups.values() {
        let text = group["source"]
            .as_str()
            .filter(|_| group["protocol_version"] == 1)
            .ok_or(Error::Invalid("invalid owned OpenCode plugin"))?;
        if source.is_some_and(|source| source != text) {
            return Err(Error::Invalid("inconsistent owned OpenCode plugin"));
        }
        source = Some(text);
    }
    Ok(source)
}

pub(super) fn capture(actual: Option<&str>, expected: &HookGroups) -> Result<HookGroups, Error> {
    let actual = actual.filter(|source| !source.trim().is_empty());
    if actual.is_none() {
        return Ok(HookGroups::new());
    }
    if actual != source(expected)? {
        return Err(Error::Invalid(
            "OpenCode Shield plugin is unowned or manually changed",
        ));
    }
    Ok(expected.clone())
}

pub(super) fn merge(
    actual: Option<&str>,
    old: &HookGroups,
    new: &HookGroups,
) -> Result<String, Error> {
    capture(actual, old)?;
    Ok(source(new)?.unwrap_or_default().into())
}

fn document(source: Option<&str>) -> Result<Value, Error> {
    let value = super::super::setup::parse(source)?;
    if !value.is_object()
        || value
            .get("mcp")
            .is_some_and(|value| !value.is_object() || value.get("servers").is_some())
        || value.get("plugins").is_some()
    {
        return Err(Error::Invalid(
            "OpenCode V1 requires an object mcp map; V2 plugins configuration is unsupported",
        ));
    }
    Ok(value)
}
pub(super) fn has_mcp(source: &str) -> Result<bool, Error> {
    Ok(document(Some(source))?["mcp"].get("shield").is_some())
}
pub(super) fn merge_mcp(
    actual: Option<&str>,
    old: Option<&str>,
    new: Option<&str>,
) -> Result<String, Error> {
    let mut value = document(actual)?;
    let decode = |source: &str| {
        crate::json::parse(source.as_bytes())
            .map_err(|_| Error::Invalid("invalid owned OpenCode MCP entry"))
    };
    let old = old.map(decode).transpose()?;
    let new = new.map(decode).transpose()?;
    let map = value
        .as_object_mut()
        .expect("checked object")
        .entry("mcp")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("checked mcp map");
    if map
        .get("shield")
        .is_some_and(|actual| Some(actual) != old.as_ref())
    {
        return Err(Error::Invalid(
            "OpenCode Shield MCP entry is unowned or manually changed",
        ));
    }
    map.remove("shield");
    if let Some(new) = new {
        map.insert("shield".into(), new);
    }
    if map.is_empty() {
        value.as_object_mut().expect("checked object").remove("mcp");
    }
    serde_json::to_string_pretty(&value)
        .map(|value| format!("{value}\n"))
        .map_err(|_| Error::Invalid("OpenCode MCP encoding"))
}
