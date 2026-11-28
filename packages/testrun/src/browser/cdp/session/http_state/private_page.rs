//! Hidden other targets never enter the public page index or application setup.
use super::*;
use std::collections::VecDeque;

pub(super) struct Collection {
    pub(super) capture: Uuid,
    pub(super) context: String,
    pub(super) target: Option<String>,
    pub(super) session: Option<String>,
    pub(super) complete: bool,
    destroyed: bool,
    requests: VecDeque<(String, String)>,
    failure: Option<&'static str>,
    deadline: Instant,
}

impl State {
    pub(in super::super) fn private_event(&mut self, event: &Event) -> Result<bool, Error> {
        let Some(collection) = self.collection.as_mut() else {
            return Ok(false);
        };
        let info = &event.params["targetInfo"];
        let target = info["targetId"]
            .as_str()
            .or_else(|| event.params["targetId"].as_str());
        let owns_target = target.is_some() && target == collection.target.as_deref();
        if owns_target {
            if info.is_object()
                && (info["type"] != "other" || info["browserContextId"] != collection.context)
            {
                collection.failure = Some("HTTP_STORAGE_PAGE_IDENTITY");
                return Err("BACKEND_PROTOCOL".into());
            }
            if event.method == "Target.attachedToTarget" {
                let session = event.params["sessionId"]
                    .as_str()
                    .ok_or("BACKEND_PROTOCOL")?;
                if collection
                    .session
                    .as_deref()
                    .is_some_and(|current| current != session)
                {
                    return Err("BACKEND_PROTOCOL".into());
                }
                collection.session = Some(session.into());
            }
            if event.method == "Target.targetDestroyed" {
                collection.destroyed = true;
            }
            return Ok(true);
        }
        // An uncertain hidden allocation loses the connection. Its events must
        // still never become a public resource while process cleanup is pending.
        if collection.target.is_none()
            && info["type"] == "other"
            && info["browserContextId"] == collection.context
        {
            return Ok(true);
        }
        let session = event.session_id.as_deref().or_else(|| {
            (event.method == "Target.detachedFromTarget")
                .then(|| event.params["sessionId"].as_str())
                .flatten()
        });
        if session.is_none() || session != collection.session.as_deref() {
            return Ok(false);
        }
        if event.method == "Fetch.requestPaused" {
            if collection.requests.len() >= 16 {
                return Err("STORAGE_BOUNDS".into());
            }
            let request = event.params["requestId"]
                .as_str()
                .ok_or("BACKEND_PROTOCOL")?;
            collection
                .requests
                .push_back((session.expect("Owned session").into(), request.into()));
        }
        Ok(true)
    }
}
impl Worker {
    pub(in super::super) async fn service_storage_requests(&mut self) {
        while let Some((session, request, deadline)) =
            self.http_state.collection.as_mut().and_then(|collection| {
                collection
                    .requests
                    .pop_front()
                    .map(|(session, request)| (session, request, collection.deadline))
            })
        {
            let Some(connection) = self.connection.clone() else {
                return;
            };
            // Remove before dispatch. A missing acknowledgement is never a
            // reason to replay a fulfillment; target teardown owns recovery.
            let result = connection.call(Some(&session), "Fetch.fulfillRequest",
                json!({"requestId":request,"responseCode":200,"responseHeaders":[{"name":"Content-Type","value":"text/html"}],"body":"PGh0bWw+PC9odG1sPg=="}),
                Lane::Control, deadline.min(Instant::now()+Duration::from_secs(5))).await;
            if result.is_err()
                && let Some(collection) = self.http_state.collection.as_mut()
            {
                collection.failure = Some("HTTP_STORAGE_PAGE_INTERCEPTION");
            }
        }
    }
    pub(super) async fn storage_page(
        &mut self,
        id: Uuid,
        context: Reference,
        capture: Uuid,
        deadline: Instant,
    ) -> Result<(String, String), Error> {
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        self.observation_ready(id, deadline)?;
        self.journal.record(json!({"operationId":id,"stage":"dispatch_admitted","method":"Target.createTarget"})).await?;
        self.http_state.collection = Some(Collection {
            capture,
            context: native.clone(),
            target: None,
            session: None,
            complete: false,
            destroyed: false,
            requests: VecDeque::new(),
            failure: None,
            deadline,
        });
        // Do not apply discovery events before the reply establishes ownership.
        // The pinned hidden target is other, including its empty-URL event;
        // it is excluded by native page auto-attachment in both launch modes.
        let created = connection.call(None,"Target.createTarget",
            json!({"url":"about:blank","browserContextId":native,"hidden":true,"background":true}),
            Lane::Ordinary, deadline.min(Instant::now()+Duration::from_secs(5))).await;
        let created = match created {
            Ok(value) => value,
            Err(error) => {
                use super::super::super::{Dispatch, Error as NativeError};
                let uncertain = matches!(
                    error,
                    NativeError::Timeout {
                        dispatch: Dispatch::PossiblyDispatched
                    } | NativeError::Lost {
                        dispatch: Dispatch::PossiblyDispatched,
                        ..
                    }
                );
                if uncertain {
                    self.lose("HTTP_STORAGE_PAGE_ALLOCATION_UNCERTAIN");
                    return Err(self.storage_cleanup_pending());
                }
                self.http_state.collection = None;
                return Err(protocol_error(error));
            }
        };
        let Some(target) = created["targetId"].as_str().map(str::to_owned) else {
            self.lose("HTTP_STORAGE_PAGE_ALLOCATION_UNCERTAIN");
            return Err(self.storage_cleanup_pending());
        };
        self.http_state
            .collection
            .as_mut()
            .expect("Allocation owner")
            .target = Some(target.clone());
        self.drain()?;
        let info = self
            .call_read(
                id,
                None,
                "Target.getTargetInfo",
                json!({"targetId":target}),
                deadline,
            )
            .await?;
        if info["targetInfo"]["targetId"] != target
            || info["targetInfo"]["browserContextId"] != native
            || info["targetInfo"]["type"] != "other"
        {
            return Err("HTTP_STORAGE_PAGE_IDENTITY".into());
        }
        let attached = self
            .call(
                id,
                None,
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                deadline,
            )
            .await?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        let collection = self
            .http_state
            .collection
            .as_mut()
            .expect("Allocation owner");
        if collection
            .session
            .as_deref()
            .is_some_and(|current| current != session)
        {
            return Err("BACKEND_PROTOCOL".into());
        }
        collection.session = Some(session.clone());
        self.configure_storage_page(id, &session, deadline).await?;
        Ok((target, session))
    }
    async fn configure_storage_page(
        &mut self,
        id: Uuid,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.call(id, Some(session), "Page.enable", json!({}), deadline)
            .await?;
        self.call(id, Some(session), "Runtime.enable", json!({}), deadline)
            .await?;
        self.call(
            id,
            Some(session),
            "Network.setBypassServiceWorker",
            json!({"bypass":true}),
            deadline,
        )
        .await?;
        self.call(
            id,
            Some(session),
            "Fetch.enable",
            json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
            deadline,
        )
        .await?;
        Ok(())
    }

    pub(super) fn storage_cleanup_pending(&self) -> Error {
        let mut error = Error::from("CLEANUP_PENDING");
        error.partial = Some(match &self.http_state.collection {
            Some(collection) if collection.complete => {
                json!({"captureId":collection.capture,"stateRetained":true,"cleanupPending":true,"nextAction":"http_evidence_finalize","replayed":false})
            }
            _ => {
                json!({"stateRetained":false,"cleanupPending":true,"nextAction":"browser_disconnect","replayed":false})
            }
        });
        error
    }
    pub(super) async fn cleanup_storage_page(&mut self, deadline: Instant) -> Result<(), Error> {
        let Some(collection) = &self.http_state.collection else {
            return Ok(());
        };
        let Some(target) = collection.target.clone() else {
            return Err(self.storage_cleanup_pending());
        };
        let acknowledged = if collection.destroyed {
            true
        } else {
            self.cleanup_call(
                None,
                "Target.closeTarget",
                json!({"targetId":target}),
                deadline,
            )
            .await
            .is_ok_and(|reply| reply["success"] == true)
        };
        let destroyed = self
            .http_state
            .collection
            .as_ref()
            .is_some_and(|collection| collection.destroyed);
        let absent = if destroyed {
            true
        } else {
            self.responsive_call(
                None,
                "Target.getTargets",
                json!({}),
                Lane::Ordinary,
                deadline,
            )
            .await
            .ok()
            .and_then(|reply| {
                reply["targetInfos"]
                    .as_array()
                    .map(|targets| !targets.iter().any(|info| info["targetId"] == target))
            })
            .unwrap_or(false)
        };
        // A query can carry a snapshot made before a destruction event which
        // is applied while awaiting that same reply. Recheck the live witness,
        // then wait after an acknowledged close without replaying the mutation.
        while !absent
            && !self
                .http_state
                .collection
                .as_ref()
                .is_some_and(|collection| collection.destroyed)
        {
            if !acknowledged || Instant::now() >= deadline {
                return Err(self.storage_cleanup_pending());
            }
            if self.observation_tick(deadline).await.is_err() {
                return Err(self.storage_cleanup_pending());
            }
        }
        self.http_state.collection = None;
        Ok(())
    }
    pub(super) fn require_storage_page(&self) -> Result<(), Error> {
        let collection = self
            .http_state
            .collection
            .as_ref()
            .ok_or("HTTP_STORAGE_PAGE_CLOSED")?;
        if collection.destroyed {
            return Err("HTTP_STORAGE_PAGE_CLOSED".into());
        }
        if let Some(failure) = collection.failure {
            return Err(failure.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};

    #[tokio::test]
    async fn native_http_state_context_retirement_includes_only_its_hidden_collector() {
        let (mut worker, mut peer, _root) = crate::browser::cdp::session::tests::fixture().await;
        let context = worker.index.register_context("context-acme").unwrap();
        let sibling = worker.index.register_context("context-sibling").unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        worker.http_state.collection = Some(Collection {
            capture: Uuid::new_v4(),
            context: "context-acme".into(),
            target: Some("Acme-hidden-target".into()),
            session: Some("Acme-hidden-session".into()),
            complete: false,
            destroyed: false,
            requests: VecDeque::new(),
            failure: None,
            deadline: until,
        });
        assert!(
            worker.index.list(Kind::Page).is_empty(),
            "The private collector must stay out of the public page graph"
        );
        let connection = worker.connection.clone().unwrap();
        let caller = connection.clone();
        let call = tokio::spawn(async move {
            caller.call(Some("Acme-hidden-session"),"Runtime.callFunctionOn",json!({"functionDeclaration":"function(){return new Promise(()=>{})}","uniqueContextId":"Acme-hidden-world","awaitPromise":true}),Lane::Ordinary,until).await
        });
        let request: Value =
            serde_json::from_str(peer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(request["method"], "Runtime.callFunctionOn");
        for scope in [sibling, context] {
            let closing = tokio::spawn(async move {
                let result = worker
                    .execute(
                        Uuid::new_v4(),
                        Request::ContextClose { context: scope },
                        until,
                    )
                    .await;
                (worker, result)
            });
            let dispose: Value =
                serde_json::from_str(peer.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(dispose["method"], "Target.disposeBrowserContext");
            assert!(
                !call.is_finished(),
                "A sibling or unacknowledged close cannot settle the hidden collector"
            );
            peer.send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":dispose["id"],"result":{}}).to_string().into(),
            ))
            .await
            .unwrap();
            let (closed, result) = closing.await.unwrap();
            worker = closed;
            assert_eq!(result.unwrap()["closed"], true);
            if scope == sibling {
                assert!(!call.is_finished());
                assert_eq!(connection.status().pending, 1);
            }
        }
        assert!(matches!(
            call.await.unwrap(),
            Err(crate::browser::cdp::Error::ScopeClosed {
                dispatch: crate::browser::cdp::Dispatch::PossiblyDispatched
            })
        ));
        assert_eq!(connection.status().pending, 0);
        assert_eq!(connection.status().retired_calls, 1);
        peer.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":request["id"],"sessionId":"Acme-hidden-session","result":{"result":{"type":"undefined"}}}).to_string().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while connection.status().retired_calls != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(connection.status().open);
        connection.close(until).await.unwrap();
    }

    #[tokio::test]
    async fn native_http_state_hidden_allocation_requires_current_operation_admission() {
        for (cancelled, closing, expired, expected) in [
            (true, false, false, "CANCELLED"),
            (false, true, false, "SESSION_NOT_READY"),
            (false, false, true, "CONDITION_TIMEOUT"),
        ] {
            let (mut worker, mut peer, _root) =
                crate::browser::cdp::session::tests::fixture().await;
            let context = worker.index.register_context("context-acme").unwrap();
            let id = Uuid::new_v4();
            {
                let mut state = worker.shared.state.lock().unwrap();
                state
                    .begin(id, "http_context_storage_state".into())
                    .unwrap();
                state.operations.get_mut(&id).unwrap().cancel_requested = cancelled;
                if closing {
                    state.phase = Phase::Closing;
                }
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            let deadline = if expired { Instant::now() } else { deadline };
            let result = tokio::select! {
                result=worker.storage_page(id,context,Uuid::new_v4(),deadline)=>result,
                message=peer.next()=>panic!("Hidden allocation dispatched without current operation admission: {message:?}"),
            };
            assert_eq!(result.unwrap_err().code, expected);
            assert!(worker.http_state.collection.is_none());
            assert!(
                worker.http_contexts.evidence_list()["pendingArtifacts"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
    }
}
