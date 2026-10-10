//! Verifies delayed acknowledgements through actual terminal input and restoration.

#[cfg(unix)]
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One owned PTY trace exercises acknowledgements, history navigation and restoration through the same terminal decoder"
)]
fn composer_pending_edit_accept_reject_and_reset_restore_terminal_modes() {
    let script = r"
import fcntl,os,pty,re,select,struct,subprocess,sys,termios,time
master,slave=pty.openpty()
before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','composer','--scenario','main','--colors','none'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
output=b''
def screen():
    cells=[[' ']*100 for _ in range(28)]
    row=column=0
    for match in re.finditer(r'\x1b\[[0-?]*[ -/]*[@-~]|.',output.decode(errors='replace'),re.S):
        token=match.group()
        if token.startswith('\x1b['):
            args=token[2:-1];command=token[-1]
            if args.startswith(('?', '>', '<')):continue
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
def wait_for(text,absent=None):
    global output
    deadline=time.monotonic()+10
    while text not in screen() or (absent is not None and absent in screen()):
        assert child.poll() is None,output[-2000:]
        assert time.monotonic()<deadline,(text,screen())
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for(b'0 submission intents')
    os.write(master,b'\x1b[200~original\x1b[201~\r\r')
    wait_for(b'1 submission intents')
    wait_for(b'20:10')
    wait_for(b'sending')
    os.write(master,b'\x1b[200~ newer\x1b[201~\x1b[15~')
    wait_for(b'Host accepted')
    wait_for(b' newer')
    os.write(master,b'\x1b[15~\r\x1b[17~')
    wait_for(b'not sent')
    wait_for(b'2 submission intents')
    os.write(master,b'\x1b[200~retry\x1b[201~\r\x1b[19~')
    wait_for(b'3 submission intents')
    wait_for(b'Stale response ignored')
    wait_for(b'sending')
    os.write(master,b'\x1b[18~\x1b[19~')
    wait_for(b'Replacement draft')
    os.write(master,b'\x1b[21~')
    wait_for(b'composer \xc2\xb7 scenarios')
    os.write(master,b'\x1b[21~')
    wait_for(b'Replacement draft',b'composer \xc2\xb7 scenarios')
    os.write(master,b'\x1b[21~\x1b[200~up-key\x1b[201~\r')
    wait_for(b'composer / up-key')
    wait_for(b'and bump the app version')
    os.write(master,b'\x1b[21~')
    wait_for(b'Search: up-key')
    wait_for(b'composer \xc2\xb7 scenarios')
    os.write(master,b'\x7f'*6+b'\x1b[200~sending\x1b[201~\r')
    wait_for(b'Run the digest tests too.')
    wait_for(b'1 submission intents')
    wait_for(b'sending')
    os.write(master,b'\x1b[21~\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x1b[200~box-history\x1b[201~\r')
    wait_for(b'3 of 12')
    wait_for(b'why does the digest skip DST weeks?')
    os.write(master,b'\x1b[A')
    wait_for(b'4 of 12',b'3 of 12')
    os.write(master,b'\x1b[B')
    wait_for(b'3 of 12',b'4 of 12')
    os.write(master,b'\x1b[B\x1b[B\x1b[B')
    wait_for(b'then tag a release candidate',b'1 of 12')
    os.write(master,b'\x1b[200~!\x1b[201~')
    wait_for(b'then tag a release candidat!e')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',12,40,0,0))
    os.write(master,b'\x03\x03\x03')
    deadline=time.monotonic()+5
    # A terminal consumes redraws while the application exits. Drain the owned
    # PTY too, so resize output cannot fill its buffer and block the next key.
    while child.poll() is None:
        assert time.monotonic()<deadline,screen()
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    os.write(master,b'restored\r')
    assert select.select([slave],[],[],1)[0]
    assert os.read(slave,1024)==b'restored\n'
    assert termios.tcgetattr(slave)==before
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
