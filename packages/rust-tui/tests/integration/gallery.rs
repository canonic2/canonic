//! Verifies gallery navigation, fixture rendering, headless output and PTY lifecycle.

use assert_cmd::Command;
use canonic_rust_tui::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    gallery::Gallery,
    ratatui::{Terminal, backend::TestBackend},
};
use predicates::prelude::*;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn gallery_opens_at_the_selector_and_both_components_are_independent_fixtures() {
    let mut gallery = Gallery::new();
    assert_eq!(gallery.current(), "catalog");
    let mut terminal = Terminal::new(TestBackend::new(64, 16)).unwrap();
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("Option picker") && text.contains("Full-screen chat"));
    insta::assert_snapshot!("component_selector", text);
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "picker");
    gallery.update(&key(KeyCode::Enter));
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("Selected skip"));
    gallery.update(&key(KeyCode::Esc));
    gallery.update(&key(KeyCode::Down));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "chat");
    gallery.update(&Event::Paste("hello".into()));
    gallery.update(&key(KeyCode::Enter));
    for _ in 0..40 {
        gallery.tick();
    }
    terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("Simulated response:") && text.contains("hello"));
    for _ in 0..6 {
        gallery.update(&key(KeyCode::F(2)));
        terminal.draw(|frame| gallery.render(frame)).unwrap();
    }
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "catalog");
}

#[test]
fn editor_fixture_routes_paste_to_focused_scope_and_resets_on_reopen() {
    let mut gallery = Gallery::new();
    gallery.update(&Event::Paste("Multiline".into()));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "editor");
    let first = gallery.editor_input(0).unwrap().to_owned();
    assert_eq!(first.lines().count(), 2);
    let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    let rendered = super::components::dump(terminal.backend().buffer());
    assert!(
        rendered.contains("Edit this draft in the middle.\nEnter or Ctrl-J inserts a newline.")
    );
    gallery.update(&key(KeyCode::Tab));
    gallery.update(&Event::Paste(" changed".into()));
    assert_eq!(gallery.editor_input(0), Some(first.as_str()));
    let second = gallery.editor_input(1).unwrap().to_owned();
    gallery.update(&key(KeyCode::F(3)));
    gallery.update(&Event::Paste("modal".into()));
    gallery.update(&key(KeyCode::Tab));
    gallery.update(&Event::Paste(" only".into()));
    assert_eq!(gallery.editor_input(0), Some(first.as_str()));
    assert_eq!(gallery.editor_input(1), Some(second.as_str()));
    assert_eq!(gallery.editor_input(2), Some("modal only"));
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    gallery.update(&key(KeyCode::Esc));
    gallery.update(&Event::Paste("!".into()));
    assert_eq!(gallery.editor_input(2), Some("modal only!"));
    assert_eq!(gallery.editor_input(1), Some(second.as_str()));
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "editor");
    gallery.update(&Event::Paste(" restored".into()));
    assert_eq!(
        gallery.editor_input(1),
        Some(format!("{second} restored").as_str())
    );
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "catalog");
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "editor");
    assert_eq!(gallery.editor_input(1), Some("Independent second draft"));
    for scenario in 0..12 {
        let draft = gallery.editor_input(0).unwrap();
        assert!(!draft.contains("\\n"));
        if scenario == 5 {
            assert!(draft.contains("Cafe\u{301}"));
        }
        gallery.update(&key(KeyCode::F(2)));
    }
    assert!(gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    ))));
}

#[test]
fn rich_picker_fixtures_isolate_the_host_draft_and_reset_after_confirmation() {
    let mut gallery = Gallery::new();
    gallery.update(&key(KeyCode::Enter));
    let original = gallery.picker_input().to_owned();
    gallery.update(&Event::Paste("ignored without a filter".into()));
    assert_eq!(gallery.picker_input(), original);
    gallery.update(&key(KeyCode::Tab));
    gallery.update(&Event::Paste(" retained".into()));
    let draft = gallery.picker_input().to_owned();
    gallery.update(&key(KeyCode::Tab));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    for _ in 0..14 {
        terminal.draw(|f| gallery.render(f)).unwrap();
        assert_eq!(gallery.picker_input(), draft);
        gallery.update(&key(KeyCode::F(2)));
    }
    for _ in 0..4 {
        gallery.update(&key(KeyCode::F(2)));
    }
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "picker");
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "catalog");
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.picker_input(), original);
    gallery.update(&key(KeyCode::Char('1')));
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("Selected once"));
}

#[test]
fn headless_listing_help_and_refusal_do_not_change_terminal_modes() {
    Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Full-screen chat"));
    Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--component"));
    Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("interactive terminal"))
        .stdout(predicate::str::contains("\x1b").not());
}

#[cfg(unix)]
#[test]
fn real_terminal_navigation_resize_and_ctrl_c_restore_terminal_state() {
    let script = r"
import fcntl,os,pty,re,select,signal,struct,subprocess,sys,termios,time
master,slave=pty.openpty()
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
before=termios.tcgetattr(slave)
child=subprocess.Popen([sys.argv[1],'--colors','truecolor'],env={**os.environ,'NO_COLOR':'1'},stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
output=b''
def wait_for(text):
    global output
    deadline=time.monotonic()+10
    # Differential rendering positions existing spaces with cursor commands.
    def emitted_text():return re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]',b'',output).replace(b' ',b'')
    while text.replace(b' ',b'') not in emitted_text():
        assert time.monotonic()<deadline,output.decode(errors='replace')
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for(b'Choose a component')
    os.write(master,b'\r')
    wait_for(b'Continue without this server')
    os.write(master,b'\r')
    wait_for(b'Selected skip')
    os.write(master,b'\x1bOQ'*4)
    wait_for(b'switch branch')
    os.write(master,b'\x07\x1b[200~example29\x1b[201~')
    wait_for(b'updated 29 hours ago')
    os.write(master,b'\r')
    wait_for(b'Selected branch-29')
    os.write(master,b'\x1b');time.sleep(0.1)
    os.write(master,b'\x1b[B\r')
    wait_for(b'Acme agent')
    os.write(master,b'hello\r')
    wait_for(b'Simulated response:')
    os.write(master,b'\x1b');time.sleep(0.1)
    os.write(master,b'\x1b[B\r')
    wait_for(b'Independent second draft')
    os.write(master,b'\x07\x1b[200~PTY FIRST\r\nline two\x1b[201~')
    wait_for(b'PTY FIRST')
    wait_for(b'line two')
    os.write(master,b'\t\x07\x1b[200~PTY SECOND\x1b[201~')
    wait_for(b'PTY SECOND')
    os.write(master,b'\x1bOR');time.sleep(0.1)
    os.write(master,b'\x1b[200~MODAL ONLY\x1b[201~')
    wait_for(b'MODAL ONLY')
    os.write(master,b'\x1b');time.sleep(0.1)
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',8,15,0,0))
    os.kill(child.pid,signal.SIGWINCH);time.sleep(0.1)
    os.write(master,b'\x03')
    # Drain redraw output so a full PTY buffer cannot block Ctrl-C processing.
    deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline,'gallery did not exit after Ctrl-C'
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    assert re.search(rb'\x1b\[(?:\d+;)*38;2;86;194;201(?:;\d+)*m',output),'explicit RGB accent suppressed by NO_COLOR'
    # Exercise restored canonical input; macOS clears its transient PENDIN flag
    # when the next input is processed, before comparing every saved mode.
    os.write(master,b'restored\r')
    assert select.select([slave],[],[],1)[0],'ordinary line input not restored'
    assert os.read(slave,1024)==b'restored\n','canonical line handling not restored'
    assert termios.tcgetattr(slave)==before,'raw mode not restored'
    while select.select([master],[],[],0.1)[0]:
        chunk=os.read(master,65536)
        if not chunk:break
        output+=chunk
    assert b'\x1b[?1049l' in output,'alternate screen not restored'
    for profile in ['auto','none']:
        output=b''
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
        child=subprocess.Popen([sys.argv[1],'--colors',profile],env={**os.environ,'NO_COLOR':'1'},stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
        wait_for(b'Choose a component')
        os.write(master,b'\x03')
        deadline=time.monotonic()+5
        while child.poll() is None:
            assert time.monotonic()<deadline,'no-color gallery did not exit'
            if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
        assert child.returncode==0
        assert not re.search(rb'\x1b\[(?:\d+;)*(?:38|48);',output),'no-color profile emitted colors'
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
