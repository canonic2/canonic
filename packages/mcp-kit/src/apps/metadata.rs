//! Typed tool linkage and resource UI metadata, serialized without cloning custom maps.
use super::{AppsError, validate_uri};
use crate::Metadata;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use std::collections::HashSet;

/// Where a host may expose a tool. These hints grant no authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiVisibility {
    /// Available to the model.
    Model,
    /// Available to the app frame through its host.
    App,
}

/// Typed tool `_meta.ui`; validated at catalog construction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolUi {
    /// Static UI document linked to this tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_uri: Option<String>,
    /// Omitted means both model and app; explicit arrays must be nonempty and unique.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Vec<UiVisibility>>,
}

impl ToolUi {
    /// Links a tool to a document, with default visibility to model and app.
    #[must_use]
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            resource_uri: Some(uri.into()),
            visibility: None,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), AppsError> {
        if let Some(uri) = &self.resource_uri {
            validate_uri(uri)?;
        }
        if self.visibility.as_ref().is_some_and(|values| {
            values.is_empty() || values.len() > 2 || (values.len() == 2 && values[0] == values[1])
        }) {
            return Err(AppsError::new(
                self.resource_uri.as_deref().unwrap_or_default(),
                "visibility",
            ));
        }
        Ok(())
    }

    pub(crate) fn app_only(&self) -> bool {
        self.visibility.as_deref() == Some(&[UiVisibility::App])
    }
}

/// Origins requested for the host's Content Security Policy; the host enforces them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiCsp {
    /// HTTPS/WSS connection origins, with loopback HTTP/WS for development.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connect_domains: Vec<String>,
    /// HTTPS origins for scripts, images, styles and other assets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_domains: Vec<String>,
    /// HTTPS origins for nested frames.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_domains: Vec<String>,
    /// HTTPS origins for the document base URI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_uri_domains: Vec<String>,
}

/// An empty permission declaration; grants are decided by the host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiPermission {}

/// Browser permissions requested from the host, without any implied grant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiPermissions {
    /// Camera access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<UiPermission>,
    /// Microphone access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub microphone: Option<UiPermission>,
    /// Geolocation access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geolocation: Option<UiPermission>,
    /// Clipboard writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_write: Option<UiPermission>,
}

/// Resource `_meta.ui`, published identically on listing and read content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiMetadata {
    /// Origins requested for network and resource access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csp: Option<UiCsp>,
    /// Requested browser permissions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<UiPermissions>,
    /// Opaque dedicated sandbox origin, interpreted by the host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Whether the host should show a border.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefers_border: Option<bool>,
}

impl UiMetadata {
    pub(super) fn validate(&self) -> Result<(), &'static str> {
        if self.domain.as_ref().is_some_and(|domain| {
            domain.is_empty()
                || domain.len() > 1024
                || domain.chars().any(|c| c == '\0' || c.is_whitespace())
        }) {
            return Err("domain");
        }
        if let Some(csp) = &self.csp {
            for (domains, connect) in [
                (&csp.connect_domains, true),
                (&csp.resource_domains, false),
                (&csp.frame_domains, false),
                (&csp.base_uri_domains, false),
            ] {
                let mut seen = HashSet::new();
                if domains.len() > 64
                    || domains
                        .iter()
                        .any(|domain| !origin(domain, connect) || !seen.insert(domain))
                {
                    return Err("CSP origins");
                }
            }
        }
        Ok(())
    }
}

fn origin(value: &str, connect: bool) -> bool {
    if value.is_empty() || value.len() > 1024 {
        return false;
    }
    let Some((scheme, authority)) = value.split_once("://") else {
        return false;
    };
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
        return false;
    }
    let (host, port) = if let Some(ipv6) = authority.strip_prefix('[') {
        let Some((ip, suffix)) = ipv6.split_once(']') else {
            return false;
        };
        if ip.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        let port = if suffix.is_empty() {
            None
        } else {
            let Some(port) = suffix.strip_prefix(':') else {
                return false;
            };
            Some(port)
        };
        (&authority[..ip.len() + 2], port)
    } else {
        authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)))
    };
    if port.is_some_and(|port| {
        port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) || port.parse::<u16>().is_err()
    }) {
        return false;
    }
    let loopback = ["localhost", "127.0.0.1", "[::1]"].contains(&host);
    let secure = scheme == "https" || (connect && scheme == "wss");
    if !secure && !(loopback && (scheme == "http" || (connect && scheme == "ws"))) {
        return false;
    }
    if host.starts_with('[') {
        return true;
    }
    let name = host.strip_prefix("*.").unwrap_or(host);
    !name.is_empty()
        && name.split('.').all(|label| {
            !label.is_empty()
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

pub(crate) struct WithUi<'a, T> {
    pub(crate) custom: Option<&'a Metadata>,
    pub(crate) ui: Option<&'a T>,
}

impl<T: Serialize> Serialize for WithUi<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let Some(custom) = self.custom {
            for (key, value) in custom.as_map() {
                map.serialize_entry(key, value)?;
            }
        }
        if let Some(ui) = self.ui {
            map.serialize_entry("ui", ui)?;
        }
        map.end()
    }
}

pub(crate) fn validate_custom(custom: Option<&Metadata>) -> Result<(), &'static str> {
    if custom.is_some_and(|meta| {
        meta.as_map().contains_key("ui") || meta.as_map().contains_key("ui/resourceUri")
    }) {
        return Err("reserved UI metadata");
    }
    Ok(())
}

pub(super) fn merged(custom: Option<&Metadata>, ui: &UiMetadata) -> Result<Metadata, &'static str> {
    validate_custom(custom)?;
    Metadata::new(
        serde_json::to_value(WithUi {
            custom,
            ui: Some(ui),
        })
        .map_err(|_| "UI metadata")?,
    )
    .map_err(|_| "UI metadata bytes")
}

pub(crate) fn tool_metadata(
    custom: Option<&Metadata>,
    ui: Option<&ToolUi>,
) -> Result<(), &'static str> {
    validate_custom(custom)?;
    crate::json::measure(
        &WithUi { custom, ui },
        crate::metadata::MAX_METADATA_BYTES,
        &|| Ok(()),
    )
    .map_err(|_| "UI metadata bytes")?;
    if let Some(ui) = ui {
        ui.validate().map_err(|_| "UI tool metadata")?;
    }
    Ok(())
}
