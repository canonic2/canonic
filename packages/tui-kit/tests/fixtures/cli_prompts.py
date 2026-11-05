"""Shared prompt filtering, retained Back state, confirmation and exact restoration."""
import os
import sys
from inline_pty import Host

command = [sys.argv[1], "--exact", "runtime::prompts::guided::tests::prompt_child",
           "--ignored", "--nocapture"]
h = Host(args=command)
try:
    h.wait("Agent 0")
    h.send(b"\r", "press space to mark at least 1")
    assert h.child.poll() is None
    h.send(b"9", "Agent 9")
    h.send(b" ", "1 marked")
    # The first Escape clears the filter. The next returns Back. Revisiting
    # the retained prompt keeps the mark and opens a fresh terminal lease.
    h.send(b"\x1b", "Agent 8")
    h.send(b"\x1b", "Select agents → back")
    h.wait("1 marked")
    os.write(h.master, b"\r")
    h.finish()
    assert "PROMPT_OK" in h.all_text(), h.all_text()
    assert h.normal_clears == 0, h.output[-1000:]
finally:
    h.close()
