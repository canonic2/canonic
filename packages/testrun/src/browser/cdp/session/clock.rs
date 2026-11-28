//! Context-owned main-world clocks; host scheduling and protocol time stay real.
use super::*;
use crate::browser::clock::Action;

const HELPER: &str = include_str!("../../reading/clock.js");
const INSTALL: &str = include_str!("clock-install.js");
const APPLY: &str = include_str!("clock-apply.js");
const MAX_ACTIONS: usize = 256;

#[derive(PartialEq)]
enum Phase {
    Ready,
    Updating,
    Failed,
}
struct Clock {
    key: String,
    history: Vec<Value>,
    installations: BTreeMap<String, Option<String>>,
    phase: Phase,
}
#[derive(Default)]
pub(super) struct Clocks(BTreeMap<Reference, Clock>);

fn command(action: &Action) -> (&'static str, Option<i64>) {
    match action {
        Action::Install { time_ms } => (
            "install",
            Some(time_ms.unwrap_or_else(|| crate::browser::capture::now_ms() as i64)),
        ),
        Action::SetFixedTime { time_ms } => ("setFixedTime", Some(*time_ms)),
        Action::SetSystemTime { time_ms } => ("setSystemTime", Some(*time_ms)),
        Action::PauseAt { time_ms } => ("pauseAt", Some(*time_ms)),
        Action::RunFor { duration_ms } => ("runFor", Some(*duration_ms as i64)),
        Action::FastForward { duration_ms } => ("fastForward", Some(*duration_ms as i64)),
        Action::Resume {} => ("resume", None),
    }
}
impl Clock {
    fn source(&self, history: &[Value]) -> String {
        // One script owns ordered history. Existing documents are mutated only
        // by the action dispatch below. Future documents reconstruct time
        // without firing callbacks belonging to prior documents.
        format!(
            "({INSTALL})({key},{history},()=>{{const module={{exports:{{}}}};{HELPER}\nreturn module.exports;}})",
            key = json!(self.key),
            history = json!(history),
        )
    }
}
impl Worker {
    pub(super) fn fence_clocks(&mut self) {
        self.evaluation.clocks.0.retain(|context, clock| {
            clock
                .installations
                .retain(|session, _| self.index.retained_attachment(session));
            self.index.retained(*context, Kind::Context)
        });
    }
    async fn install_clock(
        &mut self,
        id: Uuid,
        context: Reference,
        session: &str,
        replace: bool,
        deadline: Instant,
    ) -> Result<(), Error> {
        let clock = self
            .evaluation
            .clocks
            .0
            .get_mut(&context)
            .ok_or("CLOCK_NOT_INSTALLED")?;
        if !clock.installations.contains_key(session) && clock.installations.len() >= 512 {
            return Err("CLOCK_TARGET_CAPACITY".into());
        }
        let previous = match clock.installations.get(session) {
            Some(None) => return Err("CLOCK_PENDING".into()),
            Some(Some(_)) if !replace => return Ok(()),
            previous => previous.cloned().flatten(),
        };
        let source = clock.source(&clock.history);
        clock.installations.insert(session.into(), None);
        let result = self
            .call(
                id,
                Some(session),
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source":source,"runImmediately":!replace}),
                deadline,
            )
            .await?;
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        if !self.index.retained_attachment(session) {
            return Err("STALE_REFERENCE".into());
        }
        let identifier = result["identifier"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 1024)
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        // Keep the prior bootstrap ahead of user scripts throughout the
        // replacement. New documents may see the prior virtual state until
        // this action dispatches, but never start without a clock bootstrap.
        self.order_init_scripts_last(id, session, deadline).await?;
        if let Some(previous) = previous {
            self.call(
                id,
                Some(session),
                "Page.removeScriptToEvaluateOnNewDocument",
                json!({"identifier":previous}),
                deadline,
            )
            .await?;
        }
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        if !self.index.retained_attachment(session) {
            return Err("STALE_REFERENCE".into());
        }
        self.evaluation
            .clocks
            .0
            .get_mut(&context)
            .ok_or("CLOCK_NOT_INSTALLED")?
            .installations
            .insert(session.into(), Some(identifier));
        Ok(())
    }
    pub(super) async fn configure_clock(
        &mut self,
        id: Uuid,
        kind: Kind,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        if !matches!(kind, Kind::Page | Kind::Frame) {
            return Ok(());
        }
        let Some((context, _, _)) = self.index.diagnostic_scope(session) else {
            return Ok(());
        };
        let Some(clock) = self.evaluation.clocks.0.get(&context) else {
            return Ok(());
        };
        if clock.phase == Phase::Failed {
            // A clock failure belongs to its context. Do not turn a later
            // attachment into a session-wide startup failure that blocks close.
            return Ok(());
        }
        let result = self
            .install_clock(id, context, session, false, deadline)
            .await;
        if result.is_err()
            && let Some(clock) = self.evaluation.clocks.0.get_mut(&context)
        {
            clock.phase = Phase::Failed;
        }
        result
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One ordered clock mutation retains per-world progress and one failure fence across native awaits"
    )]
    pub(super) async fn context_clock(
        &mut self,
        id: Uuid,
        context: Reference,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        action.validate()?;
        self.drain()?;
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.fence_clocks();
        if Instant::now() >= deadline {
            return Err("NOT_DISPATCHED".into());
        }
        let sessions = self
            .script_targets(context)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let clock = self
            .evaluation
            .clocks
            .0
            .entry(context)
            .or_insert_with(|| Clock {
                key: format!("__testrunClock_{}", Uuid::new_v4().simple()),
                history: Vec::new(),
                installations: BTreeMap::new(),
                phase: Phase::Ready,
            });
        if clock.phase != Phase::Ready {
            return Err("CLOCK_PENDING".into());
        }
        if clock.history.len() >= MAX_ACTIONS {
            return Err("CLOCK_ACTION_CAPACITY".into());
        }
        if sessions.len() > 512 {
            return Err("CLOCK_TARGET_CAPACITY".into());
        }
        let (method, parameter) = command(&action);
        clock.history.push(json!([
            method,
            crate::browser::capture::now_ms(),
            parameter
        ]));
        clock.phase = Phase::Updating;
        let expression = format!(
            "{bootstrap};({APPLY})({key},{sequence},{method},{parameter})",
            bootstrap = clock.source(&clock.history[..clock.history.len() - 1]),
            key = json!(clock.key),
            sequence = clock.history.len(),
            method = json!(method),
            parameter = json!(parameter),
        );
        let mut completed = Vec::new();
        let result: Result<(), Error> = async {
            for session in sessions {
                self.install_clock(id, context, &session, true, deadline)
                    .await?;
            }
            let worlds = self
                .index
                .list(Kind::Page)
                .into_iter()
                .filter(|(_, parent)| *parent == Some(context))
                .flat_map(|(page, _)| self.index.page_worlds(page))
                .collect::<BTreeSet<_>>();
            for world in worlds {
                let route = self
                    .index
                    .route(world, Some(Kind::World))
                    .map_err(resource_error)?;
                let mut params =
                    json!({"expression":expression,"awaitPromise":true,"returnByValue":true});
                params["uniqueContextId"] =
                    json!(route.execution_unique_id.ok_or("INVALID_SCOPE")?);
                let reply = self
                    .call(
                        id,
                        Some(&route.session_id),
                        "Runtime.evaluate",
                        params,
                        deadline,
                    )
                    .await?;
                self.index
                    .route(world, Some(Kind::World))
                    .map_err(resource_error)?;
                if reply.get("exceptionDetails").is_some() {
                    return Err("CLOCK_EXECUTION_FAILED".into());
                }
                if reply["result"]["value"] != true {
                    return Err("BACKEND_PROTOCOL".into());
                }
                completed.push(world);
            }
            self.index
                .native(context, Kind::Context)
                .map_err(resource_error)?;
            if self
                .evaluation
                .clocks
                .0
                .get(&context)
                .is_some_and(|clock| clock.phase == Phase::Failed)
            {
                return Err("CLOCK_PENDING".into());
            }
            Ok(())
        }
        .await;
        if let Err(mut error) = result {
            if let Some(clock) = self.evaluation.clocks.0.get_mut(&context) {
                clock.phase = Phase::Failed;
            }
            error.partial = Some(
                json!({"context":context,"action":action,"completedWorlds":completed,"clockPending":true,"stateMayHaveChanged":true,"replayed":false,"restoration":"close_owned_context","cause":error.partial}),
            );
            return Err(error);
        }
        self.evaluation
            .clocks
            .0
            .get_mut(&context)
            .ok_or("CLOCK_NOT_INSTALLED")?
            .phase = Phase::Ready;
        Ok(
            json!({"context":context,"action":action,"backend":"chrome_cdp","implementation":"testrun-clock-v1","playwrightClockEquivalent":false,"dispatchCompleted":true,"effectsObserved":false,"scope":"context-main-world-pages-frames-and-future-documents","completedWorlds":completed,"controlledApis":["Date.now","new Date()","Date()","setTimeout","clearTimeout","setInterval","clearInterval","requestAnimationFrame","cancelAnimationFrame","requestIdleCallback","cancelIdleCallback","performance.now","performance.timeOrigin","Event.timeStamp"],"performanceFacade":"synthetic-marks-measures-empty-entries-other-methods-stubbed","animationFrames":"synthetic-16ms-callbacks-not-native-paint","excluded":["workers","other-contexts","pre-install-timers-and-cached-native-functions","CSS-and-Web-Animations","media-playback","browser-and-OS-time"],"hostDeadlinesUnchanged":true,"futureDocumentHistory":"reconstruct-time-without-replaying-old-document-callbacks","remainingActions":MAX_ACTIONS-self.evaluation.clocks.0[&context].history.len(),"replayed":false,"restoration":"close_owned_context"}),
        )
    }
}
