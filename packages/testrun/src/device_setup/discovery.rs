//! Read-only Apple inventory, reduced to device-identification fields.
use super::*;
use std::path::PathBuf;

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub(crate) async fn inventory() -> Result<BTreeMap<String, BTreeMap<String, Value>>> {
    if !cfg!(target_os = "macos") {
        return Ok(BTreeMap::new());
    }
    let directory =
        Temporary(std::env::temp_dir().join(format!("testrun-inventory-{}", uuid::Uuid::new_v4())));
    crate::artifacts::directory(&directory.0)?;
    let output = directory.0.join("inventory.json");
    crate::process::run(
        "/usr/bin/xcrun",
        &[
            "devicectl",
            "list",
            "devices",
            "--json-output",
            output
                .to_str()
                .ok_or_else(|| Error::invalid("Temporary path is not UTF-8"))?,
        ],
        Duration::from_secs(15),
        1024 * 1024,
    )
    .await?;
    let bytes = std::fs::read(output).map_err(|_| {
        Error::new(
            "PLATFORM_PROTOCOL",
            "Apple device inventory did not return JSON",
        )
    })?;
    parse(&bytes)
}
/// Whether `model` is an Apple product type such as `iPhone14,7` rather than
/// a marketing name.
pub(crate) fn is_product_type(model: &str) -> bool {
    let Some(numbers) = ["iPhone", "iPad"]
        .iter()
        .find_map(|family| model.strip_prefix(family))
    else {
        return false;
    };
    numbers.split_once(',').is_some_and(|(major, minor)| {
        [major, minor]
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    })
}

/// Marketing names by product type from the selected Xcode's device traits
/// table, for phones whose inventory omits the name. `None` without Xcode or
/// the table; the product type then stays the model.
pub(crate) async fn product_names() -> Option<BTreeMap<String, String>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let developer = crate::process::run(
        "/usr/bin/xcode-select",
        &["-p"],
        Duration::from_secs(5),
        64 * 1024,
    )
    .await
    .ok()?;
    let developer = String::from_utf8(developer.bytes).ok()?;
    let table = std::path::Path::new(developer.trim())
        .join("Platforms/iPhoneOS.platform/usr/standalone/device_traits.db");
    let output = crate::process::run(
        "/usr/bin/sqlite3",
        &[
            "-readonly",
            table.to_str()?,
            "select ProductType, ProductDescription from Devices",
        ],
        Duration::from_secs(5),
        1024 * 1024,
    )
    .await
    .ok()?;
    Some(parse_names(&output.bytes))
}

fn parse_names(bytes: &[u8]) -> BTreeMap<String, String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let (product, name) = line.split_once('|')?;
            (is_product_type(product) && !name.trim().is_empty())
                .then(|| (product.to_owned(), name.trim().to_owned()))
        })
        .collect()
}

fn parse(bytes: &[u8]) -> Result<BTreeMap<String, BTreeMap<String, Value>>> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Apple device inventory"))?;
    let devices = value["result"]["devices"]
        .as_array()
        .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Apple inventory omitted devices"))?;
    let mut result = BTreeMap::new();
    for device in devices {
        let Some(serial) = device["hardwareProperties"]["udid"]
            .as_str()
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if device["connectionProperties"]["tunnelState"] == "unavailable" {
            continue;
        }
        let hardware = &device["hardwareProperties"];
        let properties = &device["deviceProperties"];
        let connection = &device["connectionProperties"];
        let transport = match connection["transportType"].as_str() {
            Some("wired") => "usb",
            Some("localNetwork") => "network",
            _ => "unknown",
        };
        let fields = BTreeMap::from([
            ("name".into(), properties["name"].clone()),
            // devicectl can omit the marketing name, as for a phone without
            // developer support; the product type still identifies the model.
            (
                "model".into(),
                Some(&hardware["marketingName"])
                    .filter(|name| name.is_string())
                    .unwrap_or(&hardware["productType"])
                    .clone(),
            ),
            ("productType".into(), hardware["productType"].clone()),
            ("platform".into(), hardware["platform"].clone()),
            ("osVersion".into(), properties["osVersionNumber"].clone()),
            ("transport".into(), json!(transport)),
            ("pairingState".into(), connection["pairingState"].clone()),
            (
                "developerMode".into(),
                properties["developerModeStatus"].clone(),
            ),
            (
                "controlSupported".into(),
                json!(transport == "usb" && hardware["platform"] == "iOS"),
            ),
        ]);
        result.insert(serial.into(), fields);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_preserves_identity_and_marks_network_control_unsupported() {
        let value = json!({"result":{"devices":[{"hardwareProperties":{"udid":"Acme-phone","marketingName":"iPhone mini","productType":"iPhone13,1","platform":"iOS","serialNumber":"private"},"deviceProperties":{"name":"Acme phone","osVersionNumber":"27.0.1","developerModeStatus":"enabled"},"connectionProperties":{"transportType":"localNetwork","pairingState":"paired"}}]}});
        let result = parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(result["Acme-phone"]["name"], "Acme phone");
        assert_eq!(result["Acme-phone"]["transport"], "network");
        assert_eq!(result["Acme-phone"]["controlSupported"], false);
        assert!(!serde_json::to_string(&result).unwrap().contains("private"));
        assert!(parse(br#"{"result":{}}"#).is_err());
        let unnamed = json!({"result":{"devices":[{"hardwareProperties":{"udid":"Acme-phone","productType":"iPhone14,7","platform":"iOS"},"deviceProperties":{},"connectionProperties":{"transportType":"wired"}}]}});
        let result = parse(&serde_json::to_vec(&unnamed).unwrap()).unwrap();
        assert_eq!(result["Acme-phone"]["model"], "iPhone14,7");
    }

    #[test]
    fn device_traits_rows_name_product_types() {
        let names = parse_names(
            b"iPhone13,1|iPhone 12 mini\niPhone14,7|iPhone 14\nWatch6,1|Apple Watch\niPad8,1|\nmalformed\n",
        );
        assert_eq!(names.len(), 2);
        assert_eq!(names["iPhone14,7"], "iPhone 14");
        for model in ["iPhone14,7", "iPad13,18"] {
            assert!(is_product_type(model), "{model}");
        }
        for model in ["iPhone 14", "iPhone14", "iPhone,7", "iPhone14,", "Watch6,1"] {
            assert!(!is_product_type(model), "{model}");
        }
    }
}
