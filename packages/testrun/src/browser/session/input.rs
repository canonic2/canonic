//! Native input primitives keep their actual reply owned before balancing holds.
use super::*;
use crate::browser::{
    input::{Action, Button, OBSERVATION_TTL, Observation},
    keyboard,
    objects::ObjectRef,
};

#[derive(Clone)]
enum Effect {
    None,
    Key {
        code: String,
        key: String,
        down: bool,
    },
    Button {
        button: Button,
        down: bool,
    },
    Touch,
}
struct Primitive {
    method: &'static str,
    params: Value,
    effect: Effect,
    delay: u64,
}
impl Primitive {
    fn call(method: &'static str, params: Value, effect: Effect) -> Self {
        Self {
            method,
            params,
            effect,
            delay: 0,
        }
    }
    fn delay(ms: u64) -> Self {
        Self {
            method: "",
            params: Value::Null,
            effect: Effect::None,
            delay: ms,
        }
    }
}
fn key(key: &str, down: bool) -> Result<Primitive, Error> {
    Ok(Primitive::call(
        if down { "keyboardDown" } else { "keyboardUp" },
        json!({"key":key}),
        Effect::Key {
            code: keyboard::code(key)?.into(),
            key: key.into(),
            down,
        },
    ))
}
fn button(button: Button, down: bool, count: u8) -> Primitive {
    Primitive::call(
        if down { "mouseDown" } else { "mouseUp" },
        json!({"button":button,"clickCount":count}),
        Effect::Button { button, down },
    )
}
fn move_to(x: f64, y: f64, steps: u16) -> Primitive {
    Primitive::call(
        "mouseMove",
        json!({"x":x,"y":y,"steps":steps}),
        Effect::None,
    )
}
fn press(key_name: &str, delay: u16, output: &mut Vec<Primitive>) -> Result<(), Error> {
    let keys = keyboard::press_keys(key_name)?;
    for token in &keys {
        output.push(key(token, true)?);
    }
    if delay > 0 {
        output.push(Primitive::delay(u64::from(delay)));
    }
    for token in keys.iter().rev() {
        output.push(key(token, false)?);
    }
    Ok(())
}
fn primitives(action: Action) -> Result<Vec<Primitive>, Error> {
    let mut output = Vec::new();
    match action {
        Action::MouseMove { x, y, steps } => output.push(move_to(x, y, steps)),
        Action::MouseDown { x, y, button: b } => {
            output.push(move_to(x, y, 1));
            output.push(button(b, true, 1));
        }
        Action::MouseUp { button: b } => output.push(button(b, false, 1)),
        Action::MouseClick {
            x,
            y,
            button: b,
            click_count,
            delay_ms,
        } => {
            output.push(move_to(x, y, 1));
            for count in 1..=click_count {
                output.push(button(b, true, count));
                if delay_ms > 0 {
                    output.push(Primitive::delay(u64::from(delay_ms)));
                }
                output.push(button(b, false, count));
            }
        }
        Action::MouseWheel {
            x,
            y,
            delta_x,
            delta_y,
        } => {
            output.push(move_to(x, y, 1));
            output.push(Primitive::call(
                "mouseWheel",
                json!({"deltaX":delta_x,"deltaY":delta_y}),
                Effect::None,
            ));
        }
        Action::TouchTap { x, y } => output.push(Primitive::call(
            "touchscreenTap",
            json!({"x":x,"y":y}),
            Effect::Touch,
        )),
        Action::KeyDown { key: k } => output.push(key(&k, true)?),
        Action::KeyUp { key: k } => output.push(key(&k, false)?),
        Action::KeyPress { key, delay_ms } => press(&key, delay_ms, &mut output)?,
        Action::InsertText { text } => output.push(Primitive::call(
            "keyboardInsertText",
            json!({"text":text}),
            Effect::None,
        )),
        Action::Type { text, delay_ms } => {
            for character in text.chars() {
                let character = character.to_string();
                if keyboard::code(&character).is_ok() {
                    press(&character, delay_ms, &mut output)?;
                } else {
                    if delay_ms > 0 {
                        output.push(Primitive::delay(u64::from(delay_ms)));
                    }
                    output.push(Primitive::call(
                        "keyboardInsertText",
                        json!({"text":character}),
                        Effect::None,
                    ));
                }
            }
        }
        Action::Delay { duration_ms } => output.push(Primitive::delay(u64::from(duration_ms))),
    }
    Ok(output)
}
fn release(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::Key { down: false, .. } | Effect::Button { down: false, .. }
    )
}
pub(super) fn not_dispatched(error: &connection::Error) -> bool {
    matches!(
        error,
        connection::Error::Busy
            | connection::Error::Version
            | connection::Error::Protocol(_)
            | connection::Error::Bounds(_)
            | connection::Error::Timeout {
                dispatch: connection::Dispatch::NotDispatched
            }
            | connection::Error::Lost {
                dispatch: connection::Dispatch::NotDispatched
            }
    )
}

impl Host {
    /// Host-time deadlines fence old input before waiting for a dispatched call
    /// to settle. Unknown releases remain quarantined and are never replayed.
    pub(super) fn start_input_expiry(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                let Some(host) = weak.upgrade() else {
                    return;
                };
                let changed = host.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if host.stopping.load(Ordering::SeqCst) {
                    return;
                }
                let next = host
                    .inputs
                    .lock()
                    .expect("Browser input")
                    .held
                    .values()
                    .filter(|held| !held.empty() && !held.expiry_attempted)
                    .filter_map(|held| held.expires_at)
                    .min();
                let Some(next) = next else {
                    changed.await;
                    continue;
                };
                tokio::select! {
                    () = &mut changed => continue,
                    () = tokio::time::sleep_until(next.into()) => {},
                }
                host.input_epoch.fetch_add(1, Ordering::SeqCst);
                let Ok(_lane) = host
                    .control_cleanup_lane(
                        Instant::now() + Duration::from_millis(contract::MAX_TIMEOUT_MS),
                    )
                    .await
                else {
                    return;
                };
                let pages = {
                    let mut inputs = host.inputs.lock().expect("Browser input");
                    inputs
                        .held
                        .iter_mut()
                        .filter_map(|(page, held)| {
                            if !held.empty()
                                && !held.expiry_attempted
                                && held
                                    .expires_at
                                    .is_some_and(|deadline| deadline <= std::time::Instant::now())
                            {
                                held.expiry_attempted = true;
                                Some(*page)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                };
                for page in pages {
                    host.release_page_input(page, Instant::now() + Duration::from_secs(2))
                        .await;
                }
                host.changed.notify_waiters();
            }
        });
        self.control_tasks
            .lock()
            .expect("Browser control tasks")
            .push(task);
    }

    fn input_admissible(&self, id: Uuid, epoch: u64, deadline: Instant) -> Result<(), Error> {
        self.admissible(id, deadline)?;
        if self.input_epoch.load(Ordering::SeqCst) != epoch {
            return Err("INPUT_CANCELLED".into());
        }
        Ok(())
    }
    async fn input_geometry(
        &self,
        page: Reference,
        observation: &Observation,
        point: Option<(f64, f64)>,
        deadline: Instant,
    ) -> Result<(), Error> {
        let scope = self.page_scope(page, None, deadline).await?;
        if scope.reference != observation.frame {
            return Err("DOCUMENT_CHANGED".into());
        }
        {
            let inputs = self.inputs.lock().expect("Browser input");
            let epochs = inputs.epochs.get(&page).copied().unwrap_or_default();
            if epochs != (observation.document_epoch, observation.viewport_epoch) {
                return Err("INPUT_OBSERVATION_STALE".into());
            }
        }
        let current = self
            .evaluate(&scope.wire, super::page::GEOMETRY, Value::Null, deadline)
            .await?;
        for field in [
            "url",
            "viewport",
            "dpr",
            "scroll",
            "visualViewport",
            "viewportEpoch",
            "pointerLocked",
        ] {
            if current[field] != observation.geometry[field] {
                return Err("INPUT_OBSERVATION_STALE".into());
            }
        }
        if current["pointerLocked"] == true
            || current["visualViewport"]["scale"]
                .as_f64()
                .is_some_and(|scale| (scale - 1.0).abs() > 0.000001)
        {
            return Err("INPUT_GEOMETRY_UNSUPPORTED".into());
        }
        if point.is_some_and(|(x, y)| {
            x >= current["viewport"]["width"].as_f64().unwrap_or(0.0)
                || y >= current["viewport"]["height"].as_f64().unwrap_or(0.0)
        }) {
            return Err("INPUT_COORDINATE_BOUNDS".into());
        }
        if self.frame_current(&scope.wire, deadline).await? != observation.frame {
            return Err("DOCUMENT_CHANGED".into());
        }
        Ok(())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Hold reservation, retained physical dispatch and uncertain-release quarantine form one input transaction"
    )]
    async fn native_input(
        &self,
        page: Reference,
        wire: &ObjectRef,
        primitive: &Primitive,
        deadline: Instant,
    ) -> Result<bool, Error> {
        self.wire(page, Kind::Page)?;
        let previous = {
            let mut inputs = self.inputs.lock().expect("Browser input");
            let held = inputs.held.entry(page).or_default();
            if held.inflight != 0 {
                return Err("INPUT_CLEANUP_PENDING".into());
            }
            let previous = held.clone();
            match &primitive.effect {
                Effect::Key { code, key, down } => {
                    if held.blocked_keys.contains(code) {
                        return Err("INPUT_RELEASE_UNKNOWN".into());
                    }
                    if !down && !held.keys.contains_key(code) {
                        return Ok(false);
                    }
                    if *down {
                        held.keys.insert(code.clone(), key.clone());
                    }
                }
                Effect::Button { button, down } => {
                    if held.blocked_buttons.contains(button) {
                        return Err("INPUT_RELEASE_UNKNOWN".into());
                    }
                    if !down && !held.buttons.contains(button) {
                        return Ok(false);
                    }
                    if *down {
                        held.buttons.insert(*button);
                    }
                }
                Effect::Touch => held.touch_uncertain = true,
                Effect::None => {}
            }
            held.inflight = 1;
            if !held.empty() && held.expires_at.is_none() {
                held.expires_at = Some(std::time::Instant::now() + super::super::input::HOLD_TTL);
                held.expiry_attempted = false;
            }
            self.changed.notify_waiters();
            previous
        };
        // Retained waits never time out a dispatched down and race it with an up.
        let result = self
            .connection()?
            .retained(
                wire,
                primitive.method,
                primitive.params.clone(),
                deadline,
                release(&primitive.effect),
            )
            .await;
        let mut inputs = self.inputs.lock().expect("Browser input");
        let Some(held) = inputs.held.get_mut(&page) else {
            return result.map(|_| true).map_err(Error::from);
        };
        held.inflight = 0;
        match result {
            Ok(_) => {
                match &primitive.effect {
                    Effect::Key {
                        code, down: false, ..
                    } => {
                        held.keys.remove(code);
                    }
                    Effect::Button {
                        button,
                        down: false,
                    } => {
                        held.buttons.remove(button);
                    }
                    Effect::Touch => held.touch_uncertain = false,
                    _ => {}
                }
                if held.empty() {
                    held.expires_at = None;
                    held.expiry_attempted = false;
                }
                self.changed.notify_waiters();
                Ok(true)
            }
            Err(error) => {
                if not_dispatched(&error) {
                    *held = previous;
                } else {
                    held.uncertain = true;
                    match &primitive.effect {
                        Effect::Key {
                            code, down: false, ..
                        } => {
                            held.blocked_keys.insert(code.clone());
                        }
                        Effect::Button {
                            button,
                            down: false,
                        } => {
                            held.blocked_buttons.insert(*button);
                        }
                        _ => {}
                    }
                }
                Err(error.into())
            }
        }
    }
    pub(super) async fn input_release(&self, deadline: Instant) -> Result<Value, Error> {
        let pages = self
            .inputs
            .lock()
            .expect("Browser input")
            .held
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let mut results = Vec::new();
        for page in pages {
            results
                .push(json!({"page":page,"cleanup":self.release_page_input(page,deadline).await}));
        }
        Ok(json!({"pages":results,"input":self.inputs.lock().expect("Browser input").snapshot()}))
    }
    async fn release_page_input(&self, page: Reference, deadline: Instant) -> Value {
        let held = self
            .inputs
            .lock()
            .expect("Browser input")
            .held
            .get(&page)
            .cloned()
            .unwrap_or_default();
        if held.inflight != 0 {
            return json!({"complete":false,"code":"INPUT_CLEANUP_PENDING"});
        }
        let wire = match self.wire(page, Kind::Page) {
            Ok(wire) => wire,
            Err(error) => return json!({"complete":false,"code":error.code}),
        };
        let mut failures = Vec::new();
        // Release ordinary keys before modifiers so cleanup cannot type a shortcut.
        let mut keys = held.keys.iter().collect::<Vec<_>>();
        keys.sort_by_key(|(code, _)| {
            code.starts_with("Shift")
                || code.starts_with("Control")
                || code.starts_with("Alt")
                || code.starts_with("Meta")
        });
        for (code, k) in keys {
            if held.blocked_keys.contains(code) {
                continue;
            }
            match key(k, false) {
                Ok(primitive) => {
                    if let Err(error) = self.native_input(page, &wire, &primitive, deadline).await {
                        failures.push(error.code);
                    }
                }
                Err(error) => failures.push(error.code),
            }
        }
        for b in held.buttons {
            if held.blocked_buttons.contains(&b) {
                continue;
            }
            if let Err(error) = self
                .native_input(page, &wire, &button(b, false, 1), deadline)
                .await
            {
                failures.push(error.code);
            }
        }
        let mut inputs = self.inputs.lock().expect("Browser input");
        let Some(current) = inputs.held.get_mut(&page) else {
            return json!({"complete":true,"scopeEnded":true});
        };
        let complete = current.empty() && current.inflight == 0;
        if complete {
            current.uncertain = false;
        }
        current.cleanup_failed = !complete;
        json!({"complete":complete,"failures":failures,"state":current.snapshot(),"requiresOwnedPageClose":!current.blocked_keys.is_empty()||!current.blocked_buttons.is_empty()||current.touch_uncertain})
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The ordered native phase loop retains partial effects and balances only owned holds on every failure path"
    )]
    pub(super) async fn page_input(
        &self,
        id: Uuid,
        page: Reference,
        observation: Option<Reference>,
        actions: Vec<Action>,
        epoch: u64,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.input_admissible(id, epoch, deadline)?;
        let observed = if let Some(reference) = observation {
            let observed = self
                .inputs
                .lock()
                .expect("Browser input")
                .observations
                .get(&reference)
                .cloned()
                .ok_or("INPUT_OBSERVATION_STALE")?;
            if observed.page != page {
                return Err("FOREIGN_INPUT_OBSERVATION".into());
            }
            if observed.started.elapsed() >= OBSERVATION_TTL || observed.input_epoch != epoch {
                return Err("INPUT_OBSERVATION_STALE".into());
            }
            Some(observed)
        } else {
            None
        };
        let wire = self.wire(page, Kind::Page)?;
        let mut completed_actions = 0;
        let mut completed_primitives = 0;
        let result = async {
            for action in actions {
                let point = action.point();
                let release_only = action.release_only();
                if !release_only {
                    self.input_admissible(id, epoch, deadline)?;
                    self.input_geometry(
                        page,
                        observed.as_ref().ok_or("INPUT_OBSERVATION_REQUIRED")?,
                        action.point(),
                        deadline,
                    )
                    .await?;
                    let held = self
                        .inputs
                        .lock()
                        .expect("Browser input")
                        .held
                        .get(&page)
                        .cloned()
                        .unwrap_or_default();
                    if held.uncertain || held.inflight != 0 {
                        return Err("INPUT_STATE_UNCERTAIN".into());
                    }
                    if matches!(action, Action::MouseClick { .. }) && !held.buttons.is_empty() {
                        return Err("INPUT_STATE_CONFLICT".into());
                    }
                    if matches!(action, Action::TouchTap { .. }) {
                        let state = self.state.lock().expect("Browser owner");
                        let context = state
                            .ancestor(page, Kind::Context)
                            .ok_or("CONTEXT_CLOSED")?;
                        if !state
                            .context_options
                            .get(&context)
                            .is_some_and(|options| options.has_touch)
                        {
                            return Err("TOUCH_CONTEXT_REQUIRED".into());
                        }
                    }
                }
                for primitive in primitives(action)? {
                    self.input_admissible(id, epoch, deadline)?;
                    if !release(&primitive.effect) && !release_only {
                        let observed = observed.as_ref().ok_or("INPUT_OBSERVATION_REQUIRED")?;
                        if point.is_some() {
                            self.input_geometry(page, observed, point, deadline).await?;
                        }
                        self.connection()?.event_barrier(deadline).await?;
                        let inputs = self.inputs.lock().expect("Browser input");
                        if inputs.epochs.get(&page).copied().unwrap_or_default()
                            != (observed.document_epoch, observed.viewport_epoch)
                        {
                            return Err("INPUT_OBSERVATION_STALE".into());
                        }
                    }
                    self.input_admissible(id, epoch, deadline)?;
                    if primitive.method.is_empty() {
                        let end = Instant::now() + Duration::from_millis(primitive.delay);
                        while Instant::now() < end {
                            self.controller_current(id, true)?;
                            tokio::time::sleep_until(
                                end.min(Instant::now() + Duration::from_millis(20)),
                            )
                            .await;
                            self.input_admissible(id, epoch, deadline)?;
                        }
                    } else {
                        if self.native_input(page, &wire, &primitive, deadline).await? {
                            completed_primitives += 1;
                        }
                    }
                }
                completed_actions += 1;
            }
            Ok::<(), Error>(())
        }
        .await;
        if let Err(mut error) = result {
            let cleanup = self
                .release_page_input(page, Instant::now() + Duration::from_secs(2))
                .await;
            let cause = error.code;
            if completed_primitives > 0 && error.code != "OUTCOME_UNKNOWN" {
                error.code = "INPUT_PARTIAL";
            }
            error.partial = Some(
                json!({"cause":cause,"completedActions":completed_actions,"completedNativePrimitives":completed_primitives,"effectsObserved":false,"cleanup":cleanup}),
            );
            return Err(error);
        }
        Ok(
            json!({"page":page,"source":"playwright-native-input","coordinateSpace":"css-viewport","completedActions":completed_actions,"completedNativePrimitives":completed_primitives,"effectsObserved":false,"held":self.inputs.lock().expect("Browser input").held.get(&page).map(|held|held.snapshot())}),
        )
    }
}
