//! Typed annotation and PDF options plus immutable semantic-snapshot evidence.
use super::{contract::Reference, page::Locator, session::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub locator: Locator,
    #[serde(default)]
    pub label: String,
}
pub(crate) fn validate_markers(markers: &[Marker]) -> Result<(), &'static str> {
    if markers.len() > 32 {
        return Err("ANNOTATION_BOUNDS");
    }
    for marker in markers {
        marker.locator.selector()?;
        if marker.label.len() > 128 {
            return Err("ANNOTATION_BOUNDS");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfOptions {
    #[serde(default)]
    pub landscape: bool,
    #[serde(default)]
    pub print_background: bool,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default = "paper_width")]
    pub paper_width: f64,
    #[serde(default = "paper_height")]
    pub paper_height: f64,
    #[serde(default = "margin")]
    pub margin_top: f64,
    #[serde(default = "margin")]
    pub margin_bottom: f64,
    #[serde(default = "margin")]
    pub margin_left: f64,
    #[serde(default = "margin")]
    pub margin_right: f64,
    #[serde(default)]
    pub page_ranges: String,
    #[serde(default)]
    pub display_header_footer: bool,
    #[serde(default)]
    pub header_template: String,
    #[serde(default)]
    pub footer_template: String,
    #[serde(default, rename = "preferCSSPageSize")]
    pub prefer_css_page_size: bool,
    #[serde(default = "yes")]
    pub tagged: bool,
    #[serde(default)]
    pub outline: bool,
}
fn one() -> f64 {
    1.0
}
fn paper_width() -> f64 {
    8.5
}
fn paper_height() -> f64 {
    11.0
}
fn margin() -> f64 {
    0.4
}
fn yes() -> bool {
    true
}
impl Default for PdfOptions {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("PDF defaults")
    }
}
impl PdfOptions {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !self.scale.is_finite()
            || !(0.1..=2.0).contains(&self.scale)
            || [self.paper_width, self.paper_height]
                .iter()
                .any(|n| !n.is_finite() || *n <= 0.0 || *n > 200.0)
            || [
                self.margin_top,
                self.margin_bottom,
                self.margin_left,
                self.margin_right,
            ]
            .iter()
            .any(|n| !n.is_finite() || *n < 0.0 || *n > 200.0)
            || self.margin_top + self.margin_bottom >= self.paper_height
            || self.margin_left + self.margin_right >= self.paper_width
            || self.page_ranges.len() > 256
            || !self
                .page_ranges
                .bytes()
                .all(|b| b.is_ascii_digit() || b" ,-".contains(&b))
            || self.header_template.len() > 16384
            || self.footer_template.len() > 16384
        {
            return Err("INVALID_PDF_OPTIONS");
        }
        Ok(())
    }
    pub(crate) fn params(&self) -> Value {
        json!({"landscape":self.landscape,"printBackground":self.print_background,"scale":self.scale,"paperWidth":self.paper_width,"paperHeight":self.paper_height,"marginTop":self.margin_top,"marginBottom":self.margin_bottom,"marginLeft":self.margin_left,"marginRight":self.margin_right,"pageRanges":self.page_ranges,"displayHeaderFooter":self.display_header_footer,"headerTemplate":self.header_template,"footerTemplate":self.footer_template,"preferCSSPageSize":self.prefer_css_page_size,"generateTaggedPDF":self.tagged,"generateDocumentOutline":self.outline,"transferMode":"ReturnAsStream"})
    }
}
pub(crate) fn annotation_options(frame: Reference, markers: Vec<Marker>) -> Result<Value, Error> {
    Ok(
        json!({"id":Uuid::new_v4(),"markers":markers.into_iter().map(|marker|Ok(json!({"selector":marker.locator.scoped_selector(frame)?,"label":marker.label}))).collect::<Result<Vec<Value>,Error>>()?}),
    )
}
pub(crate) const ANNOTATE: &str = include_str!("reading/annotations.js");
pub(crate) async fn snapshot(
    directory: std::path::PathBuf,
    page: Reference,
    frame: Value,
    result: Value,
) -> Result<Value, Error> {
    if result["truncated"] == true {
        let mut error = Error::from("SNAPSHOT_BOUNDS");
        error.partial = Some(json!({
            "page":page,"frame":frame,"narrowingRequired":true,"complete":false,
            "hint":"Retry page_snapshot with a locator for a smaller region, dialog, or container.",
            "preview":result["value"]
        }));
        return Err(error);
    }
    let text = result["value"]
        .as_str()
        .ok_or("INVALID_SNAPSHOT")?
        .to_owned();
    let id = Uuid::new_v4();
    let mut output = json!({"captureId":id,"kind":"snapshot","page":page,"frame":frame,"source":"aria-semantic-snapshot","value":text,"privacy":"local-private","complete":true});
    let bytes = serde_json::to_vec(&output).map_err(|_| Error::from("INVALID_SNAPSHOT"))?;
    let path = directory.join(format!("snapshot-{id}.json"));
    output["attachment"] = json!({"path":path,"mimeType":"application/json","bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))});
    let target = path.clone();
    tokio::task::spawn_blocking(move || crate::artifacts::write(&target, &bytes))
        .await
        .map_err(|_| Error::from("CAPTURE_IO"))?
        .map_err(|error| Error::caused("CAPTURE_IO", error))?;
    Ok(output)
}

pub(crate) fn snapshot_diff(a: &Value, b: &Value, max_changed_ratio: f64) -> Result<Value, Error> {
    let a = a["value"].as_str().ok_or("INVALID_SNAPSHOT")?;
    let b = b["value"].as_str().ok_or("INVALID_SNAPSHOT")?;
    let (left, right) = (a.lines().collect::<Vec<_>>(), b.lines().collect::<Vec<_>>());
    if left.len() > 512 || right.len() > 512 {
        return Err("SNAPSHOT_BOUNDS".into());
    }
    let mut matrix = vec![vec![0u16; right.len() + 1]; left.len() + 1];
    for i in (0..left.len()).rev() {
        for j in (0..right.len()).rev() {
            matrix[i][j] = if left[i] == right[j] {
                matrix[i + 1][j + 1] + 1
            } else {
                matrix[i + 1][j].max(matrix[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let (mut added, mut removed) = (Vec::new(), Vec::new());
    while i < left.len() || j < right.len() {
        if i < left.len() && j < right.len() && left[i] == right[j] {
            i += 1;
            j += 1;
        } else if j < right.len() && (i == left.len() || matrix[i][j + 1] >= matrix[i + 1][j]) {
            added.push(right[j]);
            j += 1;
        } else {
            removed.push(left[i]);
            i += 1;
        }
    }
    let ratio = (added.len() + removed.len()) as f64 / (left.len() + right.len()).max(1) as f64;
    Ok(
        json!({"kind":"snapshot","matches":ratio<=max_changed_ratio,"changedRatio":ratio,"added":added,"removed":removed,"maxChangedRatio":max_changed_ratio}),
    )
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let locator = json!({"$ref":"#/$defs/browserLocator"});
    let mut options = object(
        json!({"landscape":{"type":"boolean"},"printBackground":{"type":"boolean"},"scale":{"type":"number","minimum":0.1,"maximum":2},"paperWidth":{"type":"number","minimum":0.01,"maximum":200},"paperHeight":{"type":"number","minimum":0.01,"maximum":200},"pageRanges":{"type":"string","maxLength":256},"displayHeaderFooter":{"type":"boolean"},"headerTemplate":{"type":"string","maxLength":16384},"footerTemplate":{"type":"string","maxLength":16384},"preferCSSPageSize":{"type":"boolean"},"tagged":{"type":"boolean"},"outline":{"type":"boolean"}}),
        &[],
    );
    for name in ["marginTop", "marginBottom", "marginLeft", "marginRight"] {
        options["properties"][name] = json!({"type":"number","minimum":0,"maximum":200});
    }
    vec![
        object(
            json!({"operation":{"const":"page_snapshot"},"page":reference,"frame":reference,"locator":locator}),
            &["operation", "page"],
        ),
        object(
            json!({"operation":{"const":"page_annotate"},"page":reference,"markers":{"type":"array","maxItems":32,"items":object(json!({"locator":locator,"label":{"type":"string","maxLength":128}}),&["locator"])}}),
            &["operation", "page", "markers"],
        ),
        object(
            json!({"operation":{"const":"page_pdf"},"page":reference,"options":options}),
            &["operation", "page"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_browser_snapshot_supplies_a_preview_without_publishing_evidence() {
        let root = tempfile::tempdir().unwrap();
        let page = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let error = snapshot(
            root.path().into(),
            page,
            json!(page),
            json!({"truncated":true,"value":"- main"}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "SNAPSHOT_BOUNDS");
        let partial = error.partial.unwrap();
        assert_eq!(partial["narrowingRequired"], true);
        assert_eq!(partial["preview"], "- main");
        assert!(partial["hint"].as_str().unwrap().contains("locator"));
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }

    #[test]
    fn semantic_diff_preserves_common_lines_and_reports_insertions() {
        let a = json!({"value":"- heading Acme\n- button Pay\n- link Help"});
        let b = json!({"value":"- heading Acme\n- text Saved\n- button Pay\n- link Help"});
        let diff = snapshot_diff(&a, &b, 0.).unwrap();
        assert_eq!(diff["added"], json!(["- text Saved"]));
        assert_eq!(diff["removed"], json!([]));
        assert_eq!(diff["matches"], false);
        assert_eq!(snapshot_diff(&a, &b, 0.15).unwrap()["matches"], true);
        assert_eq!(snapshot_diff(&a, &a, 0.).unwrap()["matches"], true);
    }
    #[test]
    fn invalid_print_geometry_and_marker_labels_refuse_before_dispatch() {
        let mut pdf = PdfOptions::default();
        assert!(pdf.validate().is_ok());
        pdf.margin_top = pdf.paper_height;
        assert_eq!(pdf.validate(), Err("INVALID_PDF_OPTIONS"));
        pdf = PdfOptions::default();
        pdf.page_ranges = "1; eval()".into();
        assert!(pdf.validate().is_err());
        let marker = Marker {
            locator: Locator::Css {
                selector: "button".into(),
            },
            label: "x".repeat(129),
        };
        assert_eq!(validate_markers(&[marker]), Err("ANNOTATION_BOUNDS"));
    }
}
