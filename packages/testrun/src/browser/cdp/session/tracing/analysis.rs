//! Stop-time analysis binds an existing owned frontend to immutable capture evidence.
use super::*;

impl Worker {
    fn trace_analysis_page(&self) -> Result<Reference, Error> {
        Ok(if let Some(trace) = self.trace.as_ref() {
            trace.metadata_page.ok_or("TRACE_PAGE_REQUIRED")?
        } else {
            let last = self.trace_last.as_ref().ok_or("TRACE_NOT_ACTIVE")?;
            serde_json::from_value(last.value["report"]["metadataPage"].clone())
                .map_err(|_| Error::from("TRACE_PAGE_REQUIRED"))?
        })
    }

    fn trace_analysis_frontend(&self, developer: Reference) -> Result<(), Error> {
        let page = self.trace_analysis_page()?;
        let status = self.developer_status(developer)?;
        if status["page"] != json!(page) {
            return Err("TRACE_ANALYSIS_PAGE_MISMATCH".into());
        }
        if status["frontendOwned"] != true {
            return Err("PERFORMANCE_OWNED_FRONTEND_REQUIRED".into());
        }
        self.developer_session(developer)?;
        self.index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        Ok(())
    }

    pub(in crate::browser::cdp::session) async fn trace_stop_for_analysis_page(
        &mut self,
        id: Uuid,
        file_path: Option<String>,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.trace_analysis_page()? != page {
            return Err("TRACE_ANALYSIS_PAGE_MISMATCH".into());
        }
        let cached = self.trace.is_none()
            && self
                .trace_last
                .as_ref()
                .is_some_and(|last| last.value["analysis"].is_object());
        if !cached {
            self.index
                .native(page, Kind::Page)
                .map_err(resource_error)?;
        }
        let capture = self.trace_stop_to(file_path, deadline).await?;
        if cached {
            return Ok(capture);
        }
        let allocation = async {
            if let Some(developer) = self.analysis_developer_for_page(page)? {
                return Ok((developer, false));
            }
            if capture["analysisPending"]["frontendCreationUncertain"] == true {
                return Err(Error::from("TRACE_ANALYSIS_FRONTEND_RECOVERY_REQUIRED"));
            }
            let opened = Box::pin(self.developer_open_for_analysis(
                id,
                page,
                Some("timeline".into()),
                deadline,
            ))
            .await?;
            let developer = serde_json::from_value(opened["developer"].clone())
                .map_err(|_| Error::from("BACKEND_PROTOCOL"))?;
            Ok((developer, opened["frontendOwned"] == true))
        }
        .await;
        let (developer, created) = match allocation {
            Ok(value) => value,
            Err(mut error) => {
                let frontends = self.developer_snapshots();
                let uncertain = self.developers.analysis_uncertain.contains(&page)
                    || capture["analysisPending"]["frontendCreationUncertain"] == true;
                if uncertain {
                    self.developers.analysis_uncertain.insert(page);
                }
                let last = self.trace_last.as_mut().ok_or("TRACE_NOT_ACTIVE")?;
                last.value["analysisPending"] = json!({"page":page,"code":error.code,
                    "frontends":frontends,"frontendCreationUncertain":uncertain,
                    "recollected":false,"retry":"Inspect and release retained frontend/model owners before retry; uncertain creation without a known frontend requires browser cleanup or explicit analysisDeveloper recovery"});
                error.partial = Some(last.value.clone());
                return Err(error);
            }
        };
        let last = self.trace_last.as_mut().ok_or("TRACE_NOT_ACTIVE")?;
        if last.value["analysisFrontend"]["developer"] != json!(developer) {
            last.value["analysisFrontend"] = json!({"developer":developer,"page":page,
                "createdByStop":created,"nextAction":"developer_close"});
        }
        let result =
            Box::pin(self.trace_stop_with_analysis(id, None, Some(developer), deadline)).await;
        result.map_err(|mut error| {
            if error.partial.is_none()
                && let Some(last) = self.trace_last.as_mut()
            {
                last.value["analysisPending"] = json!({"page":page,"developer":developer,
                    "code":error.code,"recollected":false,
                    "retry":"Release the retained frontend/model owner, then retry analysisPage"});
                error.partial = Some(last.value.clone());
            }
            error
        })
    }

    pub(in crate::browser::cdp::session) async fn trace_stop_with_analysis(
        &mut self,
        id: Uuid,
        file_path: Option<String>,
        developer: Option<Reference>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if let Some(developer) = developer {
            let cached = self.trace.is_none()
                && self
                    .trace_last
                    .as_ref()
                    .is_some_and(|last| last.value["analysis"].is_object());
            if cached {
                if self.trace_last.as_ref().expect("Analysis receipt").value["analysis"]["developer"]
                    != json!(developer)
                {
                    return Err("FOREIGN_PERFORMANCE_ANALYSIS".into());
                }
            } else {
                // Refuse a different page or borrowed/lost frontend before ending
                // a recording. A successful receipt can replay after model release.
                self.trace_analysis_frontend(developer)?;
            }
        }
        let capture = self.trace_stop_to(file_path, deadline).await?;
        let Some(developer) = developer else {
            return Ok(capture);
        };
        if capture["analysis"].is_object() {
            return Ok(capture);
        }
        let evidence = serde_json::from_value(capture["report"]["evidenceId"].clone())
            .map_err(|_| Error::from("EVIDENCE_NOT_FOUND"))?;
        // Complete capture/export precedes parsing. Existing capacity and exact
        // frontend ownership remain enforced by the ordinary performance owner.
        let result = match self.trace_analysis_frontend(developer) {
            Ok(()) => {
                Box::pin(self.performance_operation(
                    id,
                    Request::PerformanceLoad {
                        developer,
                        evidence_id: evidence,
                    },
                    deadline,
                ))
                .await
            }
            Err(error) => Err(error),
        };
        let retained_model = self.performance_status()["analysis"].clone();
        let last = self.trace_last.as_mut().ok_or("TRACE_NOT_ACTIVE")?;
        if last.value["report"]["evidenceId"] != json!(evidence) {
            let mut error = Error::from("TRACE_ANALYSIS_OWNER_LOST");
            error.partial =
                Some(json!({"evidenceId":evidence,"analysis":result.ok(),"recollected":false}));
            return Err(error);
        }
        match result {
            Ok(analysis) => {
                last.value
                    .as_object_mut()
                    .expect("Trace receipt")
                    .remove("analysisPending");
                last.value["analysis"] = analysis;
                last.reply()
            }
            Err(mut error) => {
                last.value["analysisPending"] = json!({"developer":developer,"code":error.code,"model":error.partial.unwrap_or(retained_model),"recollected":false,"retry":"Release any retained failed model, then repeat trace_stop with analysisDeveloper; another owned frontend for the same capture page may be selected"});
                error.partial = Some(last.value.clone());
                Err(error)
            }
        }
    }
}
