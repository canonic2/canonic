"""Owned agent terminal; JSON frames carry output and keyboard input."""
import fcntl
import json
import os
import select
import signal
import struct
import subprocess
import sys
import termios
import time

options = json.loads(sys.stdin.readline())
master, slave = os.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 45, 120, 0, 0))

def terminal():
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

child = subprocess.Popen(options["argv"], cwd=options["cwd"], env=options["env"],
                         stdin=slave, stdout=slave, stderr=slave, preexec_fn=terminal)
os.close(slave)
running = True
input_buffer = b""

def stop(*_):
    global running
    running = False

signal.signal(signal.SIGTERM, stop)
try:
    deadline = time.monotonic() + 55
    while running and time.monotonic() < deadline:
        ready, _, _ = select.select([master, sys.stdin], [], [], 0.1)
        if master in ready:
            try:
                data = os.read(master, 65536)
            except OSError:
                break
            if not data:
                break
            for _ in range(data.count(b"\x1b[6n")):
                os.write(master, b"\x1b[1;1R")
            print(json.dumps({"text": data.decode("utf8", errors="replace")}), flush=True)
        if sys.stdin in ready:
            data = os.read(sys.stdin.fileno(), 65536)
            if not data:
                break
            input_buffer += data
            while b"\n" in input_buffer:
                line, input_buffer = input_buffer.split(b"\n", 1)
                message = json.loads(line)
                if message.get("stop"):
                    running = False
                    break
                os.write(master, message["input"].encode())
        if child.poll() is not None:
            break
finally:
    # Only this retained session leader and its fixture-owned descendants.
    if child.poll() is None:
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        child.wait(timeout=2)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait(timeout=2)
    os.close(master)
    print(json.dumps({"closed": True, "code": child.returncode}), flush=True)
