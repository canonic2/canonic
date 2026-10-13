"""Owned VT subset for cursor reports, normal scrollback and alternate-screen return."""
import fcntl, os, pty, re, select, signal, struct, subprocess, sys, termios, time

class Host:
    def __init__(self, scenario=None, report_cursor=True, args=None, env_extra=None, controlling=False):
        self.master, self.slave = pty.openpty()
        self.before = termios.tcgetattr(self.slave)
        self.width, self.height = 100, 32
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 100, 0, 0))
        self.cells = self.blank(); self.history = []; self.saved = None
        self.row = self.col = 0; self.pending = ''; self.output = b''; self.normal_clears = 0
        self.report_cursor = report_cursor
        env = dict(os.environ, TERM='xterm-256color'); env.update(env_extra or {})
        command = args or [sys.argv[1], '--inline', '--component', 'inline-composition', '--scenario', scenario, '--colors', 'none', '--ascii']
        # The fixture's session survives the gallery, like a shell's session does.
        if controlling: fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)
        self.child = subprocess.Popen(command, stdin=self.slave, stdout=self.slave, stderr=self.slave, start_new_session=not controlling, env=env)
    def blank(self): return [[' '] * self.width for _ in range(self.height)]
    def newline(self):
        self.row += 1
        if self.row >= self.height:
            if self.saved is None: self.history.append(''.join(self.cells[0]))
            self.cells.pop(0); self.cells.append([' '] * self.width); self.row = self.height - 1
    def parse(self, data):
        self.output += data; self.pending += data.decode(errors='replace')
        while self.pending:
            if self.pending.startswith('\x1b['):
                m = re.match(r'\x1b\[[0-?]*[ -/]*[@-~]', self.pending)
                if not m: return
                token = m.group(); self.pending = self.pending[len(token):]
                args, cmd = token[2:-1], token[-1]
                if args == '6' and cmd == 'n':
                    if self.report_cursor: os.write(self.master, f'\x1b[{self.row+1};{self.col+1}R'.encode())
                    continue
                if args == '?1049' and cmd == 'h':
                    self.saved = (self.cells, self.row, self.col); self.cells = self.blank(); self.row = self.col = 0; continue
                if args == '?1049' and cmd == 'l':
                    if self.saved: self.cells, self.row, self.col = self.saved; self.saved = None
                    continue
                if args.startswith(('?', '>', '<')): continue
                ns = [int(v or 0) for v in args.split(';')]
                if cmd in 'Hf': self.row = min((ns[0] or 1)-1, self.height-1); self.col = (ns[1] if len(ns)>1 else 1)-1
                elif cmd == 'G': self.col = (ns[0] or 1)-1
                elif cmd == 'C': self.col += ns[0] or 1
                elif cmd == 'D': self.col = max(0, self.col-(ns[0] or 1))
                elif cmd == 'J' and ns[0] == 2:
                    if self.saved is None: self.normal_clears += 1
                    self.cells = self.blank()
                elif cmd == 'K':
                    start = 0 if ns[0] in (1,2) else self.col
                    end = self.col+1 if ns[0] == 1 else self.width
                    self.cells[self.row][start:end] = [' '] * (end-start)
                continue
            if self.pending == '\x1b': return
            char, self.pending = self.pending[0], self.pending[1:]
            if char == '\r': self.col = 0
            elif char == '\n': self.newline()
            elif ord(char) >= 32:
                if self.col >= self.width: self.col = 0; self.newline()
                self.cells[self.row][self.col] = char; self.col += 1
    def screen(self): return '\n'.join(''.join(r) for r in self.cells)
    def all_text(self): return '\n'.join(self.history) + '\n' + self.screen()
    def drain(self, timeout=.1):
        if select.select([self.master], [], [], timeout)[0]:
            data = os.read(self.master,65536)
            self.parse(data)
            return bool(data)
        return False
    def wait(self, text):
        deadline = time.monotonic()+10
        while text not in self.screen():
            assert self.child.poll() is None, self.output[-1000:]
            assert time.monotonic()<deadline, (text, self.screen(),self.output[-1000:])
            self.drain()
    def send(self, data, text): os.write(self.master,data); self.wait(text)
    def wait_mode(self, alternate):
        deadline = time.monotonic()+10
        while (self.saved is not None) != alternate:
            assert self.child.poll() is None,self.output[-1000:]
            assert time.monotonic()<deadline,self.screen()
            self.drain()
    def resize(self, width, height):
        self.width, self.height = width,height
        # Preserve terminal contents rather than clearing them. Production emulators may reflow.
        old = self.cells; self.cells = self.blank()
        if self.saved is None and len(old) > height:
            self.history.extend(''.join(row) for row in old[:-height])
        for y,row in enumerate(old[-height:]): self.cells[y][:min(len(row),width)] = row[:width]
        self.row = min(self.row,height-1); self.col = min(self.col,width-1)
        fcntl.ioctl(self.slave,termios.TIOCSWINSZ,struct.pack('HHHH',height,width,0,0)); os.kill(self.child.pid,signal.SIGWINCH)
    def finish(self):
        deadline = time.monotonic()+5
        while self.child.poll() is None:
            assert time.monotonic()<deadline,self.output[-1000:]
            self.drain()
        while select.select([self.master],[],[],.05)[0]:
            if not self.drain(0): break
        assert self.child.returncode == 0, self.output[-1000:]
        assert self.normal_clears==0, self.output
        assert self.saved is None
        os.write(self.master,b'restored\r'); assert select.select([self.slave],[],[],1)[0]
        assert os.read(self.slave,1024)==b'restored\n'
        assert termios.tcgetattr(self.slave)==self.before,(self.before,termios.tcgetattr(self.slave),self.output[-2000:])
    def close(self):
        if self.child.poll() is None: self.child.kill(); self.child.wait()
        os.close(self.master); os.close(self.slave)
