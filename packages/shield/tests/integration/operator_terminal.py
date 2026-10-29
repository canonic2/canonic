"""Owned terminal acceptance. Inputs exercise Shield only; no submitted shell command runs."""
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time

binary, project, mode = sys.argv[1:]
root = Path(project)
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 88, 0, 0))
original = termios.tcgetattr(slave)

def terminal():
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

env = dict(os.environ, HOME=project, TERM="xterm-256color", NO_COLOR="1", PATH="/usr/bin:/bin")
if mode == "agents":
    cli_dir = root / "cli"
    cli_dir.mkdir()
    for name in ("claude", "codex"):
        executable = cli_dir / name
        executable.write_text("not a runnable program")
        executable.chmod(0o755)
    env["PATH"] = str(cli_dir)
if mode == "unsupported":
    env["TERM"] = "dumb"
if mode == "initial-tiny":
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 4, 15, 0, 0))
worker = """import subprocess, sys, time
mode, args = sys.argv[1], sys.argv[2:]
if mode == 'agents':
    args.remove('--no-setup')
child = subprocess.Popen(args)
print('SHIELD_CHILD_PID=' + str(child.pid), flush=True)
code = child.wait()
if mode in ('editor','error') and code == 0:
    print('SHIELD_INIT_EXIT=0', flush=True)
    action = ['rule', 'add'] if mode == 'editor' else ['request', 'approve', 'request-\x1b[2J']
    child = subprocess.Popen([args[0], *action, '--project', args[args.index('--project')+1]])
    print('SHIELD_EDITOR_PID=' + str(child.pid), flush=True)
    code = child.wait()
print('SHIELD_CHILD_EXIT=' + str(code), flush=True)
time.sleep(60)
"""
# Keep the session owner alive after Shield exits so macOS retains the PTY for
# mode inspection. Shield is the foreground child in that owned session.
process = subprocess.Popen(
    [sys.executable, "-c", worker, mode, binary, "init", "--no-baseline", "--no-setup",
     "--project", project, "--cache-dir", str(root / ".shield/cache")],
    stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=terminal,
)
seen = bytearray()
deadline = time.monotonic() + 30

def pump(seconds):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if select.select([master], [], [], min(0.02, max(0, end - time.monotonic())))[0]:
            chunk = os.read(master, 65536)
            seen.extend(chunk)
            # An actual cursor report is mandatory; this fixture acts as the terminal.
            for _ in range(0 if mode == "missing-report" else chunk.count(b"\x1b[6n")):
                os.write(master, b"\x1b[2;1R")
        if process.poll() is not None:
            return

def activated():
    return (root / ".shield/review/state.json").exists()

def child_code():
    match = re.search(rb"SHIELD_CHILD_EXIT=(-?\d+)", seen)
    return int(match[1]) if match else None

try:
    if mode in ("unsupported", "initial-tiny", "missing-report"):
        while child_code() is None:
            pump(0.05)
            assert time.monotonic() < deadline, seen.decode(errors="replace")
        assert not activated()
        assert b"Choose a name or number" not in seen
        assert b"Confirm this change? [yes/no]" not in seen
        termios.tcflush(slave, termios.TCIFLUSH)
        os.write(master, b"restored\r")
        assert select.select([slave], [], [], 1)[0]
        assert os.read(slave, 1024) == b"restored\n"
        print(json.dumps(dict(code=child_code(), activated=False,
                              mode=mode, restored=termios.tcgetattr(slave) == original)))
        sys.exit(0)
    if mode == "agents":
        def visible():
            return re.sub(rb"\s+", b"", re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", seen))
        while b"Agentintegrations" not in visible():
            pump(0.05)
            assert child_code() is None and time.monotonic() < deadline
        assert b"Skipfornow" in visible()
        assert b"Removeintegrations" not in visible()
        # Both adapters detect the fixture CLIs; Enter accepts their defaults.
        os.write(master, b"\r")
        # Enter keeps the preselected tool permissions.
        while b"Toolpermissions" not in visible():
            pump(0.05)
            assert child_code() is None and time.monotonic() < deadline
        os.write(master, b"\r")
    # Ratatui positions separate words with cursor moves rather than literal spaces.
    while b"Applychange" not in re.sub(rb"\s+", b"", re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", seen)):
        pump(0.05)
        assert child_code() is None, seen.decode(errors="replace")
        assert time.monotonic() < deadline, seen.decode(errors="replace")
    assert not activated()
    result = {}
    if mode == "agents":
        assert b"CodexcommandsShieldwouldaskabout" not in visible()
        assert b"Askfallbackallowsrequests" in visible()
        os.write(master, b"2\r")
    elif mode == "error":
        os.write(master, b"2\r")
    elif mode == "editor":
        os.write(master, b"2\r")
        while b"Ruleid" not in re.sub(rb"\s+", b"", re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", seen)):
            pump(0.05)
            assert child_code() is None and time.monotonic() < deadline
        before = json.loads((root / ".shield/review/state.json").read_text())["policy_revision"]
        os.write(master, b"\x1b[200~acme\nrule\x1b[201~")
        pump(0.2)
        result["paste_did_not_submit"] = child_code() is None and b"Input" not in seen
        os.write(master, b"\x1b")
    elif mode == "suspend":
        os.write(master, b"2")
        pump(0.2)
        pid = int(re.search(rb"SHIELD_CHILD_PID=(\d+)", seen)[1])
        os.kill(pid, signal.SIGTSTP)
        while not termios.tcgetattr(slave)[3] & termios.ICANON:
            pump(0.05)
            assert time.monotonic() < deadline
        termios.tcflush(slave, termios.TCIFLUSH)
        os.write(master, b"shell-input\r")
        assert select.select([slave], [], [], 1)[0]
        assert os.read(slave, 1024) == b"shell-input\n"
        result["suspended_modes_restored"] = termios.tcgetattr(slave) == original
        assert not activated()
        before_resume = seen.count(b"\x1b[6n")
        os.kill(pid, signal.SIGCONT)
        while seen.count(b"\x1b[6n") == before_resume:
            pump(0.05)
            assert time.monotonic() < deadline
        pump(0.2)
        os.write(master, b"\r")
    elif mode == "apply":
        os.write(master, b"\x1b[200~yes\r\x1b[201~")
        pump(0.2)
        result["paste_did_not_confirm"] = child_code() is None and not activated()
        os.write(master, b"2")
        pump(0.2)
        result["digit_did_not_confirm"] = child_code() is None and not activated()
        os.write(master, b"\r")
    elif mode == "resize":
        os.write(master, b"2")
        pump(0.1)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 5, 15, 0, 0))
        pump(0.3)
        os.write(master, b"\r")
        pump(0.2)
        result["tiny_did_not_confirm"] = not activated()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 88, 0, 0))
        pump(0.3)
        if child_code() is None:
            os.write(master, b"\x1b")
    elif mode == "term":
        os.kill(int(re.search(rb"SHIELD_CHILD_PID=(\d+)", seen)[1]), signal.SIGTERM)
    else:
        os.write(master, b"\x1b")
    while child_code() is None:
        pump(0.05)
        assert time.monotonic() < deadline, seen.decode(errors="replace")
    # Exercise restored canonical input, consuming Darwin's transient PENDIN
    # queue marker before comparing every terminal flag and control character.
    termios.tcflush(slave, termios.TCIFLUSH)
    os.write(master, b"restored\r")
    assert select.select([slave], [], [], 1)[0]
    assert os.read(slave, 1024) == b"restored\n"
    actual = termios.tcgetattr(slave)
    result.update(code=child_code(), activated=activated(), restored=actual == original,
                  mode_changes=[(i, before, after) for i, (before, after) in enumerate(zip(original, actual)) if before != after])
    if mode == "editor":
        result["policy_unchanged"] = before == json.loads((root / ".shield/review/state.json").read_text())["policy_revision"]
    if mode == "agents":
        result["agents"] = sorted(json.loads((root / ".shield/review/state.json").read_text())["operator"]["integration"]["agents"])
    if mode == "error":
        feedback = seen.split(b'SHIELD_INIT_EXIT=0',1)[1]
        result["actionable_error"] = b'Error: Request is not available' in feedback and b'shield request list' in feedback
        result["safe_error"] = b'\x1b' not in feedback
    print(json.dumps(result))
finally:
    if process.poll() is None:
        # The retained session leader owns this exact fixture group until wait.
        os.killpg(process.pid, signal.SIGKILL)
    os.close(master)
    os.close(slave)
    process.wait(timeout=5)
