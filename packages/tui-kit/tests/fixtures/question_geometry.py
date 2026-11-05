"""Five terminal rows refuse a hidden answer while keeping cancellation usable."""
import os
import sys
from inline_pty import Host

command = [sys.argv[1], '--exact', 'runtime::prompts::guided::tests::question_geometry_child',
           '--ignored', '--nocapture']
h = Host(args=command)
try:
    h.wait('Safe answer')
    h.resize(80, 5)
    h.wait('enlarge terminal')
    assert 'Safe answer' not in h.screen(), h.screen()
    # FIFO input: a hidden choice must not submit before Escape cancels.
    os.write(h.master, b'2\x1b')
    h.finish()
    assert 'QUESTION_OK' in h.all_text(), h.all_text()
finally:
    h.close()
