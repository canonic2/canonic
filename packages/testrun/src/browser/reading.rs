//! Shared browser-side selector/ARIA adapter; Rust owns routing and byte bounds.

#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
pub(crate) const SECURE_FIXTURE: &str =
    include_str!("../../tests/fixtures/browser/secure-aria.html");

pub(crate) fn selector(browser: &str) -> String {
    format!(
        "(() => {{const key=Symbol.for('canonic.testrun.selector.v3');return globalThis[key] ||= (()=>{{const module={{exports:{{}}}};\n{}\nreturn ({}) (module.exports.InjectedScript(),generateAriaTree,ariaNodeElement,renderAriaTreeAsJSON,renderAriaSnapshotAsYaml,{},{});}})();}})()",
        include_str!("reading/selector.js"),
        include_str!("reading/factory.js"),
        serde_json::json!(browser),
        serde_json::json!(REFERENCES)
    )
}

pub(crate) fn aria_reader(browser: &str) -> String {
    format!(
        "(element, options) => {{const injected={};const text=injected.testrunAriaSnapshot(element);return JSON.stringify(({})(text,options.maxBytes));}}",
        selector(browser),
        ARIA_BOUND
    )
}

pub(crate) const ARIA_BOUND: &str = include_str!("reading/truncate.js");
pub(crate) const REFERENCES: &str = include_str!("reading/references.js");
pub(crate) const DOM: &str = include_str!("reading/dom.js");

pub(super) fn with_narrowing_hint(mut result: serde_json::Value) -> serde_json::Value {
    if result["narrowingRequired"] == true {
        result["hint"] = serde_json::json!(
            "Retry page_read with a locator such as {\"kind\":\"role\",\"role\":\"main\"}. Choose a smaller section if needed. For article text, use read: readable."
        );
    }
    result
}

/// Record only match evidence from successfully observed documents.
#[derive(Default)]
pub(super) struct LocatorProgress(Option<bool>);

impl LocatorProgress {
    pub(super) fn observe(&mut self, result: &serde_json::Value) {
        if let Some(count) = result["matchCount"].as_u64() {
            self.0 = Some(self.0 == Some(true) || count != 0);
        }
    }

    pub(super) fn timeout(&self, mut error: super::session::Error) -> super::session::Error {
        if error.code == "CONDITION_TIMEOUT"
            && let Some(matched) = self.0
        {
            let partial = error.partial.get_or_insert_with(|| serde_json::json!({}));
            partial["locatorMatched"] = serde_json::Value::Bool(matched);
            if !matched {
                partial["hint"] = serde_json::json!(
                    "No element matched the locator during the wait. Check the locator and frame with page_read using read: count."
                );
            }
        }
        error
    }
}

pub(crate) fn query_function(browser: &str) -> String {
    format!(
        "options=>{{const injected={};return ({}) (injected,{},options,{},{});}}",
        selector(browser),
        include_str!("reading/query.js"),
        DOM,
        ARIA_BOUND,
        include_str!("reading/geometry.js")
    )
}
