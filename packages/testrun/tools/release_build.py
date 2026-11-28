"""Build and smoke-test one Testrun release archive on its target host."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request
import zipfile


PACKAGE = Path(__file__).resolve().parents[1]
ROOT = PACKAGE.parents[1]
LINUX = ROOT / "packages/shield/scripts/linux_release.py"
TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc",
)
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")
# Official command-line tools downloads from developer.android.com/studio.
ANDROID_TOOLS = {
    "linux": "4e4c464f145a7512b57d088ac6c278c03c9eea610886b35a5e0804e74eedf583",
    "mac_arm64": "835b62a26162b229b441d1f6d4680383815a270809eb33522c0d480fa5002c4e",
    "mac_x86_64": "c5a6378ab5cf7e0d5701921405115befff13e9ff7417fb588389338f8bd050f3",
    "win": "90ae805d20434428bffcb699c290860f19bb5f66a67e6b330067e3de801fb04a",
}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def archive_name(version, target):
    suffix = ".zip" if target.endswith("-msvc") else ".tar.gz"
    return f"testrun-{version}-{target}{suffix}"


def release_notes(changelog, version):
    headings = list(re.finditer(r"^## (.+)$", changelog, re.MULTILINE))
    matches = [index for index, heading in enumerate(headings) if heading[1] == version]
    if len(matches) != 1:
        raise ValueError(f"Expected one '## {version}' entry in Testrun CHANGELOG.md")
    index = matches[0]
    end = headings[index + 1].start() if index + 1 < len(headings) else len(changelog)
    notes = changelog[headings[index].end():end].strip()
    if not notes:
        raise ValueError(f"Testrun CHANGELOG.md entry {version} is empty")
    return notes + "\n"


def prepared_release(package, version):
    if not VERSION.fullmatch(version):
        raise ValueError("Expected a stable Testrun version (MAJOR.MINOR.PATCH)")
    for manifest in ("Cargo.toml", "Cargo.lock"):
        packages = tomllib.loads((package / manifest).read_text(encoding="utf-8"))["package"]
        record = packages if isinstance(packages, dict) else next(
            item for item in packages if item["name"] == "canonic-testrun")
        if record["version"] != version:
            raise ValueError(f"{manifest} does not declare Testrun {version}")
    return release_notes((package / "CHANGELOG.md").read_text(encoding="utf-8"), version)


def run(*args, **options):
    return subprocess.run(args, cwd=PACKAGE, check=True, **options)


def android_sdk(target, temporary):
    windows = target.endswith("-msvc")
    sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    required = ("platforms/android-36/android.jar", "build-tools/36.0.0/lib/d8.jar")
    if not windows:
        required += ("build-tools/36.0.0/d8",)
    if sdk and all((Path(sdk) / member).is_file() for member in required):
        return Path(sdk).resolve()

    scratch_sdk = temporary / "android-sdk"
    kind = "win" if windows else "linux"
    if target.endswith("-darwin"):
        kind = "mac_arm64" if target.startswith("aarch64") else "mac_x86_64"
    archive = temporary / "android-tools.zip"
    url = f"https://dl.google.com/android/repository/commandlinetools-{kind}-15859902_latest.zip"
    with urllib.request.urlopen(url, timeout=60) as response, archive.open("xb") as stream:
        shutil.copyfileobj(response, stream)
    if digest(archive) != ANDROID_TOOLS[kind]:
        raise RuntimeError("Android command-line tools failed SHA-256 verification")
    with zipfile.ZipFile(archive) as tools:
        tools.extractall(temporary)
    manager = temporary / "cmdline-tools/bin" / ("sdkmanager.bat" if windows else "sdkmanager")
    if not windows:
        manager.chmod(0o755)
    subprocess.run(
        [str(manager), f"--sdk_root={scratch_sdk}", "platforms;android-36", "build-tools;36.0.0"],
        input="y\n" * 32, text=True, timeout=600, check=True,
    )
    if not all((scratch_sdk / member).is_file() for member in required):
        raise RuntimeError("Android SDK Platform 36 or Build Tools 36.0.0 is missing")
    return scratch_sdk


def smoke(path, version, temporary):
    environment = dict(os.environ, TESTRUN_DATA_ROOT=str(temporary / "data"))
    info = run(str(path), "--version", capture_output=True, text=True, timeout=30, env=environment)
    if info.stdout.strip() != f"testrun {version}":
        raise RuntimeError(f"Unexpected executable version: {info.stdout.strip()}")
    catalog = run(str(path), "--json", "mcp", "serve", "--check", capture_output=True,
                  text=True, timeout=30, env=environment)
    report = json.loads(catalog.stdout)
    if report.get("ok") is not True or report.get("server") != "canonic-testrun":
        raise RuntimeError("The packaged executable failed its MCP catalog check")


def native_package(target, version, commit, sdk, temporary, output):
    windows = target.endswith("-msvc")
    environment = dict(os.environ, ANDROID_HOME=str(sdk))
    target_dir = temporary / "target"
    if windows:
        flag = f"CARGO_TARGET_{target.upper().replace('-', '_')}_RUSTFLAGS"
        environment[flag] = "-C target-feature=+crt-static"
    run("cargo", "build", "--locked", "--release", "--bin", "testrun", "--target", target,
        "--target-dir", str(target_dir), env=environment)
    binary = "testrun.exe" if windows else "testrun"
    staged = temporary / output.name.removesuffix(".zip").removesuffix(".tar.gz")
    run("cargo", "xtask", "stage-release", str(target_dir / target / "release" / binary),
        str(staged), env=dict(environment, CARGO_TARGET_DIR=str(target_dir)))
    rust = run("rustc", "--print", "sysroot", capture_output=True, text=True).stdout.strip()
    shutil.copyfile(Path(rust) / "share/doc/rust/COPYRIGHT-library.html",
                    staged / "share/testrun/COPYRIGHT-rust-library.html")
    record = {
        "product": "testrun", "version": version, "target": target, "commit": commit,
        "binary_sha256": digest(staged / "bin" / binary),
        "cargo_lock_sha256": digest(PACKAGE / "Cargo.lock"),
    }
    (staged / "BUILD.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    destination = temporary / "extracted"
    if windows:
        with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED) as archive:
            for entry in sorted(staged.rglob("*")):
                if entry.is_file():
                    archive.write(entry, entry.relative_to(temporary))
        with zipfile.ZipFile(output) as archive:
            archive.extractall(destination)
    else:
        with tarfile.open(output, "x:gz") as archive:
            archive.add(staged, arcname=staged.name)
        with tarfile.open(output) as archive:
            archive.extractall(destination, filter="data")
    executable = destination / staged.name / "bin" / binary
    if digest(executable) != record["binary_sha256"]:
        raise RuntimeError("Packaged executable differs from the build")
    smoke(executable, version, temporary)
    checksum = output.with_name(output.name + ".sha256")
    checksum.write_text(f"{digest(output)}  {output.name}\n", encoding="ascii", newline="\n")


def ci_build(version, target, output):
    commit = os.environ.get("GITHUB_SHA", "")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Expected a full GitHub commit SHA")
    prepared_release(PACKAGE, version)
    output = output.resolve()
    if output.exists():
        raise ValueError(f"Release output directory already exists: {output}")
    with tempfile.TemporaryDirectory(prefix="testrun-release-") as directory:
        temporary = Path(directory)
        sdk = android_sdk(target, temporary)
        host = run("rustc", "-vV", capture_output=True, text=True).stdout.splitlines()
        if f"host: {target}" not in host:
            raise RuntimeError(f"Select the native Rust toolchain for {target}")
        artifacts = temporary / "artifacts"
        artifact = artifacts / archive_name(version, target)
        if target.endswith("-linux-gnu"):
            arch = "arm64" if target.startswith("aarch64") else "x64"
            for stage in ("builder", "verifier"):
                run(sys.executable, "-B", str(LINUX), "setup", stage, arch)
            run(sys.executable, "-B", str(LINUX), "release", "testrun", arch, "--source", str(ROOT),
                "--output", str(artifacts), "--cache", str(temporary / "cache"), "--android-sdk", str(sdk))
        else:
            artifacts.mkdir()
            native_package(target, version, commit, sdk, temporary, artifact)
        record = {"version": version, "target": target, "commit": commit,
                  "archive": artifact.name, "sha256": digest(artifact)}
        (artifacts / "candidate.json").write_text(json.dumps(record) + "\n", encoding="utf-8")
        shutil.copytree(artifacts, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("target", choices=TARGETS)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    ci_build(args.version, args.target, args.output)


if __name__ == "__main__":
    main()
