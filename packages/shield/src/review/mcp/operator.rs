//! Operator adapters over the same embedded docs and immutable diagnostics as MCP.
use super::{Context, Error, diagnostics, documentation, views::Views};
use serde_json::{Value, json};
use std::{fs::File, io::Read, path::Path};

pub(crate) fn read_json(path: &Path) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::Invalid("JSON input exceeds 1 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("invalid JSON input"))
}

pub(crate) fn docs(name: &str, args: &Value) -> Result<Value, Error> {
    documentation::call(name, args, &|| Ok(())).map_err(|error| match error {
        canonic_mcp_kit::docs::DocsError::NotFound { .. } => Error::Missing,
        canonic_mcp_kit::docs::DocsError::Cancelled => Error::Cancelled,
        _ => Error::Invalid("invalid documentation request or unavailable documentation bundle"),
    })
}

#[derive(Clone, Copy)]
pub(crate) enum Diagnostic<'a> {
    Evaluate {
        input: &'a Path,
        proposal_id: Option<&'a str>,
    },
    Compare {
        before: &'a str,
        after: &'a str,
        domain: &'a str,
        input: Option<&'a Path>,
    },
}

pub(crate) fn diagnostic(context: &Context, command: Diagnostic<'_>) -> Result<Value, Error> {
    let mut views = Views::new("operator".into());
    let (name, mut args) = match command {
        Diagnostic::Evaluate { input, proposal_id } => {
            let mut args = read_json(input)?;
            super::fields(&args, &["input", "context"])?;
            let selector =
                proposal_id.map_or_else(|| "active".into(), |id| format!("proposal:{id}"));
            args["candidate_id"] = json!(capture(context, &mut views, &selector)?);
            ("policy_evaluate", args)
        }
        Diagnostic::Compare {
            before,
            after,
            domain,
            input,
        } => {
            let mut args = input.map_or_else(|| Ok(json!({})), read_json)?;
            super::fields(&args, &["input", "context"])?;
            args["before_candidate_id"] = json!(capture(context, &mut views, before)?);
            args["after_candidate_id"] = json!(capture(context, &mut views, after)?);
            args["domain"] = json!(domain);
            ("policy_compare", args)
        }
    };
    let result = if name == "policy_evaluate" {
        diagnostics::evaluate(context, &mut views, &args, &|| Ok(()))?
    } else {
        diagnostics::compare(context, &mut views, &args, &|| Ok(()))?
    };
    let (mut result, redacted) = result.into_parts();
    if redacted {
        result["redacted"] = json!(true);
    }
    // Handles are invocation-local evidence; never suggest they are reusable selectors.
    for key in ["candidate_id", "before_candidate_id", "after_candidate_id"] {
        if let Some(object) = result.as_object_mut() {
            object.remove(key);
        }
        if let Some(object) = args.as_object_mut() {
            object.remove(key);
        }
    }
    result["authorizes_execution"] = json!(false);
    Ok(result)
}

fn capture(context: &Context, views: &mut Views, selector: &str) -> Result<String, Error> {
    if selector == "active" {
        return views.active(context);
    }
    let id = selector.strip_prefix("proposal:").ok_or(Error::Invalid(
        "candidate must be active, proposal:ID, or proposal:ID:before",
    ))?;
    let (id, before) = id
        .strip_suffix(":before")
        .map_or((id, false), |id| (id, true));
    let state = crate::review::project::load(context)?;
    let row = state
        .proposals
        .iter()
        .find(|row| row.id == id)
        .ok_or(Error::Missing)?;
    let (candidate, metadata) = if before {
        (&row.base_candidate, &row.base_metadata)
    } else {
        (&row.candidate, &row.candidate_metadata)
    };
    let source = crate::review::storage::Store::open(Path::new(&context.project_root))?
        .captured_candidate(candidate)?;
    views.retain(context, &source, metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_docs_cli_shares_bundle_defaults_and_bounds_without_enrollment() {
        let result = docs("docs_list", &json!({})).unwrap();
        assert_eq!(
            result,
            documentation::call("docs_list", &json!({}), &|| Ok(())).unwrap()
        );
        let path = result["documents"][0]["path"].as_str().unwrap();
        assert_eq!(
            docs("docs_read", &json!({"path":path})).unwrap(),
            documentation::call("docs_read", &json!({"path":path}), &|| Ok(())).unwrap()
        );
        assert!(
            docs(
                "docs_read",
                &json!({"path":"../README.md","start_line":1,"limit":10})
            )
            .is_err()
        );
        assert!(docs("docs_list", &json!({"offset":0,"limit":101})).is_err());
    }

    #[test]
    fn parity_cli_diagnostics_are_read_only_and_use_durable_selectors() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None)
                .unwrap();
        let context = super::super::initialize_fixture(root.path(), &bundle);
        let path = root.path().join("request.json");
        std::fs::write(&path, json!({"input":{"kind":"argv","executable":"acme","arguments":["read"],"original_input":{}},"context":{"cwd":context.project_root}}).to_string()).unwrap();
        let result = diagnostic(
            &context,
            Diagnostic::Evaluate {
                input: &path,
                proposal_id: None,
            },
        )
        .unwrap();
        assert_eq!(result["authorizes_execution"], false);
        assert!(result.get("candidate_id").is_none());
        let compared = diagnostic(
            &context,
            Diagnostic::Compare {
                before: "active",
                after: "active",
                domain: "sample",
                input: Some(&path),
            },
        )
        .unwrap();
        assert_eq!(compared["witnesses"][0]["relation"], "equivalent");
        assert!(
            capture(
                &context,
                &mut Views::new("test".into()),
                "candidate-other-1"
            )
            .is_err()
        );
        assert!(
            crate::review::storage::Store::open(root.path())
                .unwrap()
                .read()
                .unwrap()
                .proposals
                .is_empty()
        );
    }
}
