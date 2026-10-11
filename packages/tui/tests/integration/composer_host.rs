//! Verifies storage decoding, real editor processes and native terminal handoff.

use canonic_tui::{
    composer::{Composer, EditorResult, SavedComposer, editor_command, run_editor},
    runtime::TerminalSession,
};

#[test]
fn storage_round_trip_retains_pastes_and_rejects_truncation_and_unknown_schema() {
    let mut c = Composer::default();
    c.update(&canonic_tui::event::Event::Paste(
        "one\ntwo\nthree\nfour".into(),
    ));
    assert!(c.insert_image("acme-image"));
    c.stash_draft();
    c.set_text("session draft");
    let snapshot = c.saved();
    let mut bytes = Vec::new();
    snapshot.write_to(&mut bytes).unwrap();
    let decoded = SavedComposer::read_from(&mut bytes.as_slice(), 65_536).unwrap();
    assert_eq!(decoded, snapshot);
    let directory = assert_fs::TempDir::new().unwrap();
    let path = directory.path().join("composer.snapshot");
    {
        let mut file = std::fs::File::create(&path).unwrap();
        snapshot.write_to(&mut file).unwrap();
        file.sync_all().unwrap();
    }
    let disk = SavedComposer::read_from(&mut std::fs::File::open(path).unwrap(), 65_536).unwrap();
    assert_eq!(disk, snapshot);
    let mut restored = Composer::default();
    assert!(restored.restore_saved(&decoded));
    assert_eq!(restored.saved(), decoded);
    for length in [0, 1, 16, bytes.len() - 1] {
        assert!(SavedComposer::read_from(&mut &bytes[..length], 65_536).is_err());
    }
    let mut invalid = bytes.clone();
    invalid[15] = 99;
    assert!(SavedComposer::read_from(&mut invalid.as_slice(), 65_536).is_err());
    assert!(SavedComposer::read_from(&mut bytes.as_slice(), 1).is_err());
    let mut invalid = decoded;
    invalid.draft.cursor = usize::MAX;
    assert!(!restored.restore_saved(&invalid));
    assert_eq!(restored.editor().text(), "session draft");
}

#[test]
fn schema_one_image_snapshot_remains_readable_with_typed_payloads() {
    fn string(bytes: &mut Vec<u8>, text: &str) {
        bytes.extend_from_slice(&(text.len() as u64).to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    }
    let mut bytes = b"CANONIC-COMPOSER\x01".to_vec();
    string(&mut bytes, "\u{e000}");
    bytes.extend_from_slice(&3_u64.to_le_bytes());
    bytes.push(0); // message mode
    bytes.extend_from_slice(&0_u64.to_le_bytes()); // age
    bytes.extend_from_slice(&1_u64.to_le_bytes()); // one atom
    bytes.extend_from_slice(&0xe000_u32.to_le_bytes());
    string(&mut bytes, "[image 1]");
    string(&mut bytes, "[Image 1]");
    bytes.push(1);
    string(&mut bytes, "old-host-image");
    for _ in 0..6 {
        bytes.extend_from_slice(&0_u64.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 0, 0]); // absent recovery copies
    let saved = SavedComposer::read_from(&mut bytes.as_slice(), 65_536).unwrap();
    let atom = saved.draft.atoms.values().next().unwrap();
    assert_eq!(atom.kind, canonic_tui::editor::AtomKind::Image);
    assert_eq!(atom.reference.as_deref(), Some("old-host-image"));
    let mut c = Composer::default();
    assert!(c.restore_saved(&saved));
    assert_eq!(c.editor().expanded_text(), "[Image 1]");
}

#[test]
fn restart_protects_pending_submission_and_uncommitted_queued_edit() {
    use canonic_tui::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let mut c = Composer::default();
    c.set_text("sending");
    let sent = c.update(&enter).unwrap();
    c.set_text("queued");
    c.update(&enter);
    assert!(c.edit_queued(1));
    c.update(&Event::Paste(" edited".into()));
    let state = c.saved();
    let mut restored = Composer::default();
    assert!(restored.restore_saved(&state));
    assert!(restored.queue_paused());
    assert_eq!(restored.queue()[0].text, "queued");
    let stash = restored.stashed_drafts();
    assert!(stash.iter().any(|d| d.text == sent.text));
    assert!(stash.iter().any(|d| d.text == "queued edited"));
    assert!(restored.take_intents().is_empty());
}

#[cfg(unix)]
#[test]
fn foreground_editor_uses_private_owned_file_and_cleans_success_and_failure() {
    use std::path::Path;
    assert_eq!(
        editor_command(Some("nvim"), Some("vi")).as_deref(),
        Some("nvim")
    );
    assert_eq!(editor_command(Some(" "), Some("vi")).as_deref(), Some("vi"));
    assert!(editor_command(None, None).is_none());
    let directory = assert_fs::TempDir::new().unwrap();
    let mut c = Composer::default();
    c.set_text("original");
    let request = c.begin_editor("test editor").unwrap();
    let command = "python3 -c 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); assert p.stat().st_mode & 0o777 == 0o600; assert p.read_text() == \"original\"; p.write_text(\"returned\")'";
    let returned = run_editor(
        &request,
        command,
        Path::new("/bin/sh"),
        directory.path(),
        65_536,
    )
    .unwrap();
    assert!(c.finish_editor(&request.ticket, returned));
    assert_eq!(c.editor().text(), "returned");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    let request = c.begin_editor("test editor").unwrap();
    for command in [
        "false",
        "python3 -c 'import pathlib,sys; pathlib.Path(sys.argv[1]).write_text(\"\")'",
    ] {
        assert!(matches!(
            run_editor(
                &request,
                command,
                Path::new("/bin/sh"),
                directory.path(),
                65_536
            )
            .unwrap(),
            EditorResult::Failed(_)
        ));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
    assert!(
        run_editor(
            &request,
            "editor",
            Path::new("/missing/shell"),
            directory.path(),
            65_536
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn terminal_handoff_restores_cooked_mode_and_reacquires_after_host_failure() {
    if std::env::var_os("CANONIC_TUI_HANDOFF_CHILD").is_some() {
        let terminal = TerminalSession::open().unwrap();
        let (terminal, result) = terminal.handoff(|| {
            assert!(!crossterm::terminal::is_raw_mode_enabled()?);
            Err::<(), _>(std::io::Error::other("simulated editor failure"))
        });
        assert!(result.is_err());
        let mut terminal = terminal.unwrap();
        assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
        terminal
            .draw(|frame| {
                frame.render_widget(
                    canonic_tui::ratatui::widgets::Paragraph::new("REACQUIRED"),
                    frame.area(),
                );
            })
            .unwrap();
        drop(terminal);
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        return;
    }
    let script = r"
import fcntl,os,pty,select,struct,subprocess,sys,termios,time
master,slave=pty.openpty(); before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',12,60,0,0))
env=dict(os.environ,CANONIC_TUI_HANDOFF_CHILD='1')
child=subprocess.Popen([sys.argv[1],'--exact','composer_host::terminal_handoff_restores_cooked_mode_and_reacquires_after_host_failure','--nocapture'],stdin=slave,stdout=slave,stderr=slave,env=env,start_new_session=True)
output=b''; deadline=time.monotonic()+10
try:
    while child.poll() is None:
        assert time.monotonic()<deadline,output
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    while select.select([master],[],[],0)[0]:output+=os.read(master,65536)
    assert child.returncode==0,output
    assert b'REACQUIRED' in output,output
    os.write(master,b'restored\r'); assert select.select([slave],[],[],1)[0]
    assert os.read(slave,1024)==b'restored\n'
    # macOS clears its transient PENDIN re-echo flag on the first cooked read.
    assert termios.tcgetattr(slave)==before,(before,termios.tcgetattr(slave),output)
finally:
    if child.poll() is None:child.kill();child.wait()
    os.close(master);os.close(slave)
";
    let output = std::process::Command::new("python3")
        .args(["-c", script])
        .arg(std::env::current_exe().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
