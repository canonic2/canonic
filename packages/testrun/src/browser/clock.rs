//! Typed context virtual-time commands; host deadlines and leases remain real.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    Install {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time_ms: Option<i64>,
    },
    SetFixedTime {
        time_ms: i64,
    },
    SetSystemTime {
        time_ms: i64,
    },
    PauseAt {
        time_ms: i64,
    },
    RunFor {
        duration_ms: u64,
    },
    FastForward {
        duration_ms: u64,
    },
    Resume {},
}
impl Action {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        let time = match self {
            Self::Install { time_ms } => *time_ms,
            Self::SetFixedTime { time_ms }
            | Self::SetSystemTime { time_ms }
            | Self::PauseAt { time_ms } => Some(*time_ms),
            _ => None,
        };
        if time
            .is_some_and(|time| !(-8_640_000_000_000_000..=8_640_000_000_000_000).contains(&time))
        {
            return Err("INVALID_CLOCK_TIME");
        }
        match self {
            Self::RunFor { duration_ms } if *duration_ms > 60_000 => Err("CLOCK_DURATION_BOUNDS"),
            Self::FastForward { duration_ms } if *duration_ms > 604_800_000 => {
                Err("CLOCK_DURATION_BOUNDS")
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn wire(&self) -> Result<(&'static str, Value), &'static str> {
        self.validate()?;
        Ok(match self {
            Self::Install { time_ms } => (
                "clockInstall",
                time_ms
                    .map(|time| json!({"timeNumber":time}))
                    .unwrap_or_else(|| json!({})),
            ),
            Self::SetFixedTime { time_ms } => ("clockSetFixedTime", json!({"timeNumber":time_ms})),
            Self::SetSystemTime { time_ms } => {
                ("clockSetSystemTime", json!({"timeNumber":time_ms}))
            }
            Self::PauseAt { time_ms } => ("clockPauseAt", json!({"timeNumber":time_ms})),
            Self::RunFor { duration_ms } => ("clockRunFor", json!({"ticksNumber":duration_ms})),
            Self::FastForward { duration_ms } => {
                ("clockFastForward", json!({"ticksNumber":duration_ms}))
            }
            Self::Resume {} => ("clockResume", json!({})),
        })
    }
}
pub(crate) fn schema(reference: &Value) -> Value {
    let time = json!({"type":"integer","minimum":-8_640_000_000_000_000_i64,"maximum":8_640_000_000_000_000_i64,"description":"Unix epoch milliseconds within the JavaScript Date range"});
    let mut actions = Vec::new();
    for kind in [
        "install",
        "set_fixed_time",
        "set_system_time",
        "pause_at",
        "run_for",
        "fast_forward",
        "resume",
    ] {
        let mut properties = json!({"kind":{"const":kind}});
        let mut required = vec!["kind"];
        match kind {
            "install" | "set_fixed_time" | "set_system_time" | "pause_at" => {
                properties["timeMs"] = time.clone();
                if kind != "install" {
                    required.push("timeMs");
                }
            }
            "run_for" | "fast_forward" => {
                properties["durationMs"] = json!({"type":"integer","minimum":0,"maximum":if kind=="run_for" {60_000} else {604_800_000}});
                required.push("durationMs");
            }
            _ => {}
        }
        actions.push(json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}));
    }
    json!({"type":"object","properties":{"operation":{"const":"context_clock"},"context":reference,"action":{"oneOf":actions}},"required":["operation","context","action"],"additionalProperties":false})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_time_bounds_and_wire_commands_preserve_clock_semantics() {
        assert!(
            Action::SetFixedTime { time_ms: i64::MAX }
                .validate()
                .is_err()
        );
        assert!(
            Action::RunFor {
                duration_ms: 60_001
            }
            .validate()
            .is_err()
        );
        assert!(
            Action::FastForward {
                duration_ms: 604_800_001
            }
            .validate()
            .is_err()
        );
        assert_eq!(
            Action::Install { time_ms: None }.wire().unwrap(),
            ("clockInstall", json!({}))
        );
        assert_eq!(
            Action::RunFor { duration_ms: 2000 }.wire().unwrap(),
            ("clockRunFor", json!({"ticksNumber":2000}))
        );
        assert_eq!(
            Action::SetFixedTime { time_ms: 0 }.wire().unwrap(),
            ("clockSetFixedTime", json!({"timeNumber":0}))
        );
        assert!(
            serde_json::from_value::<Action>(
                json!({"kind":"pause_at","timeMs":0,"durationMs":100})
            )
            .is_err()
        );
    }
}
