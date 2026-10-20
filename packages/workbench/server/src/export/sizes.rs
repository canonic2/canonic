//! The sizes an export captures a page at, as `contracts/src/sizes/` decides
//! them: every size the page supports but Resizable, filled axes and Fit at
//! the export's fixed canvas, each distinct pair of dimensions once. Pure.
use serde_json::{Value, json};

/// What a filled axis, and Fit, measure in an export: there is no canvas there.
pub(super) const EXPORT_FILL: (u32, u32) = (1440, 900);
const FILL_FLOOR: f64 = 320.0;
const MAX_LENGTH: f64 = 8192.0;

/// One size an export captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExportSize {
    pub(super) key: String,
    pub(super) label: String,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// The sizes a space has when it declares none.
pub(crate) fn default_sizes() -> Vec<Value> {
    vec![
        json!({ "key": "fit", "label": "Fit", "icon": "minimize-2", "button": true, "kind": "fit", "width": null, "height": null }),
        json!({ "key": "laptop", "label": "Laptop", "icon": "monitor", "button": true, "kind": "fixed", "width": 1512, "height": 982 }),
        json!({ "key": "mobile", "label": "Mobile", "icon": "smartphone", "button": true, "kind": "fixed", "width": 393, "height": 852 }),
        json!({ "key": "resizable", "label": "Resizable", "icon": "scaling", "button": true, "kind": "resizable", "width": null, "height": null }),
    ]
}

/// The sizes a page supports, in its order: its listed keys, mapped to its own
/// sizes or the space's; every size when it lists none it can show.
pub(crate) fn supported(space: &[Value], keys: Option<&[Value]>, own: &[Value]) -> Vec<Value> {
    let everything = || space.iter().chain(own).cloned().collect::<Vec<_>>();
    let Some(keys) = keys else {
        return everything();
    };
    let mut out: Vec<Value> = Vec::new();
    for key in keys {
        let found = own
            .iter()
            .find(|size| size["key"] == *key)
            .or_else(|| space.iter().find(|size| size["key"] == *key));
        if let Some(size) = found
            && !out.contains(size)
        {
            out.push(size.clone());
        }
    }
    if out.is_empty() { everything() } else { out }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 320–8192"
)]
fn axis(length: &Value, room: u32) -> u32 {
    length.as_f64().map_or_else(
        || f64::from(room).round().clamp(FILL_FLOOR, MAX_LENGTH) as u32,
        |pixels| pixels as u32,
    )
}

fn dimensions(size: &Value) -> Option<(u32, u32)> {
    match size["kind"].as_str() {
        Some("resizable") => None,
        Some("fit") => Some((
            axis(&Value::Null, EXPORT_FILL.0),
            axis(&Value::Null, EXPORT_FILL.1),
        )),
        _ => Some((
            axis(&size["width"], EXPORT_FILL.0),
            axis(&size["height"], EXPORT_FILL.1),
        )),
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// The sizes an export captures a page at.
pub(super) fn export_sizes(sizes: &[Value]) -> Vec<ExportSize> {
    let mut out: Vec<ExportSize> = Vec::new();
    for size in sizes {
        let Some((width, height)) = dimensions(size) else {
            continue;
        };
        if out
            .iter()
            .any(|seen| seen.width == width && seen.height == height)
        {
            continue;
        }
        out.push(ExportSize {
            key: text(&size["key"]),
            label: text(&size["label"]),
            width,
            height,
        });
    }
    if out.is_empty() {
        let (key, label) = sizes
            .first()
            .map_or(("fit".to_owned(), "Fit".to_owned()), |only| {
                (text(&only["key"]), text(&only["label"]))
            });
        out.push(ExportSize {
            key,
            label,
            width: EXPORT_FILL.0,
            height: EXPORT_FILL.1,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_exports_its_listed_sizes_but_resizable_with_fit_at_the_export_canvas() {
        let keys = [
            json!("laptop"),
            json!("mobile"),
            json!("resizable"),
            json!("fit"),
        ];
        let sizes = export_sizes(&supported(&default_sizes(), Some(&keys), &[]));
        let shown: Vec<(&str, u32, u32)> = sizes
            .iter()
            .map(|size| (size.key.as_str(), size.width, size.height))
            .collect();
        assert_eq!(
            shown,
            [
                ("laptop", 1512, 982),
                ("mobile", 393, 852),
                ("fit", 1440, 900)
            ]
        );
    }

    #[test]
    fn duplicate_dimensions_are_captured_once_and_resizable_alone_still_once() {
        let own = [
            json!({ "key": "sidebar", "label": "Sidebar", "kind": "fixed", "width": 340, "height": "fill" }),
        ];
        let keys = [json!("fit"), json!("sidebar"), json!("unknown")];
        let sizes = export_sizes(&supported(&default_sizes(), Some(&keys), &own));
        assert_eq!(
            sizes[1],
            ExportSize {
                key: "sidebar".into(),
                label: "Sidebar".into(),
                width: 340,
                height: 900
            }
        );
        let wide = [
            json!({ "key": "wide", "label": "Wide", "kind": "fixed", "width": 1440, "height": "fill" }),
        ];
        assert_eq!(
            export_sizes(&[default_sizes()[0].clone(), wide[0].clone()]).len(),
            1
        );
        let only = export_sizes(&[default_sizes()[3].clone()]);
        assert_eq!(
            only,
            [ExportSize {
                key: "resizable".into(),
                label: "Resizable".into(),
                width: 1440,
                height: 900
            }]
        );
        assert_eq!(
            supported(&default_sizes(), Some(&[json!("missing")]), &[]).len(),
            4
        );
    }
}
