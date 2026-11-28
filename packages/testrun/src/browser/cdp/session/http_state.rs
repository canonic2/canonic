//! Native HTTP state origin observations and owned collection lifecycle.
use super::*;
mod capture;
mod origins;
mod private_page;
mod snapshots;

#[derive(Default)]
pub(super) struct State {
    origins: origins::Origins,
    collection: Option<private_page::Collection>,
    capturing: bool,
}
pub(super) enum Observation {
    Navigation {
        session: String,
        frame: String,
        origin: Result<Option<String>, &'static str>,
    },
    Attachment {
        session: String,
        target: String,
    },
}
impl Observation {
    pub(super) fn event(event: &Event) -> Option<Self> {
        match event.method.as_str() {
            "Page.frameNavigated" => Some(Self::Navigation {
                session: event.session_id.clone()?,
                frame: event.params["frame"]["id"].as_str()?.into(),
                origin: origins::origin(event.params["frame"]["url"].as_str()?),
            }),
            "Target.attachedToTarget" if event.params["targetInfo"]["type"] == "page" => {
                Some(Self::Attachment {
                    session: event.params["sessionId"].as_str()?.into(),
                    target: event.params["targetInfo"]["targetId"].as_str()?.into(),
                })
            }
            _ => None,
        }
    }
}
impl State {
    pub(super) fn retirement_session(&self, context: &str) -> Option<String> {
        self.collection
            .as_ref()
            .filter(|collection| collection.context == context)
            .and_then(|collection| collection.session.clone())
    }
    pub(super) fn observe(&mut self, index: &Index, observation: Observation) {
        let session = match &observation {
            Observation::Navigation { session, .. } | Observation::Attachment { session, .. } => {
                session
            }
        };
        let Some((context, Some(page), None)) = index.diagnostic_scope(session) else {
            return;
        };
        if index.native(context, Kind::Context).is_err() {
            return;
        }
        match observation {
            Observation::Navigation { frame, origin, .. } => {
                let main = index
                    .page_frame(page, None)
                    .ok()
                    .and_then(|frame| index.native(frame, Kind::Frame).ok())
                    .is_some_and(|main| main == frame);
                self.origins.navigate(context, page, origin, main);
            }
            Observation::Attachment { target, .. } => {
                if index.target_reference(&target) == Some(page) {
                    self.origins.attach(context, page);
                }
            }
        }
    }
    pub(super) fn seed(&mut self, index: &Index, session: &str, result: &Value) {
        let Some((context, Some(page), None)) = index.diagnostic_scope(session) else {
            return;
        };
        if index.native(context, Kind::Context).is_err() {
            return;
        }
        self.origins.attach(context, page);
        let main = index
            .page_frame(page, None)
            .ok()
            .and_then(|frame| index.native(frame, Kind::Frame).ok());
        let mut trees = vec![&result["frameTree"]];
        while let Some(tree) = trees.pop() {
            let frame = &tree["frame"];
            if let Some(url) = frame["url"].as_str() {
                self.origins.navigate(
                    context,
                    page,
                    origins::origin(url),
                    main.as_deref() == frame["id"].as_str(),
                );
            }
            if let Some(children) = tree["childFrames"].as_array() {
                trees.extend(children.iter().rev());
            }
        }
    }
    pub(super) fn close(&mut self, reference: Reference, kind: Kind) {
        match kind {
            Kind::Context => self.origins.close_context(reference),
            Kind::Page => self.origins.close_page(reference),
            _ => {}
        }
    }
    pub(super) fn gap(&mut self, index: &Index) {
        self.origins.gap(
            index
                .list(Kind::Context)
                .into_iter()
                .map(|(context, _)| context),
        );
    }
}
