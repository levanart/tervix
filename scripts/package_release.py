"""Build an allowlisted portable preview ZIP, inventory and SHA256 (stdlib only)."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import struct
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?")
TARGETS = {
    "x86_64-pc-windows-msvc": "windows-x86_64",
    "aarch64-pc-windows-msvc": "windows-arm64",
}


def verify_executable(data, target):
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("expected a Windows PE executable")
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset + 6 > len(data) or data[offset:offset + 4] != b"PE\0\0":
        raise ValueError("invalid PE header")
    machine = struct.unpack_from("<H", data, offset + 4)[0]
    if machine != {"x86_64-pc-windows-msvc": 0x8664, "aarch64-pc-windows-msvc": 0xAA64}[target]:
        raise ValueError(f"executable architecture does not match {target}")


def validate_version(value, root=ROOT):
    if not VERSION.fullmatch(value):
        raise ValueError("version must be a numeric SemVer without v or path characters")
    expected = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    if value != expected:
        raise ValueError(f"requested version {value} differs from workspace version {expected}")
    return value


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8").strip()


def license_files(package):
    directory = Path(package["manifest_path"]).parent.resolve()
    found = set()
    declared = package.get("license_file")
    if declared:
        path = (directory / declared).resolve()
        # Cargo packages may point outside their manifest directory. Never read
        # arbitrary external files while constructing a release archive.
        if path.is_relative_to(directory) and path.is_file():
            found.add(path)
    for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "OFL*", "license*", "licence*", "licenses/**/*"):
        for path in directory.glob(pattern):
            if path.is_file() and path.resolve().is_relative_to(directory):
                found.add(path.resolve())
    return sorted(found)


def package(exe, output, version, commit=None, target="x86_64-pc-windows-msvc"):
    validate_version(version)
    if target not in TARGETS:
        raise ValueError(f"unsupported release target: {target}")
    exe = Path(exe).resolve()
    if not exe.is_file():
        raise ValueError("--exe must be an existing Windows PE executable")
    verify_executable(exe.read_bytes(), target)
    metadata = json.loads(command("cargo", "metadata", "--locked", "--offline", "--format-version", "1"))
    members = set(metadata["workspace_members"])
    packages = sorted((p for p in metadata["packages"] if p["id"] not in members), key=lambda p: (p["name"], p["version"]))
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    platform = TARGETS[target]
    archive = output / f"tervix-v{version}-{platform}.zip"
    if archive.exists():
        raise ValueError(f"refusing to overwrite {archive}; choose an empty output directory")
    actual_commit = command("git", "rev-parse", "HEAD")
    if commit and commit != actual_commit:
        raise ValueError("supplied commit does not match the checked-out source")
    build = {"version": version, "commit": actual_commit,
             "dirty": bool(command("git", "status", "--porcelain")),
             "target": target, "signed": False,
             "rustc": command("rustc", "--version"),
             "executable_sha256": hashlib.sha256(exe.read_bytes()).hexdigest()}
    inventory = []
    missing = []
    # Registry sources can have normalized epoch mtimes older than ZIP's 1980
    # timestamp range. Clamp those timestamps without changing source files.
    with zipfile.ZipFile(archive, "x", zipfile.ZIP_DEFLATED, strict_timestamps=False) as bundle:
        prefix = f"tervix-v{version}/"
        for name in ("LICENSE", "README.md", "RELEASE.md", "RELEASE_NOTES.md", "SECURITY.md", "REVIEW.md"):
            bundle.write(ROOT / name, prefix + name)
        if (ROOT / "CHANGELOG.md").is_file():
            bundle.write(ROOT / "CHANGELOG.md", prefix + "CHANGELOG.md")
        # Preserve exact generated version inputs, because the source tag points
        # to the original commit rather than an automatically pushed version bump.
        bundle.write(ROOT / "Cargo.toml", prefix + "build-inputs/Cargo.toml")
        bundle.write(ROOT / "Cargo.lock", prefix + "build-inputs/Cargo.lock")
        for relative in ("assets/screenshots/monitor-preview.png", "assets/screenshots/charts-preview.png",
                         "assets/screenshots/README.md", "assets/screenshots/capture-window.ps1"):
            if (ROOT / relative).is_file():
                bundle.write(ROOT / relative, prefix + relative)
        bundle.write(exe, prefix + "tervix.exe")
        for p in packages:
            files = license_files(p)
            names = []
            for path in files:
                relative = path.relative_to(Path(p["manifest_path"]).parent.resolve()).as_posix()
                name = f"third-party-licenses/{p['name']}-{p['version']}/{relative}"
                bundle.write(path, prefix + name)
                names.append(name)
            inventory.append({"name": p["name"], "version": p["version"], "license": p.get("license"),
                              "source": p.get("source"), "repository": p.get("repository"), "license_files": names})
            if not files:
                missing.append(f"{p['name']} {p['version']} ({p.get('license') or 'undeclared'})")
        bundle.writestr(prefix + "THIRD_PARTY.json", json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
        bundle.writestr(prefix + "BUILD.json", json.dumps(build, indent=2) + "\n")
        bundle.writestr(prefix + "LICENSE_REVIEW_REQUIRED.txt",
                        "Automated metadata collection is not a license audit. Review all redistribution obligations,\n"
                        "upstream NOTICE and embedded fonts before publishing. Inventory includes all locked platforms.\n"
                        "Packages without an automatically discovered license file:\n" + "\n".join(missing) + "\n")
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (output / f"{archive.name}.sha256").write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    (output / f"BUILD-{platform}.json").write_text(json.dumps(build, indent=2) + "\n", encoding="utf-8")
    print(f"Packaged {archive}; {len(packages)} dependencies, {len(missing)} license files require manual resolution")
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--validate-version")
    parser.add_argument("--version")
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--output", type=Path, default=Path("dist"))
    parser.add_argument("--commit")
    parser.add_argument("--target", choices=TARGETS, default="x86_64-pc-windows-msvc")
    args = parser.parse_args()
    try:
        if args.validate_version:
            validate_version(args.validate_version)
        elif args.version and args.exe:
            package(args.exe, args.output, args.version, args.commit, args.target)
        else:
            parser.error("use --validate-version, or --version together with --exe")
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Release preparation failed: {error}\n")


if __name__ == "__main__":
    main()
