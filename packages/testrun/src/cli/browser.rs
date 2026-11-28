//! CLI envelopes use the same browser catalog and owner as MCP.
use super::{Action, BrowserCommand, Mobile};
use crate::error::{Error, Result};
use serde_json::{Value, json};

pub(super) async fn execute(input: &Mobile) -> Option<Result<Value>> {
    let Action::Browser(command) = &input.command else {
        return None;
    };
    let (name, args) = match command {
        BrowserCommand::List(page) => ("browser_list", page.arguments()),
        BrowserCommand::Doctor { engine } => ("browser_doctor", json!({"engine":engine})),
        BrowserCommand::Connect {
            engine,
            mode,
            browser_connection,
            proxy,
            downloads,
            touch,
            media_fixtures,
        } => {
            let mut args = json!({"engine":engine,"mode":mode});
            media_fixtures.add_to(&mut args);
            browser_connection.add_to(&mut args);
            if let Some(proxy) = proxy {
                match serde_json::from_str::<Value>(proxy) {
                    Ok(proxy) => args["proxy"] = proxy,
                    Err(_) => return Some(Err(Error::invalid("--proxy must be JSON"))),
                }
            }
            if *downloads || *touch {
                args["context"] = json!({"viewport":{"width":1280,"height":720},"deviceScaleFactor":1,"acceptDownloads":downloads,"hasTouch":touch});
            }
            ("browser_connect", args)
        }
        BrowserCommand::FixtureRegister {
            path,
            mime_type,
            provenance,
            cleared_by,
            clearance_method,
        } => {
            use base64::Engine as _;
            // Refused before the file is read.
            if let Err(error) = browser_target(input) {
                return Some(Err(error));
            }
            let bytes = match load_fixture(path).await {
                Ok(bytes) => bytes,
                Err(error) => return Some(Err(error)),
            };
            (
                "browser_send",
                json!({"request":{"operation":"fixture_register","fixture":{"name":path.file_name().and_then(|s|s.to_str()).unwrap_or("fixture.bin"),"mimeType":mime_type,"dataBase64":base64::engine::general_purpose::STANDARD.encode(bytes),"provenance":provenance,"cleared":{"by":cleared_by,"method":clearance_method}}}}),
            )
        }
        BrowserCommand::Status {
            operation_id,
            wait_ms,
            include_details,
            page,
        } => {
            let mut args = json!({"waitMs":wait_ms});
            page.add_to(&mut args);
            if let Some(id) = operation_id {
                args["operationId"] = json!(id);
            }
            if *include_details {
                args["includeDetails"] = json!(true);
            }
            ("browser_status", args)
        }
        BrowserCommand::Send {
            request,
            operation_id,
            timeout_ms,
            page,
        } => match send_arguments(request, operation_id, *timeout_ms, page) {
            Ok(args) => ("browser_send", args),
            Err(error) => return Some(Err(error)),
        },
        BrowserCommand::Cancel { operation_id } => {
            ("browser_cancel", json!({"operationId":operation_id}))
        }
        BrowserCommand::Disconnect => ("browser_disconnect", json!({})),
    };
    let mut args = args;
    Some(
        async {
            if let Some(target) = browser_target(input)?
                && !matches!(name, "browser_list" | "browser_doctor" | "browser_connect")
            {
                args["targetId"] = json!(target);
            } else if input.target.is_some() {
                return Err(target_refused());
            }
            crate::browser::surface::validate(name, &args)?;
            if matches!(name, "browser_list" | "browser_doctor") {
                return crate::browser::runtime::inspect(name, args);
            }
            crate::managed::Client::new().call(name, args).await
        }
        .await,
    )
}

fn send_arguments(
    request: &str,
    operation_id: &Option<String>,
    timeout_ms: u64,
    page: &super::arguments::PageArgs,
) -> Result<Value> {
    let mut request: Value =
        serde_json::from_str(request).map_err(|error| Error::invalid(error.to_string()))?;
    if page.cursor.is_some() || page.limit.is_some() {
        match request["operation"].as_str() {
            Some("events_read") => page.add_to(&mut request),
            Some("diagnostics_read") => {
                if request["query"].is_null() {
                    request["query"] = json!({});
                }
                if !request["query"].is_object() {
                    return Err(Error::invalid("diagnostics_read query must be an object"));
                }
                page.add_to(&mut request["query"]);
            }
            _ => {
                return Err(Error::invalid(
                    "--cursor and --limit apply to events_read and diagnostics_read requests",
                ));
            }
        }
    }
    let mut args = json!({"request":request,"timeoutMs":timeout_ms});
    if let Some(id) = operation_id {
        args["operationId"] = json!(id);
    }
    Ok(args)
}

/// The browser `--target` names: a `browser:` target from `browser connect`.
fn browser_target(input: &Mobile) -> Result<Option<&str>> {
    match input.target.as_deref() {
        Some(target) if !target.starts_with(crate::browser::runtime::TARGET_PREFIX) => {
            Err(target_refused())
        }
        target => Ok(target),
    }
}

fn target_refused() -> Error {
    Error::invalid(
        "Browser commands take a browser: --target from `testrun browser connect`, not a mobile target",
    )
}

async fn load_fixture(path: &std::path::Path) -> Result<Vec<u8>> {
    use crate::browser::transfer::FIXTURE_BYTES;
    use tokio::io::AsyncReadExt;
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|e| Error::new("FIXTURE_IO", e.to_string()))?;
    if !metadata.is_file() {
        return Err(Error::invalid("Fixture must be a regular file"));
    }
    if metadata.len() > FIXTURE_BYTES as u64 {
        return Err(Error::invalid("Fixture exceeds 4 MiB"));
    }
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await
        .map_err(|e| Error::new("FIXTURE_IO", e.to_string()))?
        .take(FIXTURE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| Error::new("FIXTURE_IO", e.to_string()))?;
    if bytes.len() > FIXTURE_BYTES {
        return Err(Error::invalid("Fixture exceeds 4 MiB"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn large_fixture_cli_loads_a_regular_file_at_the_limit() {
        use crate::browser::transfer::FIXTURE_BYTES;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("acme.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(FIXTURE_BYTES as u64).unwrap();
        assert_eq!(load_fixture(&path).await.unwrap().len(), FIXTURE_BYTES);
        file.set_len(FIXTURE_BYTES as u64 + 1).unwrap();
        assert!(load_fixture(&path).await.is_err());
        assert!(load_fixture(root.path()).await.is_err());
    }
}
