//! Bounded, payload-free observations from this owned driver's fixed logger.
use std::collections::VecDeque;
use tokio::time::Instant;

const MAX_LINE: usize = 2048;
const MAX_EVENTS: usize = 64;
mod owned_child;

#[derive(Clone, Debug)]
pub(super) struct Lifecycle {
    started: Instant,
    line: Vec<u8>,
    overflow: bool,
    omitted_lines: u64,
    omitted_events: u64,
    events: VecDeque<(&'static str, u64)>,
    owned_children: owned_child::Children,
}
impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            line: Vec::new(),
            overflow: false,
            omitted_lines: 0,
            omitted_events: 0,
            events: VecDeque::new(),
            owned_children: owned_child::Children::default(),
        }
    }
}
impl Lifecycle {
    pub(super) fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    pub(super) fn feed(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if *byte == b'\n' {
                if !self.overflow {
                    let elapsed = self.elapsed_ms();
                    let _ = self.owned_children.feed(&self.line, elapsed);
                    if let Some(phase) = phase(&self.line) {
                        if self.events.len() == MAX_EVENTS {
                            self.events.pop_front();
                            self.omitted_events = self.omitted_events.saturating_add(1);
                        }
                        self.events.push_back((
                            phase,
                            u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                        ));
                    }
                }
                self.line.clear();
                self.overflow = false;
            } else if !self.overflow {
                if self.line.len() == MAX_LINE {
                    self.line.clear();
                    self.overflow = true;
                    self.omitted_lines = self.omitted_lines.saturating_add(1);
                } else {
                    self.line.push(*byte);
                }
            }
        }
    }
    pub(super) fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({"source":"owned-driver-pw-browser","events":self.events.iter().map(|(phase,elapsed)|serde_json::json!({"phase":phase,"elapsedMs":elapsed})).collect::<Vec<_>>(),"omittedEvents":self.omitted_events,"omittedLines":self.omitted_lines,"partialLine":self.overflow || !self.line.is_empty(),"maxEvents":MAX_EVENTS,"maxLineBytes":MAX_LINE,"observationsOnly":true,"ownedChildren":self.owned_children.snapshot()})
    }
}

fn phase(line: &[u8]) -> Option<&'static str> {
    let line = std::str::from_utf8(line).ok()?;
    let (date, message) = line.split_once(" pw:browser ")?;
    // The pinned non-TTY logger emits an ISO timestamp. Browser stdout/stderr
    // messages have their own [out]/[err] prefix and cannot enter this grammar.
    if date.len() != 24
        || date.bytes().enumerate().any(|(index, byte)| match index {
            4 | 7 => byte != b'-',
            10 => byte != b'T',
            13 | 16 => byte != b':',
            19 => byte != b'.',
            23 => byte != b'Z',
            _ => !byte.is_ascii_digit(),
        })
    {
        return None;
    }
    let (pid, message) = message.strip_prefix("[pid=")?.split_once("] ")?;
    if pid.is_empty()
        || !pid.bytes().all(|byte| byte.is_ascii_digit())
        || pid.parse::<u32>().ok()? == 0
    {
        return None;
    }
    match message {
        "<gracefully close start>" => Some("graceful_close_started"),
        "<gracefully close end>" => Some("graceful_close_finished"),
        "starting temporary directories cleanup" => Some("temporary_cleanup_started"),
        "finished temporary directories cleanup" => Some("temporary_cleanup_finished"),
        "<forcefully close>" => Some("browser_force_close_requested"),
        "<kill>" => Some("browser_kill_requested"),
        "<will force kill>" => Some("browser_group_kill_requested"),
        message if message.starts_with("exception while removing ") => {
            Some("temporary_cleanup_error")
        }
        message if message.starts_with("exception while trying to kill process: ") => {
            Some("browser_kill_error")
        }
        message => {
            let (code, signal) = message
                .strip_prefix("<process did exit: exitCode=")?
                .strip_suffix('>')?
                .split_once(", signal=")?;
            if (code == "null" || code.parse::<i32>().is_ok())
                && matches!(
                    signal,
                    "null"
                        | "SIGTERM"
                        | "SIGKILL"
                        | "SIGINT"
                        | "SIGHUP"
                        | "SIGABRT"
                        | "SIGSEGV"
                        | "SIGTRAP"
                        | "SIGBUS"
                )
            {
                Some("browser_process_closed")
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(message: &str) -> String {
        format!("2026-01-01T00:00:00.000Z pw:browser [pid=42] {message}\n")
    }
    #[test]
    fn owned_child_lifecycle_projection_is_fragmented_bounded_and_not_child_output() {
        let mut lifecycle = Lifecycle::default();
        let observation =
            "testrun:owned-child {\"phase\":\"native-exit\",\"owner\":1,\"streams\":[]}\n";
        lifecycle.feed(line(&format!("[out] {observation}")).as_bytes());
        assert!(
            lifecycle.snapshot()["ownedChildren"]["children"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        for fragment in observation.as_bytes().chunks(3) {
            lifecycle.feed(fragment);
        }
        assert!(
            lifecycle.snapshot()["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let exited =
            lifecycle.snapshot()["ownedChildren"]["children"][0]["nativeExitElapsedMs"].clone();
        assert!(exited.is_u64());
        for _ in 0..MAX_EVENTS + 1 {
            lifecycle.feed(line("<gracefully close start>").as_bytes());
        }
        assert_eq!(
            lifecycle.snapshot()["ownedChildren"]["children"][0]["nativeExitElapsedMs"],
            exited
        );
        assert_eq!(
            lifecycle.snapshot()["ownedChildren"]["children"][0]["nativeCloseElapsedMs"],
            serde_json::Value::Null
        );
        assert!(lifecycle.snapshot()["omittedEvents"].as_u64().unwrap() > 0);
        assert!(!lifecycle.snapshot().to_string().contains("pid"));
        lifecycle.feed(&vec![b'x'; MAX_LINE + 1]);
        lifecycle.feed(b"\n");
        assert_eq!(lifecycle.snapshot()["omittedLines"], 1);
    }
    #[test]
    fn driver_lifecycle_evidence_preserves_fragmented_order_without_payloads() {
        let mut lifecycle = Lifecycle::default();
        let text = [
            "<gracefully close start>",
            "<process did exit: exitCode=0, signal=null>",
            "starting temporary directories cleanup",
            "exception while removing /private/Acme-secret: Acme-private-error",
            "finished temporary directories cleanup",
            "<gracefully close end>",
        ]
        .map(line)
        .join("");
        for bytes in text.as_bytes().chunks(7) {
            lifecycle.feed(bytes);
        }
        let snapshot = lifecycle.snapshot();
        assert_eq!(
            snapshot["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| event["phase"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "graceful_close_started",
                "browser_process_closed",
                "temporary_cleanup_started",
                "temporary_cleanup_error",
                "temporary_cleanup_finished",
                "graceful_close_finished"
            ]
        );
        assert!(!snapshot.to_string().contains("Acme"));
        assert!(!snapshot.to_string().contains("pid"));
        assert_eq!(snapshot["partialLine"], false);
    }
    #[test]
    fn driver_lifecycle_evidence_rejects_child_output_and_incomplete_or_forged_lines() {
        let mut lifecycle = Lifecycle::default();
        for message in [
            "[err] <gracefully close start>",
            "[out] starting temporary directories cleanup",
            "<process did exit: exitCode=Acme-secret, signal=null>",
            "<process did exit: exitCode=0, signal=Acme-secret>",
        ] {
            lifecycle.feed(line(message).as_bytes());
        }
        lifecycle.feed(b"Acme pw:browser [pid=42] <gracefully close start>\n");
        let complete = line("<gracefully close start>");
        lifecycle.feed(complete.trim_end().as_bytes());
        assert!(
            lifecycle.snapshot()["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(lifecycle.snapshot()["partialLine"], true);
        lifecycle.feed(b"\n");
        assert_eq!(lifecycle.snapshot()["events"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn driver_lifecycle_evidence_bounds_lines_and_events_with_visible_loss() {
        let mut lifecycle = Lifecycle::default();
        lifecycle.feed(&vec![b'x'; MAX_LINE + 1]);
        lifecycle.feed(line("<gracefully close start>").as_bytes());
        assert!(
            lifecycle.snapshot()["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        for _ in 0..MAX_EVENTS + 1 {
            lifecycle.feed(line("<gracefully close end>").as_bytes());
        }
        let report = lifecycle.snapshot();
        assert_eq!(report["events"].as_array().unwrap().len(), MAX_EVENTS);
        assert_eq!(report["omittedEvents"], 1);
        assert_eq!(report["omittedLines"], 1);
        assert!(!report.to_string().contains(&"x".repeat(MAX_LINE)));
    }
}
