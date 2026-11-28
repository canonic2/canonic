//! Reserve possible physical holds across opaque driver compound actions.
use super::*;
use crate::browser::{
    input::{Button, Held},
    keyboard,
    objects::ObjectRef,
    page::Action,
};

fn reservation(action: &Action) -> Result<Held, Error> {
    let mut held = Held::default();
    match action {
        Action::Click {} | Action::DoubleClick {} | Action::Check { .. } | Action::Drag { .. } => {
            held.buttons.insert(Button::Left);
        }
        Action::Press { key } => {
            for token in keyboard::press_keys(key)? {
                held.keys.insert(keyboard::code(&token)?.into(), token);
            }
        }
        Action::Type { text } => {
            for code in keyboard::text_codes(text) {
                held.keys.insert(code.clone(), code);
            }
        }
        // The driver fills nonempty values with native text insertion; clearing
        // an editable element can instead press Delete.
        Action::Clear {} => {
            held.keys.insert("Delete".into(), "Delete".into());
        }
        Action::Fill { value } if value.is_empty() => {
            held.keys.insert("Delete".into(), "Delete".into());
        }
        _ => {}
    }
    Ok(held)
}
fn closed_input(page: Reference, cause: Option<&'static str>) -> Error {
    let mut error = Error::from("OUTCOME_UNKNOWN");
    let mut partial = json!({"page":page,"scopeClosed":true,"effectsObserved":false,"cleanup":{"complete":true,"ownedPageClosed":true},"replayed":false});
    if let Some(cause) = cause {
        partial["cause"] = json!(cause);
        partial["replyReceived"] = json!(true);
    } else {
        partial["replyOwner"] = json!("driver-connection");
    }
    error.partial = Some(partial);
    error
}

impl Host {
    #[expect(
        clippy::too_many_lines,
        reason = "One physical-input transaction keeps reservation, retained dispatch, scope-close witnessing and unknown-release quarantine in one ownership path"
    )]
    pub(super) async fn compound_input(
        &self,
        page: Reference,
        wire: &ObjectRef,
        action: &Action,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<(), Error> {
        let (action, location) = match action {
            Action::AtSource { source, action } => {
                source.validate()?;
                if matches!(**action, Action::AtSource { .. }) {
                    return Err("CALL_SOURCE_NESTING".into());
                }
                (action.as_ref(), Some(source))
            }
            action => (action, None),
        };
        let mut reserved = reservation(action)?;
        let connection = self.connection()?;
        let physical = !reserved.empty();
        if physical {
            reserved.inflight = 1;
            reserved.expires_at = Some(std::time::Instant::now() + super::super::input::HOLD_TTL);
            let mut inputs = self.inputs.lock().expect("Browser input");
            if inputs
                .held
                .get(&page)
                .is_some_and(|held| !held.empty() || held.inflight != 0)
            {
                return Err("INPUT_STATE_CONFLICT".into());
            }
            inputs.held.insert(page, reserved);
            self.changed.notify_waiters();
        }
        let call = async {
            if let Some(location) = location {
                connection
                    .retained_at(wire, method, params, deadline, location.clone())
                    .await
            } else {
                connection
                    .retained(wire, method, params, deadline, false)
                    .await
            }
        };
        tokio::pin!(call);
        let result = loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let closed =
                self.state.lock().expect("Browser owner").retired.get(&page) == Some(&Kind::Page);
            tokio::select! {
                biased;
                result=&mut call=>break result,
                ()=std::future::ready(()),if closed=>{
                    // Page/context close acknowledgement is a lifetime witness.
                    // The connection still consumes the original nonallocating
                    // call's eventual reply; it cannot target a replacement page.
                    self.inputs.lock().expect("Browser input").held.remove(&page);
                    self.changed.notify_waiters();
                    return Err(closed_input(page,None));
                },
                ()=&mut changed=>{}
            }
        };
        if !physical {
            return result.map(|_| ()).map_err(Error::from);
        }
        let closed = {
            let state = self.state.lock().expect("Browser owner");
            state.retired.get(&page) == Some(&Kind::Page) || state.phase == Phase::Closed
        };
        let mut inputs = self.inputs.lock().expect("Browser input");
        if closed {
            inputs.held.remove(&page);
            return match result {
                Ok(_) => Ok(()),
                Err(error) if super::input::not_dispatched(&error) => Err(error.into()),
                Err(error) => Err(closed_input(page, Some(Error::from(error).code))),
            };
        }
        let Some(held) = inputs.held.get_mut(&page) else {
            // Only owned page/context teardown can retire this reservation.
            return result.map(|_| ()).map_err(Error::from);
        };
        held.inflight = 0;
        match result {
            Ok(_) => {
                *held = Held::default();
                Ok(())
            }
            Err(error) if super::input::not_dispatched(&error) => {
                *held = Held::default();
                Err(error.into())
            }
            Err(error) => {
                // A compound call can already have attempted its own Up. We
                // cannot safely replay that release or infer progress from logs.
                held.uncertain = true;
                held.cleanup_failed = true;
                held.blocked_keys.extend(held.keys.keys().cloned());
                held.blocked_buttons.extend(held.buttons.iter().copied());
                let mut error = Error::from(error);
                let cause = error.code;
                error.code = "OUTCOME_UNKNOWN";
                error.partial = Some(
                    json!({"page":page,"cause":cause,"effectsObserved":false,"cleanup":{"complete":false,"requiresOwnedPageClose":true,"state":held.snapshot()}}),
                );
                Err(error)
            }
        }
    }
}
