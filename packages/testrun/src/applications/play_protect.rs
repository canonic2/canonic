//! Google Play Protect's install-time prompt on Android: detection while an
//! install is pending, the caller's opt-in answer, and the installed-package
//! hash that settles an uncertain install.
use super::*;
use crate::android::Android;
use std::{sync::Mutex, time::Duration};

/// Play Store owns the prompt; its window taking focus during an install is the signal.
const VERIFIER: &str = "com.android.vending";
/// Prompts answered within one install before Testrun stops waiting.
const MAX_ANSWERS: usize = 3;

/// What an install does when Play Protect asks to send the app for a security check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Answer {
    /// Stop waiting and report the prompt without touching it.
    #[default]
    Fail,
    DontSend,
    Send,
}
impl Answer {
    pub(crate) fn is_fail(&self) -> bool {
        *self == Self::Fail
    }
    fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Fail => &[],
            Self::DontSend => &["don't send", "do not send"],
            Self::Send => &["send"],
        }
    }
}

/// Polling cadence. Installs that finish before `delay` never inspect the screen.
#[derive(Clone, Copy)]
pub(super) struct Cadence {
    pub delay: Duration,
    pub interval: Duration,
}
impl Default for Cadence {
    fn default() -> Self {
        Self {
            delay: Duration::from_secs(2),
            interval: Duration::from_secs(1),
        }
    }
}

/// Watches the screen while an install runs. Returns only when the install
/// must stop waiting; answers given along the way are pushed to `answers`.
pub(super) async fn watch(
    android: &Android,
    serial: &str,
    answer: Answer,
    cadence: Cadence,
    answers: &Mutex<Vec<Value>>,
) -> Error {
    // A Play Store window already in front before the prompt could appear is not the prompt.
    let baseline = focus(android, serial).await;
    tokio::time::sleep(cadence.delay).await;
    loop {
        let current = focus(android, serial).await;
        if current.is_some() && current != baseline && is_verifier(current.as_deref()) {
            let options = buttons(android, serial).await;
            let chosen = options.as_ref().ok().and_then(|nodes| {
                nodes.iter().find(|node| {
                    answer
                        .labels()
                        .contains(&normalize(node["label"].as_str().unwrap_or("")).as_str())
                })
            });
            match chosen {
                Some(node) if answers.lock().unwrap().len() < MAX_ANSWERS => {
                    if let Err(error) = tap(android, serial, &node["frame"]).await {
                        return prompt_error(answer, &options, Some(error));
                    }
                    answers
                        .lock()
                        .unwrap()
                        .push(json!({"answer":answer,"label":node["label"]}));
                    // Let the dialog close before the next focus read.
                    tokio::time::sleep(cadence.interval * 2).await;
                }
                Some(_) => return prompt_error(answer, &options, None),
                // An answer was requested but its button is not readable yet:
                // retry until the install's own deadline.
                None if !answer.is_fail() && options.is_err() => {}
                None => return prompt_error(answer, &options, None),
            }
        }
        tokio::time::sleep(cadence.interval).await;
    }
}

fn prompt_error(answer: Answer, options: &Result<Vec<Value>>, cause: Option<Error>) -> Error {
    let message = if answer.is_fail() {
        "Google Play Protect is asking whether to send the app for a security check; the install waits for that answer. Retry with playProtect dont_send or send, or answer it on the device"
    } else {
        "Google Play Protect is asking whether to send the app for a security check, and the requested answer's button was not found; answer it on the device or with act"
    };
    let buttons = match options {
        Ok(nodes) => json!(
            nodes
                .iter()
                .map(|n| json!({"label":n["label"],"frame":n["frame"]}))
                .collect::<Vec<_>>()
        ),
        Err(_) => Value::Null,
    };
    Error::new("INSTALL_AWAITING_PLAY_PROTECT", message).details(
        json!({"prompt":"play_protect_security_check","buttons":buttons,"requestedAnswer":answer,"cause":cause}),
    )
}

async fn focus(android: &Android, serial: &str) -> Option<String> {
    let output = android
        .script(
            serial,
            "dumpsys window | grep -m 1 mCurrentFocus= || true",
            Duration::from_secs(5),
            65536,
        )
        .await
        .ok()?;
    String::from_utf8_lossy(&output)
        .lines()
        .find_map(|line| line.trim().strip_prefix("mCurrentFocus="))
        .map(str::to_owned)
}

/// `Window{hash u0 com.android.vending/...}` or a Play Store dialog without an activity.
fn is_verifier(focus: Option<&str>) -> bool {
    focus
        .and_then(|f| f.strip_suffix('}'))
        .and_then(|f| f.split_whitespace().last())
        .is_some_and(|name| name == VERIFIER || name.starts_with(&format!("{VERIFIER}/")))
}

/// Labelled, clickable nodes of the current screen, in tree order.
async fn buttons(android: &Android, serial: &str) -> Result<Vec<Value>> {
    let tree = android.snapshot(serial).await?;
    let mut found = Vec::new();
    let mut stack = vec![&tree["root"]];
    while let Some(node) = stack.pop() {
        let label = node["label"].as_str().unwrap_or("");
        if !label.is_empty()
            && (node["states"]["clickable"] == true || node["role"] == "button")
            && node["frame"]["width"].is_number()
        {
            found.push(node.clone());
        }
        if let Some(children) = node["children"].as_array() {
            stack.extend(children.iter().rev());
        }
    }
    Ok(found)
}

fn normalize(label: &str) -> String {
    label.trim().replace('\u{2019}', "'").to_lowercase()
}

async fn tap(android: &Android, serial: &str, frame: &Value) -> Result<()> {
    let center = |origin: &str, size: &str| {
        frame[origin]
            .as_i64()
            .zip(frame[size].as_i64())
            .map(|(o, s)| o + s / 2)
    };
    let (Some(x), Some(y)) = (center("x", "width"), center("y", "height")) else {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Prompt button has no frame",
        ));
    };
    android
        .shell(
            serial,
            &["input", "tap", &x.to_string(), &y.to_string()],
            Duration::from_secs(5),
            4096,
        )
        .await?;
    Ok(())
}

/// SHA-256 of the installed base APK, compared with the submitted package.
pub(super) async fn reconcile(adb: &Path, serial: &str, bundle: &str, submitted: &str) -> Value {
    let android = Android { adb: adb.into() };
    let read = async {
        let paths = android
            .shell(
                serial,
                &["pm", "path", bundle],
                Duration::from_secs(5),
                65536,
            )
            .await?;
        let paths = String::from_utf8_lossy(&paths);
        let mut apks = paths
            .lines()
            .filter_map(|l| l.trim().strip_prefix("package:"));
        let Some(first) = apks.next() else {
            return Ok(None);
        };
        let base = std::iter::once(first)
            .chain(apks)
            .find(|p| p.ends_with("/base.apk"))
            .unwrap_or(first)
            .to_owned();
        let digest = android
            .shell(serial, &["sha256sum", &base], Duration::from_secs(10), 4096)
            .await?;
        String::from_utf8_lossy(&digest)
            .split_whitespace()
            .next()
            .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|d| Some(d.to_ascii_lowercase()))
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Invalid installed APK digest"))
    };
    match tokio::time::timeout(Duration::from_secs(12), read).await {
        Ok(Ok(Some(installed))) => {
            json!({"targetBuildInstalled":installed==submitted,"installedSha256":installed,"submittedSha256":submitted})
        }
        Ok(Ok(None)) => json!({"targetBuildInstalled":false,"installed":false}),
        Ok(Err(error)) => json!({"error":error}),
        Err(_) => {
            json!({"error":{"code":"TIMEOUT","message":"Installed APK digest read exceeded its budget"}})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_play_store_window_is_the_prompt() {
        for focus in [
            "Window{1a u0 com.android.vending/com.google.android.finsky.verifier.impl.ConsentDialog}",
            "Window{1a u0 com.android.vending}",
        ] {
            assert!(is_verifier(Some(focus)), "{focus}");
        }
        for focus in [
            "Window{1a u0 com.example.acme/.Main}",
            "Window{1a u0 com.android.vendingx/.Main}",
            "null",
        ] {
            assert!(!is_verifier(Some(focus)), "{focus}");
        }
    }
    #[test]
    fn answers_match_whole_labels_with_either_apostrophe() {
        assert!(
            Answer::DontSend
                .labels()
                .contains(&normalize(" Don’t send ").as_str())
        );
        assert!(
            !Answer::Send
                .labels()
                .contains(&normalize("Don't send").as_str())
        );
        assert!(Answer::Send.labels().contains(&normalize("SEND").as_str()));
        assert!(Answer::Fail.labels().is_empty());
    }
}
