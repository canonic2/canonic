"""Exercise Testrun's real signing adapter in owned PTYs, without host identities."""
import fcntl
import os
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time

for mode, answer in [
    ("saved", b"\r"),
    ("replace", b"\x1b[B\r"),
    ("number", b"2"),
    ("narrow", b"\x1b[B\r"),
    ("rgb", b"\r"),
    ("ascii", b"\r"),
    ("first", b"\r"),
    ("escape", b"\x1b"),
    ("ctrl-c", b"\x03"),
    ("ctrl-d", b"\x04"),
    ("unsupported", b""),
]:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 32 if mode == "narrow" else 120, 0, 0))
    original = termios.tcgetattr(slave)
    environment = dict(os.environ, TESTRUN_SIGNING_PROMPT_PROBE=mode,
                       TERM="dumb" if mode == "unsupported" else "xterm-256color", NO_COLOR="1")
    if mode == "rgb":
        environment.pop("NO_COLOR")
        environment["COLORTERM"] = "truecolor"
        environment["LC_ALL"] = "en_US.UTF-8"
    if mode == "ascii":
        environment["LC_ALL"] = "C"
    child = subprocess.Popen(
        [sys.argv[1], "--exact", "cli::setup::tests::shared_terminal_signing_selection", "--nocapture"],
        stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
        env=environment,
    )
    output = bytearray()
    deadline = time.monotonic() + 10
    sent = mode == "unsupported"
    try:
        while child.poll() is None:
            assert time.monotonic() < deadline, (mode, output.decode(errors="replace"))
            if not select.select([master], [], [], 0.05)[0]:
                continue
            chunk = os.read(master, 65536)
            output.extend(chunk)
            for _ in range(chunk.count(b"\x1b[6n")):
                os.write(master, b"\x1b[2;1R")
            visible = re.sub(rb"\s+", b"", re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", output))
            if not sent and b"Chooseasigningteam" in visible:
                os.write(master, answer)
                sent = True
                if mode in ("first", "number"):
                    # Neither empty Enter nor a number alone submits.
                    end = time.monotonic() + 0.2
                    while time.monotonic() < end:
                        if select.select([master], [], [], 0.02)[0]:
                            output.extend(os.read(master, 65536))
                        assert child.poll() is None, output.decode(errors="replace")
                    os.write(master, b"\x1b[B\r" if mode == "first" else b"\r")
        assert child.wait(timeout=5) == 0, (mode, output.decode(errors="replace"))
        assert sent, mode
        if mode == "rgb":
            assert b"\x1b[38;2;" in output, output.decode(errors="replace")
        if mode == "ascii":
            assert "─".encode() not in output, output.decode(errors="replace")
        termios.tcflush(slave, termios.TCIFLUSH)
        os.write(master, b"restored\r")
        assert select.select([slave], [], [], 1)[0], mode
        assert os.read(slave, 1024) == b"restored\n", mode
        assert termios.tcgetattr(slave) == original, mode
    finally:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=5)
        os.close(master)
        os.close(slave)
