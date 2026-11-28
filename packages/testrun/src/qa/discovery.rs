//! Explicit platform discovery, filtering and bounded pagination.
use super::Server;
use super::arguments::ListTargets;
use crate::error::{Error, Result};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use tokio::time::Instant;

/// Read handle shares only the authoritative discovery generations.
#[derive(Clone)]
pub(crate) struct Reader(std::sync::Arc<Registry>);

#[derive(Default)]
pub(super) struct Registry {
    state: std::sync::Mutex<crate::targets::Discovery>,
    sequence: std::sync::atomic::AtomicU64,
    published: std::sync::Mutex<std::collections::HashMap<String, u64>>,
}
impl Registry {
    pub(super) fn resolve(
        &self,
        target: &crate::targets::TargetRef,
    ) -> Result<crate::targets::Target> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resolve(target)
    }
    pub(super) async fn refresh(&self, kind: &str) -> Result<Vec<crate::targets::Target>> {
        let sequence = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut local = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let targets = match kind {
            "android" => local.refresh_android().await?,
            "device" => local.refresh_devices().await?,
            _ => local.refresh().await?,
        };
        let mut published = self.published.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if published.get(kind).is_some_and(|last| *last > sequence) {
            // A late older inventory cannot resurrect an absent or replaced target.
            return targets
                .into_iter()
                .filter_map(|target| match state.reconcile(&target) {
                    Ok(target) => Some(Ok(target)),
                    Err(error) if error.code == "STALE_TARGET" => None,
                    Err(error) => Some(Err(error)),
                })
                .collect();
        }
        published.insert(kind.into(), sequence);
        Ok(state.publish(kind, targets))
    }
}

impl Reader {
    pub(super) async fn validate(
        &self,
        target: &crate::targets::Target,
    ) -> Result<crate::targets::Target> {
        self.0.refresh(&target.kind).await?;
        let resolved = self.0.resolve(&target.identity)?;
        if resolved.kind != "device" && resolved.state != "Booted" {
            return Err(Error::new("TARGET_NOT_READY", "Target is not booted"));
        }
        Ok(resolved)
    }
    pub(crate) async fn call(&self, name: &str, args: Value) -> Result<Value> {
        match name {
            "target_list" => Server::discover(&self.0, super::arguments::parse(args)?).await,
            "doctor" => {
                let target = super::arguments::field(&args, "targetId")?;
                if target.starts_with("android:") {
                    crate::android::Android::default().doctor(target).await
                } else {
                    crate::device_setup::doctor(target).await
                }
            }
            "host_info" => Ok(
                serde_json::json!({"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"adapterProtocolVersion":1}),
            ),
            _ => Err(Error::invalid("Unsupported independent read")),
        }
    }
}

impl Server {
    pub(super) async fn list_targets(&mut self, list: ListTargets) -> Result<Value> {
        Self::discover(&self.discovery, list).await
    }

    /// Clone only discovery ownership, never session, journal or cleanup state.
    pub(crate) fn discovery_reader(&self) -> Reader {
        Reader(self.discovery.clone())
    }

    async fn discover(registry: &Registry, list: ListTargets) -> Result<Value> {
        // Leave reply/serialization headroom inside MCP's 30-second deadline.
        // Discovery time consumes this budget too; probes never extend it.
        let probe_deadline = Instant::now() + Duration::from_secs(20);
        let args = canonic_mcp_kit::page::arguments(
            list.cursor.as_deref(),
            list.limit.map(|limit| limit as u64),
        );
        let query = (&list.kind, &list.state, list.probe);
        let scope = crate::pagination::scope("target_list", &query)?;
        let (request, after) = crate::pagination::request(&args, &scope)?;
        let (targets, unavailable) = match list.kind.as_deref() {
            Some(kind) => (registry.refresh(kind).await?, Vec::new()),
            None => {
                // One platform's missing tools must not hide the others' targets.
                let mut targets = Vec::new();
                let mut unavailable = Vec::new();
                for kind in ["simulator", "device", "android"] {
                    match registry.refresh(kind).await {
                        Ok(found) => targets.extend(found),
                        Err(error) => unavailable.push(json!({"kind":kind,"error":error})),
                    }
                }
                if unavailable.len() == 3 {
                    return Err(Error::new(
                        "DISCOVERY_UNAVAILABLE",
                        "No platform could list targets; inspect each platform's error",
                    )
                    .details(json!({"unavailable":unavailable})));
                }
                (targets, unavailable)
            }
        };
        let mut filtered: Vec<_> = targets
            .into_iter()
            .filter(|t| list.state.as_ref().is_none_or(|s| s == &t.state))
            .collect();
        filtered.sort_by_cached_key(|target| {
            crate::pagination::identity_key(&target.identity.target_id)
        });
        filtered.retain(|target| {
            after.as_ref().is_none_or(|after| {
                crate::pagination::identity_key(&target.identity.target_id).as_slice()
                    > after.as_slice()
            })
        });
        let limit = usize::try_from(request.limit.get()).expect("validated limit");
        let more = filtered.len() > limit;
        let mut targets = Vec::new();
        for target in filtered.iter().take(limit) {
            let value = serde_json::to_value(target)
                .map_err(|_| Error::new("INTERNAL", "Cannot encode discovered target"))?;
            targets.push(value);
        }
        if list.probe {
            targets = attach_readiness(targets, probe_deadline, |target| async move {
                if target.starts_with("android:") {
                    crate::android::Android::default().doctor(&target).await
                } else {
                    crate::device_setup::doctor(&target).await
                }
            })
            .await;
        }
        let mut page = crate::pagination::collect(
            "target_list",
            &query,
            &args,
            targets.into_iter().map(|target| {
                (
                    crate::pagination::identity_key(
                        target["targetId"].as_str().expect("target ID"),
                    ),
                    target,
                )
            }),
            Ok,
            "doctor",
        )?;
        if more && page.next_cursor.is_none() {
            page.next_cursor = page
                .items
                .last()
                .map(|item| {
                    scope.encode(&canonic_mcp_kit::page::Position::Keyset(
                        crate::pagination::identity_key(
                            item["targetId"].as_str().expect("target ID"),
                        ),
                    ))
                })
                .transpose()?;
        }
        let mut result = json!({"targets":page.items,"nextCursor":page.next_cursor});
        if !unavailable.is_empty() {
            result["unavailable"] = json!(unavailable);
        }
        Ok(result)
    }
}

async fn attach_readiness<F, Fut>(targets: Vec<Value>, deadline: Instant, probe: F) -> Vec<Value>
where
    F: Fn(String) -> Fut + Copy,
    Fut: Future<Output = Result<Value>>,
{
    stream::iter(targets.into_iter().map(|mut target| async move {
        let avd = target["targetId"]
            .as_str()
            .is_some_and(|id| id.starts_with(crate::android::emulator::AVD_PREFIX));
        if (target["kind"] == "device" || target["kind"] == "android") && !avd {
            let id = target["targetId"].as_str().unwrap().to_owned();
            let result = if Instant::now() >= deadline {
                Err(probe_timeout(&id))
            } else {
                tokio::time::timeout_at(deadline, probe(id.clone()))
                    .await
                    .unwrap_or_else(|_| Err(probe_timeout(&id)))
            };
            target["readiness"] =
                result.unwrap_or_else(|error| json!({"ready":false,"error":error}));
        }
        target
    }))
    .buffered(4)
    .collect()
    .await
}
fn probe_timeout(target: &str) -> Error {
    Error::new(
        "READINESS_PROBE_TIMEOUT",
        "Discovery's readiness budget was exhausted; inspect the selected target with doctor",
    )
    .details(json!({"nextAction":"doctor","targetId":target}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exhausted_android_listing_probes_preserve_identity_and_name_doctor_recovery() {
        let targets = vec![json!({"targetId":"android:Acme", "kind":"android"})];
        let output = attach_readiness(targets, Instant::now(), |_| async {
            panic!("An exhausted listing budget must not start a doctor probe")
        })
        .await;
        assert_eq!(output[0]["targetId"], "android:Acme");
        assert_eq!(
            output[0]["readiness"]["error"]["code"],
            "READINESS_PROBE_TIMEOUT"
        );
        assert_eq!(
            output[0]["readiness"]["error"]["details"]["nextAction"],
            "doctor"
        );
    }

    #[tokio::test]
    async fn bounded_probes_preserve_identities_order_and_completed_checks() {
        let targets = vec![
            json!({"targetId":"device:slow", "kind":"device"}),
            json!({"targetId":"device:ready", "kind":"device"}),
            json!({"targetId":"device:failed", "kind":"device"}),
            json!({"targetId":"simulator:Acme", "kind":"simulator"}),
        ];
        let output = tokio::time::timeout(
            Duration::from_secs(1),
            attach_readiness(
                targets.clone(),
                Instant::now() + Duration::from_millis(50),
                |id| async move {
                    match id.as_str() {
                        "device:slow" => std::future::pending().await,
                        "device:ready" => Ok(json!({"ready":true})),
                        "device:failed" => Err(Error::new("USB_UNAVAILABLE", "Fixture unplugged")),
                        _ => panic!("Non-device probe"),
                    }
                },
            ),
        )
        .await
        .unwrap();
        assert_eq!(output.len(), targets.len());
        for (actual, expected) in output.iter().zip(&targets) {
            assert_eq!(actual["targetId"], expected["targetId"]);
        }
        assert_eq!(
            output[0]["readiness"]["error"]["code"],
            "READINESS_PROBE_TIMEOUT"
        );
        assert_eq!(output[1]["readiness"]["ready"], true);
        assert_eq!(output[2]["readiness"]["error"]["code"], "USB_UNAVAILABLE");
        assert!(output[3].get("readiness").is_none());
        let expired = attach_readiness(targets, Instant::now(), |_| async {
            panic!("An exhausted budget must not start more native work")
        })
        .await;
        assert!(
            expired[..3]
                .iter()
                .all(|target| target["readiness"]["error"]["code"] == "READINESS_PROBE_TIMEOUT")
        );
    }
}
