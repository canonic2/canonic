//! Verifies card scenario effects, full details and picker/draft restoration.

use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    gallery::{Component, Gallery},
    ratatui::{Terminal, backend::TestBackend},
};
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn screen(gallery: &mut Gallery, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| gallery.render(f)).unwrap();
    super::components::dump(terminal.backend().buffer())
}
#[test]
fn reduced_motion_host_advances_seconds_and_options_keep_uncovered_activity_readable() {
    use canonic_tui::ratatui::style::Modifier;
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::ToolCallCard, "run-reduced-motion")
        .unwrap();
    assert!(screen(&mut gallery, 100, 32).contains("··· working · 14s"));
    for _ in 0..25 {
        gallery.tick();
    }
    assert!(screen(&mut gallery, 100, 32).contains("··· working · 16s"));
    gallery
        .open_scenario(Component::ToolCallCard, "run-waiting-input")
        .unwrap();
    assert!(screen(&mut gallery, 100, 32).contains("? input · permission review"));
    gallery
        .open_scenario(Component::ToolCallCard, "cancel-host-options")
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
    terminal.draw(|f| gallery.render(f)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    let y = u16::try_from(
        text.lines()
            .position(|line| line.contains("couldn't cancel"))
            .unwrap(),
    )
    .unwrap();
    let x = u16::try_from(text.lines().nth(usize::from(y)).unwrap().find('✕').unwrap()).unwrap();
    assert!(
        !terminal.backend().buffer()[(x, y)]
            .modifier
            .contains(Modifier::DIM)
    );
    assert!(text.contains("cancel didn't go through"));
}

#[test]
fn card_scenarios_show_real_lifecycles_and_reviewed_rendering() {
    let mut gallery = Gallery::new();
    for (scenario, expected) in [
        ("queued", "queued"),
        ("running", "running"),
        ("streaming", "214 lines"),
        ("waiting", "waiting for your approval"),
        ("completed", "6 passed"),
        ("failed", "exit 1"),
        ("cancelled", "cancelled"),
        ("unknown", "get_issue"),
        ("not-shown", "binary, 2.1 MB"),
        ("all-succeeded", "3 at once"),
        ("many-at-once", "+5 more"),
        ("empty-output", "no output"),
    ] {
        gallery
            .open_scenario(Component::ToolCallCard, scenario)
            .unwrap();
        assert!(
            screen(&mut gallery, 100, 40).contains(expected),
            "{scenario}"
        );
    }
    for scenario in [
        "main",
        "failed-selected",
        "narrow-diff",
        "without-color-diff",
        "expanded-capped",
    ] {
        gallery
            .open_scenario(Component::ToolCallCard, scenario)
            .unwrap();
        insta::assert_snapshot!(format!("tool_{scenario}"), screen(&mut gallery, 100, 42));
    }
}

#[test]
fn extended_card_and_run_scenarios_project_host_state_and_pending_cancellation() {
    let mut gallery = Gallery::new();
    for (scenario, expected) in [
        ("proposed", "not started"),
        ("interrupted-no-result", "no result"),
        ("progress-total", "12 of 40 files"),
        ("progress-unknown", "12 files so far"),
        ("redacted", "••• redacted by host"),
        ("sanitized", "␛[32mPASS"),
        ("artifacts", "settings.diff"),
        ("child-task", "2 calls"),
        ("child-task-selected", "o open task"),
        ("before-reorder", "export function"),
        ("after-reorder", "export function"),
        ("no-details", "more lines"),
        ("card-ascii", "[#######"),
        ("card-light", "12 of 40 files"),
        ("child-light-selected", "open task"),
        ("run-idle", "ready"),
        ("run-preparing", "preparing"),
        ("run-running", "working"),
        ("run-waiting", "1 question"),
        ("run-completed", "ready"),
        ("run-failed", "model limit reached"),
        ("run-interrupted", "interrupted"),
        ("tool-failed-connected", "working"),
        ("run-disconnected", "run state unknown"),
        ("run-reconnected", "ready"),
        ("run-offline-idle", "offline"),
        ("run-cancelling", "cancelling"),
        ("cancel-already-asked", "already asked"),
        ("run-cancelled", "work so far kept"),
        ("completion-wins", "6 passed"),
        ("cancel-no-ack", "couldn't cancel"),
        ("cancel-host-options", "Try cancelling again"),
        ("next-run-clean", "working"),
    ] {
        gallery
            .open_scenario(Component::ToolCallCard, scenario)
            .expect("registered scenario");
        assert!(
            screen(&mut gallery, 110, 42).contains(expected),
            "{scenario}"
        );
    }
    gallery
        .open_scenario(Component::ToolCallCard, "run-running")
        .expect("running fixture");
    gallery.update(&key(KeyCode::Esc));
    let pending = screen(&mut gallery, 110, 42);
    assert!(pending.contains("cancelling") && !pending.contains("cancelled"));
    gallery.update(&key(KeyCode::Esc));
    assert!(screen(&mut gallery, 110, 42).contains("already asked"));
    gallery.update(&key(KeyCode::F(5)));
    let completed = screen(&mut gallery, 110, 42);
    assert!(completed.contains("6 passed") && !completed.contains("cancelled"));
}

#[test]
fn artifact_chooser_opens_each_supplied_link_and_returns_to_the_same_draft() {
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::ToolCallCard, "artifacts")
        .expect("artifact fixture");
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    gallery.update(&key(KeyCode::Char('v')));
    let before = screen(&mut gallery, 100, 40);
    assert!(before.contains("a artifacts"));
    gallery.update(&key(KeyCode::Char('a')));
    assert!(screen(&mut gallery, 100, 40).contains("enter open"));
    gallery.update(&key(KeyCode::Down));
    gallery.update(&key(KeyCode::Enter));
    assert!(!screen(&mut gallery, 100, 40).contains("Simulated artifact:"));
    for _ in 0..4 {
        gallery.tick();
    }
    gallery.update(&key(KeyCode::Enter));
    assert!(screen(&mut gallery, 100, 40).contains("Simulated artifact: coverage"));
    gallery.update(&key(KeyCode::Esc));
    let returned = screen(&mut gallery, 100, 40);
    assert!(returned.contains("and bump the app version") && returned.contains("a artifacts"));
    gallery.update(&key(KeyCode::Char('a')));
    gallery.update(&key(KeyCode::Esc));
    assert!(!screen(&mut gallery, 100, 40).contains("enter open"));
    gallery.update(&key(KeyCode::Char('a')));
    let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(!gallery.update(&ctrl_c));
    assert!(screen(&mut gallery, 100, 40).contains("ctrl+c again closes it"));
    assert!(!gallery.update(&ctrl_c));
    assert!(!screen(&mut gallery, 100, 40).contains("enter open"));
    assert_eq!(gallery.tool_input(), "and bump the app version");
}
#[test]
fn details_search_copy_and_return_preserve_the_chat_caret() {
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::ToolCallCard, "large-output")
        .unwrap();
    gallery.update(&key(KeyCode::Left));
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    gallery.update(&key(KeyCode::Char('v')));
    gallery.update(&key(KeyCode::Char('o')));
    assert!(screen(&mut gallery, 100, 28).contains("/ find"));
    gallery.update(&key(KeyCode::Char('/')));
    gallery.update(&Event::Paste("(700)".into()));
    assert!(screen(&mut gallery, 100, 28).contains("transforming (700)"));
    gallery.update(&key(KeyCode::Enter));
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    assert!(!screen(&mut gallery, 100, 28).contains("copied all"));
    gallery.update(&key(KeyCode::Char('y')));
    assert!(screen(&mut gallery, 100, 28).contains("copied all · 1204 lines"));
    gallery.update(&key(KeyCode::Esc)); // details
    gallery.update(&key(KeyCode::Esc)); // selection
    gallery.update(&key(KeyCode::Char('!')));
    assert_eq!(gallery.tool_input(), "and bump the app versio!n");
    gallery.update(&key(KeyCode::F(10)));
    gallery.update(&key(KeyCode::F(10)));
    assert_eq!(gallery.tool_input(), "and bump the app versio!n");
}
#[test]
fn retry_copy_failure_and_pending_effects_are_simulated_and_observable() {
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::ToolCallCard, "failed-selected")
        .unwrap();
    gallery.update(&key(KeyCode::Char('r')));
    let text = screen(&mut gallery, 100, 28);
    assert!(text.contains("retried below") && text.contains("retry 1") && text.contains("running"));
    gallery.update(&key(KeyCode::Esc)); // leave selection
    gallery.update(&key(KeyCode::Esc)); // stop the accepted retry's turn
    assert!(screen(&mut gallery, 100, 28).contains("you pressed esc"));
    gallery
        .open_scenario(Component::ToolCallCard, "retried")
        .unwrap();
    gallery.update(&key(KeyCode::F(5)));
    gallery.update(&key(KeyCode::Esc));
    assert!(screen(&mut gallery, 100, 28).contains("enter sends"));
    for (scenario, action) in [
        ("copy-failed", 'c'),
        ("details-failed", 'o'),
        ("retry-refused", 'r'),
    ] {
        gallery
            .open_scenario(Component::ToolCallCard, scenario)
            .unwrap();
        gallery.update(&key(KeyCode::Char(action)));
        assert!(screen(&mut gallery, 100, 28).contains("host refusal"));
        assert_eq!(gallery.tool_input(), "and bump the app version");
    }
    gallery
        .open_scenario(Component::ToolCallCard, "delayed-ack")
        .unwrap();
    assert!(!screen(&mut gallery, 100, 28).contains("c copy output"));
    gallery.update(&key(KeyCode::F(7)));
    assert!(screen(&mut gallery, 100, 28).contains("copied output"));
    gallery
        .open_scenario(Component::ToolCallCard, "stale-ack")
        .unwrap();
    gallery.update(&key(KeyCode::F(7)));
    assert!(!screen(&mut gallery, 100, 28).contains("copied output"));
}
#[test]
fn group_expansion_growth_and_queued_steps_are_interactive() {
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::ToolCallCard, "all-succeeded")
        .unwrap();
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    gallery.update(&key(KeyCode::Char('v')));
    gallery.update(&key(KeyCode::Enter));
    assert!(screen(&mut gallery, 100, 40).contains("npm test -- digest"));
    gallery
        .open_scenario(Component::ToolCallCard, "queued")
        .unwrap();
    assert!(screen(&mut gallery, 100, 28).contains("enter queues"));
    gallery.update(&key(KeyCode::F(5)));
    assert!(screen(&mut gallery, 100, 28).contains("running"));
    gallery.update(&key(KeyCode::F(8)));
    assert!(screen(&mut gallery, 100, 28).contains("8 lines"));
    gallery.update(&key(KeyCode::F(6)));
    assert!(screen(&mut gallery, 100, 28).contains("simulated failure"));
    assert!(screen(&mut gallery, 100, 28).contains("enter sends"));
}
#[cfg(unix)]
#[test]
fn native_card_keys_details_resize_and_exit_restore_terminal_modes() {
    let script = r"
import fcntl,os,pty,re,select,signal,struct,subprocess,sys,termios,time
master,slave=pty.openpty();before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','tool-call-card','--scenario','selected','--colors','none','--ascii'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
output=b''
def screen():
    cells=[[' ']*100 for _ in range(28)];row=column=0
    for match in re.finditer(r'\x1b\[[0-?]*[ -/]*[@-~]|.',output.decode(errors='replace'),re.S):
        token=match.group()
        if token.startswith('\x1b['):
            args=token[2:-1];command=token[-1]
            if args.startswith(('?','>','<')):continue
            values=[int(v or 0) for v in args.split(';')]
            if command in 'Hf':row=(values[0] or 1)-1;column=(values[1] if len(values)>1 else 1)-1
            elif command=='G':column=(values[0] or 1)-1
            elif command=='C':column+=values[0] or 1
            elif command=='D':column-=values[0] or 1
            elif command=='J' and values[0]==2:cells=[[' ']*100 for _ in range(28)]
            elif command=='K' and 0<=row<28:
                start=0 if values[0] in (1,2) else max(0,column)
                end=min(100,column+1) if values[0]==1 else 100
                for x in range(start,end):cells[row][x]=' '
            continue
        if token=='\r':column=0;continue
        if token=='\n':row+=1;continue
        if ord(token)<32:continue
        if 0<=row<28 and 0<=column<100:cells[row][column]=token
        column+=1
    return '\n'.join(''.join(line) for line in cells).encode()
def wait_for(text):
    global output
    deadline=time.monotonic()+10
    while text not in screen():
        assert child.poll() is None,output[-2000:]
        assert time.monotonic()<deadline,(text,screen())
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for(b'o details')
    os.write(master,b'\ro');wait_for(b'/ find')
    os.write(master,b'/profile\ry');wait_for(b'copied all')
    os.write(master,b'\x1b');wait_for(b'o details')
    os.write(master,b'\x1b');wait_for(b'actions on')
    os.write(master,b'\x1bOD!');wait_for(b'versio!n')
    os.write(master,b'\x1b[21~');wait_for(b'tool-call-card')
    os.write(master,b'\x1b[21~');wait_for(b'versio!n')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',12,40,0,0));os.kill(child.pid,signal.SIGWINCH)
    os.write(master,b'\x03');deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline
        if select.select([master],[],[],0.05)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    os.write(master,b'restored\r');assert select.select([slave],[],[],1)[0]
    assert os.read(slave,1024)==b'restored\n'
    assert termios.tcgetattr(slave)==before
    assert b'\x1b[?1000h' not in output and b'\x1b[?1002h' not in output
finally:
    if child.poll() is None:child.kill();child.wait()
    os.close(master);os.close(slave)
";
    let output = std::process::Command::new("python3")
        .args(["-c", script])
        .arg(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
