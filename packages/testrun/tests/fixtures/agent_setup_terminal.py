"""Exercise `mcp setup` terminal confirmation against disposable agent files."""
import errno
import fcntl
import json
import os
import pathlib
import pty
import re
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time

binary = str(pathlib.Path(sys.argv[1]).resolve())
product = sys.argv[2]
with tempfile.TemporaryDirectory(prefix="acme-mcp-setup-") as directory:
    root = pathlib.Path(directory).resolve()
    project = root / "project"
    project.mkdir()
    (project / "gate.yml").write_text("# Project boundary; setup does not load recipes\n")
    home = root / "home"
    home.mkdir()
    cli_dir = root / "cli"
    cli_dir.mkdir()
    env = dict(os.environ, HOME=str(home), TERM="xterm-256color", NO_COLOR="1",
               PATH=str(cli_dir),
               XDG_CONFIG_HOME=str(home / ".config"),
               CODEX_HOME=str(home / ".codex"), KIMI_CODE_HOME=str(home / ".kimi-code"),
               COPILOT_HOME=str(home / ".copilot"),
               CLINE_MCP_SETTINGS_PATH=str(home / "cline.json"))
    target = project / ".qwen/settings.json"

    def run(remove=False, apply=False, guided=False, agent="qwen", detected=False,
            toggle=False, unchanged=False, program=binary, user=False, expected_launch=None):
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 120, 0, 0))

        def terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        args = [program, "mcp", "setup"]
        if user:
            args += ["--scope", "user"]
        if not guided:
            args += ["--agent", agent]
        if remove:
            args.append("--remove")
        if agent == "codex" and not remove:
            args.append("--tool-permissions")
        # Drive selection with and without detected defaults. Filtering must
        # preserve Space as a toggle and Enter as confirmation.
        steps = []
        if guided:
            if not remove:
                steps.append((b"MCPintegrationaction", b"Installorrepair", b"\r"))
            if detected and not remove:
                keys = b"qwen \r" if toggle else b"\r"
                steps.append((b"Selectagents", b"spaceselect", keys))
                if toggle:
                    # Filtering and Space remove the detected mark; Enter must
                    # stay put with an empty set. Space then marks it again.
                    steps.append((b"pressspacetomarkatleast1", b"", b" \r"))
            else:
                steps += [
                    (b"Selectagents", b"spaceselect", b"\r"),
                    (b"pressspacetomarkatleast1", b"", b"qwen \r"),
                ]
        # Codex receives tool approvals: Enter keeps the marked defaults.
        if agent == "codex" and not remove:
            steps.append((b"Toolpermissions", b"Markedtoolsrunwithoutasking", b"\r"))
        child = subprocess.Popen(args, cwd=project, env=env, stdin=slave,
                                 stdout=slave, stderr=slave, preexec_fn=terminal)
        os.close(slave)
        output = bytearray()
        answered = False
        seen = 0
        deadline = time.monotonic() + 20
        try:
            while child.poll() is None:
                assert time.monotonic() < deadline, output.decode(errors="replace")
                if not select.select([master], [], [], 0.1)[0]:
                    continue
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                output.extend(chunk)
                for _ in range(chunk.count(b"\x1b[6n")):
                    os.write(master, b"\x1b[2;1R")
                visible = re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", output)
                compact = re.sub(rb"\s+", b"", visible)
                if steps:
                    title, ready, keys = steps[0]
                    # Each step reads only output rendered after the previous input.
                    if title in compact[seen:] and ready in compact[seen:]:
                        seen = len(compact)
                        os.write(master, keys)
                        steps.pop(0)
                    continue
                if not answered and b"ApplyMCPsetup?" in compact:
                    # Cursor reports and prompt rendering can be split across reads.
                    # Wait for the choices to be rendered before deliberate input.
                    if b"Applyreviewedchanges" in compact:
                        if expected_launch:
                            launch = "Launch: " + " ".join(json.dumps(arg) for arg in expected_launch)
                            before_prompt = compact.split(b"ApplyMCPsetup?", 1)[0]
                            assert re.sub(rb"\s+", b"", launch.encode()) in before_prompt, visible
                        os.write(master, b"\x1b[B\r" if apply else b"\x1b")
                        answered = True
            code = child.wait(timeout=5)
            assert answered != unchanged, output.decode(errors="replace")
            expected_code = 130 if product == "testrun" and not apply else 0
            assert code == expected_code, output.decode(errors="replace")
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)
            os.close(master)

    run()
    assert not target.exists(), "Cancellation must not create registration"
    run(apply=True)
    installed = target.read_text()
    entry = json.loads(installed)["mcpServers"][product]
    if product == "testrun":
        # Project files are shareable: the command is found on PATH.
        assert entry["command"] == product, entry
        assert "cwd" not in entry, entry
    else:
        assert entry["command"] == binary
    assert entry["args"] == (["mcp", "serve"] if product == "testrun" else ["mcp"])
    run(apply=True, unchanged=True)
    assert target.read_text() == installed, "Repeat setup must be stable"
    run(remove=True, apply=True)
    assert product not in json.loads(target.read_text())["mcpServers"]
    # Starting from no registration, only marking qwen can bring it back.
    run(apply=True, guided=True)
    guided = json.loads(target.read_text())["mcpServers"].get(product)
    assert guided == entry, "Guided selection must install the marked agent"
    run(remove=True, apply=True)
    assert product not in json.loads(target.read_text())["mcpServers"]
    run(apply=True, agent="codex")
    codex = (project / ".codex/config.toml").read_text()
    assert 'default_tools_approval_mode = "prompt"' in codex, codex
    assert 'approval_mode = "approve"' in codex, codex
    if product == "testrun":
        for tool in ["docs_list", "docs_search", "docs_read"]:
            assert re.search(rf'{tool}\s*=\s*\{{\s*approval_mode\s*=\s*"approve"\s*\}}', codex), codex
        user_config = home / ".qwen/settings.json"
        launch_args = ["mcp", "serve"]
        run(apply=True, user=True, expected_launch=[binary, *launch_args])
        original_user_config = user_config.read_text()
        moved = str(root / "relocated testrun")
        shutil.copy2(binary, moved)
        run(program=moved, user=True, expected_launch=[moved, *launch_args])
        assert user_config.read_text() == original_user_config
        run(apply=True, program=moved, user=True, expected_launch=[moved, *launch_args])
        user_entry = json.loads(user_config.read_text())["mcpServers"][product]
        assert user_entry["command"] == moved, user_entry
        assert user_entry["args"] == launch_args, user_entry
    # Detected CLIs are metadata-only fixtures, never runnable processes.
    qwen_cli = cli_dir / "qwen"
    qwen_cli.write_text("not a runnable program")
    qwen_cli.chmod(0o755)
    run(apply=True, guided=True, detected=True)
    assert json.loads(target.read_text())["mcpServers"][product] == entry
    run(apply=True, guided=True, detected=True, toggle=True, unchanged=True)
    # Removal must still open empty even with a detected, registered CLI.
    run(remove=True, apply=True, guided=True, detected=True)
    assert product not in json.loads(target.read_text())["mcpServers"]
