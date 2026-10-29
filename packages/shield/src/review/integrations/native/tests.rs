//! Adapter-owned hook and MCP configuration reconciliation.
use super::*;
use crate::agents::{AgentSpec, Event, Reply, Request};
use crate::enforcement::protocol::NativeApproval;
use serde_json::{Value, json};
use std::fs;
use toml_edit::{DocumentMut, value};

// Deliberately uses neither shipping hook schema and stores hooks and MCP in
// one TOML file. The shared workflow must not parse or independently write it.
struct Acme;
static SPEC: AgentSpec = AgentSpec {
    id: "acme",
    label: "Acme",
    history_agent: "acme",
    events: &[("Before", Event::Before)],
    hooks_path: ".codex/config.toml",
    mcp_path: Some(".codex/config.toml"),
    settings_paths: &[],
    prefix_rules_path: None,
    needs_ask_fallback: false,
    approval_action: "Approve with Acme",
    approval_notes: false,
};
fn document(source: Option<&str>) -> Result<DocumentMut, Error> {
    source
        .unwrap_or("")
        .parse()
        .map_err(|_| Error::Invalid("invalid Acme TOML"))
}
fn groups(doc: &DocumentMut) -> HookGroups {
    doc.get("shield_hooks")
        .and_then(|v| v.as_str())
        .map(|s| serde_json::from_str(s).unwrap())
        .unwrap_or_default()
}
impl AgentAdapter for Acme {
    fn working_files(&self, _: &Value, _: &Path) -> crate::location::WorkingFiles {
        crate::location::WorkingFiles::default()
    }
    fn spec(&self) -> &'static AgentSpec {
        &SPEC
    }
    fn request<'a>(&self, _: &'a Value) -> Option<Request<'a>> {
        None
    }
    fn approval(&self, _: Event) -> NativeApproval {
        NativeApproval::Fallback
    }
    fn reply(&self, _: Event, _: &Reply) -> Value {
        json!({})
    }
    fn completion(&self, _: &str, _: Option<&str>) -> Value {
        json!({})
    }
    fn hook_group(&self, command: &str) -> Value {
        json!({"run":command})
    }
    fn capture_hooks(
        &self,
        source: Option<&str>,
        expected: &HookGroups,
    ) -> Result<HookGroups, Error> {
        let actual = groups(&document(source)?);
        if !actual.is_empty() && actual != *expected {
            return Err(Error::Invalid("Acme hook conflict"));
        }
        Ok(actual)
    }
    fn merge_hooks(
        &self,
        source: Option<&str>,
        previous: &HookGroups,
        desired: &HookGroups,
    ) -> Result<String, Error> {
        let mut doc = document(source)?;
        let actual = groups(&doc);
        if !actual.is_empty() && actual != *previous {
            return Err(Error::Invalid("Acme hook conflict"));
        }
        doc.remove("shield_hooks");
        if !desired.is_empty() {
            doc["shield_hooks"] = value(serde_json::to_string(desired).unwrap());
        }
        Ok(doc.to_string())
    }
    fn mcp_entry(&self, _: &Path, _: &Path, _: &[String]) -> String {
        "acme-server".into()
    }
    fn has_mcp_entry(&self, source: &str) -> Result<bool, Error> {
        Ok(document(Some(source))?.contains_key("shield_server"))
    }
    fn merge_mcp(
        &self,
        source: Option<&str>,
        previous: Option<&str>,
        desired: Option<&str>,
    ) -> Result<String, Error> {
        let mut doc = document(source)?;
        if let Some(actual) = doc.get("shield_server")
            && actual.as_str() != previous
        {
            return Err(Error::Invalid("Acme MCP conflict"));
        }
        doc.remove("shield_server");
        if let Some(desired) = desired {
            doc["shield_server"] = value(desired);
        }
        Ok(doc.to_string())
    }
    fn diagnose(
        &self,
        _: Option<&str>,
        _: &HookGroups,
        _: Option<&str>,
        _: &mut Vec<&'static str>,
    ) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn adapter_toml_codec_composes_shared_file_install_repair_and_removal() {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir(root.path().join(".codex")).unwrap();
    let path = root.path().join(SPEC.hooks_path);
    let original = "theme = \"acme\"\n";
    fs::write(&path, original).unwrap();
    let binary = Path::new("/opt/acme/shield");
    let mut changes = Vec::new();
    let installed = prepare_adapter(root.path(), &Acme, None, Some(binary), &[], &mut changes)
        .unwrap()
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].before.as_deref(), Some(original));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    files::publish(root.path(), &changes).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let doc = document(Some(&contents)).unwrap();
    assert_eq!(doc["theme"].as_str(), Some("acme"));
    assert_eq!(doc["shield_server"].as_str(), Some("acme-server"));
    assert_eq!(groups(&doc), installed.groups);
    assert_eq!(
        Acme.capture_hooks(Some(&contents), &installed.groups)
            .unwrap(),
        installed.groups
    );

    let mut repair = Vec::new();
    prepare_adapter(
        root.path(),
        &Acme,
        Some(&installed),
        Some(binary),
        &[],
        &mut repair,
    )
    .unwrap();
    assert_eq!(repair.len(), 1);
    files::publish(root.path(), &repair).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), contents);

    let mut removal = Vec::new();
    prepare_adapter(
        root.path(),
        &Acme,
        Some(&installed),
        None,
        &[],
        &mut removal,
    )
    .unwrap();
    assert_eq!(removal.len(), 1);
    files::publish(root.path(), &removal).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    // A second component's refusal must leave the file untouched.
    fs::write(&path, "theme = \"acme\"\nshield_server = \"user-server\"\n").unwrap();
    let before = fs::read_to_string(&path).unwrap();
    assert!(prepare_adapter(root.path(), &Acme, None, Some(binary), &[], &mut Vec::new()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}
