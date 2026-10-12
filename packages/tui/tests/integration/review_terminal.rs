//! Native review paste, acknowledgement, caret return, resize and terminal cleanup.

#[cfg(unix)]
#[test]
fn question_review_paste_isolated_and_native_modes_restored() {
    let script = r"
import fcntl,os,pty,re,select,signal,struct,subprocess,sys,termios,time
master,slave=pty.openpty();before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',32,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','question-form','--scenario','typed','--colors','none','--ascii'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
output=b''
def screen():
    cells=[[' ']*100 for _ in range(32)];row=column=0
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
            elif command=='J' and values[0]==2:cells=[[' ']*100 for _ in range(32)]
            elif command=='K':
                for x in range(max(0,column),100):
                    if 0<=row<32:cells[row][x]=' '
            continue
        if token=='\r':column=0;continue
        if token=='\n':row+=1;continue
        if ord(token)<32:continue
        if 0<=row<32 and 0<=column<100:cells[row][column]=token
        column+=1
    return '\n'.join(''.join(line) for line in cells)
def wait_for(text):
    global output
    deadline=time.monotonic()+10
    while text not in screen():
        assert child.poll() is None,output[-1000:]
        assert time.monotonic()<deadline,(text,screen())
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for('Digest times')
    os.write(master,b'\x1b[200~native\nanswer\x1b[201~');wait_for('native')
    assert 'sending answers' not in screen()
    os.write(master,b'\x0a');os.write(master,b'\t');wait_for('enter send')
    os.write(master,b'\r');wait_for('sending answers')
    os.write(master,b'\r');os.write(master,b'\x1b[15~');wait_for('answered')
    os.write(master,b'!');wait_for('and bump the app !version')
    os.write(master,b'\x1a');wait_for('and bump the app version')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',20,40,0,0));os.kill(child.pid,signal.SIGWINCH)
    os.write(master,b'\x1b[21~');wait_for('scenarios')
    os.write(master,b'\x03')
    deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline,screen()
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    os.write(master,b'restored\r');assert select.select([slave],[],[],1)[0]
    assert os.read(slave,1024)==b'restored\n'
    assert termios.tcgetattr(slave)==before,(before,termios.tcgetattr(slave))
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

#[cfg(unix)]
#[test]
fn permission_native_confirmation_retry_expiry_and_draft_return_restore_modes() {
    let script = r"
import fcntl,os,pty,re,select,signal,struct,subprocess,sys,termios,time
master,slave=pty.openpty();before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',32,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','permission-review','--scenario','main','--colors','none','--ascii'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
output=b''
def screen():
    cells=[[' ']*100 for _ in range(32)];row=column=0
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
            elif command=='J' and values[0]==2:cells=[[' ']*100 for _ in range(32)]
            elif command=='K':
                for x in range(max(0,column),100):
                    if 0<=row<32:cells[row][x]=' '
            continue
        if token=='\r':column=0;continue
        if token=='\n':row+=1;continue
        if ord(token)<32:continue
        if 0<=row<32 and 0<=column<100:cells[row][column]=token
        column+=1
    return '\n'.join(''.join(line) for line in cells)
def wait_for(text):
    global output
    deadline=time.monotonic()+10
    while text not in screen():
        assert child.poll() is None,output[-1000:]
        assert time.monotonic()<deadline,(text,screen())
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for('nothing chosen')
    os.write(master,b'\x1b[200~1\r\nAllow\x1b[201~');wait_for('paste ignored')
    assert 'sending Allow' not in screen()
    os.write(master,b'2');os.write(master,b'\r');wait_for('Exact host scope')
    os.write(master,b'\r');wait_for('scope not confirmed')
    os.write(master,b'1');assert 'sending Allow' not in screen()
    os.write(master,b'\r');wait_for('sending')
    os.write(master,b'\x1b[17~');wait_for('application failure')
    os.write(master,b'\r');wait_for('press r to retry')
    os.write(master,b'r');wait_for('retry 1')
    os.write(master,b'\x1b[19~');wait_for('expired')
    os.write(master,b'\x1b[15~');assert 'expired' in screen()
    os.write(master,b'\x1b');wait_for('expired, nothing applied')
    os.write(master,b'!');wait_for('and bump the app !version')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',20,40,0,0));os.kill(child.pid,signal.SIGWINCH)
    os.write(master,b'\x1b[21~');wait_for('scenarios')
    os.write(master,b'\x03');deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline,screen()
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    os.write(master,b'restored\r');assert select.select([slave],[],[],1)[0]
    assert os.read(slave,1024)==b'restored\n'
    assert termios.tcgetattr(slave)==before,(before,termios.tcgetattr(slave))
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
