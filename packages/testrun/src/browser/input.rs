//! Typed native input, short-lived viewport observations and owned held state.

use super::{contract::Reference, keyboard};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(crate) const OBSERVATIONS: usize = 256;
pub(crate) const OBSERVATION_TTL: Duration = Duration::from_secs(10);
pub(crate) const HOLD_TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    MouseMove {
        x: f64,
        y: f64,
        #[serde(default = "steps")]
        steps: u16,
    },
    MouseDown {
        x: f64,
        y: f64,
        #[serde(default)]
        button: Button,
    },
    MouseUp {
        #[serde(default)]
        button: Button,
    },
    MouseClick {
        x: f64,
        y: f64,
        #[serde(default)]
        button: Button,
        #[serde(default = "click_count")]
        click_count: u8,
        #[serde(default)]
        delay_ms: u16,
    },
    MouseWheel {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    },
    TouchTap {
        x: f64,
        y: f64,
    },
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    KeyPress {
        key: String,
        #[serde(default)]
        delay_ms: u16,
    },
    Type {
        text: String,
        #[serde(default)]
        delay_ms: u16,
    },
    InsertText {
        text: String,
    },
    Delay {
        duration_ms: u16,
    },
}
fn steps() -> u16 {
    1
}
fn click_count() -> u8 {
    1
}

impl Action {
    pub(crate) fn point(&self) -> Option<(f64, f64)> {
        match self {
            Self::MouseMove { x, y, .. }
            | Self::MouseDown { x, y, .. }
            | Self::MouseClick { x, y, .. }
            | Self::MouseWheel { x, y, .. }
            | Self::TouchTap { x, y } => Some((*x, *y)),
            _ => None,
        }
    }
    pub(crate) fn release_only(&self) -> bool {
        matches!(self, Self::KeyUp { .. } | Self::MouseUp { .. })
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.point().is_some_and(|(x, y)| {
            !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x >= 8192.0 || y >= 8192.0
        }) {
            return Err("INPUT_COORDINATE_BOUNDS");
        }
        match self {
            Self::MouseMove { steps, .. } if !(1..=100).contains(steps) => Err("INPUT_BOUNDS"),
            Self::MouseClick {
                click_count,
                delay_ms,
                ..
            } if !(1..=3).contains(click_count) || *delay_ms > 1000 => Err("INPUT_BOUNDS"),
            Self::MouseWheel {
                delta_x, delta_y, ..
            } if !delta_x.is_finite()
                || !delta_y.is_finite()
                || delta_x.abs() > 100000.0
                || delta_y.abs() > 100000.0 =>
            {
                Err("INPUT_BOUNDS")
            }
            Self::KeyDown { key } | Self::KeyUp { key } => keyboard::code(key).map(|_| ()),
            Self::KeyPress { key, delay_ms } => {
                if *delay_ms > 1000 {
                    return Err("INPUT_BOUNDS");
                }
                keyboard::press_codes(key).map(|_| ())
            }
            Self::Type { text, delay_ms } => {
                if text.len() > 16384 || text.chars().count() > 4096 || *delay_ms > 1000 {
                    return Err("INPUT_BOUNDS");
                }
                Ok(())
            }
            Self::InsertText { text } if text.len() > 16384 => Err("INPUT_BOUNDS"),
            Self::Delay { duration_ms } if *duration_ms > 30000 => Err("INPUT_BOUNDS"),
            _ => Ok(()),
        }
    }
    pub(crate) fn delay_budget(&self) -> u64 {
        match self {
            Self::Type { text, delay_ms } => text.chars().count() as u64 * u64::from(*delay_ms),
            Self::KeyPress { delay_ms, .. } => u64::from(*delay_ms),
            Self::MouseClick {
                click_count,
                delay_ms,
                ..
            } => u64::from(*click_count) * u64::from(*delay_ms),
            Self::Delay { duration_ms } => u64::from(*duration_ms),
            _ => 0,
        }
    }
}

pub(crate) fn validate(
    actions: &[Action],
    observation: Option<Reference>,
) -> Result<(), &'static str> {
    if actions.is_empty() || actions.len() > 64 {
        return Err("INPUT_BOUNDS");
    }
    if observation.is_none() && actions.iter().any(|action| !action.release_only()) {
        return Err("INPUT_OBSERVATION_REQUIRED");
    }
    let mut delay = 0;
    let mut text_bytes = 0;
    let mut keys = 0;
    for action in actions {
        action.validate()?;
        delay += action.delay_budget();
        match action {
            Action::Type { text, .. } => {
                text_bytes += text.len();
                keys += text.chars().count() * 2;
            }
            Action::InsertText { text } => text_bytes += text.len(),
            Action::KeyPress { key, .. } => keys += keyboard::press_codes(key)?.len() * 2,
            _ => keys += 1,
        }
    }
    if delay > 30000 || text_bytes > 16384 || keys > 8192 {
        return Err("INPUT_BOUNDS");
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct Observation {
    pub page: Reference,
    pub frame: Reference,
    pub geometry: Value,
    pub started: Instant,
    pub input_epoch: u64,
    pub document_epoch: u64,
    pub viewport_epoch: u64,
}

#[derive(Clone, Default)]
pub(crate) struct Held {
    pub expires_at: Option<Instant>,
    pub expiry_attempted: bool,
    pub keys: BTreeMap<String, String>,
    pub buttons: BTreeSet<Button>,
    pub touch_uncertain: bool,
    pub uncertain: bool,
    pub cleanup_failed: bool,
    pub inflight: u32,
    pub blocked_keys: BTreeSet<String>,
    pub blocked_buttons: BTreeSet<Button>,
}
impl Held {
    pub fn empty(&self) -> bool {
        self.keys.is_empty() && self.buttons.is_empty() && !self.touch_uncertain
    }
    pub fn snapshot(&self) -> Value {
        json!({"possiblyHeldKeys":self.keys.len(),"possiblyHeldButtons":self.buttons,"touchUncertain":self.touch_uncertain,"uncertain":self.uncertain,"pendingNativeCalls":self.inflight,"cleanupFailed":self.cleanup_failed,"unknownKeyReleases":self.blocked_keys.len(),"unknownButtonReleases":self.blocked_buttons.len(),"holdExpiresInMs":self.expires_at.map(|deadline|deadline.saturating_duration_since(Instant::now()).as_millis()),"expiryAttempted":self.expiry_attempted})
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub observations: BTreeMap<Reference, Observation>,
    pub held: BTreeMap<Reference, Held>,
    pub epochs: BTreeMap<Reference, (u64, u64)>,
}
impl State {
    pub fn observe(
        &mut self,
        page: Reference,
        frame: Reference,
        geometry: Value,
        started: Instant,
        input_epoch: u64,
    ) -> Reference {
        self.observations
            .retain(|_, observation| observation.started.elapsed() < OBSERVATION_TTL);
        if self.observations.len() >= OBSERVATIONS
            && let Some(oldest) = self
                .observations
                .iter()
                .min_by_key(|(_, observation)| observation.started)
                .map(|(reference, _)| *reference)
        {
            self.observations.remove(&oldest);
        }
        let reference = Reference {
            connection_id: page.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let (document_epoch, viewport_epoch) = *self.epochs.entry(page).or_default();
        self.observations.insert(
            reference,
            Observation {
                page,
                frame,
                geometry,
                started,
                input_epoch,
                document_epoch,
                viewport_epoch,
            },
        );
        reference
    }
    pub fn invalidate(&mut self, page: Reference) {
        self.observations
            .retain(|_, observation| observation.page != page);
        self.epochs.entry(page).or_default().1 += 1;
    }
    pub fn document_changed(&mut self, page: Reference) {
        self.invalidate(page);
        self.epochs.entry(page).or_default().0 += 1;
    }
    pub fn end(&mut self, page: Reference) {
        self.invalidate(page);
        self.held.remove(&page);
        self.epochs.remove(&page);
    }
    pub fn snapshot(&self) -> Value {
        json!({"observations":self.observations.len(),"observationTtlMs":OBSERVATION_TTL.as_millis(),"pages":self.held.iter().map(|(page,held)|json!({"page":page,"state":held.snapshot()})).collect::<Vec<_>>()})
    }
}

/// One CSS viewport coordinate of native input.
pub(crate) fn coordinate_schema() -> Value {
    json!({"type":"number","minimum":0,"maximum":8192,"description":"CSS viewport coordinate; must also be strictly inside the observed viewport"})
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let point = coordinate_schema();
    let button = json!({"enum":["left","right","middle"],"default":"left"});
    let key = json!({"type":"string","minLength":1,"maxLength":128});
    let text = json!({"type":"string","maxLength":16384});
    let delay = json!({"type":"integer","minimum":0,"maximum":1000,"default":0});
    let mut actions = Vec::new();
    for kind in [
        "mouse_move",
        "mouse_down",
        "mouse_click",
        "mouse_wheel",
        "touch_tap",
    ] {
        let mut properties = json!({"kind":{"const":kind},"x":point,"y":point});
        let mut required = vec!["kind", "x", "y"];
        match kind {
            "mouse_move" => {
                properties["steps"] =
                    json!({"type":"integer","minimum":1,"maximum":100,"default":1})
            }
            "mouse_down" | "mouse_click" => {
                properties["button"] = button.clone();
                if kind == "mouse_click" {
                    properties["clickCount"] =
                        json!({"type":"integer","minimum":1,"maximum":3,"default":1});
                    properties["delayMs"] = delay.clone();
                }
            }
            "mouse_wheel" => {
                for field in ["deltaX", "deltaY"] {
                    properties[field] = json!({"type":"number","minimum":-100000,"maximum":100000});
                    required.push(field);
                }
            }
            _ => {}
        }
        actions.push(object(properties, &required));
    }
    actions.push(object(
        json!({"kind":{"const":"mouse_up"},"button":button}),
        &["kind"],
    ));
    for kind in ["key_down", "key_up", "key_press"] {
        let mut properties = json!({"kind":{"const":kind},"key":key});
        if kind == "key_press" {
            properties["delayMs"] = delay.clone();
        }
        actions.push(object(properties, &["kind", "key"]));
    }
    for kind in ["type", "insert_text"] {
        let mut properties = json!({"kind":{"const":kind},"text":text});
        if kind == "type" {
            properties["delayMs"] = delay.clone();
        }
        actions.push(object(properties, &["kind", "text"]));
    }
    actions.push(object(json!({"kind":{"const":"delay"},"durationMs":{"type":"integer","minimum":0,"maximum":30000}}),&["kind","durationMs"]));
    vec![
        object(
            json!({"operation":{"const":"page_input"},"page":reference,"observation":reference,"actions":{"type":"array","minItems":1,"maxItems":64,"items":{"oneOf":actions}}}),
            &["operation", "page", "actions"],
        ),
        object(
            json!({"operation":{"const":"input_release"}}),
            &["operation"],
        ),
    ]
}

pub(crate) fn status_schema(reference: &Value) -> Value {
    let count = json!({"type":"integer","minimum":0,"maximum":256});
    let flag = json!({"type":"boolean"});
    json!({"type":"object","additionalProperties":false,"required":["observations","observationTtlMs","pages"],"properties":{
        "observations":count,"observationTtlMs":{"const":10000},"pages":{"type":"array","maxItems":256,"items":{"type":"object","additionalProperties":false,"required":["page","state"],"properties":{"page":reference,"state":{"type":"object","additionalProperties":false,"required":["possiblyHeldKeys","possiblyHeldButtons","touchUncertain","uncertain","pendingNativeCalls","cleanupFailed","unknownKeyReleases","unknownButtonReleases"],"properties":{
            "possiblyHeldKeys":count,"possiblyHeldButtons":{"type":"array","maxItems":3,"items":{"enum":["left","right","middle"]}},"touchUncertain":flag,"uncertain":flag,"pendingNativeCalls":{"type":"integer","minimum":0,"maximum":1},"cleanupFailed":flag,"unknownKeyReleases":count,"unknownButtonReleases":count,"holdExpiresInMs":{"type":["integer","null"],"minimum":0,"maximum":30000},"expiryAttempted":flag
        }}}}}
    }})
}
