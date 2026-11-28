//! Explicit instrumentation options and native profile formats.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub(crate) mod cpu;
mod results;
pub(crate) use results::{css_result, encode_coverage, javascript_entries};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageFormat {
    #[default]
    Native,
    Playwright,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default = "enabled")]
    pub cpu: bool,
    #[serde(default)]
    pub javascript_coverage: bool,
    #[serde(default = "enabled")]
    pub javascript_call_count: bool,
    #[serde(default = "enabled")]
    pub javascript_detailed: bool,
    #[serde(default)]
    pub report_anonymous_scripts: Option<bool>,
    #[serde(default)]
    pub css_coverage: bool,
    #[serde(default)]
    pub coverage_format: CoverageFormat,
    #[serde(default)]
    pub reset_on_navigation: Option<bool>,
    #[serde(default)]
    pub include_source_texts: Option<bool>,
    #[serde(default = "interval")]
    pub sampling_interval_us: u32,
}
fn enabled() -> bool {
    true
}
fn interval() -> u32 {
    1000
}
impl Default for Options {
    fn default() -> Self {
        Self {
            cpu: true,
            javascript_coverage: false,
            javascript_call_count: true,
            javascript_detailed: true,
            report_anonymous_scripts: None,
            css_coverage: false,
            coverage_format: CoverageFormat::Native,
            reset_on_navigation: None,
            include_source_texts: None,
            sampling_interval_us: interval(),
        }
    }
}
impl Options {
    pub(crate) fn sdk(&self) -> bool {
        self.coverage_format == CoverageFormat::Playwright
    }
    pub(crate) fn reset(&self) -> bool {
        self.reset_on_navigation.unwrap_or(self.sdk())
    }
    pub(crate) fn sources(&self) -> bool {
        self.include_source_texts.unwrap_or(self.sdk())
    }
    pub(crate) fn anonymous(&self) -> bool {
        self.report_anonymous_scripts.unwrap_or(!self.sdk())
    }
    pub(crate) fn debugger(&self) -> bool {
        self.javascript_coverage && (self.sources() || self.reset() || self.sdk())
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !self.javascript_coverage
            && !self.css_coverage
            && (!self.cpu
                || self.sdk()
                || self.reset_on_navigation.is_some()
                || self.include_source_texts.is_some())
            || !(100..=100000).contains(&self.sampling_interval_us)
            || !self.javascript_coverage
                && (!self.javascript_call_count
                    || !self.javascript_detailed
                    || self.report_anonymous_scripts.is_some())
        {
            Err("PROFILE_OPTIONS")
        } else {
            Ok(())
        }
    }
}
pub(crate) fn javascript_result(mut value: Value, options: &Options) -> Value {
    if !options.anonymous()
        && let Some(entries) = value["result"].as_array_mut()
    {
        entries.retain(|entry| entry["url"].as_str().is_some_and(|url| !url.is_empty()));
    }
    value
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let mut schemas = vec![
        json!({"type":"object","properties":{"operation":{"const":"profile_start"},"page":reference,"options":{"type":"object","properties":{"cpu":{"type":"boolean","default":true},"javascriptCoverage":{"type":"boolean","default":false},"javascriptCallCount":{"type":"boolean","default":true},"javascriptDetailed":{"type":"boolean","default":true},"reportAnonymousScripts":{"type":["boolean","null"],"description":"Include empty-URL JavaScript entries; omission defaults true for native format and false for playwright format"},"cssCoverage":{"type":"boolean","default":false},"samplingIntervalUs":{"type":"integer","minimum":100,"maximum":100000,"default":1000}},"additionalProperties":false}},"required":["operation","page"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"profile_status"},"page":reference},"required":["operation","page"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"profile_stop"},"page":reference},"required":["operation","page"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"profile_take_coverage"},"page":reference},"required":["operation","page"],"additionalProperties":false}),
    ];
    let properties = &mut schemas[0]["properties"]["options"]["properties"];
    properties["coverageFormat"] = json!({"enum":["native","playwright"],"default":"native","description":"Private coverage report shape; playwright enables SDK navigation/source/anonymous defaults"});
    properties["resetOnNavigation"] = json!({"type":["boolean","null"],"description":"Forget script and stylesheet identities on execution-context clear; defaults true for playwright format, false for native"});
    properties["includeSourceTexts"] = json!({"type":["boolean","null"],"description":"Retain bounded private script/stylesheet sources; defaults true for playwright format, false for native"});
    schemas
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_defaults_are_explicit_and_native_defaults_remain_cumulative_without_sources() {
        let native = Options::default();
        assert!(!native.reset() && !native.sources() && native.anonymous());
        let mut sdk: Options=serde_json::from_value(json!({"cpu":false,"javascriptCoverage":true,"cssCoverage":true,"coverageFormat":"playwright"})).unwrap();
        assert!(sdk.reset() && sdk.sources() && sdk.debugger() && !sdk.anonymous());
        sdk.reset_on_navigation = Some(false);
        sdk.include_source_texts = Some(false);
        sdk.report_anonymous_scripts = Some(true);
        assert!(!sdk.reset() && !sdk.sources() && sdk.anonymous());
        assert!(sdk.validate().is_ok());
        sdk.javascript_coverage = false;
        sdk.css_coverage = false;
        sdk.cpu = true;
        sdk.report_anonymous_scripts = None;
        assert_eq!(sdk.validate(), Err("PROFILE_OPTIONS"));
        let schema = schemas(&json!({"type":"object"})).remove(0);
        crate::schema::validate(&schema,&json!({"operation":"profile_start","page":{},"options":{"javascriptCoverage":true,"coverageFormat":"playwright","resetOnNavigation":false,"includeSourceTexts":true}})).unwrap();
    }
    #[test]
    fn precise_controls_validate_before_collection_and_filter_only_explicit_anonymous_opt_out() {
        let mut options = Options {
            javascript_coverage: true,
            ..Default::default()
        };
        let raw = json!({"result":[{"scriptId":"1","url":"","functions":[]},{"scriptId":"2","url":"https://example.com/acme.js","functions":[]}]});
        assert_eq!(javascript_result(raw.clone(), &options), raw);
        options.report_anonymous_scripts = Some(true);
        assert_eq!(javascript_result(raw.clone(), &options), raw);
        options.report_anonymous_scripts = Some(false);
        assert_eq!(
            javascript_result(raw, &options)["result"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        options.javascript_coverage = false;
        assert!(options.validate().is_err());
        options.report_anonymous_scripts = None;
        options.javascript_call_count = false;
        assert!(options.validate().is_err());
    }
}
