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
    assert!(
        super::components::dump(terminal.backend().buffer()).contains("Selected original choice 3")
    );
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
child=subprocess.Popen([sys.argv[1]],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
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
    wait_for(b'Selected original choice 3')
    os.write(master,b'\x1b');time.sleep(0.1)
    os.write(master,b'\x1b[B\r')
    wait_for(b'Acme agent')
    os.write(master,b'hello\r')
    wait_for(b'Simulated response:')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',8,15,0,0))
    os.kill(child.pid,signal.SIGWINCH);time.sleep(0.1)
    os.write(master,b'\x03')
    assert child.wait(timeout=5)==0
    assert termios.tcgetattr(slave)==before,'raw mode not restored'
    while select.select([master],[],[],0.1)[0]:
        chunk=os.read(master,65536)
        if not chunk:break
        output+=chunk
    assert b'\x1b[?1049l' in output,'alternate screen not restored'
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
