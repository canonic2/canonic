//! Controller input requests and the exact release set for uncertain commands.
use crate::{
    error::{Error, Result},
    keyboard::Stroke,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Gesture {
        start: [f64; 2],
        end: [f64; 2],
        duration: u32,
    },
    Keys {
        strokes: Vec<Stroke>,
    },
    Release {
        touch: bool,
        keys: Vec<u32>,
    },
    Close,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Command {
    pub id: String,
    pub geometry: Option<[u32; 2]>,
    pub input: Action,
}
impl Action {
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Gesture {
                start,
                end,
                duration,
            } => {
                if start
                    .iter()
                    .chain(end)
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                {
                    return Err(Error::invalid("Gesture point is out of bounds"));
                }
                crate::ui::validate_gesture_duration(u64::from(*duration))?;
            }
            Self::Keys { strokes }
                if (strokes.len() > 65537
                    || strokes
                        .iter()
                        .any(|s| s.usage == 0 || s.usage > 255 || s.modifiers > 15)) =>
            {
                return Err(Error::invalid("Invalid native key sequence"));
            }
            Self::Release { keys, .. }
                if keys.len() > 256 || keys.iter().any(|k| *k == 0 || *k > 255) =>
            {
                return Err(Error::invalid("Invalid native release set"));
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn release(&self) -> Self {
        let mut keys = std::collections::BTreeSet::new();
        if let Self::Keys { strokes } = self {
            for s in strokes {
                keys.insert(s.usage);
                for (bit, key) in [225, 224, 226, 227].into_iter().enumerate() {
                    if s.modifiers & (1 << bit) != 0 {
                        keys.insert(key);
                    }
                }
            }
        }
        Self::Release {
            touch: matches!(self, Self::Gesture { .. }),
            keys: keys.into_iter().collect(),
        }
    }
}
