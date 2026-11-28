"""Shared GNU/Linux release environment, ELF admission and packaged startup checks."""

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import selectors
import shutil
import signal
import stat
import subprocess
import tarfile
import tempfile
import time
import urllib.request
import zipfile

DIRECTORY = Path(__file__).resolve().parent
BASE_IMAGE = "docker.io/library/ubuntu@sha256:b000e69a50ff940390233025e7a337fb8469b509e2334fa4d018ac30f3e18cdb"
GLIBC_LIMIT = (2, 35)
TARGETS = {
    "x64": ("amd64", 62, "/lib64/ld-linux-x86-64.so.2"),
    "arm64": ("arm64", 183, "/lib/ld-linux-aarch64.so.1"),
}
SYSTEM_LIBRARIES = frozenset({
    "libc.so.6", "libm.so.6", "libdl.so.2", "libpthread.so.0", "librt.so.1",
    "libresolv.so.2", "libutil.so.1", "libgcc_s.so.1", "libstdc++.so.6", "libatomic.so.1",
})
IMAGE_ID = re.compile(r"sha256:[0-9a-f]{64}")
MAX_ENTRIES = 20_000
MAX_EXPANDED_BYTES = 512 * 1024 * 1024
CLI_SOURCES = {
    "shield": ("shield", "mcp-kit", "registry-kit", "tui-kit", "process-kit"),
    "testrun": ("testrun", "agent-kit", "mcp-kit", "process-kit", "tui-kit"),
}
SOURCE_NAMES = {
    ".cargo", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "build.rs", "build_skills.rs",
    "build_support.rs", "LICENSE", "README.md", "src", "docs", "skills", "third-party",
    "fixtures", "adapters", "native", "runner", "helper", "probes", "xtask", "scripts", "examples",
}


def digest(path):
    checksum = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def run(arguments, **options):
    return subprocess.run(arguments, check=True, **options)


def output(arguments, *, cwd=None):
    return run(arguments, text=True, encoding="utf-8", stdout=subprocess.PIPE,
               cwd=cwd, env=dict(os.environ, LC_ALL="C")).stdout.strip()


def architecture():
    machine = platform.machine()
    if machine in ("x86_64", "AMD64"):
        return "x64"
    if machine in ("aarch64", "arm64"):
        return "arm64"
    raise RuntimeError(f"Unsupported release architecture {machine}")


def symbol_version(value):
    return tuple(map(int, value.split(".")))


def require_glibc(version):
    if symbol_version(version) > GLIBC_LIMIT:
        raise RuntimeError(f"glibc {version} exceeds the release ceiling 2.35")


def elf_kind(path, machine):
    with path.open("rb") as stream:
        header = stream.read(64)
    if not header.startswith(b"\x7fELF"):
        return None
    if (len(header) != 64 or header[:6] != b"\x7fELF\x02\x01"
            or int.from_bytes(header[18:20], "little") != machine):
        raise RuntimeError(f"{path.name}: wrong ELF architecture or header")
    kind = int.from_bytes(header[16:18], "little")
    if kind not in (2, 3):
        raise RuntimeError(f"{path.name}: expected an ELF executable or shared library")
    return kind


def elf_requirements(report, interpreter):
    loader = re.search(r"Requesting program interpreter:\s*([^\]]+)\]", report)
    loader = loader[1] if loader else None
    if loader and loader != interpreter:
        raise RuntimeError(f"Unexpected ELF interpreter {loader}")
    libraries = sorted(set(re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", report)))
    soname = re.search(r"\(SONAME\).*?\[([^\]]+)\]", report)
    for search_path in re.findall(r"\((?:RUNPATH|RPATH)\).*?\[([^\]]*)\]", report):
        for entry in search_path.split(":"):
            if not re.fullmatch(r"\$(?:ORIGIN|\{ORIGIN\})(?:/[A-Za-z0-9_.+-]+)*", entry):
                raise RuntimeError(f"Unsupported library search path {entry!r}")
    needs = re.split(r"^Version needs section ", report, maxsplit=1, flags=re.M)
    versions, weak = set(), set()
    if len(needs) == 2:
        section = needs[1].split("\n\n", 1)[0]
        for name, flags in re.findall(r"\bName:\s+(\S+)\s+Flags:\s+(.+?)\s+Version:", section):
            if name.startswith("GLIBC_"):
                match = re.fullmatch(r"GLIBC_(\d+(?:\.\d+)+)", name)
                if not match:
                    raise RuntimeError(f"Unsupported glibc ABI requirement {name}")
                require_glibc(match[1])
            versions.add(name)
            if "weak" in flags.lower():
                weak.add(name)
    return dict(interpreter=loader, needed=libraries, symbol_versions=sorted(versions),
                weak_symbol_versions=sorted(weak), soname=soname[1] if soname else None)


def audit_tree(root, target):
    _, machine, interpreter = TARGETS[target]
    found = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        if elf_kind(path, machine) is None:
            continue
        try:
            report = output(["readelf", "--wide", "--program-headers", "--dynamic", "--version-info", str(path)])
            record = elf_requirements(report, interpreter)
        except RuntimeError as error:
            raise RuntimeError(f"{path.relative_to(root)}: {error}") from error
        found[str(path.relative_to(root))] = dict(record, sha256=digest(path))
    if not found:
        raise RuntimeError("Package contains no Linux ELF executable")
    providers = SYSTEM_LIBRARIES | {Path(interpreter).name}
    providers |= {row["soname"] or Path(name).name for name, row in found.items()}
    for name, row in found.items():
        missing = set(row["needed"]) - providers
        if missing:
            raise RuntimeError(f"{name}: undeclared runtime libraries {', '.join(sorted(missing))}")
    return found


def unpack(artifact, destination, *, zip_archive=False):
    """Extract only regular files and directories into a fresh staging tree."""
    seen = set()
    size = 0

    def admit(name, length):
        nonlocal size
        relative = PurePosixPath(name)
        if (relative.is_absolute() or not relative.parts or ".." in relative.parts
                or "\\" in name or relative in seen):
            raise RuntimeError(f"Invalid or duplicate archive member {name!r}")
        seen.add(relative)
        size += length
        if len(seen) > MAX_ENTRIES or size > MAX_EXPANDED_BYTES:
            raise RuntimeError("Linux release archive exceeds extraction limits")
        return destination.joinpath(*relative.parts)

    def write(path, stream, mode):
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as target:
            shutil.copyfileobj(stream, target, 1024 * 1024)
        path.chmod(0o755 if mode & 0o111 else 0o644)

    destination.mkdir()
    if zip_archive:
        with zipfile.ZipFile(artifact) as archive:
            for member in archive.infolist():
                mode = member.external_attr >> 16
                if stat.S_IFMT(mode) not in (0, stat.S_IFDIR, stat.S_IFREG):
                    raise RuntimeError(f"Unsupported zip member {member.filename!r}")
                path = admit(member.filename, member.file_size)
                if member.is_dir():
                    path.mkdir(parents=True, exist_ok=True)
                else:
                    with archive.open(member) as stream:
                        write(path, stream, mode)
    else:
        with tarfile.open(artifact, "r|gz") as archive:
            for member in archive:
                path = admit(member.name, member.size)
                if member.isdir():
                    path.mkdir(parents=True, exist_ok=True)
                elif member.isfile():
                    with archive.extractfile(member) as stream:
                        write(path, stream, member.mode)
                else:
                    raise RuntimeError(f"Unsupported tar member {member.name!r}")


@contextmanager
def startup(arguments, project):
    with subprocess.Popen(arguments, cwd=project, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          start_new_session=True) as process:
        try:
            yield process
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()


def receive(process, match):
    deadline = time.monotonic() + 20
    buffer = b""
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while selector.select(max(0, deadline - time.monotonic())):
            chunk = os.read(process.stdout.fileno(), 65536)
            if not chunk:
                raise RuntimeError("Executable exited before its startup reply")
            buffer += chunk
            while b"\n" in buffer:
                line, buffer = buffer.split(b"\n", 1)
                message = json.loads(line)
                if isinstance(message, dict) and match(message):
                    return message
            if len(buffer) > 1024 * 1024 or time.monotonic() >= deadline:
                break
    raise RuntimeError("Startup reply exceeded its time or output limit")


def smoke(product, target, root):
    project = root.parent / "empty-project"
    project.mkdir()
    if product == "workbench":
        extension = root / "extension"
        binary = extension / "server/bin" / f"linux-{target}" / "workbench"
        node = root / "unpacked-node/node"
        binary.chmod(0o755)
        run([str(binary), "--version"], timeout=15, stdout=subprocess.DEVNULL)
        run([str(node), "--version"], timeout=15, stdout=subprocess.DEVNULL)
        with startup([str(binary), "serve", "--root", str(project), "--product", str(extension)], project) as process:
            reply = receive(process, lambda value: value.get("type") == "listening")
            port = reply["port"]
            if type(port) is not int or not 1 <= port <= 65535:
                raise RuntimeError("Workbench returned an invalid listening port")
            client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with client.open(f"http://127.0.0.1:{port}/_workbench/", timeout=10) as response:
                if response.status != 200:
                    raise RuntimeError("Workbench did not serve the test workspace")
        return ["workbench --version", "node --version", "workbench serve and HTTP"]
    paths = list(root.glob("*/shield" if product == "shield" else "*/bin/testrun"))
    if len(paths) != 1:
        raise RuntimeError(f"Expected one packaged {product} executable")
    binary = paths[0]
    run([str(binary), "--version"], timeout=15, stdout=subprocess.DEVNULL)
    arguments = [str(binary), "mcp", "serve"]
    if product == "shield":
        arguments.append("--skills-only")
    with startup(arguments, project) as process:
        request = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "release-smoke", "version": "1"},
        }}
        process.stdin.write(json.dumps(request).encode() + b"\n")
        process.stdin.flush()
        reply = receive(process, lambda value: value.get("id") == 1)
        if not reply.get("result", {}).get("serverInfo", {}).get("name"):
            raise RuntimeError(f"{product} did not initialize its MCP server")
    return [f"{product} --version", f"{product} {'skills-only ' if product == 'shield' else ''}MCP initialize"]


def verify_inside(product, target, artifact, builder_id):
    if architecture() != target or output(["getconf", "GNU_LIBC_VERSION"]) != "glibc 2.35":
        raise RuntimeError("Packaged checks require the target architecture and glibc 2.35")
    checksum = digest(artifact)
    with tempfile.TemporaryDirectory(prefix="canonic-package-") as temporary:
        root = Path(temporary) / "package"
        unpack(artifact, root, zip_archive=product == "workbench")
        if product == "workbench":
            bundle = root / "extension/server/node/runtime/runtime.tar.gz"
            manifest = json.loads(bundle.with_name("runtime.json").read_text())
            if (manifest.get("target") != f"linux-{target}" or manifest.get("size") != bundle.stat().st_size
                    or manifest.get("sha256") != digest(bundle)):
                raise RuntimeError("Workbench's bundled Node does not match its manifest")
            unpack(bundle, root / "unpacked-node")
        files = audit_tree(root, target)
        _, _, interpreter = TARGETS[target]
        for name, record in files.items():
            if record["needed"]:
                output([interpreter, "--list", str(root / name)])
        checks = smoke(product, target, root)
    if digest(artifact) != checksum:
        raise RuntimeError("Linux package changed during verification")
    return dict(product=product, architecture=target, base_image=BASE_IMAGE, builder_image=builder_id,
                glibc_ceiling="2.35", artifact_sha256=checksum, elf=files, startup_checks=checks)


def image_tag(stage, target):
    recipe = digest(DIRECTORY / "linux-release.Dockerfile")
    key = hashlib.sha256(f"{BASE_IMAGE}:{recipe}".encode()).hexdigest()
    return f"canonic-linux-{stage}:{target}-{key[:20]}"


def image(stage, target):
    tag = image_tag(stage, target)
    identifier = output(["docker", "image", "inspect", "--format", "{{.Id}}", tag])
    if not IMAGE_ID.fullmatch(identifier):
        raise RuntimeError(f"Docker did not resolve a pinned {stage} image")
    return identifier


def setup(stage, target):
    docker_arch, _, _ = TARGETS[target]
    run(["docker", "build", "--platform", f"linux/{docker_arch}", "--target", stage,
         "--build-arg", f"BASE_IMAGE={BASE_IMAGE}", "--build-arg", f"TARGETARCH={docker_arch}",
         "--tag", image_tag(stage, target),
         "--file", str(DIRECTORY / "linux-release.Dockerfile"), str(DIRECTORY)])
    image(stage, target)


def mount(source, target, *, read_only=False):
    source = str(source.resolve(strict=True))
    if "," in source:
        raise RuntimeError("Docker mount paths cannot contain commas")
    return ["--mount", f"type=bind,src={source},dst={target}" + (",readonly" if read_only else "")]


def verify(product, target, artifact, builder_id):
    if not IMAGE_ID.fullmatch(builder_id):
        raise RuntimeError("Expected the builder's immutable Docker image ID")
    docker_arch, _, _ = TARGETS[target]
    verifier = image("verifier", target)
    result = output([
        "docker", "run", "--rm", "--platform", f"linux/{docker_arch}", "--network", "none",
        "--read-only", "--tmpfs", "/tmp:exec", "--user", f"{os.getuid()}:{os.getgid()}",
        *mount(artifact, "/artifact", read_only=True),
        *mount(Path(__file__), "/linux_release.py", read_only=True),
        verifier, "python3", "/linux_release.py", "_verify", product, target, "/artifact", builder_id,
    ])
    return dict(json.loads(result), verifier_image=verifier)


def build_run(source, destination, cache, target, command, android_sdk=None):
    identifier = image("builder", architecture())
    cache = cache / identifier.split(":", 1)[1]
    for directory in ("home", "cargo", "rustup", "target"):
        (cache / directory).mkdir(parents=True, exist_ok=True)
    sdk = []
    if android_sdk:
        sdk = [*mount(android_sdk, "/android-sdk", read_only=True), "-e", "ANDROID_HOME=/android-sdk"]
    compiler = []
    for name, value in compiler_environment(target).items():
        compiler.extend(["-e", f"{name}={value}"])
    run(["docker", "run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}",
         "--workdir", "/canonic", "-e", "HOME=/cache/home",
         "-e", f"CANONIC_LINUX_BUILDER_IMAGE={identifier}",
         *compiler,
         *mount(source, "/canonic"), *mount(destination, "/out"), *mount(cache, "/cache"), *sdk,
         identifier, "bash", "-euc",
         'if ! command -v rustup >/dev/null; then rustup-init -y --profile minimal --default-toolchain none --no-modify-path; fi; exec "$@"',
         "canonic-linux-build", *command])
    return identifier


def snapshot_cli(product, workspace, destination):
    root_license = workspace / "LICENSE"
    if root_license.is_symlink() or not root_license.is_file():
        raise RuntimeError("Supply a source checkout with the repository license")
    destination.mkdir()
    shutil.copyfile(root_license, destination / root_license.name)
    for package in CLI_SOURCES[product]:
        source = workspace / "packages" / package
        target = destination / "packages" / package
        target.mkdir(parents=True)
        for entry in source.iterdir():
            if entry.name not in SOURCE_NAMES:
                continue
            if entry.is_symlink():
                raise RuntimeError(f"Unexpected link in release sources: {entry}")
            if entry.is_dir():
                shutil.copytree(entry, target / entry.name, symlinks=True,
                                ignore=shutil.ignore_patterns("target", "node_modules", "__pycache__",
                                                              ".git*", ".DS_Store", "labs", "sdk-tests"))
            else:
                shutil.copy2(entry, target / entry.name)
    if product == "testrun":
        scripts = destination / "packages/shield/scripts"
        scripts.mkdir(parents=True)
        shutil.copyfile(Path(__file__), scripts / "linux_release.py")
    for path in destination.rglob("*"):
        if path.is_symlink():
            raise RuntimeError(f"Unexpected link in release sources: {path.relative_to(destination)}")


def cargo_target(target):
    return ("x86_64" if target == "x64" else "aarch64") + "-unknown-linux-gnu"


def compiler_environment(target):
    triple = cargo_target(target)
    compiler = triple.replace("-unknown", "")
    key = triple.replace("-", "_")
    return {
        f"CARGO_TARGET_{key.upper()}_LINKER": compiler + "-gcc",
        f"CC_{key}": compiler + "-gcc",
        f"CXX_{key}": compiler + "-g++",
        f"PKG_CONFIG_ALLOW_CROSS_{key}": "1",
        f"PKG_CONFIG_LIBDIR_{key}": f"/usr/{compiler}/lib/pkgconfig:/usr/lib/{compiler}/pkgconfig",
    }


def build_testrun(target):
    root = Path("/canonic/packages/testrun")
    triple = cargo_target(target)
    run(["rustup", "target", "add", triple], cwd=root)
    stage = Path("/out/stage")
    run(["cargo", "xtask", "install", "--prefix", str(stage), "--", "--target", triple],
        cwd=root)
    shutil.copyfile(root / "native/browser-regexp/vendor/LICENSE",
                    stage / "share/testrun/browser/QUICKJS-LICENSE")
    rust_notice = Path(output(["rustc", "--print", "sysroot"], cwd=root)) / "share/doc/rust/COPYRIGHT-library.html"
    shutil.copyfile(rust_notice, stage / "share/testrun/COPYRIGHT-rust-library.html")
    executable = stage / "bin/testrun"
    metadata = run(["cargo", "metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"],
                   cwd=root, stdout=subprocess.PIPE)
    version = next(item["version"] for item in json.loads(metadata.stdout)["packages"]
                   if item["name"] == "canonic-testrun")
    record = dict(product="testrun", version=version, target=triple, base_image=BASE_IMAGE,
                  builder_image=os.environ["CANONIC_LINUX_BUILDER_IMAGE"],
                  binary_sha256=digest(executable), cargo_lock_sha256=digest(root / "Cargo.lock"),
                  elf=audit_tree(stage, target))
    (stage / "BUILD.json").write_text(json.dumps(record, indent=2) + "\n")
    name = f"testrun-{version}-{triple}"
    with tarfile.open(Path("/out") / f"{name}.tar.gz", "x:gz") as archive:
        for entry in ("bin", "share", "BUILD.json"):
            archive.add(stage / entry, arcname=f"{name}/{entry}", filter=normalized_entry)


def normalized_entry(entry):
    if not (entry.isdir() or entry.isfile()):
        raise RuntimeError(f"Unexpected package entry: {entry.name}")
    entry.uid = entry.gid = 0
    entry.uname = entry.gname = ""
    entry.mtime = 0
    entry.mode = 0o755 if entry.isdir() or entry.mode & 0o111 else 0o644
    entry.pax_headers = {}
    return entry


def release_cli(product, target, workspace, destination, cache, android_sdk, sign=False):
    if product == "testrun" and (android_sdk is None or
            not (android_sdk / "platforms/android-36/android.jar").is_file() or
            not (android_sdk / "build-tools/36.0.0/d8").is_file()):
        raise RuntimeError("Select the Android SDK with Platform 36 and Build Tools 36.0.0")
    image("verifier", target)
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".linux-release-", dir=destination) as temporary:
        job = Path(temporary)
        source, output_dir = job / "source", job / "out"
        output_dir.mkdir()
        snapshot_cli(product, workspace, source)
        if product == "shield":
            command = ["bash", "-euc",
                       'cd packages/shield; rustup target add "$1"; '
                       'cargo fetch --locked --target "$1"; '
                       'python3 scripts/release-linux.py build --architecture "$2"; '
                       'python3 scripts/release-linux.py package --architecture "$2" --output-directory /out',
                       "shield-linux-release", cargo_target(target), target]
        else:
            command = ["python3", "packages/shield/scripts/linux_release.py", "_build-testrun", target]
        identifier = build_run(source, output_dir, cache, target, command, android_sdk)
        [archive] = output_dir.glob("*.tar.gz")
        result = verify(product, target, archive, identifier)
        results = {
            archive.name: archive,
            f"{archive.name}.sha256": job / "sha256",
            f"{archive.name}.linux.json": job / "verification",
        }
        if sign:
            from linux_signing import bundle_path, sign_artifact

            bundle = bundle_path(archive)
            results[bundle.name] = bundle
        for name in results:
            if os.path.lexists(destination / name):
                raise RuntimeError(f"{name} already exists; choose another output directory")
        results[f"{archive.name}.sha256"].write_text(f'{result["artifact_sha256"]}  {archive.name}\n')
        results[f"{archive.name}.linux.json"].write_text(json.dumps(result, indent=2) + "\n")
        if sign:
            sign_artifact(archive, result["artifact_sha256"])
        published = []
        try:
            for name, file in results.items():
                path = destination / name
                os.link(file, path)
                published.append((file, path))
        except OSError:
            for file, path in reversed(published):
                if path.exists() and file.samefile(path):
                    path.unlink()
            raise
        print(f"Verified {archive.name}: {result['artifact_sha256']}")
        if sign:
            print(f"Signed {archive.name}: {bundle.name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    for name in ("setup", "doctor"):
        action = commands.add_parser(name)
        action.add_argument("stage", choices=("builder", "verifier"))
        action.add_argument("architecture", choices=TARGETS)
    build = commands.add_parser("run")
    build.add_argument("--source", type=Path, required=True)
    build.add_argument("--architecture", choices=TARGETS, required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--cache", type=Path, required=True)
    build.add_argument("--android-sdk", type=Path)
    build.add_argument("command", nargs=argparse.REMAINDER)
    release = commands.add_parser("release")
    release.add_argument("product", choices=CLI_SOURCES)
    release.add_argument("architecture", choices=TARGETS)
    release.add_argument("--source", type=Path, required=True)
    release.add_argument("--output", type=Path, required=True)
    release.add_argument("--cache", type=Path, required=True)
    release.add_argument("--android-sdk", type=Path)
    release.add_argument("--sign", action="store_true", help="Sign the verified archive with Cosign")
    testrun = commands.add_parser("_build-testrun")
    testrun.add_argument("architecture", choices=TARGETS)
    for name in ("verify", "_verify"):
        action = commands.add_parser(name)
        action.add_argument("product", choices=("shield", "testrun", "workbench"))
        action.add_argument("architecture", choices=TARGETS)
        action.add_argument("artifact", type=Path)
        action.add_argument("builder_image")
    args = parser.parse_args()
    if args.action == "setup":
        setup(args.stage, args.architecture)
    elif args.action == "doctor":
        print(image(args.stage, args.architecture))
    elif args.action == "run":
        command = args.command[1:] if args.command[:1] == ["--"] else args.command
        if not command:
            parser.error("Supply the package build command after --")
        build_run(args.source, args.output, args.cache, args.architecture, command, args.android_sdk)
    elif args.action == "release":
        release_cli(args.product, args.architecture, args.source, args.output, args.cache, args.android_sdk, args.sign)
    elif args.action == "_build-testrun":
        build_testrun(args.architecture)
    else:
        action = verify if args.action == "verify" else verify_inside
        print(json.dumps(action(args.product, args.architecture, args.artifact, args.builder_image)))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
