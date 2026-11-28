//! Native trusted input dispatch and balanced release for compound actions.
use super::*;
use crate::browser::page::{Action, Locator};

impl Worker {
    pub(super) async fn insert_text(
        &mut self,
        id: Uuid,
        page: Reference,
        text: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        self.call(
            id,
            Some(&session),
            "Input.insertText",
            json!({"text":text}),
            deadline,
        )
        .await?;
        Ok(())
    }
    pub(super) async fn type_text(
        &mut self,
        id: Uuid,
        page: Reference,
        text: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        for character in text.chars() {
            self.shared.control.input_current(id)?;
            let key = character.to_string();
            if crate::browser::keyboard::code(&key).is_ok() {
                self.press_key(id, page, &key, deadline).await?;
            } else {
                self.insert_text(id, page, &key, deadline).await?;
            }
        }
        Ok(())
    }
    pub(super) async fn press_key(
        &mut self,
        id: Uuid,
        page: Reference,
        key: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let keys = crate::browser::keyboard::press_keys(key)?;
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        let mut pressed = Vec::new();
        let mac = self
            .connection
            .as_ref()
            .is_some_and(Connection::uses_mac_keyboard);
        let mut modifiers = 0;
        let mut result = Ok(());
        for key in keys {
            let params = key_event(&key, true, &mut modifiers, mac)?;
            pressed.push(key);
            if let Err(error) = self
                .call(
                    id,
                    Some(&session),
                    "Input.dispatchKeyEvent",
                    params,
                    deadline,
                )
                .await
            {
                result = Err(error);
                break;
            }
        }
        for key in pressed.into_iter().rev() {
            let params = key_event(&key, false, &mut modifiers, mac)?;
            if self.connection.is_some() {
                let released = self
                    .cleanup_call(
                        Some(&session),
                        "Input.dispatchKeyEvent",
                        params,
                        deadline.max(Instant::now() + Duration::from_secs(2)),
                    )
                    .await;
                if result.is_ok() {
                    result = released.map(|_| ());
                }
            }
        }
        result
    }
    pub(super) async fn pointer_action(
        &mut self,
        id: Uuid,
        page: Reference,
        point: (f64, f64),
        action: &Action,
        deadline: Instant,
    ) -> Result<(), Error> {
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        self.call(
            id,
            Some(&session),
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":point.0,"y":point.1,"button":"none","buttons":0}),
            deadline,
        )
        .await?;
        if matches!(action, Action::Hover {}) {
            return Ok(());
        }
        for count in 1..=if matches!(action, Action::DoubleClick {}) {
            2
        } else {
            1
        } {
            let result=self.call(id,Some(&session),"Input.dispatchMouseEvent",json!({"type":"mousePressed","x":point.0,"y":point.1,"button":"left","buttons":1,"clickCount":count}),deadline).await;
            let release=self.cleanup_call(Some(&session),"Input.dispatchMouseEvent",json!({"type":"mouseReleased","x":point.0,"y":point.1,"button":"left","buttons":0,"clickCount":count}),deadline.max(Instant::now()+Duration::from_secs(2))).await;
            result?;
            release?;
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "One guarded point calculation preserves document and ancestor-frame witnesses through projection"
    )]
    pub(super) async fn action_point(
        &mut self,
        id: Uuid,
        page: Reference,
        scope: &page::Scope,
        object: &str,
        deadline: Instant,
    ) -> Result<(f64, f64), Error> {
        let value = self
            .call_read(
                id,
                Some(&scope.session),
                "DOM.getContentQuads",
                json!({"objectId":object}),
                deadline,
            )
            .await?;
        let metrics = self
            .call_read(
                id,
                Some(&scope.session),
                "Page.getLayoutMetrics",
                json!({}),
                deadline,
            )
            .await?;
        let width = metrics["cssLayoutViewport"]["clientWidth"]
            .as_f64()
            .ok_or("BACKEND_PROTOCOL")?;
        let height = metrics["cssLayoutViewport"]["clientHeight"]
            .as_f64()
            .ok_or("BACKEND_PROTOCOL")?;
        let mut point = value["quads"]
            .as_array()
            .ok_or("BACKEND_PROTOCOL")?
            .iter()
            .find_map(|quad| clipped_point(quad, width, height).ok())
            .ok_or("ELEMENT_NOT_VISIBLE")?;
        let mut session = scope.session.clone();
        let main = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        for _ in 0..32 {
            if session == main {
                return Ok(point);
            }
            let (root, _, _, _) = self
                .index
                .targets()
                .into_iter()
                .find(|(_, _, _, route)| route.as_deref() == Some(&session))
                .ok_or("INVALID_SCOPE")?;
            let parent = self
                .index
                .list(Kind::Frame)
                .into_iter()
                .find(|(frame, _)| *frame == root)
                .and_then(|(_, parent)| parent)
                .ok_or("INVALID_SCOPE")?;
            let parent_session = self
                .index
                .route(parent, None)
                .map_err(resource_error)?
                .session_id;
            let native = self
                .index
                .native(root, Kind::Frame)
                .map_err(resource_error)?;
            let owner = self
                .call_read(
                    id,
                    Some(&parent_session),
                    "DOM.getFrameOwner",
                    json!({"frameId":native}),
                    deadline,
                )
                .await?;
            let model = self
                .call_read(
                    id,
                    Some(&parent_session),
                    "DOM.getBoxModel",
                    json!({"backendNodeId":owner["backendNodeId"]}),
                    deadline,
                )
                .await?;
            let metrics = self
                .call_read(
                    id,
                    Some(&session),
                    "Page.getLayoutMetrics",
                    json!({}),
                    deadline,
                )
                .await?;
            point = project_point(
                &model["model"]["content"],
                point,
                metrics["cssLayoutViewport"]["clientWidth"]
                    .as_f64()
                    .ok_or("BACKEND_PROTOCOL")?,
                metrics["cssLayoutViewport"]["clientHeight"]
                    .as_f64()
                    .ok_or("BACKEND_PROTOCOL")?,
            )?;
            session = parent_session;
        }
        Err("FRAME_DEPTH_BOUNDS".into())
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Drag owns source and target in the same exact document"
    )]
    pub(super) async fn drag_action(
        &mut self,
        id: Uuid,
        page: Reference,
        scope: &page::Scope,
        target: &Locator,
        from: (f64, f64),
        group: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let expression = page::query_expression(
            json!({"selector":target.scoped_selector(scope.frame)?,"read":"frame"}),
        );
        let value=self.call_read(id,Some(&scope.session),"Runtime.evaluate",json!({"expression":expression,"uniqueContextId":scope.unique,"objectGroup":group,"returnByValue":false,"awaitPromise":true}),deadline).await?;
        if value.get("exceptionDetails").is_some() {
            return Err(page::execution_error(&value));
        }
        let object = value["result"]["objectId"]
            .as_str()
            .ok_or("ELEMENT_NOT_FOUND")?;
        let to = self.action_point(id, page, scope, object, deadline).await?;
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        self.interactions.drags.remove(&session);
        self.call(
            id,
            Some(&session),
            "Input.setInterceptDrags",
            json!({"enabled":true}),
            deadline,
        )
        .await?;
        let result=async{
            self.call(id,Some(&session),"Input.dispatchMouseEvent",json!({"type":"mouseMoved","x":from.0,"y":from.1}),deadline).await?;
            self.call(id,Some(&session),"Input.dispatchMouseEvent",json!({"type":"mousePressed","x":from.0,"y":from.1,"button":"left","buttons":1,"clickCount":1}),deadline).await?;
            for step in 1..=10 {let t=f64::from(step)/10.;self.call(id,Some(&session),"Input.dispatchMouseEvent",json!({"type":"mouseMoved","x":from.0+(to.0-from.0)*t,"y":from.1+(to.1-from.1)*t,"button":"left","buttons":1}),deadline).await?;}
            self.drain()?;
            let drag_until=deadline.min(Instant::now()+Duration::from_millis(100));
            while !self.interactions.drags.contains_key(&session) && Instant::now()<drag_until {
                self.observation_ready(id,deadline)?;
                self.observation_tick(drag_until).await?;
            }
            if let Some(data)=self.interactions.drags.remove(&session) {
                for kind in ["dragEnter","dragOver","drop"] {
                    self.call(id,Some(&session),"Input.dispatchDragEvent",json!({"type":kind,"x":to.0,"y":to.1,"data":data}),deadline).await?;
                }
            }
            Ok::<(),Error>(())
        }.await;
        let release=self.cleanup_call(Some(&session),"Input.dispatchMouseEvent",json!({"type":"mouseReleased","x":to.0,"y":to.1,"button":"left","buttons":0,"clickCount":1}),deadline.max(Instant::now()+Duration::from_secs(2))).await;
        let disabled = self
            .cleanup_call(
                Some(&session),
                "Input.setInterceptDrags",
                json!({"enabled":false}),
                Instant::now() + Duration::from_secs(2),
            )
            .await;
        self.interactions.drags.remove(&session);
        result?;
        release?;
        disabled?;
        Ok(())
    }
}

pub(super) fn key_event(
    key: &str,
    down: bool,
    modifiers: &mut u8,
    mac: bool,
) -> Result<Value, Error> {
    static LAYOUT: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let layout = LAYOUT.get_or_init(|| {
        serde_json::from_str(include_str!("../../keyboard-layout.json")).expect("Pinned key layout")
    });
    let code = crate::browser::keyboard::code(key)?;
    let definition = &layout["layout"][code];
    let bit = match code {
        "AltLeft" | "AltRight" => 1,
        "ControlLeft" | "ControlRight" => 2,
        "MetaLeft" | "MetaRight" => 4,
        "ShiftLeft" | "ShiftRight" => 8,
        _ => 0,
    };
    if down {
        *modifiers |= bit;
    } else {
        *modifiers &= !bit;
    }
    let shifted =
        (*modifiers & 8 != 0 && key == code) || definition["shiftKey"].as_str() == Some(key);
    let name = if shifted {
        definition["shiftKey"]
            .as_str()
            .or(definition["key"].as_str())
    } else {
        definition["key"].as_str()
    }
    .ok_or("INVALID_BROWSER_KEY")?;
    let text = if *modifiers & 7 == 0 {
        if name.chars().count() == 1 {
            name
        } else if name == "Enter" {
            "\r"
        } else {
            ""
        }
    } else {
        ""
    };
    let key_code = if shifted {
        definition.get("shiftKeyCode")
    } else {
        None
    }
    .or_else(|| definition.get("keyCodeWithoutLocation"))
    .unwrap_or(&definition["keyCode"]);
    let location = definition["location"].as_u64().unwrap_or(0);
    let mut value = json!({"type":if down{if text.is_empty(){"rawKeyDown"}else{"keyDown"}}else{"keyUp"},"key":name,"code":code,"windowsVirtualKeyCode":key_code,"location":location,"isKeypad":location==3,"modifiers":modifiers});
    if down {
        value["text"] = json!(text);
        value["unmodifiedText"] = json!(text);
        value["commands"] = if mac {
            json!(mac_commands(code, *modifiers))
        } else {
            json!([])
        };
    }
    Ok(value)
}

fn mac_commands(code: &str, modifiers: u8) -> Vec<String> {
    static COMMANDS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let commands = COMMANDS.get_or_init(|| {
        serde_json::from_str(include_str!("../mac-editing-commands.json"))
            .expect("Pinned Mac editing commands")
    });
    let mut parts = Vec::new();
    for (name, bit) in [("Shift", 8), ("Control", 2), ("Alt", 1), ("Meta", 4)] {
        if modifiers & bit != 0 {
            parts.push(name);
        }
    }
    parts.push(code);
    let mapped = &commands[parts.join("+")];
    let values = if let Some(command) = mapped.as_str() {
        vec![command]
    } else {
        mapped.as_array().map_or_else(Vec::new, |values| {
            values.iter().filter_map(Value::as_str).collect()
        })
    };
    values
        .into_iter()
        .filter(|command| !command.starts_with("insert"))
        .map(|command| command.trim_end_matches(':').to_owned())
        .collect()
}

fn clipped_point(quad: &Value, width: f64, height: f64) -> Result<(f64, f64), Error> {
    let values = quad
        .as_array()
        .filter(|quad| quad.len() == 8)
        .ok_or("BACKEND_PROTOCOL")?;
    let mut points = Vec::new();
    for pair in values.as_chunks::<2>().0 {
        points.push((
            pair[0].as_f64().ok_or("BACKEND_PROTOCOL")?,
            pair[1].as_f64().ok_or("BACKEND_PROTOCOL")?,
        ));
    }
    for (axis, bound, greater) in [
        (0, 0., true),
        (0, width, false),
        (1, 0., true),
        (1, height, false),
    ] {
        let mut clipped = Vec::new();
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            let coordinate = |p: (f64, f64)| if axis == 0 { p.0 } else { p.1 };
            let inside = |p| {
                if greater {
                    coordinate(p) >= bound
                } else {
                    coordinate(p) <= bound
                }
            };
            if inside(a) {
                clipped.push(a);
            }
            if inside(a) != inside(b) {
                let t = (bound - coordinate(a)) / (coordinate(b) - coordinate(a));
                clipped.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
            }
        }
        points = clipped;
    }
    if points.len() < 3 {
        return Err("ELEMENT_NOT_VISIBLE".into());
    }
    let area = (0..points.len())
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        .abs()
        / 2.;
    if area <= 0.99 {
        return Err("ELEMENT_NOT_VISIBLE".into());
    }
    Ok((
        points.iter().map(|p| p.0).sum::<f64>() / points.len() as f64,
        points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64,
    ))
}
fn project_point(
    quad: &Value,
    point: (f64, f64),
    width: f64,
    height: f64,
) -> Result<(f64, f64), Error> {
    let q = quad
        .as_array()
        .filter(|q| q.len() == 8)
        .ok_or("BACKEND_PROTOCOL")?;
    let mut n = [0.; 8];
    for (i, value) in q.iter().enumerate() {
        n[i] = value.as_f64().ok_or("BACKEND_PROTOCOL")?;
    }
    if width <= 0. || height <= 0. {
        return Err("ELEMENT_NOT_VISIBLE".into());
    }
    let dx = n[0] - n[2] + n[4] - n[6];
    let dy = n[1] - n[3] + n[5] - n[7];
    let (g, h) = if dx.abs() + dy.abs() < 1e-8 {
        (0., 0.)
    } else {
        let ax = n[2] - n[4];
        let bx = n[6] - n[4];
        let ay = n[3] - n[5];
        let by = n[7] - n[5];
        let det = ax * by - bx * ay;
        if det.abs() < 1e-8 {
            return Err("ELEMENT_NOT_VISIBLE".into());
        }
        ((dx * by - bx * dy) / det, (ax * dy - dx * ay) / det)
    };
    let u = point.0 / width;
    let v = point.1 / height;
    let d = g * u + h * v + 1.;
    let result = (
        ((n[2] - n[0] + g * n[2]) * u + (n[6] - n[0] + h * n[6]) * v + n[0]) / d,
        ((n[3] - n[1] + g * n[3]) * u + (n[7] - n[1] + h * n[7]) * v + n[1]) / d,
    );
    if !result.0.is_finite() || !result.1.is_finite() {
        return Err("ELEMENT_NOT_VISIBLE".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_points_clip_quads_to_the_visible_viewport() {
        assert_eq!(
            clipped_point(&json!([-100, 10, 20, 10, 20, 30, -100, 30]), 100., 100.).unwrap(),
            (10., 20.)
        );
        assert!(clipped_point(&json!([-100, 10, -20, 10, -20, 30, -100, 30]), 100., 100.).is_err());
        assert!(clipped_point(&json!([0, 0, 0, 0, 0, 0, 0, 0]), 100., 100.).is_err());
    }
    #[test]
    fn native_keys_distinguish_logical_characters_and_shifted_physical_codes() {
        let mut modifiers = 0;
        let shift = key_event("Shift", true, &mut modifiers, true).unwrap();
        assert_eq!(shift["windowsVirtualKeyCode"], 16);
        assert_eq!(
            key_event("a", true, &mut modifiers, true).unwrap()["text"],
            "a"
        );
        assert_eq!(
            key_event("KeyA", true, &mut modifiers, true).unwrap()["text"],
            "A"
        );
        assert_eq!(mac_commands("ArrowLeft", 4), vec!["moveToLeftEndOfLine"]);
        assert_eq!(mac_commands("KeyA", 4), vec!["selectAll"]);
    }
    #[test]
    fn native_keyboard_editing_commands_follow_the_browser_platform() {
        for mac in [false, true] {
            let mut modifiers = 0;
            key_event("Control", true, &mut modifiers, mac).unwrap();
            let select = key_event("KeyA", true, &mut modifiers, mac).unwrap();
            assert_eq!(
                select["commands"],
                if mac {
                    json!(["moveToBeginningOfParagraph"])
                } else {
                    json!([])
                }
            );
            key_event("KeyA", false, &mut modifiers, mac).unwrap();
            key_event("Control", false, &mut modifiers, mac).unwrap();
            assert_eq!(modifiers, 0);
            key_event("Meta", true, &mut modifiers, mac).unwrap();
            let select = key_event("KeyA", true, &mut modifiers, mac).unwrap();
            assert_eq!(
                select["commands"],
                if mac { json!(["selectAll"]) } else { json!([]) }
            );
        }
    }
    #[test]
    fn native_keyboard_layout_emits_schema_valid_down_and_up_events() {
        for key in [
            "End",
            "Delete",
            "Enter",
            "a",
            "A",
            "Shift",
            "ControlOrMeta",
            "🙂",
        ] {
            let mut modifiers = 0;
            if key == "🙂" {
                assert!(key_event(key, true, &mut modifiers, true).is_err());
                continue;
            }
            for down in [true, false] {
                let params = key_event(key, down, &mut modifiers, true).unwrap();
                crate::browser::cdp::schema::command("Input.dispatchKeyEvent", &params)
                    .unwrap_or_else(|error| panic!("{key}: {params}: {error}"));
            }
        }
    }
    #[test]
    fn frame_projection_maps_rotated_and_perspective_content_quads() {
        let axis = json!([10, 20, 210, 20, 210, 120, 10, 120]);
        assert_eq!(
            project_point(&axis, (50., 25.), 100., 50.).unwrap(),
            (110., 70.)
        );
        let rotated = json!([100, 0, 100, 100, 0, 100, 0, 0]);
        assert_eq!(
            project_point(&rotated, (25., 75.), 100., 100.).unwrap(),
            (25., 25.)
        );
        let perspective = json!([0, 0, 200, 0, 150, 100, 50, 100]);
        let center = project_point(&perspective, (50., 50.), 100., 100.).unwrap();
        assert!((center.0 - 100.).abs() < 1e-8);
        assert!(center.1 > 50.);
    }
}
