//! Readback parsing, agreement and plans against recorded lab outputs.
use super::*;

/// Readback captured from an API 31 phone with everything on and connected.
const ONLINE: &str = "#airplane\ndisabled\n#airplane_on\n0\n#wifi_on\n1\n#mobile_data\n1\n#bluetooth_on\n1\n#wifi\nWifi is enabled\nWifi is connected to \"Acme\"\n#bluetooth\n    state: ON\n#default\n  Active default network: 104\n#end\n";
/// The same phone after `cmd connectivity airplane-mode enable` settled.
const AIRPLANE: &str = "#airplane\nenabled\n#airplane_on\n1\n#wifi_on\n3\n#mobile_data\n1\n#bluetooth_on\n2\n#wifi\nWifi is disabled\nWifi is not connected\n#bluetooth\n    state: OFF\n#default\n  Active default network: none\n#end\n";
/// The same phone after `svc wifi disable` and `svc data disable`.
const RADIOS_OFF: &str = "#airplane\ndisabled\n#airplane_on\n0\n#wifi_on\n0\n#mobile_data\n0\n#bluetooth_on\n1\n#wifi\nWifi is disabled\nWifi is not connected\n#bluetooth\n    state: ON\n#default\n  Active default network: none\n#end\n";

fn request(mode: Mode, method: Method) -> Request {
    Request {
        mode,
        method,
        timeout_ms: 1000,
    }
}

#[test]
fn readback_parses_each_switch_and_falls_back_to_settings() {
    let online = Readback::parse(ONLINE);
    assert_eq!(online.airplane, Some(false));
    assert_eq!(online.wifi_enabled, Some(true));
    assert_eq!(online.wifi_connected, Some(true));
    assert_eq!(online.wifi_setting.as_deref(), Some("1"));
    assert_eq!(online.mobile_data, Some(true));
    assert_eq!(online.bluetooth, Some(true));
    assert_eq!(online.default_network, Some(true));
    assert_eq!(online.connectivity(), "online");

    let airplane = Readback::parse(AIRPLANE);
    assert_eq!(airplane.airplane, Some(true));
    assert_eq!(airplane.wifi_enabled, Some(false));
    assert_eq!(airplane.wifi_setting.as_deref(), Some("3"));
    assert_eq!(airplane.bluetooth, Some(false));
    assert_eq!(airplane.default_network, Some(false));
    assert_eq!(airplane.connectivity(), "offline");

    // A target without `cmd connectivity` or `cmd wifi` still reports through settings.
    let settings_only = Readback::parse(
        "#airplane\n#airplane_on\n1\n#wifi_on\n3\n#mobile_data\nnull\n#bluetooth_on\n2\n#wifi\n#bluetooth\n#default\n#end\n",
    );
    assert_eq!(settings_only.airplane, Some(true));
    assert_eq!(settings_only.wifi_enabled, Some(false));
    assert_eq!(settings_only.wifi_connected, None);
    assert_eq!(settings_only.mobile_data, None);
    assert_eq!(settings_only.bluetooth, Some(false));
    assert_eq!(settings_only.default_network, None);
    assert_eq!(settings_only.connectivity(), "unknown");
    assert_eq!(Readback::parse("garbage"), Readback::default());
}

#[test]
fn agreement_requires_the_default_network_to_follow_the_switch() {
    let online = Readback::parse(ONLINE);
    let airplane = Readback::parse(AIRPLANE);
    let radios = Readback::parse(RADIOS_OFF);
    assert!(online.agrees(request(Mode::Online, Method::Airplane)));
    assert!(online.agrees(request(Mode::Online, Method::Radios)));
    assert!(!online.agrees(request(Mode::Offline, Method::Airplane)));
    assert!(!online.agrees(request(Mode::Offline, Method::Radios)));
    assert!(airplane.agrees(request(Mode::Offline, Method::Airplane)));
    assert!(!airplane.agrees(request(Mode::Online, Method::Airplane)));
    assert!(radios.agrees(request(Mode::Offline, Method::Radios)));
    assert!(!radios.agrees(request(Mode::Offline, Method::Airplane)));
    // Airplane disabled but Wi-Fi still reconnecting: online is not yet agreed.
    let reconnecting = Readback::parse(
        "#airplane\ndisabled\n#airplane_on\n0\n#wifi_on\n1\n#mobile_data\n1\n#bluetooth_on\n1\n#wifi\nWifi is enabled\nWifi is not connected\n#bluetooth\n    state: ON\n#default\n  Active default network: none\n#end\n",
    );
    assert!(!reconnecting.agrees(request(Mode::Online, Method::Airplane)));
}

#[test]
fn plans_touch_only_the_switches_each_method_owns() {
    let online = Readback::parse(ONLINE);
    assert_eq!(
        plan(request(Mode::Offline, Method::Airplane), &online),
        vec![vec!["cmd", "connectivity", "airplane-mode", "enable"]]
    );
    assert_eq!(
        plan(request(Mode::Offline, Method::Radios), &online),
        vec![
            vec!["svc", "wifi", "disable"],
            vec!["svc", "data", "disable"]
        ]
    );
    // Without a SIM the mobile-data setting is absent: the plan leaves it alone.
    let mut no_sim = online.clone();
    no_sim.mobile_data = None;
    assert_eq!(
        plan(request(Mode::Offline, Method::Radios), &no_sim),
        vec![vec!["svc", "wifi", "disable"]]
    );
    let airplane = Readback::parse(AIRPLANE);
    assert_eq!(
        plan(request(Mode::Online, Method::Airplane), &airplane),
        vec![vec!["cmd", "connectivity", "airplane-mode", "disable"]]
    );
    // Online through airplane mode sends nothing when airplane mode is already off.
    assert_eq!(
        plan(request(Mode::Online, Method::Airplane), &online),
        Vec::<Command>::new()
    );
    let radios = Readback::parse(RADIOS_OFF);
    assert_eq!(
        plan(request(Mode::Online, Method::Radios), &radios),
        vec![vec!["svc", "wifi", "enable"], vec!["svc", "data", "enable"]]
    );
}

#[test]
fn side_effects_name_bluetooth_and_the_airplane_override() {
    let online = Readback::parse(ONLINE);
    let airplane = Readback::parse(AIRPLANE);
    assert_eq!(
        side_effects(&online, &airplane),
        vec!["bluetooth_off", "wifi_airplane_override"]
    );
    assert_eq!(side_effects(&airplane, &online), vec!["bluetooth_on"]);
    assert_eq!(
        side_effects(&online, &Readback::parse(RADIOS_OFF)),
        Vec::<&str>::new()
    );
}

#[test]
fn request_defaults_and_validation_follow_the_registered_schema() {
    let request = Request::parse(&serde_json::json!({"mode":"offline"})).unwrap();
    assert_eq!(request.method, Method::Airplane);
    assert_eq!(request.timeout_ms, 5000);
    let request =
        Request::parse(&serde_json::json!({"mode":"online","method":"radios","timeoutMs":2000}))
            .unwrap();
    assert_eq!(request.method, Method::Radios);
    assert_eq!(request.timeout_ms, 2000);
    assert!(validate("network_set", &serde_json::json!({"mode":"offline"})).is_ok());
    for args in [
        serde_json::json!({"mode":"offline","force":true}),
        serde_json::json!({"mode":"offline","timeoutMs":10}),
    ] {
        assert_eq!(
            validate("network_set", &args).unwrap_err().code,
            "INVALID_ARGUMENT"
        );
    }
    assert_eq!(Method::Airplane.minimum_api(), 30);
    assert_eq!(Method::Radios.minimum_api(), 21);
}
