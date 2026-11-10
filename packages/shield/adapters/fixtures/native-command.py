"""Retain the exact session leader until every group signal is finished.

Only same-group cleanup is promised. A descendant that creates a new group or
session is outside this fixture boundary. No poll()/reap precedes killpg().
"""
import json
import ctypes
import errno
import os
import select
import signal
import subprocess
import sys
import time


def request():
    data = bytearray()
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready, _, _ = select.select([0], [], [], max(0, deadline - time.monotonic()))
        if not ready:
            break
        part = os.read(0, 65536)
        if not part:
            raise RuntimeError("parent disconnected before launch")
        data.extend(part)
        if len(data) > 1024 * 1024 + 1:
            raise RuntimeError("launch exceeds bound")
        if data.endswith(b"\n"):
            return json.loads(data)
    raise RuntimeError("parent launch deadline")


def exited(child):
    # WNOWAIT keeps the child, even a zombie, reserved for subsequent killpg.
    return os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None


def darwin_quiet_group(child):
    if sys.platform != "darwin" or not exited(child):
        return False
    # Darwin can reject signals to a group containing only zombies. Match the
    # native runner: bounded libproc membership, then getpgid (ESRCH for zombies).
    library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    listing = library.proc_listpids
    listing.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_int]
    listing.restype = ctypes.c_int
    members = (ctypes.c_int * 65536)()
    ctypes.set_errno(0)
    length = listing(2, child.pid, members, ctypes.sizeof(members))  # PROC_PGRP_ONLY
    if (length < 0 or length >= ctypes.sizeof(members)
            or length % ctypes.sizeof(ctypes.c_int) != 0
            or ctypes.get_errno() not in [0, errno.ESRCH]):
        return False
    for pid in members[:length // ctypes.sizeof(ctypes.c_int)]:
        if pid == 0 or pid == child.pid:
            continue  # The exact retained leader was independently observed exited.
        try:
            if os.getpgid(pid) == child.pid:
                return False
        except ProcessLookupError:
            pass
        except OSError:
            return False
    return True


def group_signal(child, kind):
    try:
        os.killpg(child.pid, kind)
    except ProcessLookupError:
        pass
    except PermissionError:
        if not darwin_quiet_group(child):
            raise


def cleanup(child, ownership):
    if not ownership["signals_closed"]:
        ownership["stage"] = "term"
        group_signal(child, signal.SIGTERM)
        time.sleep(0.25)
        # Close the signaling phase BEFORE the final signal or any wait. Even
        # an exception immediately after wait returns cannot signal a reaped PID.
        ownership["signals_closed"] = True
        ownership["stage"] = "kill"
        group_signal(child, signal.SIGKILL)
    ownership["stage"] = "reap"
    return child.wait(timeout=3)


def main():
    child = None
    reaped = False
    ownership = {"signals_closed": False, "stage": "input"}
    stage = "capability"
    result = {"reaped": False, "error": "Owned native controller failed"}
    try:
        if not all(hasattr(os, name) for name in ["waitid", "WNOWAIT", "P_PID"]):
            raise RuntimeError("retained leader observation unavailable")
        stage = "input"
        value = request()
        stage = "input"
        timeout = value["timeout_ms"]
        if not isinstance(timeout, int) or timeout <= 0:
            raise RuntimeError("invalid deadline")
        deadline = time.monotonic() + timeout / 1000
        stage = "spawn"
        child = subprocess.Popen([value["program"], *value["args"]], cwd=value["cwd"],
                                 env=value["env"], stdin=subprocess.DEVNULL, start_new_session=True)
        stage = "observe"
        stopped = None
        while not exited(child):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                stopped = "Owned native host exceeded its deadline"
                break
            ready, _, _ = select.select([0], [], [], min(remaining, 0.01))
            if ready:
                # EOF/disconnect or any stop input requests owned cleanup.
                os.read(0, 1)
                stopped = "Owned native fixture cancelled"
                break
        stage = "cleanup"
        code = cleanup(child, ownership)
        reaped = True
        result = {"reaped": True, "code": code if code >= 0 else None,
                  "signal": signal.Signals(-code).name if code < 0 else None}
        if stopped is not None:
            result["error"] = stopped
    except Exception as error:
        # Exception text can contain launch paths or environment; emit a fixed
        # controller failure instead. Finally still owns a successfully spawned child.
        result["stage"] = stage
        number = getattr(error, "errno", None)
        result["errno"] = number if isinstance(number, int) and 0 < number < 4096 else None
    finally:
        if child is not None and not reaped:
            try:
                cleanup(child, ownership)
                reaped = True
            except Exception:
                result["cleanup_stage"] = ownership["stage"]
            result["reaped"] = reaped
        try:
            os.write(3, json.dumps(result).encode() + b"\n")
        except OSError:
            pass  # A dead parent must not prevent target cleanup and reaping.
    return 0 if reaped else 2


sys.exit(main())
