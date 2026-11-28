//! Parent-frame visibility and trusted-event hit guards, including OOPIF owners.
use super::*;
use crate::browser::page::Action;

impl Worker {
    #[expect(
        clippy::too_many_arguments,
        reason = "Preflight retains every ancestor group until the input transaction ends"
    )]
    pub(super) async fn ancestor_guards(
        &mut self,
        id: Uuid,
        page: Reference,
        scope: &page::Scope,
        action: &Action,
        force: bool,
        group: &str,
        retained: &mut Vec<page::Scope>,
        deadline: Instant,
    ) -> Result<(), Error> {
        if force
            || matches!(
                action,
                Action::Dispatch { .. }
                    | Action::Focus {}
                    | Action::Blur {}
                    | Action::Press { .. }
                    | Action::Type { .. }
            )
        {
            return Ok(());
        }
        let mut child = scope.frame;
        for _ in 0..32 {
            let parent = self
                .index
                .list(Kind::Frame)
                .into_iter()
                .find(|(frame, _)| *frame == child)
                .and_then(|(_, parent)| parent)
                .ok_or("INVALID_SCOPE")?;
            if self.index.route(parent, Some(Kind::Page)).is_ok() {
                return Ok(());
            }
            let parent_scope = self.page_scope(id, page, Some(parent), deadline).await?;
            let child_native = self
                .index
                .native(child, Kind::Frame)
                .map_err(resource_error)?;
            let owner = self
                .call_read(
                    id,
                    Some(&parent_scope.session),
                    "DOM.getFrameOwner",
                    json!({"frameId":child_native}),
                    deadline,
                )
                .await?;
            let object=self.call_read(id,Some(&parent_scope.session),"DOM.resolveNode",json!({"backendNodeId":owner["backendNodeId"],"executionContextId":parent_scope.numeric,"objectGroup":group}),deadline).await?;
            retained.push(parent_scope.clone());
            let object = object["object"]["objectId"]
                .as_str()
                .ok_or("FRAME_UNAVAILABLE")?;
            self.call(
                id,
                Some(&parent_scope.session),
                "DOM.scrollIntoViewIfNeeded",
                json!({"objectId":object}),
                deadline,
            )
            .await?;
            let pointer = matches!(
                action,
                Action::Click {}
                    | Action::DoubleClick {}
                    | Action::Check { .. }
                    | Action::Drag { .. }
            );
            loop {
                self.observation_ready(id, deadline)?;
                let result=self.action_eval(id,&parent_scope,object,json!({"stage":"prepare","action":if pointer{Action::Click{}}else{Action::Hover{}},"force":false}),deadline,false).await?;
                if result["retry"] != true {
                    break;
                }
                self.observation_tick(deadline).await?;
            }
            child = parent;
        }
        Err("FRAME_DEPTH_BOUNDS".into())
    }
}
