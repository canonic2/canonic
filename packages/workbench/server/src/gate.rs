//! Resolve explicit Gate service references through its public CLI, scoped to
//! this space's checkout. No private profile files or lifecycle commands.
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt as _;

/// A service name and optional base path; no credentials, port or query.
pub(crate) fn target(base: &str) -> Option<(&str, &str)> {
    let rest = base.strip_prefix("gate://")?;
    let (service, path) = rest
        .find('/')
        .map_or((rest, ""), |at| (&rest[..at], &rest[at..]));
    (!service.is_empty()
        && service.as_bytes()[0].is_ascii_alphanumeric()
        && service
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !path
            .chars()
            .any(|c| c.is_whitespace() || c == '?' || c == '#'))
    .then_some((service, path))
}

fn binary() -> PathBuf {
    let local = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin"));
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .chain(local)
        .map(|dir| dir.join("gate"))
        .find(|file| {
            let Ok(meta) = std::fs::metadata(file) else {
                return false;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                meta.is_file() && meta.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                meta.is_file()
            }
        })
        .unwrap_or_else(|| PathBuf::from("gate"))
}

async fn discover(root: &Path) -> Result<Value> {
    let mut child = tokio::process::Command::new(binary())
        .args(["urls", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("Gate is not available on PATH or in ~/.local/bin")?;
    let stdout = child.stdout.take().context("Gate stdout unavailable")?;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut body = Vec::new();
        stdout.take(262_145).read_to_end(&mut body).await?;
        if body.len() > 262_144 {
            bail!("Gate URL response exceeds 256 KiB");
        }
        let status = child.wait().await?;
        if !status.success() {
            bail!(
                "Gate could not resolve this checkout's task; run gate urls --json here for details"
            );
        }
        decode(&body)
    })
    .await
    .context("Gate URL discovery timed out")?
}

fn decode(body: &[u8]) -> Result<Value> {
    let reply: Value = serde_json::from_slice(body).context("Invalid Gate URL response")?;
    if !matches!(reply["version"].as_u64(), Some(1 | 2))
        || reply["ok"] != true
        || !reply["data"].is_object()
    {
        bail!("Invalid Gate URL response envelope; run gate urls --json here for details");
    }
    Ok(reply["data"].clone())
}

fn endpoint(report: &Value, service: &str, path: &str) -> Result<String> {
    let route = &report["routes"][service];
    if route["state"] != "published" {
        bail!(
            "Gate service {service} is not ready; start it with Gate from this checkout, then refresh Workbench (gate urls --json shows its status)"
        );
    }
    let direct = route["direct_url"]
        .as_str()
        .context("Gate omitted the service endpoint")?;
    let url = url::Url::parse(direct).context("Invalid Gate endpoint")?;
    // Gate's public direct endpoints are loopback HTTP; reject arbitrary
    // origins even when a malformed tool response claims a published route.
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port_or_known_default().is_none_or(|port| port == 0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("Invalid Gate direct endpoint");
    }
    Ok(format!("{}{path}", direct.trim_end_matches('/')))
}

/// One space's last explicit configuration refresh. Ordinary requests never
/// spawn discovery, including requests before the first refresh.
#[derive(Default)]
pub(crate) struct Discovery(tokio::sync::Mutex<Option<Result<Value, String>>>);

impl Discovery {
    pub(crate) async fn refresh(&self, root: &Path, trusted: bool, view: &Value) {
        let needed = view["implementations"].as_object().is_some_and(|items| {
            items.values().any(|item| {
                item["kind"] == "url"
                    && item["base"]
                        .as_str()
                        .is_some_and(|base| target(base).is_some())
            })
        });
        let report = if !needed {
            None
        } else if trusted {
            Some(discover(root).await.map_err(|error| format!("{error:#}")))
        } else {
            Some(Err("Gate discovery requires a trusted workspace".into()))
        };
        *self.0.lock().await = report;
    }

    pub(crate) async fn resolve(&self, implementations: &mut Map<String, Value>) {
        let report =
            self.0.lock().await.clone().unwrap_or_else(|| {
                Err("Refresh Workbench to discover Gate service endpoints".into())
            });
        for (key, implementation) in implementations {
            if implementation["kind"] != "url" {
                continue;
            }
            let Some(base) = implementation["base"].as_str().map(str::to_owned) else {
                continue;
            };
            let Some((service, path)) = target(&base) else {
                continue;
            };
            match report
                .as_ref()
                .map_err(|error| anyhow::anyhow!("{error:#}"))
                .and_then(|report| endpoint(report, service, path))
            {
                Ok(base) => implementation["base"] = json!(base),
                Err(error) => {
                    let message = format!("implementations › {key}: {error:#}");
                    implementation["problem"] = json!(message);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_gate_envelopes_share_the_bounded_route_contract() {
        let report =
            json!({"routes":{"demo":{"state":"published","direct_url":"http://127.0.0.1:49173/"}}});
        for version in [1, 2] {
            let body =
                serde_json::to_vec(&json!({"version":version,"ok":true,"data":report})).unwrap();
            let decoded = decode(&body).unwrap();
            assert_eq!(
                endpoint(&decoded, "demo", "/dev").unwrap(),
                "http://127.0.0.1:49173/dev"
            );
        }
        for reply in [
            json!({"version":3,"ok":true,"data":report}),
            json!({"version":1,"ok":false,"data":report}),
            json!({"version":2,"ok":true,"data":null}),
            report,
        ] {
            assert!(decode(&serde_json::to_vec(&reply).unwrap()).is_err());
        }
    }
    #[test]
    fn service_references_are_bounded_and_endpoints_require_readiness() {
        assert_eq!(target("gate://demo/dev"), Some(("demo", "/dev")));
        for bad in [
            "gate://",
            "gate://Demo",
            "gate://demo:4317",
            "gate://user@demo",
            "gate://demo/dev?q=1",
            "gate://demo/#x",
        ] {
            assert!(target(bad).is_none(), "{bad}");
        }
        let mut report =
            json!({"routes":{"demo":{"state":"published","direct_url":"http://127.0.0.1:49173/"}}});
        assert_eq!(
            endpoint(&report, "demo", "/dev").unwrap(),
            "http://127.0.0.1:49173/dev"
        );
        report["routes"]["demo"]["state"] = json!("suspended");
        assert!(endpoint(&report, "demo", "/dev").is_err());
        report["routes"]["demo"]["state"] = json!("published");
        report["routes"]["demo"]["direct_url"] = json!("https://example.com/");
        assert!(endpoint(&report, "demo", "/dev").is_err());
    }
}
