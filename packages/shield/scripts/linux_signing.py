"""Sign verified Linux release artifacts and check them against a trusted key."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from linux_release import digest


def signing_inputs(*, sign=False):
    if not shutil.which("cosign"):
        raise RuntimeError("Install Cosign 3.1.3 or newer and put it on PATH")
    version = subprocess.check_output(["cosign", "version", "--json"], text=True, timeout=10)
    match = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)(?:\+[\w.-]+)?", json.loads(version).get("gitVersion", ""))
    if not match or tuple(map(int, match.groups())) < (3, 1, 3):
        raise RuntimeError("Install Cosign 3.1.3 or newer and put it on PATH")
    public_key = os.environ.get("CANONIC_LINUX_SIGNING_PUBLIC_KEY")
    if not public_key or not Path(public_key).is_file():
        raise RuntimeError("Set CANONIC_LINUX_SIGNING_PUBLIC_KEY to the trusted public key file")
    signing_key = os.environ.get("CANONIC_LINUX_SIGNING_KEY")
    if sign and not signing_key:
        raise RuntimeError("Set CANONIC_LINUX_SIGNING_KEY to the company signing key reference")
    return Path(public_key).resolve(), signing_key


def bundle_path(artifact):
    return artifact.with_name(artifact.name + ".sigstore.json")


def cosign(*arguments):
    try:
        result = subprocess.run(["cosign", *arguments], timeout=180, check=False)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"Cosign {arguments[0]} timed out") from error
    if result.returncode:
        raise RuntimeError(f"Cosign {arguments[0]} failed ({result.returncode})")


def verify_signature(artifact, bundle, public_key):
    if not bundle.is_file():
        raise RuntimeError(f"Missing signature bundle for {artifact.name}")
    cosign("verify-blob", "--key", str(public_key), "--bundle", str(bundle),
           "--insecure-ignore-tlog=true", str(artifact))


def sign_artifact(artifact, expected_digest):
    if not re.fullmatch(r"[a-f0-9]{64}", expected_digest) or digest(artifact) != expected_digest:
        raise RuntimeError(f"{artifact.name} does not match the verified SHA-256")
    public_key, signing_key = signing_inputs(sign=True)
    bundle = bundle_path(artifact)
    if bundle.exists():
        verify_signature(artifact, bundle, public_key)
        return bundle

    with tempfile.TemporaryDirectory(prefix=".cosign-", dir=artifact.parent) as temporary:
        staged = Path(temporary) / artifact.name
        shutil.copyfile(artifact, staged)
        if digest(staged) != expected_digest:
            raise RuntimeError(f"{artifact.name} changed before signing")
        pending = bundle_path(staged)
        cosign("sign-blob", "--key", signing_key, "--bundle", str(pending),
               "--use-signing-config=false", "--tlog-upload=false", str(staged))
        verify_signature(staged, pending, public_key)
        if digest(artifact) != expected_digest:
            raise RuntimeError(f"{artifact.name} changed during signing")
        os.link(pending, bundle)
    return bundle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("check", "sign", "verify"))
    parser.add_argument("artifact", nargs="?", type=Path)
    parser.add_argument("--sha256", help="Artifact digest from the trusted release verification step")
    args = parser.parse_args()
    if args.action == "check":
        if args.artifact or args.sha256:
            parser.error("check takes no artifact or digest")
        signing_inputs(sign=True)
        return
    if not args.artifact:
        parser.error("Supply a Linux release artifact")
    artifact = args.artifact.resolve(strict=True)
    if args.action == "verify":
        if args.sha256:
            parser.error("verify uses the signature bundle and the trusted public key")
        public_key, _ = signing_inputs()
        verify_signature(artifact, bundle_path(artifact), public_key)
    else:
        if not artifact.name.endswith((".tar.gz", ".vsix")) or not args.sha256:
            parser.error("Supply a .tar.gz or .vsix artifact and its verified --sha256")
        print(f"Signed {artifact.name}: {sign_artifact(artifact, args.sha256).name}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
