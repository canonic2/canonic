//! Native status/notice focus, acknowledged actions, resize and terminal restoration.
#[cfg(unix)]
#[test]
fn status_native_focus_actions_resize_and_modes_are_owned() {
    let script = r"
import fcntl,os,pty,re,select,signal,struct,subprocess,sys,termios,time
master,slave=pty.openpty();before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',32,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','status-panels-and-notices','--scenario','main','--colors','none','--ascii'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
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
def send(data,text):
    os.write(master,data);wait_for(text)
try:
    wait_for('and bump the app version')
    send(b'\x18i','Status');send(b'\x1b','enter send')
    send(b'!','and bump the app versio!n');send(b'\x1a','and bump the app version')
    send(b'\x18n','Notices');send(b'\r','waiting for host')
    send(b'\x1b[17~','Host could not reconnect');send(b'\r','waiting for host')
    send(b'\x1b[15~','Context nearing limit');send(b'\x1b','enter send')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',20,40,0,0));os.kill(child.pid,signal.SIGWINCH)
    wait_for('! 0 42%')
    send(b'\x18i','Context');send(b'\x1b','enter send')
    send(b'\x1b[21~','status-panels-and-notices');os.write(master,b'\x03')
    deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline,output[-1000:]
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    os.write(master,b'restored\r');assert select.select([slave],[],[],1)[0];assert os.read(slave,1024)==b'restored\n'
    assert termios.tcgetattr(slave)==before,(before,termios.tcgetattr(slave))
finally:
    if child.poll() is None:child.kill();child.wait()
    os.close(master);os.close(slave)
";
    let output = std::process::Command::new("python3")
        .args(["-c", script])
        .arg(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .output()
        .expect("valid status fixture or operation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
