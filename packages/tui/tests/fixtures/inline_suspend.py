"""Own a controlling PTY and verify stopped-state cleanup before foreground resume."""
import os, select, signal, subprocess, sys, termios, time
from inline_pty import Host

if len(sys.argv) == 2:
    for case in ['success', 'external', 'failure', 'unsupported']:
        subprocess.run([sys.executable, '-B', __file__, sys.argv[1], case], check=True, timeout=30)
    sys.exit(0)

# Own a shell-like session independently of the gallery's lifetime.
os.setsid()
signal.signal(signal.SIGHUP, signal.SIG_IGN)
signal.signal(signal.SIGTTOU, signal.SIG_IGN)

def stopped(h, shell_input=True):
    h.wait_stopped(shell_input)

def suspend(h):
    if sys.argv[2] == 'external': os.kill(h.child.pid, signal.SIGTSTP)
    else: os.write(h.master, b'\x1b\x1a')

if sys.argv[2] in ['success', 'external']:
    scenario = 'streaming'
else:
    scenario = 'main'
h = Host(scenario, controlling=sys.argv[2] != 'unsupported')
try:
    if sys.argv[2] == 'unsupported':
        h.wait('Deploy')
        h.send(b'\x1b\x1a', 'suspend unavailable')
        os.write(h.master, b'\x03'); h.finish()
        sys.exit(0)
    if sys.argv[2] == 'failure':
        h.wait('Deploy'); os.write(h.master, b'\x1b\x1a'); stopped(h)
        h.resize(18, 4)
        os.kill(h.child.pid, signal.SIGCONT)
        deadline = time.monotonic() + 10
        while h.child.poll() is None:
            assert time.monotonic() < deadline, h.output[-1000:]
            h.drain()
        h.drain()
        assert h.child.returncode != 0
        assert b'window too small' in h.output, h.output[-1000:]
        assert b'preview \xe2\x86\x92' not in h.output
        os.write(h.master, b'failure-input\r')
        assert select.select([h.slave], [], [], 1)[0]
        assert os.read(h.slave, 1024) == b'failure-input\n'
        assert termios.tcgetattr(h.slave) == h.before
        assert h.saved is None
        sys.exit(0)
    h.wait('and bump the app version')
    h.send(b'!', 'and bump the app versio!n')
    suspend(h)
    stopped(h)
    other = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'], preexec_fn=os.setpgrp)
    try:
        os.tcsetpgrp(h.slave, other.pid)
        before = h.output
        os.kill(h.child.pid, signal.SIGCONT)
        stopped(h, shell_input=False)
        assert h.output == before, h.output[-1000:]
    finally:
        os.tcsetpgrp(h.slave, os.getpgrp())
        other.terminate(); other.wait(timeout=5)
    # Simulate output and resize while the shell owns restored terminal modes.
    os.write(h.slave, b'SHELL SENTINEL\n')
    h.resize(80, 24)
    os.kill(h.child.pid, signal.SIGCONT)
    h.wait('and bump the app versio!n')
    assert 'SHELL SENTINEL' in h.all_text(), h.all_text()
    h.send(b'\x18o', 'sessions')
    suspend(h)
    stopped(h)
    os.kill(h.child.pid, signal.SIGCONT)
    h.wait_mode(True); h.wait('sessions')
    os.write(h.master, b'\x1b')
    h.wait_mode(False); h.wait('and bump the app versio!n')
    os.write(h.master, b'\x1b[H\x0b'); h.drain(.2)
    os.write(h.master, b'\x03'); h.drain(.2)
    if h.child.poll() is None: os.write(h.master, b'\x03')
    h.finish()
finally: h.close()
