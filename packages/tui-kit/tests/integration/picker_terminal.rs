//! Checks the actual emitted ANSI styles independently of terminal profile rendering.

#[cfg(unix)]
#[test]
fn picker_emits_default_bold_labels_and_bold_titles_in_every_color_mode() {
    let script = r"
import fcntl,os,pty,re,select,struct,subprocess,sys,termios,time,unicodedata
for mode in ['ansi16','ansi256','truecolor','none']:
    master,slave=pty.openpty()
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
    child=subprocess.Popen([sys.argv[1],'--component','picker','--scenario','main','--colors',mode],stdin=slave,stdout=slave,stderr=slave)
    output=b''
    def screen():
        cells=[[(' ',None)]*100 for _ in range(28)]
        row=column=0;fg='default';bold=dim=reverse=False
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
                elif command=='J' and values[0]==2:cells=[[(' ',None)]*100 for _ in range(28)]
                elif command=='m':
                    i=0
                    while i<len(values):
                        v=values[i]
                        if v==0:fg='default';bold=dim=reverse=False
                        elif v==1:bold=True
                        elif v==2:dim=True
                        elif v==22:bold=dim=False
                        elif v==7:reverse=True
                        elif v==27:reverse=False
                        elif v==39:fg='default'
                        elif 30<=v<=37 or 90<=v<=97:fg=v
                        elif v in (38,48,58):
                            count=4 if values[i+1]==2 else 2
                            if v==38:fg=tuple(values[i+1:i+count+1])
                            i+=count
                        i+=1
                continue
            if token=='\r':column=0;continue
            if token=='\n':row+=1;continue
            if ord(token)<32 or unicodedata.combining(token):continue
            if 0<=row<28 and 0<=column<100:cells[row][column]=(token,(fg,bold,dim,reverse))
            column+=2 if unicodedata.east_asian_width(token) in 'WF' else 1
        return cells
    def find(text):
        for line in screen():
            at=''.join(c[0] for c in line).find(text)
            if at>=0:return line[at:at+len(text)]
        return None
    def wait_for(text,selected=False):
        global output
        deadline=time.monotonic()+10
        while True:
            cells=find(text)
            if cells and (not selected or all(c[1][1] for c in cells)):return cells
            assert child.poll() is None,(mode,output[-1000:])
            assert time.monotonic()<deadline,(mode,text,output[-1000:])
            if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    def check_label(text):
        cells=wait_for(text,True)
        assert all(c[1][0]=='default' and c[1][1] and not c[1][2] for c in cells),(mode,cells)
        assert all(c[1][3]==(mode in ['ansi16','none']) for c in cells),(mode,cells)
    try:
        wait_for('! needs you')
        assert all(c[1][1] and not c[1][2] for c in wait_for('github server'))
        check_label('Continue without this server')
        os.write(master,b'\x1b[H');check_label('Use this server')
        os.write(master,b'\x1b[F');check_label('Continue without this server')
        os.write(master,b'\x03')
        # Drain redraw output so a full PTY buffer cannot block the exit.
        deadline=time.monotonic()+5
        while child.poll() is None:
            assert time.monotonic()<deadline,(mode,output[-1000:])
            if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
        assert child.returncode==0
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
