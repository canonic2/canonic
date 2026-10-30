//! Native menu acceptance, retry and terminal restoration through Crossterm.
#[cfg(unix)]
#[test]
fn menu_insert_then_invoke_retry_and_exit_restore_the_terminal() {
    let script = r"
import fcntl,os,pty,re,select,struct,subprocess,sys,termios,time
master,slave=pty.openpty();before=termios.tcgetattr(slave)
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
child=subprocess.Popen([sys.argv[1],'--component','completion-menu','--scenario','main','--colors','none','--ascii'],stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
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
    wait_for(b'commands ---')
    os.write(master,b'rev\r')
    wait_for(b'enter runs command',b'commands ---')
    assert b'Host invoked' not in screen() and b'you' not in screen()
    os.write(master,b'123\r');wait_for(b'Host invoked /review 123')
    os.write(master,b'\x1b[21~');wait_for(b'completion-menu')
    os.write(master,b'\x1b[200~error\x1b[201~\r')
    wait_for(b'git ls-files exited 128');wait_for(b'tab retries')
    os.write(master,b'\t');wait_for(b'Repository source retried successfully',b'git ls-files exited 128')
    os.write(master,b'\r');wait_for(b'@src/digest/settings.ts',b'files ---')
    assert b'Literal message accepted' not in screen()
    os.write(master,b'\x1b[21~');wait_for(b'completion-menu')
    os.write(master,b'\x7f'*5+b'\x1b[200~partly-failed\x1b[201~\r')
    wait_for(b'3 commands failed to load');wait_for(b'alt+r retries')
    os.write(master,b'\x1br');wait_for(b'release-tools source retried successfully',b'failed to load')
    assert b'/handoff' in screen()
    os.write(master,b'\x12');wait_for(b'enter recalls')
    os.write(master,b'\x1b');wait_for(b'actions on')
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',12,40,0,0))
    os.write(master,b'\x03\x03\x03')
    # Drain redraw output so a full PTY buffer cannot block the exit.
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
