//! Android input planning, supported key catalog and bounded dispatch.
use super::Android;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

impl Android {
    pub(crate) async fn tap(
        &self,
        serial: &str,
        point: [f64; 2],
        display: Option<&crate::ui::Display>,
        timeout: Duration,
    ) -> Result<Value> {
        let (width, height) = self.dimensions(serial).await?;
        if let Some(display) = display {
            display.validate(width, height)?;
        }
        let (x, y) = pixels(point, width, height)?;
        self.shell(
            serial,
            &["input", "tap", &x.to_string(), &y.to_string()],
            timeout,
            4096,
        )
        .await?;
        Ok(json!({"completed":true,"observed":false}))
    }
    pub(crate) async fn swipe(
        &self,
        serial: &str,
        points: [[f64; 2]; 2],
        duration: u64,
        display: Option<&crate::ui::Display>,
        timeout: Duration,
    ) -> Result<Value> {
        crate::ui::validate_gesture_duration(duration)?;
        let (width, height) = self.dimensions(serial).await?;
        if let Some(display) = display {
            display.validate(width, height)?;
        }
        let [from, to] = points;
        let (x, y) = pixels(from, width, height)?;
        let (end_x, end_y) = pixels(to, width, height)?;
        self.shell(
            serial,
            &[
                "input",
                "swipe",
                &x.to_string(),
                &y.to_string(),
                &end_x.to_string(),
                &end_y.to_string(),
                &duration.to_string(),
            ],
            timeout,
            4096,
        )
        .await?;
        Ok(json!({"completed":true,"observed":false}))
    }
    /// Types bounded chunks in order. One `input text` command stamps all of
    /// its key events with its start time, and Android drops events still
    /// queued 10 s after their time, so one long command loses its end.
    pub async fn type_text(&self, serial: &str, text: &str, timeout: Duration) -> Result<Value> {
        text_argument(text)?;
        let deadline = Instant::now() + timeout;
        let chunks = text_chunks(text);
        let mut sent = 0;
        for chunk in &chunks {
            let started = Instant::now();
            let result = self
                .shell(
                    serial,
                    &["input", "text", &text_argument(chunk)?],
                    deadline.saturating_duration_since(started),
                    4096,
                )
                .await;
            let delivered = sent;
            let unknown = |cause: &str| {
                Error::new(
                    "OUTCOME_UNKNOWN",
                    "Android text input may be incomplete; read the field before typing again",
                )
                .details(
                    json!({"cause":cause,"sentCharacters":delivered,"textCharacters":text.len()}),
                )
            };
            match result {
                Err(error) if sent == 0 => return Err(error),
                Err(error) => return Err(unknown(&error.code)),
                Ok(_) if started.elapsed() >= STALE_RISK => return Err(unknown("CHUNK_TOO_SLOW")),
                Ok(_) => sent += chunk.len(),
            }
        }
        Ok(json!({"completed":true,"observed":false,"chunks":chunks.len()}))
    }
    pub async fn key(
        &self,
        serial: &str,
        name: &str,
        modifiers: &[String],
        timeout: Duration,
    ) -> Result<Value> {
        let codes = key_codes(name, modifiers)?;
        let mut command = vec![if codes.len() == 1 {
            "keyevent"
        } else {
            "keycombination"
        }];
        command.extend(codes.iter().map(String::as_str));
        command.insert(0, "input");
        self.shell(serial, &command, timeout, 4096).await?;
        Ok(json!({"completed":true,"observed":false}))
    }
    /// Reports whether the selected target's `input` utility lists
    /// `keycombination`; a failed or unrecognized help reply means unavailable.
    pub(crate) async fn key_chords(&self, serial: &str) -> bool {
        self.shell(serial, &["input", "help"], Duration::from_secs(5), 65536)
            .await
            .is_ok_and(|help| {
                String::from_utf8_lossy(&help)
                    .lines()
                    .any(|line| line.trim_start().starts_with("keycombination"))
            })
    }
}

/// Key codes for one key and its ordered, distinct modifiers (pressed first).
pub(crate) fn key_codes(name: &str, modifiers: &[String]) -> Result<Vec<String>> {
    if modifiers.len() > 4 {
        return Err(Error::invalid("A key chord has at most four modifiers"));
    }
    let mut codes = Vec::new();
    for modifier in modifiers {
        let code = match modifier.as_str() {
            "shift" => "KEYCODE_SHIFT_LEFT",
            "control" => "KEYCODE_CTRL_LEFT",
            "alt" => "KEYCODE_ALT_LEFT",
            "meta" => "KEYCODE_META_LEFT",
            _ => return Err(Error::invalid("Unknown key modifier")),
        };
        if codes.iter().any(|existing| existing == code) {
            return Err(Error::invalid("Duplicate key modifier"));
        }
        codes.push(code.to_owned());
    }
    codes.push(keycode(name)?);
    Ok(codes)
}

/// Characters per `input text` command. A Samsung phone took about 37 ms per
/// character, so a chunk stays far below the 10 s stale-event limit.
const TEXT_CHUNK: usize = 32;
/// A chunk that took this long may have reached the stale-event limit.
const STALE_RISK: Duration = Duration::from_secs(8);

/// Most characters one typing action sends; its default deadline then fits
/// inside one 120 s `act` call.
const MAX_TYPED: usize = 2000;

/// Length-based typing budget, combined with the normal action minimum: the same
/// Samsung phone delivered about 24 characters per second including chunk
/// overhead, so this allows about twice that time.
pub(crate) fn text_timeout(text: &str) -> Duration {
    Duration::from_secs(15) + Duration::from_millis(50) * text.len() as u32
}

/// Splits validated ASCII text into ordered `input text` chunks.
pub(crate) fn text_chunks(text: &str) -> Vec<String> {
    text.chars()
        .collect::<Vec<_>>()
        .chunks(TEXT_CHUNK)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// Encodes text for `input text`, refusing text the utility cannot type literally.
pub(crate) fn text_argument(text: &str) -> Result<String> {
    if text.len() > MAX_TYPED {
        return Err(Error::invalid(format!(
            "Android typing sends at most {MAX_TYPED} characters per action; split longer text across actions, or set a field's whole value with replace"
        )));
    }
    if !text.is_ascii() || text.chars().any(char::is_control) {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "adb text input supports bounded printable ASCII",
        ));
    }
    if text.contains("%s") {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "adb cannot type a literal percent-s sequence safely",
        ));
    }
    Ok(text.replace(' ', "%s"))
}

/// Maps the supported Android key catalog to `input keyevent` codes.
///
/// `home` and `end` are the keyboard keys, as on iOS; navigation Home is
/// `home_screen`.
pub(crate) fn keycode(name: &str) -> Result<String> {
    let code = match name {
        "home_screen" => "HOME",
        "back" => "BACK",
        "app_switch" => "APP_SWITCH",
        "enter" => "ENTER",
        "escape" => "ESCAPE",
        "backspace" => "DEL",
        "delete" => "FORWARD_DEL",
        "tab" => "TAB",
        "space" => "SPACE",
        "home" => "MOVE_HOME",
        "end" => "MOVE_END",
        "page_up" => "PAGE_UP",
        "page_down" => "PAGE_DOWN",
        "arrow_up" => "DPAD_UP",
        "arrow_down" => "DPAD_DOWN",
        "arrow_left" => "DPAD_LEFT",
        "arrow_right" => "DPAD_RIGHT",
        "volume_up" => "VOLUME_UP",
        "volume_down" => "VOLUME_DOWN",
        "power" => "POWER",
        _ if name.len() == 1 && name.as_bytes()[0].is_ascii_lowercase() => {
            return Ok(format!("KEYCODE_{}", name.to_ascii_uppercase()));
        }
        _ => {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Android key is not in its supported catalog",
            ));
        }
    };
    Ok(format!("KEYCODE_{code}"))
}

pub(super) fn pixels(point: [f64; 2], width: u32, height: u32) -> Result<(u32, u32)> {
    if point
        .into_iter()
        .any(|n| !n.is_finite() || !(0.0..=1.0).contains(&n))
    {
        return Err(Error::invalid("Input points must be normalized in 0..1"));
    }
    Ok((
        (point[0] * f64::from(width - 1)).round() as u32,
        (point[1] * f64::from(height - 1)).round() as u32,
    ))
}
