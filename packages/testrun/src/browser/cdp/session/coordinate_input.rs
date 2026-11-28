//! Fresh viewport input observations, explicit held phases and owned release.
use super::*;
use crate::browser::input::{self, Action, Button};

impl Worker {
    pub(super) async fn page_input(
        &mut self,
        id: Uuid,
        page: Reference,
        observation: Option<Reference>,
        actions: Vec<Action>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if actions.iter().any(|action| !action.release_only()) {
            let reference = observation.ok_or("INPUT_OBSERVATION_REQUIRED")?;
            self.transfer_scope(reference)?;
            let witnessed = self
                .inputs
                .observations
                .get(&reference)
                .cloned()
                .or_else(|| {
                    self.media
                        .observations
                        .lock()
                        .expect("Media observations")
                        .observations
                        .get(&reference)
                        .cloned()
                })
                .ok_or("STALE_INPUT_OBSERVATION")?;
            if witnessed.page != page {
                return Err("FOREIGN_REFERENCE".into());
            }
            if witnessed.started.elapsed() >= input::OBSERVATION_TTL {
                return Err("STALE_INPUT_OBSERVATION".into());
            }
            let scope = self.page_scope(id, page, None, deadline).await?;
            if scope.frame != witnessed.frame {
                return Err("STALE_INPUT_OBSERVATION".into());
            }
            let geometry = self
                .page_query(
                    id,
                    &scope,
                    json!({"selector":"css=body","read":"geometry","maxBytes":8192}),
                    deadline,
                )
                .await?;
            if geometry != witnessed.geometry {
                return Err("STALE_INPUT_OBSERVATION".into());
            }
            if geometry["pointerLocked"] == true
                || geometry["visualViewport"]["scale"]
                    .as_f64()
                    .is_some_and(|scale| (scale - 1.).abs() > 0.001)
            {
                return Err("INPUT_GEOMETRY_UNSUPPORTED".into());
            }
            for action in &actions {
                if action.point().is_some_and(|(x, y)| {
                    x >= geometry["viewport"]["width"].as_f64().unwrap_or(0.)
                        || y >= geometry["viewport"]["height"].as_f64().unwrap_or(0.)
                }) {
                    return Err("INPUT_COORDINATE_BOUNDS".into());
                }
            }
        }
        self.index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?;
        if actions
            .iter()
            .any(|action| matches!(action, Action::TouchTap { .. }))
        {
            let context = self
                .index
                .list(Kind::Page)
                .into_iter()
                .find(|(reference, _)| *reference == page)
                .and_then(|(_, parent)| parent)
                .ok_or("INVALID_SCOPE")?;
            if !self.touch_contexts.contains(&context) {
                return Err("TOUCH_CONTEXT_REQUIRED".into());
            }
        }
        let mut completed = 0;
        for action in actions {
            if let Err(mut error) = self.coordinate_action(id, page, action, deadline).await {
                let released = self.release_page_input(page).await.is_ok();
                error.partial = Some(
                    json!({"completedActions":completed,"replayed":false,"heldInputReleased":released}),
                );
                return Err(error);
            }
            completed += 1;
        }
        self.inputs.held.retain(|_, held| !held.empty());
        Ok(
            json!({"page":page,"completedActions":completed,"source":"chrome-cdp-native-input","dispatchCompleted":true,"input":self.inputs.snapshot()}),
        )
    }
    pub(super) async fn input_key(
        &mut self,
        id: Uuid,
        page: Reference,
        key: &str,
        down: bool,
        deadline: Instant,
    ) -> Result<(), Error> {
        let code = crate::browser::keyboard::code(key)?.to_owned();
        let mac = self
            .connection
            .as_ref()
            .is_some_and(Connection::uses_mac_keyboard);
        let held = self.inputs.held.entry(page).or_default();
        let mut modifiers = modifier_bits(held.keys.keys().map(String::as_str));
        let mut params = super::native_input::key_event(key, down, &mut modifiers, mac)?;
        params["autoRepeat"] = json!(down && held.keys.contains_key(&code));
        if down {
            held.keys.insert(code.clone(), key.into());
        }
        held.expires_at = Some(std::time::Instant::now() + input::HOLD_TTL);
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        self.call(
            id,
            Some(&session),
            "Input.dispatchKeyEvent",
            params,
            deadline,
        )
        .await?;
        if !down {
            self.inputs.held.entry(page).or_default().keys.remove(&code);
        }
        Ok(())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The native input interpreter keeps each typed key, pointer and touch phase with its owned hold updates"
    )]
    async fn coordinate_action(
        &mut self,
        id: Uuid,
        page: Reference,
        action: Action,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.shared.control.input_current(id)?;
        use Action as A;
        self.observation_ready(id, deadline)?;
        match action {
            A::KeyDown { key } => self.input_key(id, page, &key, true, deadline).await,
            A::KeyUp { key } => self.input_key(id, page, &key, false, deadline).await,
            A::KeyPress { key, delay_ms } => {
                let keys = crate::browser::keyboard::press_keys(&key)?;
                let mut added = Vec::new();
                for key in keys {
                    let code = crate::browser::keyboard::code(&key)?;
                    if !self
                        .inputs
                        .held
                        .get(&page)
                        .is_some_and(|held| held.keys.contains_key(code))
                    {
                        self.input_key(id, page, &key, true, deadline).await?;
                        added.push(key);
                    }
                }
                if delay_ms > 0 {
                    self.input_delay(id, u64::from(delay_ms), deadline).await?;
                }
                for key in added.into_iter().rev() {
                    self.input_key(id, page, &key, false, deadline).await?;
                }
                Ok(())
            }
            A::Type { text, delay_ms } => {
                for character in text.chars() {
                    self.shared.control.input_current(id)?;
                    let key = character.to_string();
                    if crate::browser::keyboard::code(&key).is_ok() {
                        self.input_key(id, page, &key, true, deadline).await?;
                        self.input_key(id, page, &key, false, deadline).await?;
                    } else {
                        self.insert_text(id, page, &key, deadline).await?;
                    }
                    if delay_ms > 0 {
                        self.input_delay(id, u64::from(delay_ms), deadline).await?;
                    }
                }
                Ok(())
            }
            A::InsertText { text } => self.insert_text(id, page, &text, deadline).await,
            A::Delay { duration_ms } => {
                self.input_delay(id, u64::from(duration_ms), deadline).await
            }
            A::MouseMove { x, y, steps } => {
                let from = self
                    .pointer_positions
                    .get(&page)
                    .copied()
                    .unwrap_or((0., 0.));
                for step in 1..=steps {
                    let t = f64::from(step) / f64::from(steps);
                    self.mouse_event(
                        id,
                        page,
                        "mouseMoved",
                        (from.0 + (x - from.0) * t, from.1 + (y - from.1) * t),
                        None,
                        0,
                        None,
                        deadline,
                    )
                    .await?;
                }
                Ok(())
            }
            A::MouseDown { x, y, button } => {
                self.mouse_event(
                    id,
                    page,
                    "mousePressed",
                    (x, y),
                    Some(button),
                    1,
                    None,
                    deadline,
                )
                .await
            }
            A::MouseUp { button } => {
                let point = self
                    .pointer_positions
                    .get(&page)
                    .copied()
                    .unwrap_or_default();
                self.mouse_event(
                    id,
                    page,
                    "mouseReleased",
                    point,
                    Some(button),
                    1,
                    None,
                    deadline,
                )
                .await
            }
            A::MouseClick {
                x,
                y,
                button,
                click_count,
                delay_ms,
            } => {
                for count in 1..=click_count {
                    self.mouse_event(id, page, "mouseMoved", (x, y), None, 0, None, deadline)
                        .await?;
                    self.mouse_event(
                        id,
                        page,
                        "mousePressed",
                        (x, y),
                        Some(button),
                        count,
                        None,
                        deadline,
                    )
                    .await?;
                    if delay_ms > 0 {
                        self.input_delay(id, u64::from(delay_ms), deadline).await?;
                    }
                    self.mouse_event(
                        id,
                        page,
                        "mouseReleased",
                        (x, y),
                        Some(button),
                        count,
                        None,
                        deadline,
                    )
                    .await?;
                }
                Ok(())
            }
            A::MouseWheel {
                x,
                y,
                delta_x,
                delta_y,
            } => {
                self.mouse_event(
                    id,
                    page,
                    "mouseWheel",
                    (x, y),
                    None,
                    0,
                    Some((delta_x, delta_y)),
                    deadline,
                )
                .await
            }
            A::TouchTap { x, y } => {
                let session = self
                    .index
                    .route(page, Some(Kind::Page))
                    .map_err(resource_error)?
                    .session_id;
                self.inputs.held.entry(page).or_default().touch_uncertain = true;
                let result = self
                    .call(
                        id,
                        Some(&session),
                        "Input.dispatchTouchEvent",
                        json!({"type":"touchStart","touchPoints":[{"x":x,"y":y,"id":0}]}),
                        deadline,
                    )
                    .await;
                let release = self
                    .connection
                    .as_ref()
                    .ok_or("BACKEND_LOST")?
                    .call(
                        Some(&session),
                        "Input.dispatchTouchEvent",
                        json!({"type":"touchEnd","touchPoints":[]}),
                        Lane::Control,
                        Instant::now() + Duration::from_secs(2),
                    )
                    .await
                    .map_err(protocol_error);
                if release.is_ok() {
                    self.inputs.held.entry(page).or_default().touch_uncertain = false;
                }
                result?;
                release?;
                Ok(())
            }
        }
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "One native mouse event includes its button and wheel phases"
    )]
    async fn mouse_event(
        &mut self,
        id: Uuid,
        page: Reference,
        kind: &str,
        point: (f64, f64),
        button: Option<Button>,
        count: u8,
        wheel: Option<(f64, f64)>,
        deadline: Instant,
    ) -> Result<(), Error> {
        let held = self.inputs.held.entry(page).or_default();
        if kind == "mousePressed" {
            held.buttons.insert(button.ok_or("INVALID_INPUT")?);
        }
        let mut buttons = held.buttons.clone();
        if kind == "mouseReleased" {
            buttons.remove(&button.ok_or("INVALID_INPUT")?);
        }
        held.expires_at = Some(std::time::Instant::now() + input::HOLD_TTL);
        let mut params = json!({"type":kind,"x":point.0,"y":point.1,"button":button.map_or("none",button_name),"buttons":buttons.iter().map(|button|match button{Button::Left=>1,Button::Right=>2,Button::Middle=>4}).sum::<u8>(),"modifiers":modifier_bits(held.keys.keys().map(String::as_str)),"clickCount":count});
        if let Some((x, y)) = wheel {
            params["deltaX"] = json!(x);
            params["deltaY"] = json!(y);
        }
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        self.pointer_positions.insert(page, point);
        self.call(
            id,
            Some(&session),
            "Input.dispatchMouseEvent",
            params,
            deadline,
        )
        .await?;
        if kind == "mouseReleased" {
            self.inputs
                .held
                .entry(page)
                .or_default()
                .buttons
                .remove(&button.ok_or("INVALID_INPUT")?);
        }
        Ok(())
    }
    async fn input_delay(&mut self, id: Uuid, millis: u64, deadline: Instant) -> Result<(), Error> {
        let until = (Instant::now() + Duration::from_millis(millis)).min(deadline);
        while Instant::now() < until {
            self.controller_current(id, true)?;
            self.observation_ready(id, deadline)?;
            self.observation_tick(until).await?;
        }
        Ok(())
    }
    pub(super) async fn release_page_input(&mut self, page: Reference) -> Result<(), Error> {
        let Some(held) = self.inputs.held.get(&page).cloned() else {
            return Ok(());
        };
        if held.empty() {
            self.inputs.held.remove(&page);
            return Ok(());
        }
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut modifiers = modifier_bits(held.keys.keys().map(String::as_str));
        for key in held.keys.values().rev() {
            connection
                .call(
                    Some(&session),
                    "Input.dispatchKeyEvent",
                    super::native_input::key_event(
                        key,
                        false,
                        &mut modifiers,
                        connection.uses_mac_keyboard(),
                    )?,
                    Lane::Control,
                    deadline,
                )
                .await
                .map_err(protocol_error)?;
        }
        let point = self
            .pointer_positions
            .get(&page)
            .copied()
            .unwrap_or_default();
        for button in held.buttons {
            connection.call(Some(&session),"Input.dispatchMouseEvent",json!({"type":"mouseReleased","x":point.0,"y":point.1,"button":button_name(button),"buttons":0,"clickCount":1}),Lane::Control,deadline).await.map_err(protocol_error)?;
        }
        if held.touch_uncertain {
            connection
                .call(
                    Some(&session),
                    "Input.dispatchTouchEvent",
                    json!({"type":"touchCancel","touchPoints":[]}),
                    Lane::Control,
                    deadline,
                )
                .await
                .map_err(protocol_error)?;
        }
        self.inputs.held.remove(&page);
        Ok(())
    }
    pub(super) async fn release_inputs(&mut self) -> Result<Value, Error> {
        self.inputs.observations.clear();
        self.media
            .observations
            .lock()
            .expect("Media observations")
            .observations
            .clear();
        let pages = self.inputs.held.keys().copied().collect::<Vec<_>>();
        for page in pages {
            self.release_page_input(page).await?;
        }
        Ok(json!({"released":true,"input":self.inputs.snapshot()}))
    }
    pub(super) async fn expire_inputs(&mut self) {
        let pages = self
            .inputs
            .held
            .iter()
            .filter(|(_, held)| {
                held.expires_at
                    .is_some_and(|expiry| expiry <= std::time::Instant::now())
            })
            .map(|(page, _)| *page)
            .collect::<Vec<_>>();
        for page in pages {
            if self.release_page_input(page).await.is_err() {
                self.lose("INPUT_CLEANUP_PENDING");
            }
        }
        self.publish();
    }
}
fn button_name(button: Button) -> &'static str {
    match button {
        Button::Left => "left",
        Button::Right => "right",
        Button::Middle => "middle",
    }
}
fn modifier_bits<'a>(keys: impl Iterator<Item = &'a str>) -> u8 {
    keys.fold(0, |bits, key| {
        bits | match key {
            "AltLeft" | "AltRight" => 1,
            "ControlLeft" | "ControlRight" => 2,
            "MetaLeft" | "MetaRight" => 4,
            "ShiftLeft" | "ShiftRight" => 8,
            _ => 0,
        }
    })
}
