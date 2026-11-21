"""Version/changelog and multi-target artifact acceptance regressions."""

import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch
import zipfile

from package_release import TARGETS, verify_executable
from prepare_release import changelog, validate_version, version_checkout
from verify_release import verify


def pe(machine):
    data = bytearray(128)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 68, machine)
    return data


class ReleaseTests(unittest.TestCase):
    def test_new_version_changes_only_workspace_packages_and_local_requirements(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            crate = root / "crate"
            crate.mkdir()
            (crate / "Cargo.toml").write_text('[package]\nname="local"\nversion.workspace=true\n', encoding="utf-8")
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers=["crate"]\n[workspace.package]\nversion="0.1.0"\n'
                '[workspace.dependencies]\nlocal={version="0.1.0",path="crate"}\nexternal="0.1.0"\n', encoding="utf-8")
            (root / "Cargo.lock").write_text(
                'version=4\n[[package]]\nname="local"\nversion="0.1.0"\n'
                '[[package]]\nname="external"\nversion="0.1.0"\nsource="registry+example"\n', encoding="utf-8")
            version_checkout(root, "0.3.0-rc.1")
            manifest = tomllib.loads((root / "Cargo.toml").read_text())
            self.assertEqual(manifest["workspace"]["package"]["version"], "0.3.0-rc.1")
            self.assertEqual(manifest["workspace"]["dependencies"]["local"]["version"], "0.3.0-rc.1")
            self.assertEqual(manifest["workspace"]["dependencies"]["external"], "0.1.0")
            packages = tomllib.loads((root / "Cargo.lock").read_text())["package"]
            self.assertEqual([p["version"] for p in packages], ["0.3.0-rc.1", "0.1.0"])

    def test_version_rejects_injection_and_invalid_semver(self):
        for bad in ("v1.2.3", "1.2.3/xx", "1.2.3\n", "01.2.3", "1.2.3-rc.01", "1.2.3;exit"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                validate_version(bad)
        self.assertEqual(validate_version("1.2.3-rc.1"), "1.2.3-rc.1")

    def test_changelog_uses_previous_tag_and_commit_range(self):
        def git_fixture(root, *args):
            if args[0] == "rev-parse":
                return "a" * 40
            if args[0] == "describe":
                self.assertIn(("--exclude", "v0.2.0"), zip(args, args[1:]))
                return "v0.1.0"
            self.assertEqual(args[-1], "v0.1.0.." + "a" * 40)
            return "abc1234\tfeat(ui): useful change\ndef5678\tfix: regression"

        with patch("prepare_release.git", side_effect=git_fixture):
            notes = changelog(Path("."), "0.2.0", "HEAD")
        self.assertIn("## Features", notes)
        self.assertIn("## Fixes", notes)
        self.assertIn("v0.1.0", notes)
        self.assertIn("useful change", notes)

    def test_first_release_without_prior_tag(self):
        def git_fixture(root, *args):
            if args[0] == "rev-parse":
                return "a" * 40
            if args[0] == "describe":
                raise subprocess.CalledProcessError(128, "git")
            return "abc1234\tInitial commit"

        with patch("prepare_release.git", side_effect=git_fixture):
            self.assertIn("First release", changelog(Path("."), "0.1.0", "HEAD"))

    def test_wrong_cpu_binary_cannot_be_labeled_arm64(self):
        with self.assertRaises(ValueError):
            verify_executable(pe(0x8664), "aarch64-pc-windows-msvc")
        verify_executable(pe(0xAA64), "aarch64-pc-windows-msvc")

    def test_publish_requires_both_archives_and_valid_checksums(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for target, platform in TARGETS.items():
                name = f"tervix-v0.2.0-{platform}.zip"
                with zipfile.ZipFile(root / name, "w") as bundle:
                    prefix = "tervix-v0.2.0/"
                    for required in ("LICENSE", "CHANGELOG.md", "THIRD_PARTY.json"):
                        bundle.writestr(prefix + required, "fixture")
                    bundle.writestr(prefix + "tervix.exe", pe(0x8664 if "x86_64" in target else 0xAA64))
                    bundle.writestr(prefix + "BUILD.json", json.dumps({"target": target, "version": "0.2.0"}))
                digest = hashlib.sha256((root / name).read_bytes()).hexdigest()
                (root / f"{name}.sha256").write_text(f"{digest}  {name}\n", encoding="utf-8")
            verify(root, "0.2.0")
            self.assertEqual(len((root / "SHA256SUMS.txt").read_text().splitlines()), 2)
            name = "tervix-v0.2.0-windows-arm64.zip"
            (root / name).write_bytes(b"corrupt")
            with self.assertRaises(ValueError):
                verify(root, "0.2.0")


if __name__ == "__main__":
    unittest.main()
