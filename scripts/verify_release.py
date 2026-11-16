"""Verify all release targets before creating a GitHub Release."""

import argparse
import hashlib
import json
from pathlib import Path
import zipfile

from package_release import TARGETS, verify_executable
from prepare_release import validate_version


def verify(directory, version):
    validate_version(version)
    sums = []
    for target, platform in TARGETS.items():
        name = f"tervix-v{version}-{platform}.zip"
        archive = directory / name
        expected = (directory / f"{name}.sha256").read_text(encoding="utf-8").split()
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if expected != [digest, name]:
            raise ValueError(f"checksum mismatch: {name}")
        with zipfile.ZipFile(archive) as bundle:
            if bundle.testzip() is not None:
                raise ValueError(f"corrupt ZIP: {name}")
            prefix = f"tervix-v{version}/"
            for required in ("tervix.exe", "LICENSE", "CHANGELOG.md", "BUILD.json", "THIRD_PARTY.json"):
                if prefix + required not in bundle.namelist():
                    raise ValueError(f"missing {required} in {name}")
            build = json.loads(bundle.read(prefix + "BUILD.json"))
            if build["target"] != target or build["version"] != version:
                raise ValueError(f"wrong target/version in {name}")
            verify_executable(bundle.read(prefix + "tervix.exe"), target)
        sums.append(f"{digest}  {name}")
    (directory / "SHA256SUMS.txt").write_text("\n".join(sums) + "\n", encoding="utf-8")
    print(f"Verified all {len(sums)} platform archives and wrote SHA256SUMS.txt")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    try:
        verify(args.directory, args.version)
    except (ValueError, OSError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Release verification failed: {error}\n")
