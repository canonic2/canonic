"""Focused candidate-baseline regressions; no compiler or VM required."""
import importlib.util
from pathlib import Path
import struct
import platform
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("release_linux", Path(__file__).with_name("release-linux.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class TargetGlibcTests(unittest.TestCase):
    @unittest.skipUnless(platform.system() == "Linux" and shutil.which("cc"), "requires a native GNU/Linux compiler")
    def test_native_compiler_libc_matches_the_native_runtime(self):
        machine = {"x86_64": 62, "aarch64": 183}.get(platform.machine())
        if machine is None:
            self.skipTest("unsupported native architecture")
        expected = subprocess.check_output(["getconf", "GNU_LIBC_VERSION"], text=True).strip().removeprefix("glibc ")
        self.assertEqual(release.target_glibc(shutil.which("cc"), machine), expected)

    def test_reads_selected_target_libc_without_executing_it(self):
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory) / "libc.so.6"
            for machine, version in [(62, "2.39"), (183, "2.43")]:
                header = bytearray(64)
                header[:6] = b"\x7fELF\x02\x01"
                struct.pack_into("<H", header, 18, machine)
                library.write_bytes(header + f"GNU C Library (Acme) stable release version {version}.\n".encode())
                with patch.object(release, "run", return_value=str(library)) as run:
                    self.assertEqual(release.target_glibc("target-gcc", machine), version)
                    run.assert_called_once_with(["target-gcc", "-print-file-name=libc.so.6"])
                with patch.object(release, "run", return_value=str(library)):
                    with self.assertRaisesRegex(RuntimeError, "architecture"):
                        release.target_glibc("target-gcc", 183 if machine == 62 else 62)

    def test_refuses_unknown_or_missing_target_libc(self):
        with patch.object(release, "run", return_value="libc.so.6"):
            with self.assertRaisesRegex(RuntimeError, "baseline"):
                release.target_glibc("target-gcc", 183)
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory) / "libc.so.6"
            header = bytearray(64)
            header[:6] = b"\x7fELF\x02\x01"
            struct.pack_into("<H", header, 18, 183)
            library.write_bytes(header + b"unrecognized version")
            with patch.object(release, "run", return_value=str(library)):
                with self.assertRaisesRegex(RuntimeError, "baseline"):
                    release.target_glibc("target-gcc", 183)


if __name__ == "__main__":
    unittest.main()
