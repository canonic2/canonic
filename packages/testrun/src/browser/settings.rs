//! Typed browser environment settings and engine/origin validation.

use super::contract::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Geolocation {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub accuracy: f64,
}
impl Geolocation {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !self.latitude.is_finite()
            || !(-90.0..=90.0).contains(&self.latitude)
            || !self.longitude.is_finite()
            || !(-180.0..=180.0).contains(&self.longitude)
            || !self.accuracy.is_finite()
            || !(0.0..=1_000_000.0).contains(&self.accuracy)
        {
            return Err("INVALID_GEOLOCATION");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    Geolocation,
    Notifications,
    PersistentStorage,
    Push,
    ScreenWakeLock,
    LocalNetworkAccess,
    ClipboardRead,
    Camera,
    Microphone,
    ClipboardWrite,
    Midi,
    MidiSysex,
    BackgroundSync,
    AmbientLightSensor,
    Accelerometer,
    Gyroscope,
    Magnetometer,
    PaymentHandler,
    StorageAccess,
    LocalFonts,
}
/// Every permission name, in schema order.
const PERMISSIONS: [Permission; 20] = [
    Permission::Geolocation,
    Permission::Notifications,
    Permission::PersistentStorage,
    Permission::Push,
    Permission::ScreenWakeLock,
    Permission::LocalNetworkAccess,
    Permission::ClipboardRead,
    Permission::Camera,
    Permission::Microphone,
    Permission::ClipboardWrite,
    Permission::Midi,
    Permission::MidiSysex,
    Permission::BackgroundSync,
    Permission::AmbientLightSensor,
    Permission::Accelerometer,
    Permission::Gyroscope,
    Permission::Magnetometer,
    Permission::PaymentHandler,
    Permission::StorageAccess,
    Permission::LocalFonts,
];
impl Permission {
    /// Pinned SDK Chromium `webPermissionToProtocol`, before its
    /// local-network fallback; `None` is an unsupported Chrome permission.
    pub(crate) fn chromium(self) -> Option<&'static [&'static str]> {
        Some(match self {
            Self::Geolocation => &["geolocation"],
            Self::Midi => &["midi"],
            Self::Notifications => &["notifications"],
            Self::Camera => &["videoCapture"],
            Self::Microphone => &["audioCapture"],
            Self::BackgroundSync => &["backgroundSync"],
            Self::AmbientLightSensor
            | Self::Accelerometer
            | Self::Gyroscope
            | Self::Magnetometer => &["sensors"],
            Self::ClipboardRead => &["clipboardReadWrite"],
            Self::ClipboardWrite => &["clipboardSanitizedWrite"],
            Self::PaymentHandler => &["paymentHandler"],
            Self::MidiSysex => &["midiSysex"],
            Self::StorageAccess => &["storageAccess"],
            Self::LocalFonts => &["localFonts"],
            Self::LocalNetworkAccess => &["localNetworkAccess", "localNetwork", "loopbackNetwork"],
            Self::ScreenWakeLock => &["wakeLockScreen"],
            Self::PersistentStorage | Self::Push => return None,
        })
    }
    pub(crate) fn supported(self, engine: Engine) -> bool {
        match engine {
            Engine::Chrome => self.chromium().is_some(),
            Engine::Firefox => matches!(
                self,
                Self::Geolocation
                    | Self::Notifications
                    | Self::PersistentStorage
                    | Self::Push
                    | Self::ScreenWakeLock
                    | Self::LocalNetworkAccess
            ),
            Engine::Webkit => matches!(
                self,
                Self::Geolocation
                    | Self::Notifications
                    | Self::ScreenWakeLock
                    | Self::ClipboardRead
                    | Self::Camera
                    | Self::Microphone
            ),
            _ => false,
        }
    }
}
pub(crate) fn permissions(engine: Engine) -> Vec<Permission> {
    PERMISSIONS
        .into_iter()
        .filter(|permission| permission.supported(engine))
        .collect()
}
/// A nonempty distinct list of names the engine maps; checked before any grant.
pub(crate) fn validate_permissions(
    permissions: &[Permission],
    engine: Option<Engine>,
) -> Result<(), &'static str> {
    if permissions.is_empty()
        || permissions.len() > PERMISSIONS.len()
        || permissions.iter().collect::<BTreeSet<_>>().len() != permissions.len()
    {
        return Err("INVALID_PERMISSIONS");
    }
    if engine.is_some_and(|engine| {
        permissions
            .iter()
            .any(|permission| !permission.supported(engine))
    }) {
        return Err("PERMISSION_UNSUPPORTED");
    }
    Ok(())
}
/// Pinned SDK Chromium protocol names, flattened in request order.
pub(crate) fn chromium_permissions(permissions: &[Permission]) -> Vec<&'static str> {
    permissions
        .iter()
        .filter_map(|permission| permission.chromium())
        .flatten()
        .copied()
        .collect()
}

/// Observed CSS media queries after a media change, on every backend.
pub(crate) const MEDIA_OBSERVATION: &str = "({screen:matchMedia('screen').matches,print:matchMedia('print').matches,dark:matchMedia('(prefers-color-scheme: dark)').matches,light:matchMedia('(prefers-color-scheme: light)').matches,reducedMotion:matchMedia('(prefers-reduced-motion: reduce)').matches,forcedColors:matchMedia('(forced-colors: active)').matches,moreContrast:matchMedia('(prefers-contrast: more)').matches})";

impl MediaOptions {
    /// Pinned `Page.emulateMedia` merging: each given field replaces the page's.
    pub(crate) fn merged(&self, update: &Self) -> Self {
        Self {
            media: update.media.or(self.media),
            color_scheme: update.color_scheme.or(self.color_scheme),
            reduced_motion: update.reduced_motion.or(self.reduced_motion),
            forced_colors: update.forced_colors.or(self.forced_colors),
            contrast: update.contrast.or(self.contrast),
        }
    }
}

/// chrome-devtools-mcp `emulate` network presets, from its pinned Puppeteer
/// `PredefinedNetworkConditions`; `none` clears throttling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPreset {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "offline")]
    Offline,
    #[serde(rename = "slow-3g")]
    Slow3g,
    #[serde(rename = "fast-3g")]
    Fast3g,
    #[serde(rename = "slow-4g")]
    Slow4g,
    #[serde(rename = "fast-4g")]
    Fast4g,
}
impl NetworkPreset {
    /// Bytes per second down and up, and latency in milliseconds.
    pub(crate) fn conditions(self) -> Option<(f64, f64, f64)> {
        match self {
            Self::None | Self::Offline => None,
            Self::Slow3g => Some((
                500.0 * 1000.0 / 8.0 * 0.8,
                500.0 * 1000.0 / 8.0 * 0.8,
                400.0 * 5.0,
            )),
            // Puppeteer's Slow 4G is an alias of Fast 3G.
            Self::Fast3g | Self::Slow4g => Some((
                1.6 * 1000.0 * 1000.0 / 8.0 * 0.9,
                750.0 * 1000.0 / 8.0 * 0.9,
                150.0 * 3.75,
            )),
            Self::Fast4g => Some((
                9.0 * 1000.0 * 1000.0 / 8.0 * 0.9,
                1.5 * 1000.0 * 1000.0 / 8.0 * 0.9,
                60.0 * 2.75,
            )),
        }
    }
}

/// Chrome-only page CPU and network throttling; omitted fields keep their value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThrottlingOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkPreset>,
}
impl ThrottlingOptions {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.cpu_rate.is_none() && self.network.is_none() {
            return Err("EMPTY_THROTTLING_OPTIONS");
        }
        if self
            .cpu_rate
            .is_some_and(|rate| !rate.is_finite() || !(1.0..=20.0).contains(&rate))
        {
            return Err("INVALID_THROTTLING_OPTIONS");
        }
        Ok(())
    }
    pub(crate) fn merged(&self, update: &Self) -> Self {
        Self {
            cpu_rate: update.cpu_rate.or(self.cpu_rate),
            network: update.network.or(self.network),
        }
    }
}
pub(crate) fn throttling_schema(reference: &Value) -> Value {
    json!({"type":"object","properties":{"operation":{"const":"page_emulate_throttling"},"page":reference,"options":{"type":"object","properties":{"cpuRate":{"type":"number","minimum":1,"maximum":20,"description":"Slowdown; 1 clears"},"network":{"enum":["none","offline","slow-3g","fast-3g","slow-4g","fast-4g"]}},"additionalProperties":false}},"required":["operation","page","options"],"additionalProperties":false})
}

/// Pinned `serviceWorkers` context policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceWorkers {
    Allow,
    Block,
}
/// The pinned SDK's context init script for `serviceWorkers: "block"`.
pub(crate) const SERVICE_WORKER_BLOCK: &str = "\nif (self.ServiceWorkerContainer) ServiceWorkerContainer.prototype.register = async () => { console.warn('Service Worker registration blocked by Playwright'); };\n";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextSetting {
    HttpCredentials {
        #[serde(deserialize_with = "required_credentials")]
        credentials: Option<super::http::Credentials>,
    },
    ExtraHttpHeaders {
        headers: Vec<super::http::Header>,
    },
    Offline {
        offline: bool,
    },
    Geolocation {
        geolocation: Option<Geolocation>,
    },
    PermissionsGrant {
        origin: String,
        permissions: Vec<Permission>,
    },
    PermissionsClear {},
}
fn required_credentials<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<super::http::Credentials>, D::Error> {
    Option::deserialize(deserializer)
}
impl ContextSetting {
    pub(crate) fn summary(&self) -> Value {
        match self {
            Self::HttpCredentials { credentials } => {
                json!({"kind":"http_credentials","credentialCount":credentials.as_ref().map_or(0,super::http::Credentials::len)})
            }
            Self::ExtraHttpHeaders { headers } => {
                json!({"kind":"extra_http_headers","headerCount":headers.len()})
            }
            _ => serde_json::to_value(self).expect("Typed context setting"),
        }
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::HttpCredentials { credentials } => credentials
                .as_ref()
                .map_or(Ok(()), super::http::Credentials::validate),
            Self::ExtraHttpHeaders { headers } => {
                super::http::validate_headers(headers).map_err(|error| error.code)
            }
            Self::Geolocation {
                geolocation: Some(location),
            } => location.validate(),
            Self::PermissionsGrant {
                origin,
                permissions,
            } => {
                validate_origin(origin)?;
                validate_permissions(permissions, None)
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn wire(&self, engine: Engine) -> Result<(&'static str, Value), &'static str> {
        self.validate()?;
        match self {
            Self::HttpCredentials { credentials } => {
                if engine == Engine::Chrome {
                    return Err("CAPABILITY_UNAVAILABLE");
                }
                Ok((
                    "setHTTPCredentials",
                    credentials
                        .as_ref()
                        .and_then(super::http::Credentials::wire)
                        .map(|credentials| json!({"httpCredentials":credentials}))
                        .unwrap_or_else(|| json!({})),
                ))
            }
            Self::ExtraHttpHeaders { headers } => {
                Ok(("setExtraHTTPHeaders", json!({"headers":headers})))
            }
            Self::Offline { offline } => Ok(("setOffline", json!({"offline":offline}))),
            Self::Geolocation { geolocation } => Ok((
                "setGeolocation",
                match geolocation {
                    Some(location) => json!({"geolocation":location}),
                    None => json!({}),
                },
            )),
            Self::PermissionsClear {} => Ok(("clearPermissions", json!({}))),
            Self::PermissionsGrant {
                origin,
                permissions,
            } => {
                validate_permissions(permissions, Some(engine))?;
                Ok((
                    "grantPermissions",
                    json!({"origin":origin,"permissions":permissions}),
                ))
            }
        }
    }
}

/// Explicit ASCII HTTP(S) origins only: no wildcard, credentials, path or query.
/// The pinned backend applies its native URL-origin normalization.
pub(crate) fn validate_origin(origin: &str) -> Result<(), &'static str> {
    if origin.len() > 2048 || origin.contains('#') {
        return Err("INVALID_PERMISSION_ORIGIN");
    }
    let uri: axum::http::Uri = origin.parse().map_err(|_| "INVALID_PERMISSION_ORIGIN")?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.query().is_some()
        || !matches!(uri.path(), "" | "/")
    {
        return Err("INVALID_PERMISSION_ORIGIN");
    }
    let authority = uri.authority().ok_or("INVALID_PERMISSION_ORIGIN")?;
    if authority.as_str().contains('@') {
        return Err("INVALID_PERMISSION_ORIGIN");
    }
    let host = authority.host();
    let suffix = authority
        .as_str()
        .strip_prefix(host)
        .ok_or("INVALID_PERMISSION_ORIGIN")?;
    if !suffix.is_empty()
        && suffix
            .strip_prefix(':')
            .filter(|port| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|port| port.parse::<u16>().ok())
            .is_none()
    {
        return Err("INVALID_PERMISSION_ORIGIN");
    }
    if host.starts_with('[') {
        host.strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .and_then(|host| host.parse::<std::net::Ipv6Addr>().ok())
            .ok_or("INVALID_PERMISSION_ORIGIN")?;
    } else {
        let host = host.strip_suffix('.').unwrap_or(host);
        if host.is_empty()
            || host.len() > 253
            || host.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            })
        {
            return Err("INVALID_PERMISSION_ORIGIN");
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Media {
    Screen,
    Print,
    NoOverride,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    Dark,
    Light,
    NoPreference,
    NoOverride,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReducedMotion {
    Reduce,
    NoPreference,
    NoOverride,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForcedColors {
    Active,
    None,
    NoOverride,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Contrast {
    More,
    NoPreference,
    NoOverride,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Media>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<ColorScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_motion: Option<ReducedMotion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forced_colors: Option<ForcedColors>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<Contrast>,
}
impl MediaOptions {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.media.is_none()
            && self.color_scheme.is_none()
            && self.reduced_motion.is_none()
            && self.forced_colors.is_none()
            && self.contrast.is_none()
        {
            return Err("EMPTY_MEDIA_OPTIONS");
        }
        Ok(())
    }
}

/// Distinct permission names; each engine refuses names it does not map.
pub(crate) fn permission_schema() -> Value {
    json!({"type":"array","minItems":1,"maxItems":PERMISSIONS.len(),"uniqueItems":true,"items":{"enum":PERMISSIONS},"description":"Engine names: capabilities settings.permissionNames"})
}
/// Optional creation-time geolocation, as in `context_configure`.
pub(crate) fn geolocation_schema() -> Value {
    json!({"type":"object","properties":{"latitude":{"type":"number","minimum":-90,"maximum":90},"longitude":{"type":"number","minimum":-180,"maximum":180},"accuracy":{"type":"number","minimum":0,"maximum":1000000,"default":0}},"required":["latitude","longitude"],"additionalProperties":false})
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let location = geolocation_schema();
    let permissions = permission_schema();
    let settings = vec![
        object(
            json!({"kind":{"const":"offline"},"offline":{"type":"boolean"}}),
            &["kind", "offline"],
        ),
        object(
            json!({"kind":{"const":"geolocation"},"geolocation":{"anyOf":[location,{"type":"null"}]}}),
            &["kind", "geolocation"],
        ),
        object(
            json!({"kind":{"const":"permissions_grant"},"origin":{"type":"string","minLength":1,"maxLength":2048,"description":"Explicit ASCII HTTP(S) origin; no credentials, wildcard, path, query or fragment"},"permissions":permissions}),
            &["kind", "origin", "permissions"],
        ),
        object(json!({"kind":{"const":"permissions_clear"}}), &["kind"]),
        object(
            json!({"kind":{"const":"extra_http_headers"},"headers":super::http::header_schema()}),
            &["kind", "headers"],
        ),
        object(
            json!({"kind":{"const":"http_credentials"},"credentials":super::http::Credentials::schema()}),
            &["kind", "credentials"],
        ),
    ];
    let options = object(
        json!({"media":{"enum":["screen","print","no-override"]},"colorScheme":{"enum":["dark","light","no-preference","no-override"]},"reducedMotion":{"enum":["reduce","no-preference","no-override"]},"forcedColors":{"enum":["active","none","no-override"]},"contrast":{"enum":["more","no-preference","no-override"]}}),
        &[],
    );
    vec![
        object(
            json!({"operation":{"const":"context_configure"},"context":reference,"setting":{"oneOf":settings}}),
            &["operation", "context", "setting"],
        ),
        object(
            json!({"operation":{"const":"page_emulate_media"},"page":reference,"options":options}),
            &["operation", "page", "options"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origin_scope_and_engine_permissions_validate_before_native_mutation() {
        for origin in [
            "https://example.com",
            "https://EXAMPLE.COM./",
            "https://acme_dev.example.com",
            "http://localhost:8080/",
            "http://[::1]:8080",
        ] {
            validate_origin(origin).unwrap();
        }
        for origin in [
            "*",
            "https://user@example.com",
            "https://example.com/acme",
            "https://example.com/?secret=1",
            "https://example.com/#secret",
            "file:///acme",
            "https://*.example.com",
            "https://example.com:99999",
            "https://example.com:+80",
            "https://example.com/\0",
        ] {
            assert!(validate_origin(origin).is_err(), "{origin:?}");
        }
        let setting = ContextSetting::PermissionsGrant {
            origin: "https://example.com".into(),
            permissions: vec![Permission::Geolocation, Permission::Camera],
        };
        assert_eq!(
            setting.wire(Engine::Firefox).unwrap_err(),
            "PERMISSION_UNSUPPORTED"
        );
        assert_eq!(setting.wire(Engine::Webkit).unwrap().0, "grantPermissions");
        assert!(
            validate_permissions(
                &[Permission::Geolocation, Permission::Camera],
                Some(Engine::Chrome)
            )
            .is_ok()
        );
        for unmapped in [Permission::PersistentStorage, Permission::Push] {
            assert_eq!(
                validate_permissions(&[unmapped], Some(Engine::Chrome)),
                Err("PERMISSION_UNSUPPORTED")
            );
        }
        assert_eq!(
            chromium_permissions(&[
                Permission::Camera,
                Permission::Gyroscope,
                Permission::LocalNetworkAccess
            ]),
            [
                "videoCapture",
                "sensors",
                "localNetworkAccess",
                "localNetwork",
                "loopbackNetwork"
            ]
        );
        assert_eq!(validate_permissions(&[], None), Err("INVALID_PERMISSIONS"));
        let duplicated = ContextSetting::PermissionsGrant {
            origin: "https://example.com".into(),
            permissions: vec![Permission::Geolocation, Permission::Geolocation],
        };
        assert_eq!(duplicated.validate(), Err("INVALID_PERMISSIONS"));
    }
    #[test]
    fn geolocation_bounds_and_explicit_media_reset_preserve_wire_meaning() {
        assert!(
            Geolocation {
                latitude: f64::NAN,
                longitude: 0.0,
                accuracy: 0.0
            }
            .validate()
            .is_err()
        );
        assert!(
            Geolocation {
                latitude: 0.0,
                longitude: 181.0,
                accuracy: 0.0
            }
            .validate()
            .is_err()
        );
        assert!(
            Geolocation {
                latitude: 0.0,
                longitude: 0.0,
                accuracy: -1.0
            }
            .validate()
            .is_err()
        );
        assert_eq!(
            ContextSetting::Geolocation { geolocation: None }
                .wire(Engine::Firefox)
                .unwrap()
                .1,
            json!({})
        );
        assert!(MediaOptions::default().validate().is_err());
        let options = MediaOptions {
            color_scheme: Some(ColorScheme::NoOverride),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(options).unwrap(),
            json!({"colorScheme":"no-override"})
        );
    }
}
