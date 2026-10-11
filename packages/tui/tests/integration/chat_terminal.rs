//! Native Chat search, selection, copy and terminal restoration through Crossterm.
#[cfg(unix)]
#[test]
fn delayed_copy_and_link_refusal_preserve_the_draft_in_a_native_terminal() {
    let script = r"
import fcntl,os,pty,re,select,struct,subprocess,sys,termios,time
for scenario,pending,failed,success,retry in [
    ('copy-pending',b'copying',b'clipboard unavailable',b'copied Markdown',b'y'),
    ('link-pending',b'opening https://example.com/reference',b'not opened',b'opened https://example.com/reference',b'o')]:
    master,slave=pty.openpty();before=termios.tcgetattr(slave)
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,120,0,0))
    child=subprocess.Popen([sys.argv[1],'--component','chat','--scenario',scenario,'--colors','none','--ascii'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
    output=b''
    def screen():
        cells=[[' ']*120 for _ in range(28)];row=column=0
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
                elif command=='J' and values[0]==2:cells=[[' ']*120 for _ in range(28)]
                elif command=='K' and 0<=row<28:
                    start=0 if values[0] in (1,2) else max(0,column)
                    end=min(120,column+1) if values[0]==1 else 120
                    for x in range(start,end):cells[row][x]=' '
                continue
            if token=='\r':column=0;continue
            if token=='\n':row+=1;continue
            if ord(token)<32:continue
            if 0<=row<28 and 0<=column<120:cells[row][column]=token
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
        wait_for(pending);wait_for(b'Independent draft')
        os.write(master,b'\x1b[17~');wait_for(failed)
        os.write(master,retry);wait_for(pending,failed)
        os.write(master,b'\x1b[15~');wait_for(success,pending)
        os.write(master,b'\x1b');time.sleep(0.08)
        os.write(master,b'\x1b[D!');wait_for(b'care!t')
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',12,40,0,0))
        os.write(master,b'\x03');deadline=time.monotonic()+5
        while child.poll() is None:
            assert time.monotonic()<deadline,output[-2000:]
            if select.select([master],[],[],0.05)[0]:output+=os.read(master,65536)
        assert child.returncode==0
        os.write(master,b'restored\r');assert select.select([slave],[],[],1)[0]
        assert os.read(slave,1024)==b'restored\n'
        assert termios.tcgetattr(slave)==before,(scenario,before,termios.tcgetattr(slave))
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
fn search_selection_copy_picker_return_and_exit_restore_the_terminal() {
    let script = r"
import fcntl,os,pty,re,select,struct,subprocess,sys,termios,time
master,slave=pty.openpty();before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','chat','--scenario','select-code','--colors','none','--ascii'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
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
def wait_for(text,absent=None):
    global output
    deadline=time.monotonic()+10
    while text not in screen() or (absent is not None and absent in screen()):
        assert child.poll() is None,output[-2000:]
        assert time.monotonic()<deadline,(text,screen())
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for(b'select')
    os.write(master,b'c');wait_for(b'copied code')
    os.write(master,b'\x1b');wait_for(b'actions on')
    os.write(master,b'abc\x1b[D\x06');wait_for(b'search')
    os.write(master,b'timezone');wait_for(b'1 of')
    os.write(master,b'\x1b[B\x1b');wait_for(b'actions on')
    os.write(master,b'!');wait_for(b'ab!c')
    os.write(master,b'\x1b[21~');wait_for(b'main')
    os.write(master,b'\x1b[21~');wait_for(b'ab!c')
    assert b'\x1b[?1000h' not in output and b'\x1b[?1002h' not in output
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',12,40,0,0))
    os.write(master,b'\x03');deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline,screen()
        if select.select([master],[],[],0.05)[0]:output+=os.read(master,65536)
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
