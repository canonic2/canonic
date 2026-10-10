//! Exercises nested dialogs and restored terminal modes through an owned native PTY.

#[cfg(unix)]
#[test]
fn nested_dialog_paste_resize_and_exit_restore_terminal() {
    let script = r"
import fcntl,os,pty,re,select,signal,struct,subprocess,sys,termios,time,unicodedata
master,slave=pty.openpty()
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
before=termios.tcgetattr(slave)
child=subprocess.Popen([sys.argv[1],'--component','dialog'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
output=b''
def screen():
    cells=[[' ']*100 for _ in range(28)];row=column=0
    for match in re.finditer(r'\x1b\[[0-?]*[ -/]*[@-~]|.',output.decode(errors='replace'),re.S):
        token=match.group()
        if token.startswith('\x1b['):
            args=token[2:-1];command=token[-1]
            if args.startswith('?'):continue
            values=[int(v or 0) for v in args.split(';')]
            if command in 'Hf':row=(values[0] or 1)-1;column=(values[1] if len(values)>1 else 1)-1
            elif command=='G':column=(values[0] or 1)-1
            elif command=='C':column+=values[0] or 1
            elif command=='D':column-=values[0] or 1
            elif command=='J' and values[0]==2:cells=[[' ']*100 for _ in range(28)]
            elif command=='K' and 0<=row<28:
                for x in range(max(0,column),100):cells[row][x]=' '
            continue
        if token=='\r':column=0;continue
        if token=='\n':row+=1;continue
        if ord(token)<32 or unicodedata.combining(token):continue
        if 0<=row<28 and 0<=column<100:cells[row][column]=token
        column+=2 if unicodedata.east_asian_width(token) in 'WF' else 1
    return '\n'.join(''.join(line) for line in cells)
def wait_for(text):
    global output
    deadline=time.monotonic()+10
    while text not in screen():
        assert time.monotonic()<deadline,screen()
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for('Draft prompt')
    os.write(master,b'\x07\x1b[200~PARENT ALPHA 42\x1b[201~\x1bOR');wait_for('Nested prompt')
    os.write(master,b'\x07\x1b[200~CHILD OMEGA 93\x1b[201~\r');wait_for('Prompt result: CHILD OMEGA 93')
    wait_for('PARENT ALPHA 42')
    os.write(master,b'\r');wait_for('Prompt result: PARENT ALPHA 42')
    wait_for('Background draft one');wait_for('Background draft two')
    os.write(master,b'\x1bOR');wait_for('Nested prompt')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',8,15,0,0));os.kill(child.pid,signal.SIGWINCH)
    os.write(master,b'\x03')
    deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline,'dialog gallery did not exit'
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    os.write(master,b'restored\r');assert select.select([slave],[],[],1)[0]
    assert os.read(slave,1024)==b'restored\n';assert termios.tcgetattr(slave)==before
    while select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert b'\x1b[?1049l' in output
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
