"""Cosign release signing and rejection of changed package bytes."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import linux_signing as signing


class LinuxSigningTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("CANONIC_TEST_COSIGN"), "set CANONIC_TEST_COSIGN to the Cosign executable")
    def test_signs_tarball_and_vsix_and_rejects_tampering(self):
        executable = Path(os.environ["CANONIC_TEST_COSIGN"]).resolve()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            local_cosign = root / "cosign"
            local_cosign.symlink_to(executable)
            with patch.dict(os.environ, {
                "PATH": str(root) + os.pathsep + os.environ.get("PATH", ""),
                "COSIGN_PASSWORD": "acme-test-fixture-only",
                "SIGSTORE_TUF_CACHE": str(root / "tuf"),
                "CANONIC_LINUX_SIGNING_KEY": str(root / "acme.key"),
                "CANONIC_LINUX_SIGNING_PUBLIC_KEY": str(root / "acme.pub"),
            }):
                for prefix in ("acme", "unrelated"):
                    subprocess.run([str(local_cosign), "generate-key-pair", "--output-key-prefix", prefix],
                                   cwd=root, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                for name in ("shield-x64.tar.gz", "workbench-x64.vsix"):
                    with self.subTest(artifact=name):
                        artifact = root / name
                        artifact.write_bytes(b"the exact package verified for release")
                        expected = signing.digest(artifact)
                        runner = Path(signing.__file__)
                        command = ["python3", "-B", str(runner), "sign", str(artifact), "--sha256", expected]
                        result = subprocess.run(command, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                        bundle = signing.bundle_path(artifact)
                        original_bundle = bundle.read_bytes()
                        self.assertFalse(json.loads(original_bundle)["verificationMaterial"].get("tlogEntries"))
                        self.assertEqual(signing.sign_artifact(artifact, expected), bundle)
                        self.assertEqual(bundle.read_bytes(), original_bundle)
                        with self.assertRaisesRegex(RuntimeError, "verify-blob failed"):
                            signing.verify_signature(artifact, bundle, root / "unrelated.pub")
                        artifact.write_bytes(b"different bytes")
                        with self.assertRaisesRegex(RuntimeError, "verified SHA-256"):
                            signing.sign_artifact(artifact, expected)
                        with self.assertRaisesRegex(RuntimeError, "verify-blob failed"):
                            signing.verify_signature(artifact, bundle, root / "acme.pub")

    def test_standalone_tarball_signing_ignores_an_adjacent_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            artifact = Path(temporary) / "shield.tar.gz"
            artifact.write_bytes(b"verified archive bytes")
            verified = signing.digest(artifact)
            artifact.write_bytes(b"replacement bytes")
            receipt = artifact.with_name(artifact.name + ".linux.json")
            receipt.write_text(json.dumps({
                "product": "shield", "glibc_ceiling": "2.35", "artifact_sha256": signing.digest(artifact),
                "startup_checks": ["shield --version"],
            }))
            command = ["python3", "-B", str(Path(signing.__file__)), "sign", str(artifact)]
            missing_digest = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(missing_digest.returncode, 2, missing_digest.stderr)
            self.assertIn("verified --sha256", missing_digest.stderr)
            replaced_archive = subprocess.run([*command, "--sha256", verified], capture_output=True, text=True)
            self.assertEqual(replaced_archive.returncode, 1, replaced_archive.stderr)
            self.assertIn("does not match the verified SHA-256", replaced_archive.stderr)
            self.assertFalse(signing.bundle_path(artifact).exists())


if __name__ == "__main__":
    unittest.main()
