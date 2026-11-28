//! Static reproduction plans and host-observed marker construction; no target I/O.
use super::model::{Binding, label};
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Plan {
    preset: String,
    platform: String,
    target_id: Option<String>,
    app_id: Option<String>,
    user_id: Option<u32>,
    duration_ms: Option<u64>,
    interval_ms: Option<u64>,
    fault: Option<String>,
}
pub(super) fn plan(args: &Value) -> Result<Value> {
    let p: Plan = serde_json::from_value(args.clone())
        .map_err(|_| Error::invalid("Invalid reproduction plan"))?;
    let duration = p.duration_ms.unwrap_or(30000);
    let interval = p.interval_ms.unwrap_or(1000);
    let fault = p.fault.as_deref().unwrap_or("none");
    if !matches!(
        p.preset.as_str(),
        "crash"
            | "hang"
            | "memory_growth"
            | "slow_interaction"
            | "startup"
            | "network"
            | "background_media"
            | "soak"
    ) || !matches!(
        p.platform.as_str(),
        "android" | "ios" | "browser" | "import"
    ) || !(1000..=300000).contains(&duration)
        || !(500..=60000).contains(&interval)
        || interval > duration
        || p.user_id.is_some_and(|u| u > 99999)
    {
        return Err(Error::invalid(
            "Invalid preset/platform or bounded duration/interval",
        ));
    }
    for s in p.target_id.iter().chain(p.app_id.iter()) {
        label(s, 256)?;
    }
    let mut missing = vec![];
    if p.platform != "import" && p.target_id.is_none() {
        missing.push("targetId");
    }
    if matches!(p.platform.as_str(), "android" | "ios") && p.app_id.is_none() {
        missing.push("appId");
    }
    if p.platform == "android" && p.user_id.is_none() {
        missing.push("userId");
    }
    let groups = match p.preset.as_str() {
        "memory_growth" | "soak" => vec!["process", "memory", "power"],
        "slow_interaction" | "startup" => vec!["process", "scheduling", "frames"],
        "network" => vec!["process", "connectivity"],
        "background_media" => vec!["process", "scheduling", "power", "media", "device_context"],
        _ => vec!["process", "scheduling"],
    };
    let source = if !missing.is_empty() {
        Value::Null
    } else {
        match p.platform.as_str() {
            "android" if p.preset == "crash" => {
                json!({"kind":"android","targetId":p.target_id,"appId":p.app_id,"userId":p.user_id,"mode":"follow","includeReports":true})
            }
            "android" => {
                json!({"kind":"android_state","targetId":p.target_id,"appId":p.app_id,"userId":p.user_id,"groups":groups,"mode":"follow","intervalMs":interval,"label":p.preset})
            }
            "ios" => {
                json!({"kind":"ios","targetId":p.target_id,"appId":p.app_id,"transport":"usb","collectLogs":true,"logSeconds":(duration/1000).clamp(1,if p.target_id.as_deref().is_some_and(|t|t.starts_with("device:")){4}else{30})})
            }
            _ => Value::Null,
        }
    };
    let (fault_owner, restore) = fault_control(&p.platform, fault)?;
    Ok(
        json!({"version":1,"preset":p.preset,"platform":p.platform,"readiness":"not_probed","missing":missing,
        "capture":{"source":source,"limits":{"durationMs":duration,"maxBytes":1048576,"maxEvents":4096},"repeatedRead":{"owner":"existing bounded source follow loop; no repeated input","intervalMs":interval,"maximumCycles":256,"totalBudgetMs":duration,"cancel":"diagnostics_stop","earlyStop":["byte/event budget","source exit/loss","explicit cancellation"]}},
        "browserCapture":{"requires":"exact owned context/page and engine capability discovery","owners":["diagnostics_start","trace_start","profile_start"],"retain":"borrow selected settled result with browser_operation; never recapture during query"},
        "fault":{"requested":fault,"availableRoute":fault_owner.is_some()||fault=="none","owner":fault_owner,"scope":if fault=="offline"&&matches!(p.platform.as_str(),"android"|"ios"){"selected device network; can affect other apps"}else if p.platform=="browser"{"selected owned browser context/page; inspect engine capability"}else{"selected app or explicitly unsupported route"},"restore":restore,"executed":false},
        "determinism":{"browserClock":"context_clock on an owned context, capability checked; preserve restore/close receipt","networkFixtures":"owned network_route response fixtures; no trace-to-production replay","randomness":"only an explicit app-owned test control; no implicit Math.random replacement","nativeData":"only declared app fixture controls; no automatic database/cache reset"},
        "sequence":["retain historical evidence if it already establishes failure","open investigation with exact binding/actionIds","start selected collectors before input","write before_action marker; apply explicit fault through its existing owner","perform existing act batch/wait or record externally performed action","write after_action marker; inspect outcome before further input","remove owned faults and verify cleanup/readback; record failures","stop collectors and wait for settlement; analyze retained evidence"],
        "markers":{"sourceKind":"marker","binding":"actionId must be listed in investigation binding.actionIds","clock":"host wall milliseconds; request observation only, not app action completion"},
        "comparison":{"format":"reproduction-v1","operation":"diagnostics_compare","requires":"exact per-run build/device/runtime/warmup/sampling/instrumentation/contention/fixture definitions; preserve every attempt and cleanup outcome"},
        "effects":[],"automaticExecution":false,"gaps":["plan does not probe target/tool/version readiness","source reads may be unavailable; no fallback or false success","soak captures are finite observation windows, not an automated action repeater","native pressure/permission setters, arbitrary seeded randomness and long campaigns unregistered"]}),
    )
}
pub(super) fn marker(name: &str, phase: &str, action: &str, binding: &Binding) -> Result<Vec<u8>> {
    label(name, 256)?;
    label(action, 256)?;
    if !binding.action_ids.iter().any(|id| id == action)
        || !matches!(
            phase,
            "capture_start"
                | "before_action"
                | "after_action"
                | "fault_applied"
                | "fault_removed"
                | "capture_stop"
                | "cleanup"
        )
    {
        return Err(Error::invalid(
            "Marker requires a declared investigation actionId and supported phase",
        ));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::new("CLOCK_UNAVAILABLE", "Host clock precedes epoch"))?
        .as_millis();
    serde_json::to_vec(&json!({"schemaVersion":1,"clocks":[{"id":"host-marker-wall","unit":"milliseconds"}],"records":[{
        "kind":"marker","name":name,"timestamp":now,"clockDomain":"host-marker-wall","correlations":[action],
        "data":{"phase":phase,"actionId":action,"binding":binding,"actionOutcomeVerified":false,"faultRestorationVerified":false,"clockMeaning":"host marker admission; wall clock may jump; not app execution time"}
    }]})).map_err(|_|Error::invalid("Cannot encode marker"))
}
fn fault_control(platform: &str, fault: &str) -> Result<(Option<&'static str>, &'static str)> {
    Ok(match (platform, fault) {
        (_, "none") => (None, "no fault requested"),
        ("browser", "offline") => (
            Some("context_configure: offline"),
            "read current owned-context settings; restore through same owner or close that owned context",
        ),
        ("android" | "ios", "offline") => (
            Some("network_set"),
            "use the existing network owner receipt/readback; do not infer restoration from a marker",
        ),
        ("browser", "delay" | "response") => (
            Some("network_route + callback_resolve"),
            "resolve every owned pending route; remove only the installed handler; no automatic replay",
        ),
        ("browser", "permission") => (
            Some("context_configure: permissions_grant/permissions_clear"),
            "use a fresh owned context for permission fixtures; close it for cleanup",
        ),
        ("android" | "ios", "lifecycle") => (
            Some("act: app_launch/app_terminate/app_restart"),
            "select exact app/target and existing controller; terminate/restart is not a reversible state snapshot",
        ),
        (_, "offline" | "delay" | "response" | "lifecycle" | "permission" | "pressure") => (
            None,
            "unsupported by this plan; do not substitute a shell command or silently skip",
        ),
        _ => return Err(Error::invalid("Unknown fault selection")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_reproduction_plans_are_bounded_static_and_refuse_unsupported_faults() {
        for preset in [
            "crash",
            "hang",
            "memory_growth",
            "slow_interaction",
            "startup",
            "network",
            "background_media",
            "soak",
        ] {
            let p=plan(&json!({"preset":preset,"platform":"android","targetId":"android:Acme","appId":"com.example.acme","userId":0})).unwrap();
            assert!(p["capture"]["source"].is_object());
            assert_eq!(p["automaticExecution"], false);
            let args = json!({"investigationId":"diag-a","submissionId":"capture","source":p["capture"]["source"],"limits":p["capture"]["limits"]});
            crate::schema::validate(
                &super::super::definition("diagnostics_start").unwrap()["inputSchema"],
                &args,
            )
            .unwrap();
        }
        let p = plan(&json!({"preset":"network","platform":"ios","fault":"delay"})).unwrap();
        assert_eq!(p["fault"]["availableRoute"], false);
        assert!(p["capture"]["source"].is_null());
        assert!(plan(&json!({"preset":"soak","platform":"android","durationMs":300001})).is_err());
    }
    #[test]
    fn diagnostic_reproduction_markers_bind_actions_without_claiming_execution() {
        let mut b = Binding::default();
        assert!(marker("before", "before_action", "a1", &b).is_err());
        b.action_ids.push("a1".into());
        let raw = marker("before", "before_action", "a1", &b).unwrap();
        let index = super::super::analysis::decode::decode("testrun-events-v1", &raw).unwrap();
        assert_eq!(index.rows[0].correlations, ["a1"]);
        assert_eq!(index.rows[0].data["actionOutcomeVerified"], false);
        assert!(index.rows[0].timestamp.unwrap() > 0.0);
    }
    #[test]
    fn diagnostic_reproduction_apple_plans_preserve_physical_and_simulator_window_limits() {
        for (target, seconds) in [
            ("device:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 4),
            ("simulator:11111111-1111-4111-8111-111111111111", 30),
        ] {
            let p=plan(&json!({"preset":"crash","platform":"ios","targetId":target,"appId":"com.example.acme","durationMs":300000})).unwrap();
            assert_eq!(p["capture"]["source"]["logSeconds"], seconds);
            crate::schema::validate(&super::super::definition("diagnostics_start").unwrap()["inputSchema"],&json!({"investigationId":"diag-a","submissionId":"capture","source":p["capture"]["source"]})).unwrap();
        }
    }
}
