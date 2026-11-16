"""Prepare a versioned build checkout and a changelog without creating commits/tags."""

import argparse
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?")


def validate_version(version):
    match = SEMVER.fullmatch(version)
    if not match:
        raise ValueError("expected SemVer without v, for example 0.2.0 or 0.2.0-rc.1")
    if match[4] and any(part.isdigit() and len(part) > 1 and part[0] == "0" for part in match[4].split(".")):
        raise ValueError("numeric prerelease identifiers cannot have leading zeros")
    return version


def version_checkout(root, version):
    validate_version(version)
    manifest = root / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    workspace = tomllib.loads(text)["workspace"]
    names = set()
    for member in workspace["members"]:
        for directory in root.glob(member):
            package = tomllib.loads((directory / "Cargo.toml").read_text(encoding="utf-8"))["package"]
            if package.get("version") != {"workspace": True}:
                raise ValueError(f"{directory} must inherit the workspace version")
            names.add(package["name"])
    section = ""
    lines = []
    for line in text.splitlines(keepends=True):
        if line.startswith("["):
            section = line.strip()
        if section == "[workspace.package]" and re.match(r"version\s*=", line):
            line = re.sub(r'"[^"]+"', f'"{version}"', line, count=1)
        elif section == "[workspace.dependencies]":
            name = line.partition("=")[0].strip()
            if name in names:
                line = re.sub(r'version\s*=\s*"[^"]+"', f'version = "{version}"', line, count=1)
        lines.append(line)
    lock = root / "Cargo.lock"
    blocks = re.split(r"(?=^\[\[package\]\]$)", lock.read_text(encoding="utf-8"), flags=re.MULTILINE)
    for index, block in enumerate(blocks):
        if not block.startswith("[[package]]"):
            continue
        package = tomllib.loads(block)["package"][0]
        if package["name"] in names and not package.get("source"):
            blocks[index] = re.sub(r'^version\s*=\s*"[^"]+"', f'version = "{version}"', block, count=1, flags=re.MULTILINE)
    manifest.write_text("".join(lines), encoding="utf-8")
    lock.write_text("".join(blocks), encoding="utf-8")


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True, encoding="utf-8", stderr=subprocess.PIPE).strip()


def changelog(root, version, commit):
    validate_version(version)
    commit = git(root, "rev-parse", "--verify", f"{commit}^{{commit}}")
    try:
        previous = git(root, "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", commit)
    except subprocess.CalledProcessError as error:
        if error.returncode != 128:
            raise
        previous = None
    revision = f"{previous}..{commit}" if previous else commit
    entries = git(root, "log", "--no-merges", "--format=%h%x09%s", revision)
    groups = {"Features": [], "Fixes": [], "Performance": [], "Other changes": []}
    for entry in entries.splitlines():
        sha, subject = entry.split("\t", 1)
        category = subject.split("(", 1)[0].split(":", 1)[0].rstrip("!")
        key = {"feat": "Features", "fix": "Fixes", "perf": "Performance"}.get(category, "Other changes")
        groups[key].append(f"- {subject} (`{sha}`)")
    lines = [f"# Tervix {version}", "", f"Source commit: `{commit}`.",
             f"Changes since `{previous}`." if previous else "First release: changes from repository history.", ""]
    for title, items in groups.items():
        if items:
            lines += [f"## {title}", "", *items, ""]
    if not entries:
        lines += ["No new non-merge commits since the previous version tag.", ""]
    lines += ["## Downloads", "", "- Windows x64 portable ZIP.",
              "- Windows ARM64 portable ZIP (experimental; native device acceptance pending).",
              "- SHA256SUMS.txt contains checksums for both packages.", "",
              "Linux/macOS are not released: the current terminal adapter explicitly supports Windows only.",
              "Both Windows architectures are cross-compiled on the self-hosted Linux X64 runner; native Windows acceptance is separate.",
              "Build versions are applied to the checkout manifests/lockfile; the tag identifies the original source commit.",
              "Artifacts are unsigned. Preview limitations and third-party license review are described in RELEASE.md.", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", default="HEAD")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    try:
        validate_version(args.version)
        notes = changelog(ROOT, args.version, args.commit)
        if args.apply:
            version_checkout(ROOT, args.version)
            (ROOT / "CHANGELOG.md").write_text(notes, encoding="utf-8")
        else:
            print(notes)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Release preparation failed: {error}\n")


if __name__ == "__main__":
    main()
