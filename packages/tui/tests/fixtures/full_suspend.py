"""Verify external/key full-screen suspension under an owned shell-like session."""
import os, signal, subprocess, sys
from inline_pty import Host

if len(sys.argv) == 2:
    for case in ['key', 'external', 'unsupported']:
        subprocess.run([sys.executable, '-B', __file__, sys.argv[1], case], check=True, timeout=30)
    sys.exit(0)

os.setsid()
signal.signal(signal.SIGHUP, signal.SIG_IGN)
signal.signal(signal.SIGTTOU, signal.SIG_IGN)
h = Host(args=[sys.argv[1], '--component', 'composer', '--scenario', 'box-draft', '--colors', 'none', '--ascii'], controlling=sys.argv[2] != 'unsupported')
try:
    h.wait('then tag a release candidate')
    h.send(b'!', 'then tag a release candidat!e')
    if sys.argv[2] == 'unsupported':
        h.send(b'\x1b\x1a', 'suspend unavailable')
    else:
        if sys.argv[2] == 'external': os.kill(h.child.pid, signal.SIGTSTP)
        else: os.write(h.master, b'\x1b\x1a')
        h.wait_stopped()
        other = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'], preexec_fn=os.setpgrp)
        try:
            os.tcsetpgrp(h.slave, other.pid)
            before = h.output
            os.kill(h.child.pid, signal.SIGCONT)
            h.wait_stopped(shell_input=False)
            assert h.output == before
        finally:
            os.tcsetpgrp(h.slave, os.getpgrp())
            other.terminate(); other.wait(timeout=5)
        os.write(h.slave, b'SHELL SENTINEL\n'); h.drain()
        h.resize(80, 24)
        os.kill(h.child.pid, signal.SIGCONT)
        h.wait_mode(True); h.wait('then tag a release candidat!e')
        assert h.saved[0] is not None
        normal = h.history + [''.join(row) for row in h.saved[0]]
        assert 'SHELL SENTINEL' in '\n'.join(normal)
        # Ordinary Ctrl-Z remains editor undo after return.
        h.send(b'\x1a', 'then tag a release candidate')
        h.send(b'?', 'then tag a release candidat?e')
        os.kill(h.child.pid, signal.SIGTSTP)
        h.wait_stopped()
        os.kill(h.child.pid, signal.SIGCONT)
        h.wait_mode(True); h.wait('then tag a release candidat?e')
    # Leave the live composer through scenario navigation, then quit the catalog.
    os.write(h.master, b'\x1b[21~'); h.wait('scenarios')
    os.write(h.master, b'\x1b'); h.drain(.2)
    os.write(h.master, b'\x03'); h.finish()
finally: h.close()
