//! Adapter-owned hook and MCP configuration reconciliation.
use super::*;

#[cfg(not(windows))]
#[test]
fn portable_hooks_accept_exact_legacy_box_callbacks_but_reject_duplicates_and_other_roots() {
    let root = assert_fs::TempDir::new().unwrap();
    let old = assert_fs::TempDir::new().unwrap();
    let binary = std::env::current_exe().unwrap();
    for agent in ["claude", "codex"] {
        let adapter = lookup(agent).unwrap();
        let portable = installation(adapter, &binary, root.path(), &[])
            .unwrap()
            .groups;
        let historical: HookGroups = adapter
            .spec()
            .events
            .iter()
            .map(|(event, _)| {
                let command = posix_hook_command(
                    &quoted(&binary).unwrap(),
                    &quoted(old.path()).unwrap(),
                    agent,
                    event,
                );
                ((*event).into(), adapter.hook_group(&command))
            })
            .collect();
        let source = adapter
            .merge_hooks(None, &HookGroups::new(), &historical)
            .unwrap();
        assert!(
            published_groups(
                adapter,
                &binary,
                root.path(),
                &[],
                Some(&source),
                portable.clone()
            )
            .is_err()
        );
        assert_eq!(
            published_groups(
                adapter,
                &binary,
                root.path(),
                &[old.path()],
                Some(&source),
                portable.clone()
            )
            .unwrap(),
            historical
        );
        assert_eq!(
            published_groups(
                adapter,
                &binary,
                old.path(),
                &[],
                Some(&source),
                portable.clone()
            )
            .unwrap(),
            historical
        );
        let mut duplicate: Value = serde_json::from_str(&source).unwrap();
        duplicate["hooks"]["PreToolUse"]
            .as_array_mut()
            .unwrap()
            .push(portable["PreToolUse"].clone());
        assert!(
            published_groups(
                adapter,
                &binary,
                root.path(),
                &[old.path()],
                Some(&duplicate.to_string()),
                portable
            )
            .is_err()
        );
    }
}

#[test]
fn historical_native_profiles_keep_actual_coverage_without_requiring_advisory_upgrade() {
    let root = assert_fs::TempDir::new().unwrap();
    let binary = std::env::current_exe().unwrap();
    for (agent, events) in [
        (
            "claude",
            vec!["PreToolUse", "PostToolUse", "PostToolUseFailure"],
        ),
        ("codex", vec!["PreToolUse", "PostToolUse"]),
        (
            "codex",
            vec!["PreToolUse", "PermissionRequest", "PostToolUse"],
        ),
    ] {
        let adapter = lookup(agent).unwrap();
        let groups = events
            .iter()
            .map(|event| {
                let command = hook_command(root.path(), agent, event).unwrap();
                ((*event).into(), adapter.hook_group(&command))
            })
            .collect();
        let installed = Installation {
            groups,
            mcp: None,
            settings: None,
        };
        assert!(compatible_historical_hooks(
            root.path(),
            adapter,
            &binary,
            &installed
        ));
        assert!(!installed.groups.contains_key("Stop"));
        let mut incomplete = installed.clone();
        incomplete.groups.remove("PreToolUse");
        assert!(!compatible_historical_hooks(
            root.path(),
            adapter,
            &binary,
            &incomplete
        ));
        #[cfg(not(windows))]
        {
            let mut installed = installed;
            let command = format!(
                "{} __internal native-hook --project {} --agent {agent} || exit 2",
                quoted(&binary).unwrap(),
                quoted(root.path()).unwrap()
            );
            for value in installed.groups.values_mut() {
                *value = adapter.hook_group(&command);
            }
            assert!(compatible_historical_hooks(
                root.path(),
                adapter,
                &binary,
                &installed
            ));
            installed.groups.get_mut("PreToolUse").unwrap()["hooks"][0]["command"] =
                serde_json::json!("acme-unowned-command");
            assert!(!compatible_historical_hooks(
                root.path(),
                adapter,
                &binary,
                &installed
            ));
        }
    }
}
use crate::agents::{AgentSpec, Event, Reply, Request};
use crate::enforcement::protocol::NativeApproval;
use serde_json::{Value, json};
use std::fs;
use toml_edit::{DocumentMut, value};

#[test]
fn windows_hook_paths_remain_literal_inside_an_encoded_command() {
    use base64::Engine as _;
    let binary = Path::new(r"C:\Acme tools\shield.exe");
    let project = Path::new(r"C:\Acme's $HOME & %TEMP%\project");
    let command = windows_command(
        Path::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
        binary,
        project,
        Some("policy/acme's shield.yml"),
        "codex",
        "PreToolUse",
    )
    .unwrap();
    assert!(command.starts_with("C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe "));
    assert!(!command.contains("Acme"));
    let (_, encoded) = command.split_once(" -EncodedCommand ").unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let utf16: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes(*bytes))
        .collect();
    let script = String::from_utf16(&utf16).unwrap();
    assert!(script.starts_with("& 'C:\\Acme tools\\shield.exe' __internal native-hook "));
    assert!(script.contains(
        "--project 'C:\\Acme''s $HOME & %TEMP%\\project' --config 'policy/acme''s shield.yml'"
    ));
    assert!(script.contains("--agent 'codex' --event 'PreToolUse'"));
    assert!(script.ends_with("if ($LASTEXITCODE -ne 0) { exit 2 }"));
}

#[test]
fn windows_hooks_refuse_shell_lookup_and_control_characters_in_paths() {
    let binary = Path::new(r"C:\Tools\shield.exe");
    let project = Path::new(r"C:\Acme");
    for shell in [
        "",
        "powershell.exe",
        "C:\\Windows tools\\powershell.exe",
        "C:\\shell&other.exe",
        "C:\\shell%PATH%.exe",
    ] {
        assert!(
            windows_command(
                Path::new(shell),
                binary,
                project,
                None,
                "codex",
                "PreToolUse"
            )
            .is_err()
        );
    }
    assert!(
        windows_command(
            Path::new(r"C:\Windows\powershell.exe"),
            binary,
            Path::new("C:\\Acme\nother"),
            None,
            "codex",
            "PreToolUse",
        )
        .is_err()
    );
    assert!(
        windows_command(
            Path::new(r"C:\Windows\powershell.exe"),
            binary,
            Path::new(&format!(r"C:\{}", "a".repeat(4096))),
            None,
            "codex",
            "PreToolUse"
        )
        .is_err()
    );
}

// Deliberately uses neither shipping hook schema and stores hooks and MCP in
// one TOML file. The shared workflow must not parse or independently write it.
struct Acme;
static SPEC: AgentSpec = AgentSpec {
    id: "acme",
    label: "Acme",
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
    let installed = prepare_adapter(
        root.path(),
        &Acme,
        None,
        Some(binary),
        &[],
        &mut changes,
        false,
    )
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
        false,
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
        false,
    )
    .unwrap();
    assert_eq!(removal.len(), 1);
    files::publish(root.path(), &removal).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    // A second component's refusal must leave the file untouched.
    fs::write(&path, "theme = \"acme\"\nshield_server = \"user-server\"\n").unwrap();
    let before = fs::read_to_string(&path).unwrap();
    assert!(
        prepare_adapter(
            root.path(),
            &Acme,
            None,
            Some(binary),
            &[],
            &mut Vec::new(),
            false
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn google_adapters_install_repair_remove_preserve_settings_and_shared_files() {
    for agent in ["gemini", "agy"] {
        let root = assert_fs::TempDir::new().unwrap();
        let adapter = crate::agents::find(agent).unwrap();
        let path = root.path().join(adapter.spec().hooks_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = if agent == "gemini" {
            json!({"ui":{"theme":"acme"},"mcpServers":{"acme":{"command":"acme-mcp"}}})
        } else {
            json!({"acme-audit":{"PreToolUse":[{"matcher":"*","hooks":[{"command":"acme-audit"}]}]}})
        };
        fs::write(&path, original.to_string()).unwrap();
        let binary = std::env::current_exe().unwrap();
        let mut changes = Vec::new();
        let installed = prepare_adapter(
            root.path(),
            adapter,
            None,
            Some(&binary),
            &[],
            &mut changes,
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(changes.len(), if agent == "gemini" { 1 } else { 2 });
        files::publish(root.path(), &changes).unwrap();
        let source = fs::read_to_string(&path).unwrap();
        let mut repair = Vec::new();
        prepare_adapter(
            root.path(),
            adapter,
            Some(&installed),
            Some(&binary),
            &[],
            &mut repair,
            false,
        )
        .unwrap();
        files::publish(root.path(), &repair).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        let mut removal = Vec::new();
        prepare_adapter(
            root.path(),
            adapter,
            Some(&installed),
            None,
            &[],
            &mut removal,
            false,
        )
        .unwrap();
        files::publish(root.path(), &removal).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(&path).unwrap()).unwrap(),
            original
        );
        let mcp_path = root.path().join(adapter.spec().mcp_path.unwrap());
        if agent == "agy" {
            assert!(
                !mcp_path.exists(),
                "Remove the empty, Shield-created MCP file"
            );
        } else {
            let mcp = fs::read_to_string(mcp_path).unwrap();
            assert!(!adapter.has_mcp_entry(&mcp).unwrap());
        }
    }
}
