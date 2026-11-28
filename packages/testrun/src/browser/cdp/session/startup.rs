//! Own debugger-start pauses until target setup completes or owned teardown wins.
use super::*;

#[derive(Default)]
pub(super) struct Startup {
    pub enabled: bool,
    pub busy: bool,
    pub operation: Uuid,
    pub failed: Option<&'static str>,
    paused: BTreeMap<String, bool>,
    resumed_pages: usize,
    resumed_frames: usize,
    resumed_workers: usize,
}
impl Worker {
    pub(super) async fn retire_edge_browser_ui(
        &mut self,
        id: Uuid,
        session: &str,
        deadline: Instant,
    ) -> Result<bool, Error> {
        let Some((context, Some(page), None)) = self.index.diagnostic_scope(session) else {
            return Ok(false);
        };
        let native = self
            .index
            .native(page, Kind::Page)
            .map_err(resource_error)?
            .to_owned();
        let info = self
            .call_read(
                id,
                None,
                "Target.getTargetInfo",
                json!({"targetId":native}),
                deadline,
            )
            .await?;
        if info["targetInfo"]["type"] != "browser_ui" {
            return Ok(false);
        }
        if info["targetInfo"]["targetId"] != native
            || info["targetInfo"]["browserContextId"]
                != self
                    .index
                    .native(context, Kind::Context)
                    .map_err(resource_error)?
        {
            return Err("INVALID_SCOPE".into());
        }
        // Edge initially reports downloads-hub as a paused application page.
        // Page.enable cannot reply until resumed; browser-level metadata can
        // identify the UI first, without running application code prematurely.
        self.cleanup_call(
            Some(session),
            "Runtime.runIfWaitingForDebugger",
            json!({}),
            deadline,
        )
        .await?;
        self.cleanup_call(
            None,
            "Target.detachFromTarget",
            json!({"sessionId":session}),
            deadline,
        )
        .await?;
        if self.index.retained(page, Kind::Page) {
            self.index.retire(page).map_err(resource_error)?;
        }
        self.startup.paused.remove(session);
        self.remember_close(page, Kind::Page)?;
        self.interactions.history.push(
            context,
            page,
            crate::browser::activity::Event::PageClosed,
            page,
        );
        self.publish();
        Ok(true)
    }

    pub(super) fn startup_event(&mut self, event: &Event) {
        if event.method == "Target.attachedToTarget" && event.params["waitingForDebugger"] == true {
            if let Some(session) = event.params["sessionId"].as_str() {
                if self.startup.paused.len() >= 512 && !self.startup.paused.contains_key(session) {
                    self.startup.failed = Some("RESOURCE_CAPACITY");
                    return;
                }
                self.startup.paused.insert(
                    session.into(),
                    self.index.diagnostic_scope(session).is_some(),
                );
            }
        } else if event.method == "Target.detachedFromTarget"
            && let Some(session) = event.params["sessionId"].as_str()
        {
            self.startup.paused.remove(session);
        }
    }
    pub(super) fn startup_summary(&self) -> Value {
        json!({"automaticAttachment":self.startup.enabled,"pausedTargets":self.startup.paused.len(),"setupPending":self.startup.busy,"failure":self.startup.failed,"resumeAcknowledgements":{"pages":self.startup.resumed_pages,"frames":self.startup.resumed_frames,"workers":self.startup.resumed_workers}})
    }
    pub(super) fn startup_paused(&self, session: &str) -> bool {
        self.startup.paused.get(session) == Some(&true)
    }
    pub(super) fn startup_pending(&self) -> bool {
        self.startup.enabled
            && self.startup.failed.is_none()
            && !self.startup.paused.is_empty()
            && matches!(
                self.shared.state.lock().expect("Chrome lifecycle").phase,
                Phase::Ready | Phase::Starting
            )
    }
    pub(super) async fn enable_startup(
        &mut self,
        id: Uuid,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.startup.operation = id;
        self.startup.enabled = true;
        // Page attachments own their related frames/workers recursively. Do not
        // install a competing root attachment for those descendants.
        self.call(id,None,"Target.setAutoAttach",json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true,"filter":[{"type":"page"},{"exclude":true}]}),deadline).await?;
        self.reconcile(id, deadline).await
    }
    pub(super) async fn service_startup(&mut self, deadline: Instant) {
        if self.startup.busy || !self.startup_pending() {
            return;
        }
        // Setup is internal ownership work, independent from a caller's lease
        // or cancellation. Its failure must not replace a pending write receipt.
        if let Err(error) = Box::pin(self.reconcile(
            self.startup.operation,
            deadline.min(Instant::now() + Duration::from_secs(5)),
        ))
        .await
        {
            self.startup.failed = Some(error.code);
            self.publish();
        }
    }
    pub(super) async fn resume_startup(&mut self, deadline: Instant) -> Result<(), Error> {
        let sessions = self
            .startup
            .paused
            .iter()
            .filter(|(session, owned)| {
                !**owned
                    || self.configured.contains(*session)
                    || !self.index.retained_attachment(session)
            })
            .map(|(session, owned)| (session.clone(), *owned))
            .collect::<Vec<_>>();
        for (session, owned) in sessions {
            let kind = self
                .index
                .targets()
                .into_iter()
                .find(|(_, _, _, attached)| attached.as_deref() == Some(&session))
                .map(|(_, kind, _, _)| kind);
            if owned && self.index.diagnostic_scope(&session).is_none() {
                if !self.index.retained_attachment(&session) {
                    self.startup.paused.remove(&session);
                }
                continue;
            }
            let resumed = self
                .cleanup_call(
                    Some(&session),
                    "Runtime.runIfWaitingForDebugger",
                    json!({}),
                    deadline,
                )
                .await;
            if let Err(error) = resumed {
                if !self.startup.paused.contains_key(&session)
                    || (owned && !self.index.retained_attachment(&session))
                {
                    self.startup.paused.remove(&session);
                    continue;
                }
                return Err(error);
            }
            self.startup.paused.remove(&session);
            match kind {
                Some(Kind::Page) => {
                    self.startup.resumed_pages = self.startup.resumed_pages.saturating_add(1)
                }
                Some(Kind::Frame) => {
                    self.startup.resumed_frames = self.startup.resumed_frames.saturating_add(1)
                }
                Some(Kind::Worker) => {
                    self.startup.resumed_workers = self.startup.resumed_workers.saturating_add(1)
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(super) async fn startup_attachment(
        &mut self,
        id: Uuid,
        page: Reference,
        deadline: Instant,
    ) -> Result<String, Error> {
        loop {
            self.drain()?;
            if let Some((_, _, _, Some(session))) = self
                .index
                .targets()
                .into_iter()
                .find(|(reference, _, _, _)| *reference == page)
            {
                self.reconcile(id, deadline).await?;
                return Ok(session);
            }
            self.index
                .native(page, Kind::Page)
                .map_err(resource_error)?;
            if Instant::now() >= deadline {
                return Err("TARGET_ATTACHMENT_PENDING".into());
            }
            self.observation_tick(deadline).await?;
        }
    }
}
