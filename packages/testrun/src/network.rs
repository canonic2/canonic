//! Network connectivity control: readback tuples, offline/online plans and
//! agreement rules. Testrun changes switches only on request and never writes
//! them back on its own; restoring connectivity is the caller's explicit request.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) mod android;
pub(crate) mod schema;
#[cfg(test)]
mod tests;

/// The managed tool that owns network control; only Android targets support it.
pub(crate) fn operation(name: &str) -> bool {
    name == "network_set"
}

/// Validate arguments against the registered schema.
pub(crate) fn validate(name: &str, args: &Value) -> Result<()> {
    let definition = schema::tools()
        .into_iter()
        .find(|tool| tool["name"] == name)
        .ok_or_else(|| Error::invalid("Network operation is not registered"))?;
    crate::schema::validate(&definition["inputSchema"], args)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    Offline,
    Online,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Method {
    Airplane,
    Radios,
}
impl Method {
    /// The lowest Android API level whose shell offers this method.
    pub(crate) fn minimum_api(self) -> u32 {
        match self {
            Self::Airplane => 30,
            Self::Radios => 21,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Airplane => "airplane",
            Self::Radios => "radios",
        }
    }
}

/// Validated `network_set` arguments; `timeout_ms` bounds the switch readback.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Request {
    pub(crate) mode: Mode,
    pub(crate) method: Method,
    pub(crate) timeout_ms: u64,
}
impl Request {
    pub(crate) fn parse(args: &Value) -> Result<Self> {
        let mode = match args["mode"].as_str() {
            Some("offline") => Mode::Offline,
            Some("online") => Mode::Online,
            _ => return Err(Error::invalid("mode must be offline or online")),
        };
        let method = match args["method"].as_str() {
            None | Some("airplane") => Method::Airplane,
            Some("radios") => Method::Radios,
            _ => return Err(Error::invalid("method must be airplane or radios")),
        };
        let timeout_ms = args["timeoutMs"].as_u64().unwrap_or(match mode {
            Mode::Offline => 5000,
            Mode::Online => 15000,
        });
        Ok(Self {
            mode,
            method,
            timeout_ms,
        })
    }
}

/// One observation of the target's connectivity switches. Fields the target did
/// not report stay `None`; nothing is inferred from a missing read.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Readback {
    pub(crate) airplane: Option<bool>,
    pub(crate) wifi_enabled: Option<bool>,
    pub(crate) wifi_connected: Option<bool>,
    pub(crate) wifi_setting: Option<String>,
    pub(crate) mobile_data: Option<bool>,
    pub(crate) bluetooth: Option<bool>,
    pub(crate) default_network: Option<bool>,
}
impl Readback {
    /// Parse the marker-separated output of [`android::READBACK`].
    pub(crate) fn parse(output: &str) -> Self {
        let mut sections: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
        let mut current = "";
        for line in output.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(marker) = line.strip_prefix('#') {
                current = marker.trim();
                sections.entry(current).or_default();
            } else if !current.is_empty() && !line.trim().is_empty() {
                sections.entry(current).or_default().push(line.trim());
            }
        }
        let first = |name: &str| sections.get(name).and_then(|lines| lines.first().copied());
        let flag = |text: Option<&str>| match text {
            Some("1") => Some(true),
            Some("0") => Some(false),
            _ => None,
        };
        let airplane = match first("airplane") {
            Some("enabled") => Some(true),
            Some("disabled") => Some(false),
            _ => flag(first("airplane_on")),
        };
        let wifi_setting = first("wifi_on")
            .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_owned);
        let wifi_lines = sections.get("wifi").cloned().unwrap_or_default();
        let wifi_enabled = wifi_lines
            .iter()
            .find_map(|line| match *line {
                "Wifi is enabled" => Some(true),
                "Wifi is disabled" => Some(false),
                _ => None,
            })
            .or(match wifi_setting.as_deref() {
                Some("1") => Some(true),
                // 0 is off; 3 is Android's "off because of airplane mode" override.
                Some("0" | "3") => Some(false),
                _ => None,
            });
        let wifi_connected = wifi_lines.iter().find_map(|line| {
            if line.starts_with("Wifi is connected to") {
                Some(true)
            } else if line.starts_with("Wifi is not connected") {
                Some(false)
            } else {
                None
            }
        });
        let bluetooth = first("bluetooth")
            .and_then(|line| line.strip_prefix("state:"))
            .map(str::trim)
            .and_then(|state| match state {
                "ON" => Some(true),
                "OFF" => Some(false),
                _ => None,
            })
            .or(match first("bluetooth_on") {
                Some("1") => Some(true),
                // 2 is Android's "off because of airplane mode" override.
                Some("0" | "2") => Some(false),
                _ => None,
            });
        let default_network = first("default")
            .and_then(|line| line.strip_prefix("Active default network:"))
            .map(str::trim)
            .map(|value| value != "none");
        Self {
            airplane,
            wifi_enabled,
            wifi_connected,
            wifi_setting,
            mobile_data: flag(first("mobile_data")),
            bluetooth,
            default_network,
        }
    }
    /// Whether the target has a default network: `online`, `offline` or `unknown`.
    pub(crate) fn connectivity(&self) -> &'static str {
        match self.default_network {
            Some(true) => "online",
            Some(false) => "offline",
            None => "unknown",
        }
    }
    pub(crate) fn to_json(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        value["connectivity"] = json!(self.connectivity());
        value
    }
    /// Whether this readback already satisfies `request`.
    pub(crate) fn agrees(&self, request: Request) -> bool {
        match (request.mode, request.method) {
            (Mode::Offline, Method::Airplane) => {
                self.airplane == Some(true) && self.default_network == Some(false)
            }
            (Mode::Offline, Method::Radios) => {
                self.wifi_enabled == Some(false)
                    && self.mobile_data != Some(true)
                    && self.default_network == Some(false)
            }
            (Mode::Online, Method::Airplane) => {
                self.airplane == Some(false) && self.default_network == Some(true)
            }
            (Mode::Online, Method::Radios) => {
                self.airplane == Some(false)
                    && self.default_network == Some(true)
                    && (self.wifi_enabled == Some(true) || self.mobile_data == Some(true))
            }
        }
    }
}

/// An adb shell command, as the argument vector the adapter quotes.
pub(crate) type Command = Vec<&'static str>;

/// Commands that move `current` toward `request`.
pub(crate) fn plan(request: Request, current: &Readback) -> Vec<Command> {
    let mut commands = Vec::new();
    let airplane = |on: bool| {
        vec![
            "cmd",
            "connectivity",
            "airplane-mode",
            if on { "enable" } else { "disable" },
        ]
    };
    match (request.mode, request.method) {
        (Mode::Offline, Method::Airplane) => commands.push(airplane(true)),
        (Mode::Online, Method::Airplane) => {
            if current.airplane != Some(false) {
                commands.push(airplane(false));
            }
        }
        (Mode::Offline, Method::Radios) => {
            commands.push(vec!["svc", "wifi", "disable"]);
            if current.mobile_data == Some(true) {
                commands.push(vec!["svc", "data", "disable"]);
            }
        }
        (Mode::Online, Method::Radios) => {
            if current.airplane == Some(true) {
                commands.push(airplane(false));
            }
            commands.push(vec!["svc", "wifi", "enable"]);
            if current.mobile_data == Some(false) {
                commands.push(vec!["svc", "data", "enable"]);
            }
        }
    }
    commands
}

/// Switches a mutation moved beyond the requested connectivity change.
pub(crate) fn side_effects(before: &Readback, after: &Readback) -> Vec<&'static str> {
    let mut effects = Vec::new();
    match (before.bluetooth, after.bluetooth) {
        (Some(true), Some(false)) => effects.push("bluetooth_off"),
        (Some(false), Some(true)) => effects.push("bluetooth_on"),
        _ => {}
    }
    if before.wifi_setting != after.wifi_setting && after.wifi_setting.as_deref() == Some("3") {
        effects.push("wifi_airplane_override");
    }
    effects
}

pub(crate) fn utc() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
