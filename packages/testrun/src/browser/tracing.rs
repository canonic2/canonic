//! Trace options shared at the API boundary; formats remain engine specific.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub(crate) mod export;
pub(crate) mod sources;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Snapshots {
    Enabled(bool),
    Selected(SnapshotOptions),
}
impl Default for Snapshots {
    fn default() -> Self {
        Self::Enabled(false)
    }
}
impl From<bool> for Snapshots {
    fn from(value: bool) -> Self {
        Self::Enabled(value)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SnapshotOptions {
    pub dom: bool,
    pub aria: bool,
    pub screen: bool,
}
impl Snapshots {
    pub(crate) fn selected(&self) -> SnapshotOptions {
        match self {
            Self::Enabled(dom) => SnapshotOptions {
                dom: *dom,
                ..Default::default()
            },
            Self::Selected(options) => options.clone(),
        }
    }
    fn any(&self) -> bool {
        let selected = self.selected();
        selected.dom || selected.aria || selected.screen
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reload {
    #[serde(default)]
    pub ignore_cache: bool,
}

/// Opt-in capture defaults from the pinned performance tool. Asynchronous
/// completion and export retain Testrun's contracts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Performance {
    #[serde(default = "enabled")]
    pub reload: bool,
    #[serde(default = "enabled")]
    pub auto_stop: bool,
    #[serde(default)]
    pub ignore_cache: bool,
}
fn enabled() -> bool {
    true
}
impl Default for Performance {
    fn default() -> Self {
        Self {
            reload: true,
            auto_stop: true,
            ignore_cache: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub sources: bool,
    #[serde(default)]
    pub source_files: Vec<String>,
    #[serde(default)]
    pub screenshots: bool,
    #[serde(default)]
    pub cpu_sampling: bool,
    #[serde(default)]
    pub snapshots: Snapshots,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub max_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload: Option<Reload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<super::contract::Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_stop: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<Performance>,
}
impl Options {
    pub(crate) fn automatic_stop(&self) -> Option<bool> {
        self.performance
            .as_ref()
            .map(|options| options.auto_stop)
            .or(self.auto_stop)
    }
    pub(crate) fn validate(&self, native: bool) -> Result<(), &'static str> {
        sources::validate(self)?;
        if self.performance.is_some()
            && (self.reload.is_some() || self.auto_stop.is_some() || self.max_duration_ms.is_some())
        {
            return Err("TRACE_PERFORMANCE_OPTIONS_CONFLICT");
        }
        if self.auto_stop.is_some() && self.max_duration_ms.is_some() {
            return Err("TRACE_DURATION_CONFLICT");
        }
        if !native
            && (self.page.is_some()
                || self.auto_stop.is_some()
                || self.performance.is_some()
                || self.file_path.is_some())
        {
            return Err("TRACE_NATIVE_OPTIONS_UNSUPPORTED");
        }
        if let Some(path) = &self.file_path {
            export::validate(path)?;
        }
        if self
            .max_duration_ms
            .is_some_and(|value| !(1000..=300000).contains(&value))
        {
            return Err("TRACE_BOUNDS");
        }
        if let Some(name) = &self.name {
            prefix(name)?;
        }
        if let Some(title) = &self.title {
            label(title)?;
        }
        if native
            && (self.live
                || self.sources
                || self.snapshots.any()
                || self.name.is_some()
                || self.title.is_some())
        {
            return Err("TRACE_ARCHIVE_OPTIONS_UNSUPPORTED");
        }
        if !native && self.cpu_sampling {
            return Err("TRACE_CPU_SAMPLING_UNSUPPORTED");
        }
        if !native && self.reload.is_some() {
            return Err("TRACE_RELOAD_UNSUPPORTED");
        }
        Ok(())
    }
    pub(crate) fn summary(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("Trace options");
        value
            .as_object_mut()
            .expect("Trace options")
            .remove("sourceFiles");
        value["sourceFileCount"] = json!(self.source_files.len());
        value
    }
}
pub(crate) fn prefix(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err("TRACE_NAME_BOUNDS")
    } else {
        Ok(())
    }
}
pub(crate) fn default_title() -> String {
    "Testrun investigation".into()
}
pub(crate) fn label(value: &str) -> Result<(), &'static str> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err("TRACE_LABEL_BOUNDS")
    } else {
        Ok(())
    }
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let mut schemas = vec![
        json!({"type":"object","properties":{"operation":{"const":"trace_start"},"context":reference,"options":{"type":"object","properties":{"sources":{"type":"boolean","default":false,"description":"Include immutable snapshots of explicitly selected sourceFiles; automatic client-stack discovery is unavailable"},"sourceFiles":{"type":"array","maxItems":16,"items":{"type":"string","minLength":1,"maxLength":4096},"default":[],"description":"Absolute selected regular files; requires sources:true; 256 KiB per file and 2 MiB total"},"live":{"type":"boolean","default":false,"description":"Playwright live recording including script resources; no live-reader endpoint; native Chrome refuses"},"screenshots":{"type":"boolean","default":false},"cpuSampling":{"type":"boolean","default":false},"snapshots":{"oneOf":[{"type":"boolean"},{"type":"object","properties":{"dom":{"type":"boolean","default":false},"aria":{"type":"boolean","default":false},"screen":{"type":"boolean","default":false}},"additionalProperties":false}],"default":false,"description":"Playwright DOM/network, ARIA and per-action screen snapshots; unavailable for native Chrome traces"},"name":{"type":["string","null"],"minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_-]+$"},"title":{"type":["string","null"],"minLength":1,"maxLength":256},"maxDurationMs":{"type":["integer","null"],"minimum":1000,"maximum":300000,"description":"Playwright capture duration; default 30000 ms"}},"additionalProperties":false}},"required":["operation","context"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"trace_status"}},"required":["operation"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"trace_stop"}},"required":["operation"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"trace_chunk_start"},"title":{"type":"string","minLength":1,"maxLength":256,"default":"Testrun investigation"},"name":{"type":["string","null"],"minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_-]+$"}},"required":["operation"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"trace_chunk_stop"}},"required":["operation"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"trace_group_start"},"name":{"type":"string","minLength":1,"maxLength":256},"location":crate::browser::automation_debugger::location_schema()},"required":["operation","name"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"trace_group_end"}},"required":["operation"],"additionalProperties":false}),
    ];
    let file_path = json!({"type":["string","null"],"minLength":1,"maxLength":4096,"description":"Native Chrome only: caller-relative or absolute destination; replaces the final extension with .gz for an exact lowercase .gz suffix, otherwise .json. Existing ancestor must be user-owned without group/other writes, or user/root-owned and sticky. New parents are private and created only for complete evidence; output stays mode 0600. Never overwrites. Stop may override the start path or retry retained evidence. Negotiated roots and URL inputs remain unsupported."});
    schemas[0]["properties"]["options"]["properties"]["filePath"] = file_path.clone();
    schemas[2]["properties"]["filePath"] = file_path;
    schemas[2]["properties"]["analysisPage"] = json!({"anyOf":[reference,{"type":"null"}],"description":"Native Chrome only: exact capture metadata page. After complete capture/export, reuse its live owned DevTools frontend or allocate one, then analyze the retained bytes. Mutually exclusive with analysisDeveloper. Receipt retains frontend/model cleanup handles; uncertain frontend creation requires explicit recovery. Does not analyze timed completion implicitly."});
    schemas[2]["properties"]["analysisDeveloper"] = json!({"anyOf":[reference,{"type":"null"}],"description":"Native Chrome only: after complete capture/export, automatically load retained bytes into this explicitly owned DevTools frontend for the capture metadata page. Does not open a frontend. Analysis failure retains complete evidence for explicit retry; release any retained failed model before retrying. Raw stop omission does not request analysis."});
    schemas[0]["properties"]["options"]["properties"]["reload"] = json!({"anyOf":[{"type":"object","properties":{"ignoreCache":{"type":"boolean","default":false}},"additionalProperties":false},{"type":"null"}],"description":"Native Chrome only: after tracing starts, reload the metadata page in the selected context and wait for load. No global cache setting is changed."});
    schemas[0]["properties"]["options"]["properties"]["page"] = json!({"anyOf":[reference,{"type":"null"}],"description":"Native Chrome only: exact owned page in context used for trace metadata and optional reload; omission uses the selected page or first page in context. Does not change page selection."});
    schemas[0]["properties"]["options"]["properties"]["autoStop"] = json!({"type":["boolean","null"],"description":"Native Chrome only: true requests finalization five seconds after start and optional reload complete; false keeps manual stopping. Cannot combine with maxDurationMs. Omission preserves raw trace duration defaults, not the performance tool's implicit defaults."});
    schemas[0]["properties"]["options"]["properties"]["performance"] = json!({"anyOf":[{"type":"object","properties":{"reload":{"type":"boolean","default":true},"autoStop":{"type":"boolean","default":true},"ignoreCache":{"type":"boolean","default":false}},"additionalProperties":false},{"type":"null"}],"description":"Native Chrome preset: reload and autoStop default true; DevTools default categories, CPU samples and screenshots are included. Reload resets to about:blank before recording, then returns to the exact saved URL and waits for load. ignoreCache bypasses the return navigation's HTTP cache and restores caching, including after failure; ignored when reload:false. Cannot combine with raw reload, autoStop or maxDurationMs. Analysis and full SDK tool responses require separate operations."});
    schemas[0]["properties"]["options"]["properties"]["maxDurationMs"]["description"] = json!(
        "Request finalization after this duration from start acknowledgement; native Chrome is manual when omitted, Playwright defaults to 30000 ms. Finalization may remain pending."
    );
    schemas
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn performance_lifecycle_defaults_are_opt_in_and_conflicting_raw_options_refuse() {
        let raw: Options = serde_json::from_value(json!({})).unwrap();
        assert!(raw.performance.is_none());
        assert_eq!(raw.automatic_stop(), None);
        for reload in [true, false] {
            for auto_stop in [true, false] {
                let options: Options = serde_json::from_value(
                    json!({"performance":{"reload":reload,"autoStop":auto_stop}}),
                )
                .unwrap();
                options.validate(true).unwrap();
                assert_eq!(options.automatic_stop(), Some(auto_stop));
                assert_eq!(
                    options.validate(false),
                    Err("TRACE_NATIVE_OPTIONS_UNSUPPORTED")
                );
            }
        }
        let defaults: Options = serde_json::from_value(json!({"performance":{}})).unwrap();
        assert!(defaults.performance.as_ref().unwrap().reload);
        assert_eq!(defaults.automatic_stop(), Some(true));
        for raw in [
            json!({"reload":{}}),
            json!({"autoStop":false}),
            json!({"maxDurationMs":5000}),
        ] {
            let mut value = raw;
            value["performance"] = json!({});
            let options: Options = serde_json::from_value(value).unwrap();
            assert_eq!(
                options.validate(true),
                Err("TRACE_PERFORMANCE_OPTIONS_CONFLICT")
            );
        }
        for invalid in [
            json!({"reload":null}),
            json!({"autoStop":"true"}),
            json!({"ignoreCache":null}),
        ] {
            assert!(serde_json::from_value::<Performance>(invalid).is_err());
        }
        let reference = super::super::contract::Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        crate::schema::validate(
            &crate::browser::contract::schemas()["request"],
            &json!({"operation":"trace_start","context":reference,"options":{"performance":{}}}),
        )
        .unwrap();
    }
    #[test]
    fn native_trace_boolean_stop_and_page_options_preserve_manual_defaults() {
        for auto_stop in [Some(false), Some(true)] {
            let options = Options {
                auto_stop,
                ..Default::default()
            };
            assert!(options.validate(true).is_ok());
            assert_eq!(
                options.validate(false),
                Err("TRACE_NATIVE_OPTIONS_UNSUPPORTED")
            );
            assert_eq!(
                Options {
                    max_duration_ms: Some(5000),
                    ..options
                }
                .validate(true),
                Err("TRACE_DURATION_CONFLICT")
            );
        }
        let options: Options = serde_json::from_value(json!({})).unwrap();
        assert_eq!(options.auto_stop, None);
        assert_eq!(options.page, None);
        assert_eq!(options.max_duration_ms, None);
        let reference = super::super::contract::Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        for auto_stop in [false, true] {
            let request = json!({"operation":"trace_start","context":reference,"options":{"page":reference,"autoStop":auto_stop}});
            crate::schema::validate(&crate::browser::contract::schemas()["request"], &request)
                .unwrap();
            let request: crate::browser::contract::Request =
                serde_json::from_value(request).unwrap();
            let crate::browser::contract::Request::TraceStart { options, .. } = request else {
                unreachable!()
            };
            assert_eq!(options.auto_stop, Some(auto_stop));
            assert_eq!(options.page, Some(reference));
        }
    }
    #[test]
    fn native_trace_duration_and_reload_are_explicit_and_engine_fenced() {
        let options = Options {
            max_duration_ms: Some(1000),
            reload: Some(Reload { ignore_cache: true }),
            ..Default::default()
        };
        assert!(options.validate(true).is_ok());
        assert_eq!(options.validate(false), Err("TRACE_RELOAD_UNSUPPORTED"));
        for duration in [0, 999, 300001] {
            assert_eq!(
                Options {
                    max_duration_ms: Some(duration),
                    ..Default::default()
                }
                .validate(true),
                Err("TRACE_BOUNDS")
            );
        }
        let request = crate::browser::contract::Request::TraceStart {
            context: crate::browser::contract::Reference {
                connection_id: uuid::Uuid::new_v4(),
                generation: uuid::Uuid::new_v4(),
                id: uuid::Uuid::new_v4(),
            },
            options,
        };
        crate::schema::validate(
            &crate::browser::contract::schemas()["request"],
            &serde_json::to_value(request).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn tracing_group_location_optional_and_present_requests_match_the_registered_schema() {
        for location in [
            None,
            Some(crate::browser::automation_debugger::Location {
                file: "Acme.spec.ts".into(),
                line: Some(42),
                column: Some(3),
            }),
        ] {
            let value = serde_json::to_value(crate::browser::contract::Request::TraceGroupStart {
                name: "Acme".into(),
                location,
            })
            .unwrap();
            crate::schema::validate(&crate::browser::contract::schemas()["request"], &value)
                .unwrap();
        }
    }
    #[test]
    fn snapshot_selection_and_names_are_engine_fenced_before_allocation() {
        for (value, expected) in [
            (json!(true), (true, false, false)),
            (json!({"aria":true,"screen":true}), (false, true, true)),
            (json!(false), (false, false, false)),
        ] {
            let snapshots: Snapshots = serde_json::from_value(value).unwrap();
            let selected = snapshots.selected();
            assert_eq!((selected.dom, selected.aria, selected.screen), expected);
            let options = Options {
                snapshots,
                ..Default::default()
            };
            assert!(options.validate(false).is_ok());
            assert_eq!(
                options.validate(true).is_err(),
                expected.0 || expected.1 || expected.2
            );
        }
        for name in [
            "../Acme",
            "/Acme",
            "Acme.trace",
            "Acme/name",
            "Acme\\name",
            "",
        ] {
            assert!(prefix(name).is_err());
        }
        assert!(prefix("Acme_trace-2").is_ok());
        assert!(serde_json::from_value::<Snapshots>(json!({"unknown":true})).is_err());
        assert!(
            Options {
                name: Some("Acme".into()),
                ..Default::default()
            }
            .validate(true)
            .is_err()
        );
    }
}
