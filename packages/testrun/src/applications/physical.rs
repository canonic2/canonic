//! Read-only physical app inventory over the selected USB installation-proxy service.
use crate::error::{Error, Result};
use idevice::{Idevice, IdeviceService};
use serde_json::{Value, json};
use std::time::Duration;

struct Inventory(Idevice);
impl IdeviceService for Inventory {
    fn service_name() -> std::borrow::Cow<'static, str> {
        "com.apple.mobile.installation_proxy".into()
    }
    async fn from_stream(device: Idevice) -> std::result::Result<Self, idevice::IdeviceError> {
        Ok(Self(device))
    }
}

pub(super) async fn inventory(serial: &str) -> Result<Value> {
    parse(&lookup(serial).await?)
}

pub(super) async fn executable(serial: &str, bundle: &str) -> Result<String> {
    let response = lookup(serial).await?;
    parse(&response)?;
    let fields = response
        .get("LookupResult")
        .and_then(plist::Value::as_dictionary)
        .and_then(|apps| apps.get(bundle))
        .and_then(plist::Value::as_dictionary)
        .ok_or_else(|| Error::new("APP_NOT_INSTALLED", "The requested app is not installed"))?;
    let path = fields
        .get("Path")
        .and_then(plist::Value::as_string)
        .filter(|path| path.starts_with('/') && !path.split('/').any(|part| part == ".."))
        .ok_or_else(|| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "App inventory omitted its absolute bundle path",
            )
        })?;
    let name = fields
        .get("CFBundleExecutable")
        .and_then(plist::Value::as_string)
        .filter(|name| !name.is_empty() && !name.contains('/') && *name != "." && *name != "..")
        .ok_or_else(|| {
            Error::new(
                "PLATFORM_PROTOCOL",
                "App inventory omitted its executable name",
            )
        })?;
    Ok(format!("{}/{name}", path.trim_end_matches('/')))
}

async fn lookup(serial: &str) -> Result<plist::Dictionary> {
    tokio::time::timeout(Duration::from_secs(15), async {
        let provider = crate::device_setup::provider(serial).await?;
        let mut client = Inventory::connect(&provider).await.map_err(crate::device_media::platform)?;
        let request = json!({
            "Command":"Lookup",
            "ClientOptions":{
                "ApplicationType":"Any",
                "ReturnAttributes":["CFBundleIdentifier","CFBundleName","CFBundleDisplayName","CFBundleShortVersionString","CFBundleVersion","Path","CFBundleExecutable"]
            }
        });
        let mut bytes=Vec::new();
        plist::to_writer_xml(&mut bytes,&request).map_err(|_|Error::new("INTERNAL","Cannot encode app inventory request"))?;
        let mut framed=(bytes.len() as u32).to_be_bytes().to_vec();
        framed.extend(bytes);
        client.0.send_raw(&framed).await.map_err(crate::device_media::platform)?;
        let header=client.0.read_raw(4).await.map_err(crate::device_media::platform)?;
        let length=u32::from_be_bytes(header.try_into().map_err(|_|Error::new("PLATFORM_PROTOCOL","Invalid app inventory frame"))?) as usize;
        if length==0 || length>4*1024*1024 { return Err(Error::new("OUTPUT_LIMIT","Physical app inventory exceeds its bounded response size")); }
        let bytes=client.0.read_raw(length).await.map_err(crate::device_media::platform)?;
        let response=plist::from_bytes(&bytes).map_err(|_|Error::new("PLATFORM_PROTOCOL","Invalid app inventory reply"))?;
        Ok(response)
    }).await.map_err(|_|Error::new("TIMEOUT","Physical app inventory exceeded fifteen seconds"))?
}

fn parse(response: &plist::Dictionary) -> Result<Value> {
    let values = response
        .get("LookupResult")
        .and_then(plist::Value::as_dictionary)
        .filter(|values| !values.is_empty())
        .ok_or_else(|| {
            Error::new(
                "CAPABILITY_UNAVAILABLE",
                "The selected phone did not provide a readable installed-app inventory",
            )
        })?;
    let mut apps = Vec::new();
    for (bundle, fields) in values {
        super::package::validate_id(bundle)?;
        let fields = fields
            .as_dictionary()
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Invalid physical app metadata"))?;
        let text = |key: &str| fields.get(key).and_then(plist::Value::as_string);
        if text("CFBundleIdentifier").is_some_and(|identity| identity != bundle) {
            return Err(Error::new(
                "PLATFORM_PROTOCOL",
                "Physical app identity does not match its inventory key",
            ));
        }
        apps.push(json!({"appId":bundle,"name":text("CFBundleDisplayName").or_else(||text("CFBundleName")).unwrap_or(bundle),"version":text("CFBundleShortVersionString").unwrap_or(""),"build":text("CFBundleVersion").unwrap_or("")}));
    }
    apps.sort_by(|a, b| a["appId"].as_str().cmp(&b["appId"].as_str()));
    Ok(json!({"apps":apps}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_requires_real_identity_and_observed_versions() {
        let mut fields = plist::Dictionary::new();
        fields.insert("CFBundleIdentifier".into(), "com.example.acme".into());
        fields.insert("CFBundleVersion".into(), "42".into());
        let mut apps = plist::Dictionary::new();
        apps.insert("com.example.acme".into(), fields.clone().into());
        let mut reply = plist::Dictionary::new();
        reply.insert("LookupResult".into(), apps.into());
        let result = parse(&reply).unwrap();
        assert_eq!(result["apps"][0]["build"], "42");
        assert_eq!(result["apps"][0]["version"], "");
        fields.insert("CFBundleIdentifier".into(), "com.example.other".into());
        reply
            .get_mut("LookupResult")
            .unwrap()
            .as_dictionary_mut()
            .unwrap()
            .insert("com.example.acme".into(), fields.into());
        assert_eq!(parse(&reply).unwrap_err().code, "PLATFORM_PROTOCOL");
        assert!(parse(&plist::Dictionary::new()).is_err());
        reply.insert("LookupResult".into(), plist::Dictionary::new().into());
        assert!(parse(&reply).is_err());
    }
}
