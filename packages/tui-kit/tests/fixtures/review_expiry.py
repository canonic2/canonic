"""Real guided review confirmations expire while idle, busy and suspended."""
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
command = [sys.argv[1], '--exact', 'runtime::prompts::guided::tests::review_expiry_child',
           '--ignored', '--nocapture']
h = Host(args=command, controlling=True)
try:
    h.wait('Delete Acme')
    h.send(b'1', 'Delete Acme')
    h.send(b'\r', 'armed · enter again within 3 s')
    if sys.argv[2] == 'suspended':
        os.write(h.master, b'\x1b\x1a')
        h.wait_stopped()
        # Deliberately exceed the confirmation lifetime while no host ticks run.
        until = time.monotonic() + 3.2
        while time.monotonic() < until:
            h.drain(.05)
        h.cells = h.blank()
        os.kill(h.child.pid, signal.SIGCONT)
        h.wait('Delete Acme')
        deadline = time.monotonic() + 1.5
    else:
        deadline = time.monotonic() + 4.5
    while 'armed · enter again within 3 s' in h.screen():
        assert h.child.poll() is None, h.output[-2000:]
        assert time.monotonic() < deadline, (sys.argv[2], h.screen())
        if sys.argv[2] == 'busy':
            h.resize(100, 32)
        h.drain(.02)
    # Expiry keeps the review open. The next Enter must arm it afresh;
    # only a second deliberate Enter inside the new window may submit.
    assert h.child.poll() is None
    h.send(b'\r', 'armed · enter again within 3 s')
    assert h.child.poll() is None
    os.write(h.master, b'\r')
    h.finish()
    assert 'REVIEW_OK' in h.all_text(), h.all_text()
finally:
    h.close()
