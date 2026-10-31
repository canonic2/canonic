#!/usr/bin/env python3
"""Build and package GNU/Linux workspace candidates without signing or publishing."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import struct
import subprocess
import tarfile
import tempfile

PACKAGE = Path(__file__).resolve().parents[1]
TARGETS = {"x64": ("x86_64-unknown-linux-gnu", 62), "arm64": ("aarch64-unknown-linux-gnu", 183)}
FIRST_PARTY = {"canonic-shield", "canonic-mcp-kit", "canonic-registry-kit", "canonic-tui-kit"}


def run(arguments, *, env=None):
    return subprocess.check_output(arguments, cwd=PACKAGE, env=env, text=True, encoding="utf-8").strip()


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def binary_path(triple):
    root = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    return (root if root.is_absolute() else PACKAGE / root) / triple / "release/shield"


def check_image(path, machine):
    with path.open("rb") as source:
        header = source.read(64)
    if len(header) < 64 or header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != machine:
        raise RuntimeError("Executable does not match the requested Linux architecture")


def target_glibc(compiler, machine):
    library = Path(run([compiler, "-print-file-name=libc.so.6"]))
    if not library.is_absolute() or not library.is_file() or library.stat().st_size > 67_108_864:
        raise RuntimeError("Cannot determine the compiler's target glibc baseline: libc.so.6 is unavailable")
    check_image(library, machine)
    versions = set(re.findall(
        rb"GNU C Library[^\x00\n]{0,512}?release version (\d+(?:\.\d+)+)", library.read_bytes()))
    if len(versions) != 1:
        raise RuntimeError("Cannot determine the compiler's target glibc baseline from libc.so.6")
    return versions.pop().decode("ascii")


def metadata(triple):
    return json.loads(run(["cargo", "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", triple]))


def dependencies(data):
    packages = {item["id"]: item for item in data["packages"]}
    nodes = {node["id"]: node for node in data["resolve"]["nodes"]}
    seen, pending = set(), [data["resolve"]["root"]]
    while pending:
        identity = pending.pop()
        if identity in seen:
            continue
        seen.add(identity)
        if len(seen) > 2048:
            raise RuntimeError("Unexpected dependency graph size")
        pending.extend(dep["pkg"] for dep in nodes[identity]["deps"] if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]))
    return [packages[identity] for identity in sorted(seen)]


def build(architecture, triple, machine):
    prefix = "CARGO_TARGET_" + triple.upper().replace("-", "_")
    for name in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", prefix + "_RUSTFLAGS", "CFLAGS", "CXXFLAGS", "CFLAGS_" + triple.replace("-", "_")]:
        if os.environ.get(name):
            raise RuntimeError(f"Clear {name} before a distribution build")
    host = {"x86_64": "x64", "aarch64": "arm64"}.get(platform.machine())
    compiler = shutil.which("cc" if host == architecture else triple.replace("unknown-", "").replace("-gnu", "-gnu-gcc"))
    if not compiler:
        raise RuntimeError("Install the selected architecture's GNU C cross-compiler and Rust target")
    libc_version = target_glibc(compiler, machine)
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    if not cargo_home.is_absolute():
        cargo_home = PACKAGE / cargo_home
    mappings = [(Path.home(), "/publisher"), (PACKAGE.parents[1], "/canonic"), (cargo_home.resolve(), "/cargo")]
    if any(any(ord(character) < 32 for character in str(path)) for path, _ in mappings):
        raise RuntimeError("Unsupported control character in a build path")
    env = dict(os.environ, LC_ALL="C")
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(f"--remap-path-prefix={path}={neutral}" for path, neutral in mappings)
    env[prefix + "_LINKER"] = compiler
    env["CC_" + triple.replace("-", "_")] = compiler
    env["CFLAGS_" + triple.replace("-", "_")] = shlex.join(f"-ffile-prefix-map={path}={neutral}" for path, neutral in mappings)
    subprocess.run(["cargo", "build-linux-" + architecture], cwd=PACKAGE, env=env, check=True)
    binary = binary_path(triple)
    check_image(binary, machine)
    content = binary.read_bytes()
    if any(str(path).encode("utf-8") in content for path, _ in mappings):
        raise RuntimeError("Executable contains a publisher path; do not distribute it")
    data = metadata(triple)
    root = next(item for item in data["packages"] if item["id"] == data["resolve"]["root"])
    versions = run(["readelf", "--version-info", str(binary)], env=env)
    required = {match for match in re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", versions)}
    version_key = lambda version: tuple(map(int, version.split(".")))
    record = {
        "kind": "workspace-candidate", "product": "Shield", "version": root["version"], "target": triple,
        "compiler": run(["rustc", "--version"]), "binary_sha256": digest(binary),
        "cargo_lock_sha256": digest(PACKAGE / "Cargo.lock"), "source_revision": None,
        # Conservatively retain the target libc baseline; symbol versions alone do
        # not account for every loader ABI or distribution compatibility detail.
        "target_glibc": libc_version,
        "minimum_glibc": max(required | {libc_version}, key=version_key),
        "symbol_glibc_requirement": max(required, key=version_key) if required else None,
        "glibc_abi_requirements": sorted(set(re.findall(r"\bGLIBC_ABI_[A-Z0-9_]+\b", versions))),
    }
    binary.with_suffix(".build.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(f"Built {triple}; required glibc: {record['minimum_glibc']}; SHA256: {record['binary_sha256']}")


def package(triple, machine, output):
    binary = binary_path(triple)
    check_image(binary, machine)
    record = json.loads(binary.with_suffix(".build.json").read_text(encoding="utf-8"))
    if record["target"] != triple or record["binary_sha256"] != digest(binary) or record["cargo_lock_sha256"] != digest(PACKAGE / "Cargo.lock"):
        raise RuntimeError("Build or dependency bytes changed; rebuild before packaging")
    output.mkdir(parents=True, exist_ok=True)
    name = f"shield-{record['version']}-{triple}"
    archive = output / (name + ".tar.gz")
    checksum = output / (archive.name + ".sha256")
    if archive.exists() or checksum.exists():
        raise RuntimeError("Output exists; select another output directory")
    with tempfile.TemporaryDirectory(prefix="shield-package-") as temporary:
        stage = Path(temporary)
        count, total = 0, 0

        def notice(source, relative):
            nonlocal count, total
            if source.is_symlink() or not source.is_file():
                raise RuntimeError("A notice must be an ordinary file")
            size = source.stat().st_size
            count, total = count + 1, total + size
            if size > 8_388_608 or count > 4096 or total > 67_108_864:
                raise RuntimeError("Notice size bound exceeded")
            target = stage / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
            return relative.as_posix()

        shutil.copyfile(binary, stage / "shield")
        if digest(stage / "shield") != record["binary_sha256"]:
            raise RuntimeError("Executable changed while packaging")
        shutil.copyfile(PACKAGE / "LICENSE", stage / "LICENSE")
        rows, first_party = [], []
        for item in dependencies(metadata(triple)):
            if item["name"] in FIRST_PARTY and not item["source"]:
                first_party.append(f"{item['name']} {item['version']}")
                continue
            if not (item["source"] or "").startswith("registry+"):
                raise RuntimeError(f"Unreviewed dependency source: {item['name']}")
            root = Path(item["manifest_path"]).parent.resolve()
            files = {file for file in root.rglob("*") if file.is_file() and re.search(r"licen[sc]e|copying|copyright|notice", file.name, re.I)}
            if item.get("license_file"):
                files.add(root / item["license_file"])
            if not files:
                raise RuntimeError(f"Missing notices: {item['name']} {item['version']}")
            notices = []
            for file in sorted(files):
                relative = file.resolve().relative_to(root)
                notices.append(notice(file, Path("licenses") / f"{item['name']}-{item['version']}" / relative))
            rows.append(dict(name=item["name"], version=item["version"], license=item["license"], notices=notices))
        rust_notice = Path(run(["rustc", "--print", "sysroot"])) / "share/doc/rust/COPYRIGHT-library.html"
        notice(rust_notice, Path("licenses/rust-standard-library/COPYRIGHT-library.html"))
        record.update(first_party=first_party, dependencies=rows)
        (stage / "BUILD.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        (stage / "README.txt").write_text(
            "Shield GNU/Linux portable workspace candidate\n\n"
            "Extract and copy shield to ~/.local/bin/shield (mode 755); put ~/.local/bin on PATH.\n"
            "Check shield --version, then run shield init from your project.\n"
            "Close Shield and connected agents before replacing the executable.\n"
            "After an update or move, run shield mcp setup --repair for each project registration.\n"
            "Before removal, run shield mcp remove for each connected agent in each project.\n"
            "Keep project .shield folders to preserve policy and history.\n\n"
            "BUILD.json records the binary's required glibc version; older hosts need a build from an older supported builder.\n"
            "Checked execution additionally requires a supported glibc 1.1 loader cache, procfs and the documented native command forms.\n"
            "This candidate does not attest source provenance or unperformed ARM64 acceptance.\n"
            "LICENSE covers Canonic material; third-party terms are under licenses/.\n",
            encoding="utf-8",
        )

        def public_entry(info):
            if not (info.isfile() or info.isdir()):
                raise RuntimeError("Unexpected archive entry")
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = 0
            info.mode = 0o755 if info.isdir() or info.name == name + "/shield" else 0o644
            info.pax_headers = {}
            return info

        with tarfile.open(archive, "x:gz", format=tarfile.PAX_FORMAT) as stream:
            stream.add(stage, arcname=name, filter=public_entry)
    checksum.write_text(f"{digest(archive)}  {archive.name}\n", encoding="utf-8")
    print(f"Created {archive.name}; {count} notices; SHA256: {digest(archive)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["build", "package"])
    native = {"x86_64": "x64", "aarch64": "arm64"}.get(platform.machine(), "x64")
    parser.add_argument("--architecture", choices=TARGETS, default=native)
    parser.add_argument("--output-directory", type=Path, default=PACKAGE / "dist")
    args = parser.parse_args()
    if platform.system() != "Linux":
        parser.error("Use a native Linux build host")
    triple, machine = TARGETS[args.architecture]
    if args.action == "build":
        build(args.architecture, triple, machine)
    else:
        package(triple, machine, args.output_directory)


if __name__ == "__main__":
    main()
