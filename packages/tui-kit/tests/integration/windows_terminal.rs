//! Native console leases and keyboard interaction in a private Windows `ConPTY`.
use canonic_tui_kit::{
    picker::Choice,
    runtime::{Capabilities, InlineOptions, InlineSession, TerminalSession, prompts},
};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

const FIXTURE: &str = "windows_terminal::console_fixture";

fn modes() -> (u32, u32) {
    (
        winapi_util::console::mode(io::stdin()).expect("fixture console input mode"),
        winapi_util::console::mode(io::stdout()).expect("fixture console output mode"),
    )
}

#[test]
#[ignore = "owned ConPTY subprocess of native_windows_console_restores_modes_and_accepts_filtered_selection"]
fn console_fixture() {
    let root = std::env::var_os("TUI_WINDOWS_FIXTURE_ROOT").unwrap();
    let root = Path::new(&root);
    let (input, output) = modes();
    winapi_util::console::set_mode(io::stdout(), output & !0x0004).unwrap();
    let original = modes();
    if std::env::var_os("TUI_WINDOWS_FIXTURE_PICKER").is_some() {
        let selected = prompts::choose_many(
            "Acme selection",
            "Choose the first fixture",
            vec![
                Choice::new("first", "Acme first"),
                Choice::new("second", "Acme second"),
            ],
            &[],
            1,
            "continue",
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected, vec!["first"]);
    } else {
        let lease = TerminalSession::open_input().unwrap();
        assert!(TerminalSession::open_input().is_err());
        let (lease, operation) = lease.handoff(|| {
            assert_eq!(modes(), original);
            Ok(())
        });
        operation.unwrap();
        assert_ne!(modes().1 & 0x0004, 0);
        lease.unwrap().close().unwrap();
        assert_eq!(modes(), original);
        let options = InlineOptions {
            height: 6,
            capabilities: Capabilities {
                cursor: true,
                alternate: true,
            },
        };
        let mut inline = InlineSession::open(options).unwrap();
        inline.print_before("Acme checkpoint").unwrap();
        inline.enter_alternate().unwrap();
        let paused = inline.pause().unwrap();
        assert_eq!(modes(), original);
        paused.resume().unwrap().finish("").unwrap();
        assert_eq!(modes(), original);
        let unwind = std::panic::catch_unwind(|| {
            let _lease = TerminalSession::open_input().unwrap();
            panic!("owned unwind fixture");
        });
        assert!(unwind.is_err());
    }
    assert_eq!(modes(), original);
    winapi_util::console::set_mode(io::stdin(), input).unwrap();
    winapi_util::console::set_mode(io::stdout(), output).unwrap();
    fs::write(root.join("result"), "restored").unwrap();
}

struct Console {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    reader: Option<JoinHandle<()>>,
    output: Arc<Mutex<Vec<u8>>>,
}
impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.writer.take();
        self.master.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one bounded console lifetime owns capture, cursor replies, selection and cleanup"
)]
fn native_windows_console_restores_modes_and_accepts_filtered_selection() {
    for picker in [false, true] {
        let root = assert_fs::TempDir::new().unwrap();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            FIXTURE,
            "--ignored",
            "--nocapture",
            "--color",
            "never",
        ]);
        command.env("TUI_WINDOWS_FIXTURE_ROOT", root.path());
        command.env_remove("TERM");
        if picker {
            command.env("TUI_WINDOWS_FIXTURE_PICKER", "1");
        }
        let reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let output = Arc::new(Mutex::new(Vec::new()));
        let mut console = Console {
            child,
            master: Some(pair.master),
            writer: Some(writer),
            reader: None,
            output: output.clone(),
        };
        console.reader = Some(std::thread::spawn(move || {
            let mut reader = reader;
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let mut output = output
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                output.extend_from_slice(&buffer[..count]);
                let excess = output.len().saturating_sub(65_536);
                output.drain(..excess);
            }
        }));
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut sent = false;
        let mut answered_queries = 0;
        let status = loop {
            if let Some(status) = console.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "owned native console timed out (picker={picker}): {}",
                String::from_utf8_lossy(&console.output.lock().unwrap())
            );
            // ConPTY asks its terminal host for the initial cursor position
            // before it starts the client. This private empty terminal starts
            // at row/column one; answer as its terminal emulator would.
            let queries = console
                .output
                .lock()
                .unwrap()
                .windows(4)
                .filter(|bytes| *bytes == b"\x1b[6n")
                .count();
            assert!(queries <= 16, "unexpected cursor-query loop");
            while answered_queries < queries {
                console
                    .writer
                    .as_mut()
                    .unwrap()
                    .write_all(b"\x1b[1;1R")
                    .unwrap();
                console.writer.as_mut().unwrap().flush().unwrap();
                answered_queries += 1;
            }
            if picker
                && !sent
                && console
                    .output
                    .lock()
                    .unwrap()
                    .windows(b"Acme selection".len())
                    .any(|bytes| bytes == b"Acme selection")
            {
                console
                    .writer
                    .as_mut()
                    .unwrap()
                    .write_all(b"first \r")
                    .unwrap();
                console.writer.as_mut().unwrap().flush().unwrap();
                sent = true;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            status.success(),
            "{}",
            String::from_utf8_lossy(&console.output.lock().unwrap())
        );
        assert_eq!(
            fs::read_to_string(root.path().join("result")).unwrap(),
            "restored"
        );
    }
}
