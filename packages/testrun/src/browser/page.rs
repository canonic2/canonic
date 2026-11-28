//! Typed page controls and bounded selector compilation for Firefox/WebKit.
//!
//! Selector spelling follows Playwright 1.64.1 locatorUtils/stringUtils,
//! Copyright Microsoft Corporation, Apache-2.0; see browser upstream notices.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Locator {
    Element {
        frame: super::contract::Reference,
        id: uuid::Uuid,
    },
    Css {
        selector: String,
    },
    Xpath {
        selector: String,
    },
    Role {
        role: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        exact: bool,
    },
    Text {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    Label {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    Placeholder {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    Alt {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    Title {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    TestId {
        value: String,
    },
    Within {
        parent: Box<Locator>,
        child: Box<Locator>,
    },
    Filter {
        locator: Box<Locator>,
        #[serde(rename = "hasText")]
        has_text: String,
    },
    Nth {
        locator: Box<Locator>,
        index: u32,
    },
}
impl Locator {
    pub(crate) fn selector(&self) -> Result<String, &'static str> {
        let result = self.compile(0)?;
        if result.len() > 8192 {
            return Err("SELECTOR_BOUNDS");
        }
        Ok(result)
    }
    fn compile(&self, depth: usize) -> Result<String, &'static str> {
        if depth > 8 {
            return Err("SELECTOR_BOUNDS");
        }
        let text =
            |value: &str, exact: bool| format!("{}{}", json!(value), if exact { "s" } else { "i" });
        let attr = |value: &str, exact: bool| {
            format!(
                "\"{}\"{}",
                value.replace('\\', "\\\\").replace('"', "\\\""),
                if exact { "s" } else { "i" }
            )
        };
        let result = match self {
            Self::Element { frame, id } => {
                format!("testrun={}.{}.{id}", frame.id, frame.generation)
            }
            Self::Css { selector } | Self::Xpath { selector } => {
                if selector.is_empty()
                    || selector.len() > 4096
                    || selector.contains(">>")
                    || selector.contains('\0')
                {
                    return Err("INVALID_SELECTOR");
                }
                format!(
                    "{}={selector}",
                    if matches!(self, Self::Css { .. }) {
                        "css"
                    } else {
                        "xpath"
                    }
                )
            }
            Self::Role { role, name, exact } => {
                if role.is_empty()
                    || role.len() > 64
                    || !role.bytes().all(|c| c.is_ascii_lowercase())
                {
                    return Err("INVALID_ROLE");
                }
                format!(
                    "internal:role={role}{}",
                    name.as_ref()
                        .map(|name| format!("[name={}]", attr(name, *exact)))
                        .unwrap_or_default()
                )
            }
            Self::Text { text: value, exact } => format!("internal:text={}", text(value, *exact)),
            Self::Label { text: value, exact } => format!("internal:label={}", text(value, *exact)),
            Self::Placeholder { text: value, exact }
            | Self::Alt { text: value, exact }
            | Self::Title { text: value, exact } => {
                let name = match self {
                    Self::Placeholder { .. } => "placeholder",
                    Self::Alt { .. } => "alt",
                    _ => "title",
                };
                format!("internal:attr=[{name}={}]", attr(value, *exact))
            }
            Self::TestId { value } => {
                format!("internal:testid=[data-testid={}]", attr(value, true))
            }
            Self::Within { parent, child } => format!(
                "{} >> {}",
                parent.compile(depth + 1)?,
                child.compile(depth + 1)?
            ),
            Self::Filter { locator, has_text } => format!(
                "{} >> internal:has-text={}",
                locator.compile(depth + 1)?,
                text(has_text, false)
            ),
            Self::Nth { locator, index } => {
                if *index > 10000 {
                    return Err("SELECTOR_BOUNDS");
                }
                format!("{} >> nth={index}", locator.compile(depth + 1)?)
            }
        };
        if result.len() > 8192 {
            return Err("SELECTOR_BOUNDS");
        }
        Ok(result)
    }

    pub(crate) fn scoped_selector(
        &self,
        frame: super::contract::Reference,
    ) -> Result<String, &'static str> {
        self.check_frame(frame)?;
        self.selector()
    }

    fn check_frame(&self, current: super::contract::Reference) -> Result<(), &'static str> {
        match self {
            Self::Element { frame, .. } if frame.connection_id != current.connection_id => {
                Err("FOREIGN_REFERENCE")
            }
            Self::Element { frame, .. } if *frame != current => Err("STALE_REFERENCE"),
            Self::Within { parent, child } => {
                parent.check_frame(current)?;
                child.check_frame(current)
            }
            Self::Filter { locator, .. } | Self::Nth { locator, .. } => {
                locator.check_frame(current)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Read {
    Text,
    Dom,
    Aria,
    Form,
    Count,
    State,
    Geometry,
    Messages,
    Snapshot,
    Styles,
    Readable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    AtSource {
        source: super::automation_debugger::Location,
        action: Box<Action>,
    },
    Click {},
    DoubleClick {},
    Hover {},
    Fill {
        value: String,
    },
    Clear {},
    Focus {},
    Blur {},
    Check {
        checked: bool,
    },
    Select {
        values: Vec<String>,
    },
    Press {
        key: String,
    },
    Type {
        text: String,
    },
    Drag {
        target: Locator,
    },
    /// Explicit DOM event dispatch has different semantics from user input.
    Dispatch {
        event: String,
    },
}
impl Action {
    pub(crate) fn validate(&self, force: bool) -> Result<(), &'static str> {
        match self {
            Self::AtSource { source, action } => {
                source.validate()?;
                if matches!(**action, Self::AtSource { .. }) {
                    return Err("CALL_SOURCE_NESTING");
                }
                action.validate(force)
            }
            Self::Fill { value } | Self::Type { text: value } if value.len() > 16384 => {
                Err("INPUT_BOUNDS")
            }
            Self::Press { key } if key.is_empty() || key.len() > 128 => Err("INPUT_BOUNDS"),
            Self::Select { values }
                if values.len() > 100 || values.iter().any(|v| v.len() > 1024) =>
            {
                Err("INPUT_BOUNDS")
            }
            Self::Dispatch { event }
                if force
                    || event.is_empty()
                    || event.len() > 128
                    || !event
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-') =>
            {
                Err("INVALID_EVENT")
            }
            Self::Drag { target } => target.selector().map(|_| ()),
            Self::Press { .. } | Self::Type { .. } | Self::Focus {} | Self::Blur {} if force => {
                Err("FORCE_UNAVAILABLE")
            }
            Self::Press { key } => super::keyboard::press_keys(key).map(|_| ()),
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Load {
    Commit,
    Domcontentloaded,
    #[default]
    Load,
    Networkidle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Navigation {
    Goto {
        url: String,
        #[serde(default, rename = "waitUntil")]
        wait_until: Load,
    },
    SetContent {
        html: String,
        #[serde(default, rename = "waitUntil")]
        wait_until: Load,
    },
    Reload {
        #[serde(default, rename = "waitUntil")]
        wait_until: Load,
    },
    Back {
        #[serde(default, rename = "waitUntil")]
        wait_until: Load,
    },
    Forward {
        #[serde(default, rename = "waitUntil")]
        wait_until: Load,
    },
}
impl Navigation {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Goto { url, .. }
                if url.len() > 8192
                    || !(url.starts_with("https://")
                        || url.starts_with("http://")
                        || url == "about:blank"
                        || url.starts_with("data:text/html")) =>
            {
                Err("INVALID_NAVIGATION_URL")
            }
            Self::SetContent { html, wait_until }
                if html.len() > 262144 || matches!(wait_until, Load::Commit) =>
            {
                Err("INVALID_CONTENT")
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Attached {},
    Detached {},
    Visible {},
    Hidden {},
    Enabled {},
    Disabled {},
    Editable {},
    Checked {},
    Text {
        value: String,
        #[serde(default)]
        contains: bool,
    },
    Value {
        value: String,
    },
    Count {
        value: u32,
    },
    Url {
        value: String,
    },
    /// Pinned `waitForURL` glob/regex matching of the frame's current URL, then
    /// its document reaching `waitUntil` (default `load`, as pinned).
    UrlMatch {
        matcher: super::routing::Matcher,
        #[serde(default, rename = "waitUntil")]
        wait_until: Load,
        #[serde(skip)]
        compiled: CompiledUrl,
    },
}
/// One compilation per wait. Clones start empty, so a submission-time probe
/// resolved against a placeholder base never fills the dispatched request's cell.
#[derive(Default)]
pub struct CompiledUrl(
    std::sync::Arc<std::sync::OnceLock<Result<super::routing::Compiled, &'static str>>>,
);
impl Clone for CompiledUrl {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl std::fmt::Debug for CompiledUrl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CompiledUrl")
    }
}
impl CompiledUrl {
    fn get(
        &self,
        matcher: &super::routing::Matcher,
    ) -> Result<&super::routing::Compiled, &'static str> {
        self.0
            .get_or_init(|| matcher.compile())
            .as_ref()
            .map_err(|code| *code)
    }
}
impl Condition {
    /// Turn a `url_match` page report (`href` and the document's `loadState`)
    /// into the shared `satisfied` shape.
    pub(crate) fn settle(&self, value: Value) -> Value {
        let Self::UrlMatch {
            matcher,
            wait_until,
            compiled,
        } = self
        else {
            return value;
        };
        let rank = |state: &str| match state {
            "load" => 2,
            "domcontentloaded" => 1,
            _ => 0,
        };
        let required = match wait_until {
            Load::Commit => 0,
            Load::Domcontentloaded => 1,
            Load::Load | Load::Networkidle => 2,
        };
        // `networkIdle` is the backend's lifecycle witness, added by the caller.
        let idle = !matches!(wait_until, Load::Networkidle) || value["networkIdle"] == true;
        let satisfied = value["href"]
            .as_str()
            .is_some_and(|href| compiled.get(matcher).is_ok_and(|url| url.is_match(href)))
            && value["loadState"].as_str().map_or(0, rank) >= required
            && idle;
        serde_json::json!({"satisfied":satisfied})
    }
    /// Whether settling needs the backend's network-idle witness.
    pub(crate) fn needs_network_idle(&self) -> bool {
        matches!(
            self,
            Self::UrlMatch {
                wait_until: Load::Networkidle,
                ..
            }
        )
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::UrlMatch {
                matcher, compiled, ..
            } => compiled.get(matcher).map(|_| ()),
            Self::Text { value, .. } | Self::Value { value } | Self::Url { value }
                if value.len() > 8192 =>
            {
                Err("CONDITION_BOUNDS")
            }
            Self::Count { value } if *value > 10000 => Err("CONDITION_BOUNDS"),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Png,
    Jpeg,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    #[default]
    Css,
    Device,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Screenshot {
    #[serde(default)]
    pub format: Format,
    #[serde(default)]
    pub quality: Option<u8>,
    #[serde(default)]
    pub full_page: bool,
    #[serde(default)]
    pub clip: Option<Clip>,
    #[serde(default)]
    pub element: Option<Locator>,
    #[serde(default)]
    pub masks: Vec<Locator>,
    #[serde(default)]
    pub scale: Scale,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default)]
    pub disable_animations: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
}
impl Screenshot {
    pub(crate) fn is_viewport(&self) -> bool {
        !self.full_page && self.element.is_none() && self.clip.is_none()
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Some(path) = &self.output_path {
            let destination = std::path::Path::new(path);
            if path.len() > 4096
                || path.chars().any(char::is_control)
                || !destination.is_absolute()
                || destination.file_name().is_none()
                || destination
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
            {
                return Err("CAPTURE_OUTPUT_PATH_INVALID");
            }
        }
        if self.quality.is_some_and(|q| q > 100)
            || self.quality.is_some() && matches!(self.format, Format::Png)
            || self.transparent && matches!(self.format, Format::Jpeg)
            || self.full_page && (self.clip.is_some() || self.element.is_some())
            || self.clip.is_some() && self.element.is_some()
            || self.masks.len() > 32
        {
            return Err("INVALID_CAPTURE_OPTIONS");
        }
        if let Some(clip) = self.clip
            && (![clip.x, clip.y, clip.width, clip.height]
                .iter()
                .all(|n| n.is_finite())
                || clip.x < 0.0
                || clip.y < 0.0
                || clip.width <= 0.0
                || clip.height <= 0.0
                || clip.width * clip.height > 32_000_000.0)
        {
            return Err("CAPTURE_BOUNDS");
        }
        for locator in self.masks.iter().chain(self.element.iter()) {
            locator.selector()?;
        }
        Ok(())
    }
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let string = |max| json!({"type":"string","maxLength":max});
    let locator = json!({"$ref":"#/$defs/browserLocator"});
    let scope = json!({"page":reference,"frame":reference,"locator":locator});
    let mut result = Vec::new();
    result.extend(super::evidence::schemas(reference));
    result.push(object(json!({"operation":{"const":"capture_compare"},"baseline":{"type":"string","format":"uuid"},"actual":{"type":"string","format":"uuid"},"threshold":{"type":"integer","minimum":0,"maximum":255,"default":0},"maxChangedRatio":{"type":"number","minimum":0,"maximum":1,"default":0}}),&["operation","baseline","actual"]));
    result.push(object(json!({"operation":{"const":"page_css"},"developer":reference,"page":reference,"frame":reference,"locator":locator,"offset":{"type":"integer","minimum":0,"maximum":100000,"default":0},"limit":{"type":"integer","minimum":1,"maximum":50,"default":10}}),&["operation","developer","page","locator"]));
    result.push(object(json!({"operation":{"const":"page_styles"},"page":reference,"frame":reference,"locator":locator,"properties":{"type":"array","maxItems":64,"description":"Duplicate property names are rejected","items":{"type":"string","minLength":1,"maxLength":128}},"maxBytes":{"type":"integer","minimum":256,"maximum":8192,"default":8192}}),&["operation","page","locator"]));
    result.push(object(json!({"operation":{"const":"page_read_url"},"page":reference,"url":{"type":"string","minLength":1,"maxLength":8192},"maxBytes":{"type":"integer","minimum":256,"maximum":8192,"default":8192}}),&["operation","page","url"]));
    let clipboard_read = object(json!({"kind":{"const":"read"}}), &["kind"]);
    let clipboard_write = object(
        json!({"kind":{"const":"write"},"text":{"type":"string","maxLength":16384}}),
        &["kind", "text"],
    );
    result.push(object(json!({"operation":{"const":"page_clipboard"},"page":reference,"origin":{"type":"string","maxLength":2048},"action":{"oneOf":[clipboard_read,clipboard_write]}}),&["operation","page","origin","action"]));
    for (operation, extra, required) in [
        (
            "page_read",
            json!({"read":{"enum":["text","dom","aria","form","count","state","geometry","messages","snapshot","styles","readable"]},"maxBytes":{"type":"integer","minimum":256,"maximum":8192,"default":8192}}),
            vec!["operation", "page", "read"],
        ),
        (
            "page_action",
            json!({"action":action_schema(&locator),"force":{"type":"boolean","default":false},"postcondition":condition_schema()}),
            vec!["operation", "page", "locator", "action"],
        ),
        (
            "page_wait",
            json!({"condition":condition_schema()}),
            vec!["operation", "page", "condition"],
        ),
        (
            "frame_enter",
            json!({}),
            vec!["operation", "page", "locator"],
        ),
    ] {
        let mut props = scope.clone();
        props["operation"] = json!({"const":operation});
        props
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        result.push(object(props, &required));
    }
    let load = json!({"enum":["commit","domcontentloaded","load","networkidle"],"default":"load"});
    let navigation = json!({"oneOf":[
        variant("goto",json!({"url":string(8192),"waitUntil":load}),&["url"]),
        variant("set_content",json!({"html":string(262144),"waitUntil":load}),&["html"]),
        variant("reload",json!({"waitUntil":load}),&[]),variant("back",json!({"waitUntil":load}),&[]),variant("forward",json!({"waitUntil":load}),&[])
    ]});
    result.push(object(
        json!({"operation":{"const":"page_navigate"},"page":reference,"navigation":navigation}),
        &["operation", "page", "navigation"],
    ));
    let clip = object(
        json!({"x":{"type":"number","minimum":0},"y":{"type":"number","minimum":0},"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0}}),
        &["x", "y", "width", "height"],
    );
    let capture = object(
        json!({"format":{"enum":["png","jpeg"]},"quality":{"type":"integer","minimum":0,"maximum":100},"fullPage":{"type":"boolean"},"clip":clip,"element":locator,"masks":{"type":"array","maxItems":32,"items":locator},"scale":{"enum":["css","device"]},"transparent":{"type":"boolean"},"disableAnimations":{"type":"boolean"},"outputPath":{"type":"string","minLength":1,"maxLength":4096,"description":"Write the screenshot to this new local file in an existing directory. Relative paths resolve from the caller. No overwrite."}}),
        &[],
    );
    result.push(object(
        json!({"operation":{"const":"page_screenshot"},"page":reference,"capture":capture}),
        &["operation", "page"],
    ));
    result
}
fn object(props: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}
pub(crate) fn validate_read_url(url: &str, max_bytes: usize) -> Result<(), &'static str> {
    if !(256..=8192).contains(&max_bytes) {
        return Err("READ_BOUNDS");
    }
    let uri = url
        .parse::<axum::http::Uri>()
        .map_err(|_| "INVALID_NAVIGATION_URL")?;
    if url.len() > 8192
        || !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.host().is_none()
        || uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
    {
        return Err("INVALID_NAVIGATION_URL");
    }
    Ok(())
}
pub(crate) fn validate_styles(
    locator: &Locator,
    properties: &[String],
    max_bytes: usize,
) -> Result<(), &'static str> {
    locator.selector()?;
    if !(256..=8192).contains(&max_bytes) {
        return Err("READ_BOUNDS");
    }
    if properties.len() > 64
        || properties
            .iter()
            .any(|property| property.is_empty() || property.len() > 128 || property.contains('\0'))
        || properties
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != properties.len()
    {
        return Err("INVALID_STYLE_PROPERTIES");
    }
    Ok(())
}
fn variant(kind: &str, mut props: Value, fields: &[&str]) -> Value {
    props["kind"] = json!({"const":kind});
    let mut required = vec!["kind"];
    required.extend_from_slice(fields);
    object(props, &required)
}
pub(crate) fn locator_schema() -> Value {
    let s = json!({"type":"string","minLength":1,"maxLength":4096});
    let exact = json!({"type":"boolean","default":false});
    let mut variants = vec![
        variant(
            "element",
            json!({"frame":object(json!({"connectionId":{"type":"string","format":"uuid"},"id":{"type":"string","format":"uuid"},"generation":{"type":"string","format":"uuid"}}), &["connectionId","id","generation"]),"id":{"type":"string","format":"uuid"}}),
            &["frame", "id"],
        ),
        variant("css", json!({"selector":s}), &["selector"]),
        variant("xpath", json!({"selector":s}), &["selector"]),
        variant("role", json!({"role":s,"name":s,"exact":exact}), &["role"]),
        variant("test_id", json!({"value":s}), &["value"]),
    ];
    for kind in ["text", "label", "placeholder", "alt", "title"] {
        variants.push(variant(kind, json!({"text":s,"exact":exact}), &["text"]));
    }
    let nested = json!({"$ref":"#/$defs/browserLocator"});
    variants.push(variant(
        "within",
        json!({"parent":nested,"child":nested}),
        &["parent", "child"],
    ));
    variants.push(variant(
        "filter",
        json!({"locator":nested,"hasText":s}),
        &["locator", "hasText"],
    ));
    variants.push(variant(
        "nth",
        json!({"locator":nested,"index":{"type":"integer","minimum":0,"maximum":10000}}),
        &["locator", "index"],
    ));
    json!({"oneOf":variants})
}
fn action_schema(locator: &Value) -> Value {
    let mut variants = Vec::new();
    for kind in ["click", "double_click", "hover", "clear", "focus", "blur"] {
        variants.push(variant(kind, json!({}), &[]));
    }
    for (kind, field, max) in [
        ("fill", "value", 16384),
        ("type", "text", 16384),
        ("press", "key", 128),
        ("dispatch", "event", 128),
    ] {
        let mut props = json!({});
        props[field] = json!({"type":"string","maxLength":max});
        variants.push(variant(kind, props, &[field]));
    }
    variants.push(variant(
        "check",
        json!({"checked":{"type":"boolean"}}),
        &["checked"],
    ));
    variants.push(variant("select",json!({"values":{"type":"array","maxItems":100,"items":{"type":"string","maxLength":1024}}}),&["values"]));
    variants.push(variant("drag", json!({"target":locator}), &["target"]));
    let plain = json!({"oneOf":variants.clone()});
    variants.push(variant(
        "at_source",
        json!({"source":super::automation_debugger::location_schema(),"action":plain}),
        &["source", "action"],
    ));
    json!({"oneOf":variants})
}
pub(super) fn condition_schema() -> Value {
    let mut variants = Vec::new();
    for kind in [
        "attached", "detached", "visible", "hidden", "enabled", "disabled", "editable", "checked",
    ] {
        variants.push(variant(kind, json!({}), &[]));
    }
    for kind in ["text", "value", "url"] {
        let mut props = json!({"value":{"type":"string","maxLength":8192}});
        if kind == "text" {
            props["contains"] = json!({"type":"boolean","default":false});
        }
        variants.push(variant(kind, props, &["value"]));
    }
    variants.push(variant(
        "count",
        json!({"value":{"type":"integer","minimum":0,"maximum":10000}}),
        &["value"],
    ));
    variants.push(variant(
        "url_match",
        json!({"matcher":super::routing::Matcher::schema(),"waitUntil":{"enum":["commit","domcontentloaded","load","networkidle"],"default":"load","description":"Document state required after the URL matches; networkidle also needs 500 ms without network requests"}}),
        &["matcher"],
    ));
    json!({"oneOf":variants})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_public_schemas_validate_recursive_locators_and_capture_options() {
        let tool = crate::browser::surface::tools()
            .into_iter()
            .find(|tool| tool["name"] == "browser_send")
            .unwrap();
        let bytes = tool.to_string().len();
        canonic_mcp_kit::schema::check(&tool["inputSchema"]).unwrap();
        assert!(
            // Keep the expanded browser API below half the MCP schema limit.
            bytes < 128 * 1024,
            "Browser tool schema exceeds its byte budget: {bytes}"
        );
        let reference = json!({"connectionId":uuid::Uuid::new_v4(),"id":uuid::Uuid::new_v4(),"generation":uuid::Uuid::new_v4()});
        let request = json!({"request":{"operation":"page_action","page":reference,"locator":{"kind":"within","parent":{"kind":"css","selector":"main"},"child":{"kind":"role","role":"button","name":"Save"}},"action":{"kind":"click"}}});
        crate::schema::validate(&tool["inputSchema"], &request).unwrap();
        let mut sourced = request.clone();
        sourced["request"]["action"] = json!({"kind":"at_source","source":{"file":"src/Acme-journey.rs","line":30,"column":3},"action":{"kind":"click"}});
        crate::schema::validate(&tool["inputSchema"], &sourced).unwrap();
        let mut nested = sourced.clone();
        nested["request"]["action"]["action"] = sourced["request"]["action"].clone();
        assert!(crate::schema::validate(&tool["inputSchema"], &nested).is_err());
        sourced["request"]["action"]["source"]["column"] = json!(-1);
        assert!(crate::schema::validate(&tool["inputSchema"], &sourced).is_err());
        let run = json!({"request":{"operation":"context_debugger","context":reference,"action":{"kind":"run_to","pauseId":uuid::Uuid::new_v4(),"location":{"file":"Acme-journey.rs","line":30}}}});
        crate::schema::validate(&tool["inputSchema"], &run).unwrap();
        let mut invalid = request;
        invalid["request"]["locator"]["child"]["unknown"] = json!(true);
        assert!(crate::schema::validate(&tool["inputSchema"], &invalid).is_err());
    }
    #[test]
    fn selectors_quote_semantic_values_and_bound_composition() {
        let locator = Locator::Role {
            role: "button".into(),
            name: Some("Acme \" >> text=Delete".into()),
            exact: true,
        };
        assert_eq!(
            locator.selector().unwrap(),
            "internal:role=button[name=\"Acme \\\" >> text=Delete\"s]"
        );
        assert!(
            Locator::Css {
                selector: "button >> internal:text=Delete".into()
            }
            .selector()
            .is_err()
        );
        assert!(
            Locator::Role {
                role: "button[name=Delete]".into(),
                name: None,
                exact: false
            }
            .selector()
            .is_err()
        );
        assert!(
            Screenshot {
                quality: Some(90),
                ..Screenshot::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn snapshot_locators_are_bound_to_the_exact_connection_frame_and_document() {
        let frame = super::super::contract::Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let locator = Locator::Element {
            frame,
            id: uuid::Uuid::new_v4(),
        };
        assert!(locator.scoped_selector(frame).is_ok());
        let mut stale = frame;
        stale.generation = uuid::Uuid::new_v4();
        assert_eq!(locator.scoped_selector(stale), Err("STALE_REFERENCE"));
        stale = frame;
        stale.connection_id = uuid::Uuid::new_v4();
        assert_eq!(locator.scoped_selector(stale), Err("FOREIGN_REFERENCE"));
        let composed = Locator::Within {
            parent: Box::new(Locator::Css {
                selector: "main".into(),
            }),
            child: Box::new(locator),
        };
        assert_eq!(composed.scoped_selector(stale), Err("FOREIGN_REFERENCE"));
    }
}
