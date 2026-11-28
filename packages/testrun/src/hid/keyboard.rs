//! Physical virtual-keyboard registration and complete pressed-set reports.
use crate::{
    error::{Error, Result},
    keyboard::Stroke,
};
use idevice::xpc::{Dictionary, XPCObject as X};
use serde_json::Value;

// Universal Control's keyboard descriptor and typed CoreDevice property envelope.
// Protocol reference: pymobiledevice3/remote/core_device/hid_service.py.
const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, 0x15, 0x00, 0x25, 0x01,
    0x95, 0x08, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x05, 0x07, 0x19, 0x00,
    0x29, 0xff, 0x15, 0x00, 0x26, 0xff, 0x00, 0x95, 0x06, 0x75, 0x08, 0x81, 0x00, 0x05, 0x08, 0x19,
    0x01, 0x29, 0x05, 0x15, 0x00, 0x25, 0x01, 0x95, 0x05, 0x75, 0x01, 0x91, 0x02, 0x95, 0x01, 0x75,
    0x03, 0x91, 0x01, 0xc0,
];
fn dict(items: impl IntoIterator<Item = (&'static str, X)>) -> X {
    X::Dictionary(
        items
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}
fn tagged(tag: &'static str, value: X) -> X {
    dict([(tag, value)])
}
/// Wait after a confirmed registration before the first key. iOS reports no
/// attachment readiness, and registration also swaps out the software keyboard.
pub(super) const ATTACH_SETTLE: std::time::Duration = std::time::Duration::from_millis(600);
/// A key pressed and released once after registration, before the caller's
/// keys. Some phones ignore the first key press of a new keyboard, whatever the
/// wait; this key has no effect of its own.
pub(super) const PRIME: u32 = 225;
/// Keys sent right after registration get slower pacing while iOS finishes
/// switching to the hardware keyboard.
const WARM_UP_STROKES: usize = 3;

/// How long a key (and its modifiers) stays down, and the pause after release.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Pace {
    pub(super) hold: std::time::Duration,
    pub(super) gap: std::time::Duration,
}
pub(super) fn pace(registered_now: bool, index: usize) -> Pace {
    let (hold, gap) = if registered_now && index < WARM_UP_STROKES {
        (80, 60)
    } else {
        (40, 20)
    };
    Pace {
        hold: std::time::Duration::from_millis(hold),
        gap: std::time::Duration::from_millis(gap),
    }
}
pub(super) fn create(service_id: u64) -> Dictionary {
    let pair = dict([
        ("DeviceUsage", X::Int64(6)),
        ("DeviceUsagePage", X::Int64(1)),
    ]);
    let stored_pair = dict([
        ("DeviceUsage", tagged("int", X::Int64(6))),
        ("DeviceUsagePage", tagged("int", X::Int64(1))),
    ]);
    let storage = dict([
        (
            "Manufacturer",
            tagged("string", X::String("Canonic".into())),
        ),
        (
            "Product",
            tagged("string", X::String("Testrun keyboard".into())),
        ),
        ("ProductID", tagged("int", X::Int64(0x0250))),
        ("VendorID", tagged("int", X::Int64(0x05ac))),
        ("PrimaryUsage", tagged("int", X::Int64(6))),
        ("PrimaryUsagePage", tagged("int", X::Int64(1))),
        (
            "DeviceUsagePairs",
            tagged("array", X::Array(vec![tagged("dictionary", stored_pair)])),
        ),
        ("Transport", tagged("string", X::String("USB".into()))),
        (
            "ReportDescriptor",
            tagged("data", X::Data(DESCRIPTOR.into())),
        ),
        (
            "UniversalControlVirtualService",
            tagged("bool", X::Bool(true)),
        ),
        ("_ServiceID", tagged("uint", X::UInt64(service_id))),
    ]);
    let properties = dict([
        ("DeviceUsagePairs", X::Array(vec![pair])),
        ("PrimaryUsage", X::UInt64(6)),
        ("PrimaryUsagePage", X::UInt64(1)),
        ("Product", X::String("Testrun keyboard".into())),
        ("ProductID", X::Int64(0x0250)),
        ("VendorID", X::Int64(0x05ac)),
        ("_CoreDevice_codablePropertyStorage", storage),
        ("_ServiceID", X::UInt64(service_id)),
    ]);
    [("createService".into(), dict([("_0", properties)]))]
        .into_iter()
        .collect()
}
pub(super) fn remove(service_id: u64) -> Dictionary {
    [(
        "removeService".into(),
        dict([("_0", X::UInt64(service_id))]),
    )]
    .into_iter()
    .collect()
}
pub(super) fn list() -> Dictionary {
    [("connectedServices".into(), X::Dictionary(Dictionary::new()))]
        .into_iter()
        .collect()
}
pub(super) fn absent(reply: &Value, service_id: u64) -> bool {
    reply["connectedServices"]
        .as_array()
        .is_some_and(|services| {
            services.iter().all(|service| {
                service["_ServiceID"]
                    .as_u64()
                    .is_some_and(|id| id != service_id)
            })
        })
}
pub(super) fn strokes(operation: &str, args: &Value) -> Result<Vec<Stroke>> {
    crate::mobile::validate_backend_options("device", Some("hid"), operation, args)?;
    if operation == "type_text" {
        crate::keyboard::text(
            args["text"]
                .as_str()
                .ok_or_else(|| Error::invalid("text is required"))?,
        )
    } else {
        let modifiers: Vec<String> = serde_json::from_value(
            args.get("modifiers")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .map_err(|_| Error::invalid("Invalid key modifiers"))?;
        Ok(vec![crate::keyboard::key(
            args["key"]
                .as_str()
                .ok_or_else(|| Error::invalid("key is required"))?,
            &modifiers,
        )?])
    }
}
pub(super) fn modifiers(stroke: Stroke) -> Vec<u32> {
    [(1, 225), (2, 224), (4, 226), (8, 227)]
        .into_iter()
        .filter_map(|(bit, usage)| (stroke.modifiers & bit != 0).then_some(usage))
        .collect()
}
pub(super) fn report(usages: &[u32], timestamp: u64) -> Vec<u8> {
    let mut bytes = vec![0; 39];
    bytes[0] = 1;
    for &usage in usages {
        debug_assert!(usage < 240);
        bytes[1 + usage as usize / 8] |= 1 << (usage % 8);
    }
    bytes[31..37].copy_from_slice(&timestamp.to_le_bytes()[..6]);
    bytes
}
