"""Check real TTY output profiles and machine envelopes without host resources."""
import fcntl
import json
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

for mode in ("human", "no-color", "json", "json-error"):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 40, 0, 0))
    original = termios.tcgetattr(slave)
    environment = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", LC_ALL="en_US.UTF-8")
    environment.pop("NO_COLOR", None)
    if mode == "no-color":
        environment["NO_COLOR"] = "1"
    args = ["mcp", "serve", "--check"]
    if mode.startswith("json"):
        args = ["--json"] + args
    if mode == "json-error":
        args = ["--json", "--unknown-acme-flag"]
    child = subprocess.Popen([sys.argv[1]] + args, stdin=slave, stdout=slave,
                             stderr=subprocess.PIPE, env=environment)
    output = bytearray()
    deadline = time.monotonic() + 10
    try:
        while child.poll() is None or select.select([master], [], [], 0)[0]:
            assert time.monotonic() < deadline, (mode, output.decode(errors="replace"))
            if select.select([master], [], [], 0.05)[0]:
                output.extend(os.read(master, 65536))
        error = child.stderr.read()
        if mode == "json-error":
            assert child.returncode == 2 and not output
            assert json.loads(error)["error"]["code"] == "INVALID_ARGUMENT"
        else:
            assert child.returncode == 0 and not error, (mode, error)
            if mode == "json":
                assert json.loads(output)["ok"] is True
                assert b"\x1b" not in output
            elif mode == "no-color":
                assert b"\x1b" not in output
                assert b"MCP registration" in output
            else:
                assert b"\x1b[38;2;" in output, output.decode(errors="replace")
        assert termios.tcgetattr(slave) == original, mode
    finally:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=5)
        child.stderr.close()
        os.close(master)
        os.close(slave)
