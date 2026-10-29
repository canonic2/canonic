"""Drive real shared TUI prompts for operator tests and settled-output snapshots.

Live cursor-addressed regions are verified by operator_terminal.py. This fixture
responds to cursor reports and sends one deliberate answer after each first frame.
"""
import fcntl
import json
import os
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 120, 0, 0))
os.set_blocking(master, False)

def terminal():
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

child = subprocess.Popen([sys.argv[1]] + json.loads(sys.argv[2]),
                         stdin=slave, stdout=slave, stderr=subprocess.PIPE,
                         env=dict(os.environ, PATH="/usr/bin:/bin"),
                         preexec_fn=terminal)
os.close(slave)
operator = len(sys.argv) > 4 and sys.argv[4] == "operator"
answers = json.loads(sys.argv[3][1:]) if sys.argv[3].startswith("@") else sys.argv[3].splitlines()
settled = bytearray()
pending = bytearray()
live_text = bytearray()
live = False
awaiting = False
confirm_accept = False
sent = []
frame_answers = {"yes": b"2\r", "no": b"\r", "argv": b"2\r",
                 "allow": b"3\r", "repair": b"3\r", "history": b"4\r",
                 "manual": b"2\r", "review": b"1\r", "leave": b"\x1b",
                 "reject": b"2\r", "accept": b"1\r", "block": b"2\r",
                 "strict": b"2\r", "loose": b"3\r", "recommended": b"1\r",
                 "done": b"\r", "cloud": b"2\r", "off": b"2\r", "back": b"\x1b"}

def key_bytes(answer, view):
    if isinstance(answer, dict):
        return answer["keys"].encode()
    if answer == "back":
        return b"\x1b"
    compact = re.sub(rb"\s+", b"", view)
    if answer.isdigit() and b"tabnext" in compact and b"enterchoose" in compact:
        # Question Form's digits choose a single answer immediately; an extra
        # Enter would leak into the next host step.
        return answer.encode()
    if b"Agentintegrations" in compact:
        return {"claude": b"1\r", "codex": b"2\r", "claude,codex": b"12\r"}.get(answer, b"\r")
    if b"Recordrequesthistory" in compact:
        return {"yes": b"1\r", "no": b"2\r"}.get(answer, b"\r")
    if b"Clear existing history".replace(b" ", b"") in compact:
        return {"yes": b"2\r", "no": b"1\r"}.get(answer, b"\r")
    if b"Reviewpendingproposals" in compact and answer in ("manual", "history", "repair"):
        return {"manual": b"3\r", "repair": b"4\r", "history": b"5\r"}[answer]
    return frame_answers.get(answer, answer.encode() + b"\r")

def parse(chunk):
    global live, awaiting, confirm_accept
    pending.extend(chunk)
    while pending:
        if pending[0] == 27:
            token = re.match(rb"\x1b\[[0-?]*[ -/]*[@-~]", pending)
            if token is None:
                return
            code = token[0]
            del pending[:len(code)]
            if code == b"\x1b[6n":
                os.write(master, b"\x1b[2;1R")
                live_text.clear()
                awaiting = True
            elif code == b"\x1b[?2004h":
                live = True
            elif code == b"\x1b[?2004l":
                live = False
            elif code in (b"\x1b[?25l", b"\x1b[?25h") and live and live_text.strip():
                explicit = answers and isinstance(answers[0], dict) and answers[0].get("wait")
                matches = explicit and re.sub(rb"\s+", b"", explicit.encode()) in re.sub(rb"\s+", b"", live_text)
                if (awaiting and not explicit) or matches:
                    assert answers, ("unexpected prompt", live_text.decode(errors="replace"))
                    answer = answers.pop(0)
                    sent.append((answer, live_text.decode(errors="replace")[:100]))
                    os.write(master, key_bytes(answer, live_text))
                    confirm_accept = answer == "accept"
                    awaiting = False
                    live_text.clear()
                elif confirm_accept and b"Thischanges" in re.sub(rb"\s+", b"", live_text):
                    os.write(master, b"2\r")
                    confirm_accept = False
            continue
        byte = pending.pop(0)
        if live:
            live_text.append(byte)
        else:
            settled.append(byte)

def drain(timeout):
    if select.select([master], [], [], timeout)[0]:
        try:
            chunk = os.read(master, 65536)
        except (BlockingIOError, OSError):
            return False
        parse(chunk)
        return bool(chunk)
    return False

try:
    deadline = time.monotonic() + 20
    while child.poll() is None:
        assert time.monotonic() < deadline, (settled.decode(errors="replace"), live_text.decode(errors="replace"), sent, answers)
        drain(0.05)
    while drain(0.05):
        pass
    _, error = child.communicate(timeout=5)
    if not operator:
        assert not answers, ("unused answers", answers, settled.decode(errors="replace"), error.decode())
    print(json.dumps(dict(code=child.returncode, terminal=settled.decode(errors="replace"),
                          stdout=settled.decode(errors="replace") if operator else "", stderr=error.decode())))
finally:
    if child.poll() is None:
        child.kill()
    os.close(master)
    child.wait(timeout=5)
