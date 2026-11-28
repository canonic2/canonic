"""ELF and archive release checks without Docker or a Linux VM."""

import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import linux_release as release
import linux_signing as signing

LOADER = "/lib64/ld-linux-x86-64.so.2"
REPORT = f"""Program Headers:
      [Requesting program interpreter: {LOADER}]

 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]

Version definition section '.gnu.version_d' contains 1 entry:
  0x001c: Rev: 1 Flags: none Index: 2 Cnt: 1 Name: GLIBC_2.99

Version needs section '.gnu.version_r' contains 1 entry:
  000000: Version: 1  File: libc.so.6  Cnt: 2
  0x0010:   Name: GLIBC_2.35  Flags: none  Version: 2
  0x0020:   Name: GLIBC_2.34  Flags: WEAK  Version: 3
"""


def elf(machine):
    header = bytearray(64)
    header[:6] = b"\x7fELF\x02\x01"
    header[16:18] = (3).to_bytes(2, "little")
    header[18:20] = machine.to_bytes(2, "little")
    return bytes(header)


class LinuxReleaseTests(unittest.TestCase):
    def test_enforces_numeric_glibc_version_needs_including_weak(self):
        report = release.elf_requirements(REPORT, LOADER)
        self.assertEqual(report["symbol_versions"], ["GLIBC_2.34", "GLIBC_2.35"])
        self.assertEqual(report["weak_symbol_versions"], ["GLIBC_2.34"])
        self.assertEqual(report["needed"], ["libc.so.6"])
        for future in ("2.38", "2.39", "2.100"):
            with self.assertRaisesRegex(RuntimeError, "exceeds.*2.35"):
                release.elf_requirements(REPORT.replace("GLIBC_2.34", "GLIBC_" + future), LOADER)
        with self.assertRaisesRegex(RuntimeError, "Unsupported glibc ABI"):
            release.elf_requirements(REPORT.replace("GLIBC_2.35", "GLIBC_ABI_DT_RELR"), LOADER)
        with self.assertRaisesRegex(RuntimeError, "Unexpected ELF interpreter"):
            release.elf_requirements(REPORT.replace(LOADER, "/custom/ld"), LOADER)

    def test_audit_walks_elf_libraries_and_refuses_wrong_abi(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "app").write_bytes(elf(62))
            nested = root / "bundled"
            nested.mkdir()
            library = nested / "libexample.so"
            library.write_bytes(elf(62))
            report = REPORT.replace("libc.so.6", "libexample.so")
            with patch.object(release, "output", return_value=report):
                self.assertEqual(set(release.audit_tree(root, "x64")), {"app", "bundled/libexample.so"})
                library.write_bytes(elf(183))
                with self.assertRaisesRegex(RuntimeError, "wrong ELF architecture"):
                    release.audit_tree(root, "x64")
                library.unlink()
                with self.assertRaisesRegex(RuntimeError, "undeclared runtime libraries libexample.so"):
                    release.audit_tree(root, "x64")

    def test_archive_reader_refuses_escape_and_preserves_executable_mode(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "release.tar.gz"
            with tarfile.open(archive, "w:gz") as bundle:
                member = tarfile.TarInfo("pkg/bin/executable")
                member.mode = 0o755
                member.size = 3
                bundle.addfile(member, io.BytesIO(b"app"))
            release.unpack(archive, root / "good")
            installed = root / "good/pkg/bin/executable"
            self.assertEqual(installed.read_bytes(), b"app")
            self.assertEqual(installed.stat().st_mode & 0o777, 0o755)
            zipped = root / "release.vsix"
            with zipfile.ZipFile(zipped, "w") as bundle:
                bundle.writestr("../escape", "outside")
            with self.assertRaisesRegex(RuntimeError, "Invalid or duplicate"):
                release.unpack(zipped, root / "bad", zip_archive=True)
            self.assertFalse((root / "escape").exists())

    def test_smoke_launches_target_image_by_id_with_network_disabled(self):
        builder, verifier = "sha256:" + "1" * 64, "sha256:" + "2" * 64
        with tempfile.TemporaryDirectory() as temporary:
            artifact = Path(temporary) / "app.tar.gz"
            artifact.touch()
            with patch.object(release, "image", return_value=verifier) as lookup, \
                 patch.object(release, "output", return_value=json.dumps({"product": "shield"})) as run:
                result = release.verify("shield", "arm64", artifact, builder)
                self.assertEqual(result["verifier_image"], verifier)
                lookup.assert_called_once_with("verifier", "arm64")
                args = run.call_args.args[0]
                self.assertEqual(args[:8], ["docker", "run", "--rm", "--platform", "linux/arm64",
                                             "--network", "none", "--read-only"])
                self.assertEqual(args[-8:], [verifier, "python3", "/linux_release.py", "_verify",
                                             "shield", "arm64", "/artifact", builder])

    def test_builder_supplies_writable_home_for_unmapped_numeric_user(self):
        identifier = "sha256:" + "1" * 64
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / "cache" / ("1" * 64)
            with patch.object(release, "image", return_value=identifier), \
                 patch.object(release.os, "getuid", return_value=41234), \
                 patch.object(release.os, "getgid", return_value=41235), \
                 patch.object(release, "run") as run:
                release.build_run(root, root, root / "cache", "x64", ["true"])
            self.assertTrue((cache / "home").is_dir())
            cache_mount = f"type=bind,src={cache.resolve()},dst=/cache"
        args = run.call_args.args[0]
        self.assertEqual(args[args.index("--user") + 1], "41234:41235")
        self.assertIn("HOME=/cache/home", args)
        self.assertIn(cache_mount, args)

    def test_signed_release_refuses_existing_outputs_before_calling_cosign(self):
        archive_name = "shield-1.0.0-x64.tar.gz"

        def build_archive(source, output, cache, target, command, android_sdk):
            (output / archive_name).write_bytes(b"new archive")
            return "builder-image"

        for suffix in ("", ".sha256", ".linux.json", ".sigstore.json"):
            with self.subTest(suffix=suffix), tempfile.TemporaryDirectory() as temporary:
                destination = Path(temporary)
                existing = destination / (archive_name + suffix)
                if suffix == ".sigstore.json":
                    existing.symlink_to("missing-signature")
                else:
                    existing.write_bytes(b"previous output")
                with patch.object(release, "image"), patch.object(release, "snapshot_cli"), \
                     patch.object(release, "build_run", side_effect=build_archive), \
                     patch.object(release, "verify", return_value={"artifact_sha256": "a" * 64}), \
                     patch.object(signing, "signing_inputs") as inputs, \
                     patch.object(signing, "cosign") as cosign:
                    with self.assertRaisesRegex(RuntimeError, "already exists"):
                        release.release_cli("shield", "x64", destination, destination, destination, None, sign=True)
                inputs.assert_not_called()
                cosign.assert_not_called()
                self.assertEqual(list(destination.iterdir()), [existing])
                if existing.is_symlink():
                    self.assertEqual(existing.readlink(), Path("missing-signature"))
                else:
                    self.assertEqual(existing.read_bytes(), b"previous output")

    def test_release_snapshot_includes_cargo_examples_and_install_notices(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "checkout"
            for package in release.CLI_SOURCES["testrun"]:
                (workspace / "packages" / package).mkdir(parents=True)
            (workspace / "LICENSE").write_text("Acme license")
            examples = workspace / "packages/mcp-kit/examples"
            examples.mkdir()
            (examples / "authorization.rs").write_text("fn main() {}")
            notices = workspace / "packages/testrun/third-party/browser/upstream-notices"
            notices.mkdir(parents=True)
            (notices / "LICENSE").write_text("Browser license")
            specs = workspace / "packages/testrun/specs/browser"
            specs.mkdir(parents=True)
            (specs / "internal.md").write_text("Internal notes")
            (workspace / ".git").write_text("Local Git location")
            snapshot = root / "snapshot"
            release.snapshot_cli("testrun", workspace, snapshot)
            self.assertTrue((snapshot / "packages/mcp-kit/examples/authorization.rs").is_file())
            self.assertTrue((snapshot / "packages/testrun/third-party/browser/upstream-notices/LICENSE").is_file())
            self.assertFalse((snapshot / "packages/testrun/specs").exists())
            self.assertFalse((snapshot / ".git").exists())


if __name__ == "__main__":
    unittest.main()
