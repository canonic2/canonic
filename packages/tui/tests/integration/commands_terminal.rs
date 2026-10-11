//! Verifies command sequences through the real terminal decoder and lease.

#[cfg(unix)]
#[test]
fn command_sequences_timeout_and_exit_restore_the_terminal() {
    let script = r"
import fcntl,os,pty,re,select,struct,subprocess,sys,termios,time
master,slave=pty.openpty()
before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','commands','--scenario','main','--colors','none'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
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
    wait_for(b'copy handoff')
    os.write(master,b'\x1b[200~/branch\x1b[201~\r')
    wait_for(b'intent: rename')
    os.write(master,b'\x18')
    wait_for('ctrl+x …'.encode())
    wait_for(b'copy handoff')
    wait_for(b'ready')
    os.write(master,b'\x1bOP')
    wait_for(b'everywhere')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',12,40,0,0))
    os.write(master,b'\x03')
    wait_for(b'ctrl+c again closes it')
    assert child.poll() is None
    os.write(master,b'\x03')
    wait_for(b'message ')
    assert child.poll() is None
    os.write(master,b'\x03');child.wait(timeout=5)
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
