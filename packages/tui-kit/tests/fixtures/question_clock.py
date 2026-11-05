"""Question editor clocks advance while idle, receiving events and suspended."""
import os
import signal
import subprocess
import sys
import time
from inline_pty import Host

if len(sys.argv) == 2:
    for case in ['idle', 'busy', 'suspended']:
        subprocess.run([sys.executable, '-B', __file__, sys.argv[1], case], check=True, timeout=20)
    sys.exit(0)

os.setsid()
signal.signal(signal.SIGHUP, signal.SIG_IGN)
signal.signal(signal.SIGTTOU, signal.SIG_IGN)
command = [sys.argv[1], '--exact', 'runtime::prompts::guided::tests::question_clock_child',
           '--ignored', '--nocapture']
h = Host(args=command, controlling=True)
try:
    h.wait('Enter a target')
    if sys.argv[2] == 'suspended':
        os.write(h.master, b'\x1b\x1a')
        h.wait_stopped()
    # Exceed the editor notice's lifetime, including when no host ticks can run.
    until = time.monotonic() + 3.3
    while time.monotonic() < until:
        assert h.child.poll() is None, h.output[-2000:]
        if sys.argv[2] == 'busy':
            h.resize(100, 32)
        h.drain(.02)
    if sys.argv[2] == 'suspended':
        h.cells = h.blank()
        os.kill(h.child.pid, signal.SIGCONT)
        h.wait('Enter a target')
    h.send(b'\x1b', '1–9 choose')
    os.write(h.master, b'\x1b')
    h.finish()
    assert 'CLOCK_OK' in h.all_text(), h.all_text()
finally:
    h.close()
