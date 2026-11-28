//! Incremental coverage keeps drained counters owned until immutable evidence is written.
use super::*;

#[derive(Clone)]
pub(super) struct Capture {
    pub(super) id: Uuid,
    uncertain: bool,
    reports: Vec<Value>,
}
impl Worker {
    pub(in crate::browser::cdp::session) async fn profile_take_coverage(
        &mut self,
        id: Uuid,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?;
        let profile = self
            .profiling
            .items
            .get(&page)
            .ok_or("PROFILE_NOT_ACTIVE")?;
        if matches!(
            profile.state,
            "starting" | "finalizing" | "capturing-coverage"
        ) {
            return Err("PROFILE_BUSY".into());
        }
        if let Some(delta) = &profile.delta {
            if delta.uncertain {
                return Err(self.coverage_error(page, "PROFILE_COVERAGE_UNCERTAIN"));
            }
            return self.coverage_finalize(page);
        }
        if profile.state == "coverage-uncertain" {
            return Err(self.coverage_error(page, "PROFILE_COVERAGE_UNCERTAIN"));
        }
        if profile.state != "recording" {
            return Err("PROFILE_BUSY".into());
        }
        let count = usize::from(profile.javascript) + usize::from(profile.css);
        if count == 0 {
            return Err("PROFILE_COVERAGE_NOT_ENABLED".into());
        }
        let capture = Uuid::new_v4();
        self.developers
            .evidence
            .reserve(capture, count, count * 8 * 1024 * 1024)?;
        let session = profile.session.clone();
        let sdk = profile.options.sdk();
        let javascript = profile.javascript;
        let css = profile.css;
        let p = self.profiling.items.get_mut(&page).expect("Profile owner");
        p.delta = Some(Capture {
            id: capture,
            uncertain: false,
            reports: Vec::new(),
        });
        p.state = "capturing-coverage";
        for (enabled, method, format) in [
            (
                javascript,
                "Profiler.takePreciseCoverage",
                if sdk {
                    "playwright-javascript-coverage-delta"
                } else {
                    "v8-precise-javascript-coverage-delta"
                },
            ),
            (
                css,
                "CSS.takeCoverageDelta",
                if sdk {
                    "playwright-css-coverage-delta"
                } else {
                    "cdp-css-rule-usage-delta"
                },
            ),
        ] {
            if !enabled {
                continue;
            }
            let result = self
                .call(id, Some(&session), method, json!({}), deadline)
                .await;
            let result = match result {
                Ok(value) => {
                    let value = self
                        .profile_result(page, value, method == "CSS.takeCoverageDelta", deadline)
                        .await;
                    self.coverage_stage(page, format, value)
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                return Err(self.coverage_capture_failed(page, method, error));
            }
        }
        self.profiling
            .items
            .get_mut(&page)
            .expect("Profile owner")
            .state = "coverage-finalization-pending";
        self.coverage_finalize(page)
    }
    fn coverage_capture_failed(
        &mut self,
        page: Reference,
        method: &str,
        mut error: Error,
    ) -> Error {
        let p = self.profiling.items.get_mut(&page).expect("Profile owner");
        p.interrupted = true;
        p.state = "coverage-uncertain";
        p.delta.as_mut().expect("Coverage owner").uncertain = true;
        // Never issue the same counter-draining command again after loss.
        if method == "Profiler.takePreciseCoverage" {
            p.javascript_read = true;
        }
        let _=self.coverage_stage(page,"native-coverage-capture-status-json",json!({"complete":false,"failure":error.code,"counterOutcomeUnknown":error.code=="OUTCOME_UNKNOWN","captureIncomplete":true,"replayed":false}));
        error.partial = self.coverage_error(page, error.code).partial;
        error
    }

    fn coverage_error(&self, page: Reference, code: &'static str) -> Error {
        let p = &self.profiling.items[&page];
        let mut error = Error::from(code);
        error.partial = Some(
            json!({"page":page,"profileId":p.id,"captureId":p.delta.as_ref().map(|delta|delta.id),"nextAction":if p.state=="coverage-uncertain"||p.delta.as_ref().is_some_and(|delta|delta.uncertain){"profile_stop"}else{"profile_take_coverage"},"replayed":false}),
        );
        error
    }
    fn coverage_stage(
        &mut self,
        page: Reference,
        format: &'static str,
        value: Value,
    ) -> Result<(), Error> {
        let p = &self.profiling.items[&page];
        let capture = p.delta.as_ref().expect("Coverage owner").id;
        let (bytes, format, within_bound) =
            crate::browser::profiling::encode_coverage(&value, format)?;
        if !within_bound {
            p.sources.gap();
        }
        self.developers.evidence.stage(capture,Format{prefix:"coverage",extension:"json",mime:"application/json",name:format,max_bytes:8*1024*1024},bytes,json!({"page":page,"profileId":p.id,"private":true,"redacted":false,"sourceTextsIncluded":format!="native-coverage-capture-status-json"&&p.options.sources(),"coverageSourceComplete":p.sources.complete(),"complete":!p.interrupted&&(!format.contains("css")||!p.sheet_loss)&&p.sources.complete(),"coverageWindow":"since-previous-take","navigation":if p.options.reset(){"reset-on-navigation"}else{"cumulative"}}))
    }
    pub(in crate::browser::cdp::session) fn coverage_finalize_capture(
        &mut self,
        capture: Uuid,
    ) -> Option<Result<Value, Error>> {
        let page = self.profiling.items.iter().find_map(|(page, profile)| {
            profile
                .delta
                .as_ref()
                .is_some_and(|delta| delta.id == capture)
                .then_some(*page)
        })?;
        Some(self.coverage_finalize(page))
    }
    pub(super) fn coverage_finalize(&mut self, page: Reference) -> Result<Value, Error> {
        let capture = self.profiling.items[&page]
            .delta
            .as_ref()
            .expect("Coverage owner")
            .id;
        let result = self
            .developers
            .evidence
            .finalize(self.journal.directory(), capture);
        let p = self.profiling.items.get_mut(&page).expect("Profile owner");
        let delta = p.delta.as_mut().expect("Coverage owner");
        match result {
            Ok(value) => {
                delta
                    .reports
                    .extend(value["reports"].as_array().into_iter().flatten().cloned());
                let reports = std::mem::take(&mut delta.reports);
                let uncertain = delta.uncertain;
                p.delta = None;
                p.state = if uncertain || p.interrupted {
                    "coverage-uncertain"
                } else {
                    "recording"
                };
                Ok(
                    json!({"page":page,"profileId":p.id,"captureId":capture,"reports":reports,"finalized":true,"complete":!uncertain&&!p.interrupted&&(!p.options.css_coverage||!p.sheet_loss)&&p.sources.complete(),"recording":true,"replayed":false}),
                )
            }
            Err(mut error) => {
                if let Some(reports) = error
                    .partial
                    .as_ref()
                    .and_then(|value| value["reports"].as_array())
                {
                    delta.reports.extend(reports.iter().cloned());
                }
                error.partial = self.coverage_error(page, error.code).partial;
                Err(error)
            }
        }
    }
}
