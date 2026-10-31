//! Drive the actual operator prompts through the owned Windows console fixture.
use super::{Console, Duration, Instant, Path, fs};
use serde_json::{Value, json};
use std::collections::VecDeque;

pub(crate) fn run(root: &Path, args: &[&str], answers: &str) -> Value {
    let mut selected = vec!["__operator-probe"];
    selected.extend_from_slice(args);
    if args.first() == Some(&"init") && !args.contains(&"--no-baseline") {
        selected.push("--no-baseline");
    }
    let (mut console, output) = Console::start(root, &selected, 0);
    let mut parser = Prompts::new(answers);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut position = 0;
    loop {
        let exited = console.child.try_wait().expect("poll operator fixture");
        if exited.is_some() {
            console.finish_capture();
        }
        let bytes = output.lock().expect("operator capture").clone();
        parser.read(&bytes[position..], &mut console);
        position = bytes.len();
        assert!(
            Instant::now() < deadline,
            "operator deadline: {}",
            String::from_utf8_lossy(&bytes)
        );
        if let Some(status) = exited {
            assert!(
                status.success(),
                "operator fixture exit {status:?}: {}",
                String::from_utf8_lossy(&bytes)
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let code: i32 = fs::read_to_string(root.join("console-operator.code"))
        .expect("operator exit receipt")
        .parse()
        .expect("operator exit code");
    let stderr = fs::read_to_string(root.join("console-operator.stderr")).expect("operator errors");
    let settled = String::from_utf8_lossy(&parser.settled);
    json!({"code":code,"stdout":settled,"terminal":settled,"stderr":stderr})
}

struct Prompts {
    answers: VecDeque<Value>,
    pending: Vec<u8>,
    live_text: Vec<u8>,
    settled: Vec<u8>,
    live: bool,
    awaiting: bool,
    confirm_accept: bool,
}

impl Prompts {
    fn new(answers: &str) -> Self {
        let answers = answers.strip_prefix('@').map_or_else(
            || answers.lines().map(|answer| json!(answer)).collect(),
            |source| serde_json::from_str(source).expect("explicit prompt answers"),
        );
        Self {
            answers,
            pending: vec![],
            live_text: vec![],
            settled: vec![],
            live: false,
            awaiting: false,
            confirm_accept: false,
        }
    }

    fn read(&mut self, bytes: &[u8], console: &mut Console) {
        self.pending.extend_from_slice(bytes);
        let mut consumed = 0;
        while let Some(&byte) = self.pending.get(consumed) {
            if byte != 27 {
                consumed += 1;
                if self.live {
                    self.live_text.push(byte);
                } else {
                    self.settled.push(byte);
                }
                continue;
            }
            let Some(length) = escape_length(&self.pending[consumed..]) else {
                break;
            };
            let code = self.pending[consumed..consumed + length].to_vec();
            consumed += length;
            match code.as_slice() {
                b"\x1b[6n" => {
                    console.send(b"\x1b[2;1R");
                    self.live_text.clear();
                    self.awaiting = true;
                }
                b"\x1b[?2004h" => {
                    self.live = true;
                    self.awaiting = true;
                    self.live_text.clear();
                }
                b"\x1b[?2004l" => self.live = false,
                b"\x1b[?25h" | b"\x1b[?25l"
                    if self.live && !compact(&self.live_text).is_empty() =>
                {
                    self.answer(console);
                }
                _ => {}
            }
        }
        self.pending.drain(..consumed);
        if self
            .answers
            .front()
            .is_some_and(|answer| answer["wait"].is_string())
        {
            self.answer(console);
        }
        // ConPTY can suppress a redundant cursor-visibility transition. The
        // rendered choice footer still establishes that its first frame exists.
        let view: Vec<_> = compact(&self.live_text)
            .into_iter()
            .map(|byte| byte.to_ascii_lowercase())
            .collect();
        if self.live
            && contains(&view, b"enter")
            && [b"esccancel".as_slice(), b"escback", b"escclears"]
                .iter()
                .any(|footer| contains(&view, footer))
        {
            self.answer(console);
        }
    }

    fn answer(&mut self, console: &mut Console) {
        let explicit = self
            .answers
            .front()
            .and_then(|answer| answer["wait"].as_str());
        let view = compact(&self.live_text);
        let matches = explicit.is_some_and(|wait| contains(&view, &compact(wait.as_bytes())));
        if (self.awaiting && explicit.is_none()) || matches {
            let answer = self.answers.pop_front().unwrap_or_else(|| {
                panic!(
                    "unexpected operator prompt: {}",
                    String::from_utf8_lossy(&self.live_text)
                )
            });
            console.send(&keys(&answer, &view));
            self.confirm_accept = answer == "accept";
            self.awaiting = false;
            self.live_text.clear();
        } else if self.confirm_accept && contains(&view, b"Thischanges") {
            console.send(b"2\r");
            self.confirm_accept = false;
        }
    }
}

fn compact(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|part| part == needle)
}

/// Preserve partial escapes across reads, including `ConPTY`'s title sequences.
fn escape_length(bytes: &[u8]) -> Option<usize> {
    match *bytes.get(1)? {
        b'[' => bytes
            .iter()
            .enumerate()
            .skip(2)
            .find(|(_, byte)| (0x40..=0x7e).contains(*byte))
            .map(|(index, _)| index + 1),
        b']' => bytes
            .iter()
            .position(|byte| *byte == 7)
            .map(|index| index + 1)
            .or_else(|| {
                bytes
                    .windows(2)
                    .position(|part| part == b"\x1b\\")
                    .map(|index| index + 2)
            }),
        _ => Some(2),
    }
}

fn keys(answer: &Value, view: &[u8]) -> Vec<u8> {
    if let Some(keys) = answer["keys"].as_str() {
        return keys.as_bytes().to_vec();
    }
    let answer = answer
        .as_str()
        .expect("prompt answer string or explicit keys");
    if answer == "back" {
        return b"\x1b".to_vec();
    }
    if !answer.is_empty()
        && answer.bytes().all(|byte| byte.is_ascii_digit())
        && contains(view, b"tabnext")
        && contains(view, b"enterchoose")
    {
        return answer.as_bytes().to_vec();
    }
    let selected = if contains(view, b"Agentintegrations") && contains(view, b"spaceselect") {
        match answer {
            "claude" => "1\r",
            "codex" => "2\r",
            "claude,codex" => "12\r",
            _ => "\r",
        }
    } else if contains(view, b"Recordrequesthistory") && ["yes", "no"].contains(&answer) {
        match answer {
            "yes" => "1\r",
            "no" => "2\r",
            _ => "\r",
        }
    } else if contains(view, b"Clearexistinghistory") && ["yes", "no"].contains(&answer) {
        if answer == "yes" { "2\r" } else { "1\r" }
    } else if contains(view, b"Reviewpendingproposals")
        && ["manual", "repair", "history"].contains(&answer)
    {
        match answer {
            "manual" => "3\r",
            "repair" => "4\r",
            _ => "5\r",
        }
    } else {
        match answer {
            "no" | "done" => "\r",
            "yes" | "argv" | "manual" | "reject" | "deny" | "block" | "strict" | "cloud"
            | "off" => "2\r",
            "allow" | "repair" | "loose" => "3\r",
            "history" => "4\r",
            "review" | "accept" | "recommended" => "1\r",
            "leave" => "\x1b",
            _ => return format!("{answer}\r").into_bytes(),
        }
    };
    selected.as_bytes().to_vec()
}
