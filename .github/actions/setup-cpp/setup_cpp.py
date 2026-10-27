#!/usr/bin/env python3
"""Install the toolchains pinned in toolchains.lock.

Every archive is verified against its SHA-256 before it is unpacked, every apt package is
installed at its exact locked version (QEMU: upstream version asserted, see the lock file), and
every installed tool's version is checked against the lock and printed. Nothing floats.

usage: setup_cpp.py [--lock FILE] [--downloads DIR] [--tools-dir DIR] COMMAND [TOOL...]

commands:
  check-lock           validate the lock file's format (keys, URLs, checksums)
  cache-keys TOOL...   print "<tool>=<key>" for each requested archive tool available on this
                       host, for $GITHUB_OUTPUT (the key changes whenever a checksum changes)
  install TOOL...      download, verify, unpack or apt-install, and check versions
  versions TOOL...     check the versions of tools already installed and on PATH (the
                       verification image, VER-056)

tools: llvm gcc gcc-s390x qemu, or "all" for every
tool the lock file provides for this host's architecture.

On GitHub Actions the tools' bin directories are appended to $GITHUB_PATH and their roots
exported through $GITHUB_ENV (ALDIS_LLVM_ROOT, ...).
"""

from __future__ import annotations

import argparse
import hashlib
import os
import platform
import re
import shutil
import subprocess
import sys
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DEFAULT_LOCK = REPO / "toolchains.lock"
KEY_RE = re.compile(r"^[A-Z][A-Z0-9_]*$")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")


class Failure(Exception):
    """A check failed; the message says which and why."""


def lock_values(path: Path) -> dict[str, str]:
    """Parses KEY=VALUE lines; rejects anything else."""
    values: dict[str, str] = {}
    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, sep, value = line.partition("=")
        if not sep or not KEY_RE.match(key) or not value or value != value.strip():
            raise Failure(f"{path}:{n}: expected KEY=VALUE, found {raw!r}")
        if key in values:
            raise Failure(f"{path}:{n}: duplicate key {key}")
        values[key] = value
    return values


def host_arch() -> str:
    machine = platform.machine().lower()
    if machine in ("x86_64", "amd64"):
        return "X64"
    if machine in ("aarch64", "arm64"):
        return "ARM64"
    raise Failure(f"unsupported host architecture {machine!r}")


@dataclass
class Archive:
    """One downloadable archive of a tool."""

    prefix: str  # lock key prefix, e.g. LLVM
    members: list[str] = field(default_factory=list)  # unpack only these (relative to the top dir)
    strip_top: bool = True  # the archive has one top-level directory


@dataclass
class Tool:
    name: str
    version_key: str
    archives: list[Archive] = field(default_factory=list)
    apt_packages_key: str = ""
    apt_version_key: str = ""  # empty: upstream version asserted only
    env: str = ""  # exported root variable
    bin_dir: str = "bin"
    version_cmds: list[list[str]] = field(default_factory=list)  # each must print the version


def llvm_members(version: str) -> list[str]:
    major = version.split(".")[0]
    tools = [
        "clang", "clang++", f"clang-{major}", "clang-tidy", "run-clang-tidy", "clang-format",
        "clang-scan-deps", "lld", "ld.lld", "llvm-cov", "llvm-profdata", "llvm-nm",
        "llvm-symbolizer", "llvm-readobj", "llvm-readelf", "llvm-objdump", "llvm-objcopy",
        "llvm-ar", "llvm-ranlib", "llvm-size", "llvm-cxxfilt",
    ]
    return [f"bin/{t}" for t in tools] + ["lib/clang"]


def tools(lock: dict[str, str]) -> dict[str, Tool]:
    llvm_version = lock.get("LLVM_VERSION", "0")
    return {
        t.name: t
        for t in [
            Tool(
                "llvm",
                "LLVM_VERSION",
                [Archive("LLVM", llvm_members(llvm_version))],
                env="ALDIS_LLVM_ROOT",
                version_cmds=[
                    ["clang++", "-dumpversion"],
                    ["clang-tidy", "--version"],
                    ["clang-format", "--version"],
                    ["llvm-cov", "--version"],
                ],
            ),
            Tool(
                "gcc",
                "GCC_VERSION",
                apt_packages_key="GCC_APT_PACKAGES",
                apt_version_key="GCC_APT_VERSION",
                version_cmds=[["gcc-14", "-dumpfullversion"], ["g++-14", "-dumpfullversion"]],
            ),
            Tool(
                "gcc-s390x",
                "GCC_S390X_VERSION",
                apt_packages_key="GCC_S390X_APT_PACKAGES",
                apt_version_key="GCC_S390X_APT_VERSION",
                version_cmds=[["s390x-linux-gnu-g++-14", "-dumpfullversion"]],
            ),
            Tool(
                "qemu",
                "QEMU_VERSION",
                apt_packages_key="QEMU_APT_PACKAGES",
                version_cmds=[
                    ["qemu-system-arm", "--version"],
                    ["qemu-system-riscv32", "--version"],
                    ["qemu-s390x", "--version"],
                ],
            ),
        ]
    }


def check_lock(lock: dict[str, str]) -> None:
    problems = []
    for key, value in lock.items():
        if key.endswith("_URL"):
            if not value.startswith("https://"):
                problems.append(f"{key}: not an https URL")
            sha_key = key[: -len("_URL")] + "_SHA256"
            if sha_key not in lock:
                problems.append(f"{key}: no {sha_key}")
        elif key.endswith("_SHA256"):
            if not SHA_RE.match(value):
                problems.append(f"{key}: not 64 lowercase hex digits")
            if key[: -len("_SHA256")] + "_URL" not in lock:
                problems.append(f"{key}: no matching _URL")
    for tool in tools(lock).values():
        needed = [tool.version_key] + [k for k in (tool.apt_packages_key, tool.apt_version_key) if k]
        problems += [f"tool {tool.name}: {k} missing" for k in needed if k not in lock]
        for archive in tool.archives:
            arches = [a for a in ("X64", "ARM64") if f"{archive.prefix}_{a}_URL" in lock]
            if not arches:
                problems.append(f"tool {tool.name}: no {archive.prefix}_<ARCH>_URL")
    if problems:
        raise Failure("lock file problems:\n  " + "\n  ".join(problems))


def available(tool: Tool, lock: dict[str, str], arch: str) -> bool:
    return all(f"{a.prefix}_{arch}_URL" in lock for a in tool.archives)


def select(names: list[str], table: dict[str, Tool], lock: dict[str, str], arch: str) -> list[Tool]:
    if names == ["all"]:
        chosen = [t for t in table.values() if available(t, lock, arch)]
        skipped = [t.name for t in table.values() if not available(t, lock, arch)]
        if skipped:
            print(
                f"setup-cpp: not provided for {arch} by the lock file: {', '.join(skipped)}",
                file=sys.stderr,
            )
        return chosen
    unknown = [n for n in names if n not in table]
    if unknown or not names:
        raise Failure(f"unknown tool(s) {unknown}; known: {', '.join(table)} or all")
    missing = [n for n in names if not available(table[n], lock, arch)]
    if missing:
        raise Failure(f"the lock file provides no {arch} archive for: {', '.join(missing)}")
    return [table[n] for n in names]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fetch(url: str, expected: str, dest_dir: Path) -> Path:
    """Returns a verified copy of `url`, reusing a cached download only if its checksum matches."""
    dest_dir.mkdir(parents=True, exist_ok=True)
    path = dest_dir / url.rsplit("/", 1)[-1]
    if path.exists() and sha256(path) == expected:
        print(f"setup-cpp: cached {path.name} (sha256 ok)")
        return path
    partial = path.with_name(path.name + ".part")
    for attempt in (1, 2):  # one retry around the download only (c0 plan risk table)
        try:
            print(f"setup-cpp: downloading {url}")
            request = urllib.request.Request(url, headers={"User-Agent": "aldis-setup-cpp"})
            with urllib.request.urlopen(request, timeout=300) as response, partial.open("wb") as out:
                shutil.copyfileobj(response, out, 1 << 20)
            break
        except OSError as e:
            if attempt == 2:
                raise Failure(f"download failed: {url}: {e}") from e
            print(f"setup-cpp: download failed ({e}); retrying once")
    actual = sha256(partial)
    if actual != expected:
        partial.unlink()
        raise Failure(
            f"checksum mismatch for {url}: expected {expected}, got {actual} "
            "(toolchains.lock is the reference; never update it without checking the source)"
        )
    partial.replace(path)
    return path


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, check=True, **kwargs)


def unpack(archive: Path, spec: Archive, dest: Path) -> None:
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    name = archive.name
    if name.endswith(".zip"):
        run(["unzip", "-q", str(archive), "-d", str(dest)])
        return
    strip = ["--strip-components=1"] if spec.strip_top else []
    members = []
    if spec.members:
        top = re.sub(r"\.tar\.(zst|xz|gz)$", "", name)
        members = [f"{top}/{m}" for m in spec.members]
    if name.endswith(".tar.zst"):
        # The LLVM archives use a 1 GiB window, which zstd refuses without --long.
        with subprocess.Popen(
            ["zstd", "-dc", "--long=31", str(archive)], stdout=subprocess.PIPE
        ) as zstd:
            tar = subprocess.run(["tar", "-x", "-C", str(dest), *strip, *members], stdin=zstd.stdout)
            if zstd.stdout:
                zstd.stdout.close()
        if zstd.returncode != 0 or tar.returncode != 0:
            raise Failure(f"unpacking {name} failed (zstd {zstd.returncode}, tar {tar.returncode})")
    elif name.endswith((".tar.xz", ".tar.gz")):
        run(["tar", "-xf", str(archive), "-C", str(dest), *strip, *members])
    else:
        raise Failure(f"unknown archive format: {name}")


def export(path_dirs: list[Path], env: dict[str, str]) -> None:
    for d in path_dirs:
        os.environ["PATH"] = f"{d}{os.pathsep}{os.environ['PATH']}"
    os.environ.update(env)
    if gh_path := os.environ.get("GITHUB_PATH"):
        with open(gh_path, "a", encoding="utf-8") as f:
            f.writelines(f"{d}\n" for d in path_dirs)
    if gh_env := os.environ.get("GITHUB_ENV"):
        with open(gh_env, "a", encoding="utf-8") as f:
            f.writelines(f"{k}={v}\n" for k, v in env.items())


def sudo() -> list[str]:
    """`sudo` on a runner; nothing as root (the verification image, VER-056)."""
    return [] if os.geteuid() == 0 else ["sudo"]


def apt_install(tool: Tool, lock: dict[str, str]) -> None:
    packages = lock[tool.apt_packages_key].split()
    version = lock.get(tool.apt_version_key, "") if tool.apt_version_key else ""
    specs = [f"{p}={version}" if version else p for p in packages]
    run(
        [*sudo(), "apt-get", "install", "-y", "-q", "--no-install-recommends", "--allow-downgrades",
         *specs],
        env={**os.environ, "DEBIAN_FRONTEND": "noninteractive"},
    )
    if version:
        for p in packages:
            got = subprocess.run(
                ["dpkg-query", "-W", "-f=${Version}", p], capture_output=True, text=True, check=True
            ).stdout.strip()
            if got != version:
                raise Failure(f"{p}: installed {got}, locked {version}")


def check_versions(tool: Tool, lock: dict[str, str]) -> list[str]:
    expected = lock[tool.version_key]
    lines = []
    for cmd in tool.version_cmds:
        try:
            out = subprocess.run(cmd, capture_output=True, text=True, check=True).stdout.strip()
        except (OSError, subprocess.CalledProcessError) as e:
            raise Failure(f"{tool.name}: `{' '.join(cmd)}` failed: {e}") from e
        first = out.splitlines()[0] if out else ""
        # A version is matched as a whole token, so 23.1.3 does not match 23.1.30.
        if not re.search(rf"(?<![0-9.]){re.escape(expected)}(?![0-9])", out):
            raise Failure(f"{tool.name}: `{' '.join(cmd)}` reports {first!r}, locked {expected}")
        lines.append(f"{tool.name:10} {' '.join(cmd):40} {first}")
    return lines


def install(selected: list[Tool], lock: dict[str, str], arch: str, downloads: Path, root: Path) -> None:
    if any(t.apt_packages_key for t in selected):
        run([*sudo(), "apt-get", "update", "-q"])
    for tool in selected:
        if tool.apt_packages_key:
            apt_install(tool, lock)
            continue
        dest = root / f"{tool.name}-{lock[tool.version_key]}"
        for i, spec in enumerate(tool.archives):
            url = lock[f"{spec.prefix}_{arch}_URL"]
            archive = fetch(url, lock[f"{spec.prefix}_{arch}_SHA256"], downloads / tool.name)
            unpack(archive, spec, dest if i == 0 else dest / "packs")
        env = {tool.env: str(dest)} if tool.env else {}
        export([(dest / tool.bin_dir).resolve()], env)
    report = [line for tool in selected for line in check_versions(tool, lock)]
    print("setup-cpp: installed versions (all match toolchains.lock)")
    for line in report:
        print(f"  {line}")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--lock", type=Path, default=Path(os.environ.get("ALDIS_TOOLCHAINS_LOCK", DEFAULT_LOCK)))
    parser.add_argument("--downloads", type=Path, default=Path.home() / ".cache" / "aldis-cpp")
    default_root = Path(os.environ.get("RUNNER_TOOL_CACHE", Path.home() / ".local")) / "aldis-cpp"
    parser.add_argument("--tools-dir", type=Path, default=default_root)
    parser.add_argument("command", choices=["check-lock", "cache-keys", "install", "versions"])
    parser.add_argument("tools", nargs="*")
    args = parser.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)  # keep progress and errors in order in CI logs
    try:
        lock = lock_values(args.lock)
        check_lock(lock)
        if args.command == "check-lock":
            print(f"setup-cpp: {args.lock}: {len(lock)} keys, ok")
            return 0
        arch = host_arch()
        table = tools(lock)
        selected = select(args.tools, table, lock, arch)
        if args.command == "cache-keys":
            for tool in selected:
                if tool.archives:
                    shas = [lock[f"{a.prefix}_{arch}_SHA256"] for a in tool.archives]
                    print(f"{tool.name}={arch.lower()}-{'-'.join(shas)}")
            return 0
        if args.command == "versions":
            print("setup-cpp: versions on PATH (all match toolchains.lock)")
            for line in (line for tool in selected for line in check_versions(tool, lock)):
                print(f"  {line}")
            return 0
        install(selected, lock, arch, args.downloads, args.tools_dir)
        return 0
    except (Failure, OSError, subprocess.CalledProcessError) as e:
        print(f"::error::setup-cpp: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
