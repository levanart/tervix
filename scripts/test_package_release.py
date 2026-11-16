"""Safety checks for portable packaging without building or opening the GUI."""

from pathlib import Path
import hashlib
import json
import struct
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package_release
from package_release import license_files, package, validate_version


class PackageTests(unittest.TestCase):
    def test_version_matches_workspace(self):
        with tempfile.TemporaryDirectory() as value:
            root = Path(value)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\n', encoding="utf-8")
            (root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
            self.assertEqual(validate_version("0.1.0", root), "0.1.0")
            for version in ("v0.1.0", "0.2.0", "../0.1.0", "0.1.0;exit", "0.1.0\n"):
                with self.subTest(version=version), self.assertRaises(ValueError):
                    validate_version(version, root)

    def test_license_discovery_never_follows_external_path(self):
        with tempfile.TemporaryDirectory() as value:
            root = Path(value)
            crate = root / "crate"
            crate.mkdir()
            (root / "secret.txt").write_text("not a license", encoding="utf-8")
            license_path = crate / "LICENSE-MIT"
            license_path.write_text("test license", encoding="utf-8")
            found = license_files({"manifest_path": str(crate / "Cargo.toml"), "license_file": "../secret.txt"})
            self.assertEqual(found, [license_path.resolve()])

    def test_zip_allowlist_checksum_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as value:
            root = Path(value)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\n', encoding="utf-8")
            (root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
            for name in ("LICENSE", "README.md", "RELEASE.md", "RELEASE_NOTES.md", "SECURITY.md", "REVIEW.md"):
                (root / name).write_text("fixture", encoding="utf-8")
            (root / ".env").write_text("SECRET=must-not-package", encoding="utf-8")
            exe = root / "tervix.exe"
            executable = bytearray(128)
            executable[:2] = b"MZ"
            struct.pack_into("<I", executable, 60, 64)
            executable[64:68] = b"PE\0\0"
            struct.pack_into("<H", executable, 68, 0x8664)
            exe.write_bytes(executable)
            output = root / "dist"

            def fixture_command(*args):
                if args[0] == "cargo":
                    return json.dumps({"workspace_members": [], "packages": []})
                if args[:2] == ("git", "rev-parse"):
                    return "a" * 40
                if args[0] == "rustc":
                    return "rustc fixture"
                return ""

            with patch.object(package_release, "ROOT", root), patch.object(package_release, "command", fixture_command), patch.object(package_release, "validate_version"):
                archive = package(exe, output, "0.1.0", "a" * 40)
                with zipfile.ZipFile(archive) as bundle:
                    self.assertIsNone(bundle.testzip())
                    names = bundle.namelist()
                    self.assertIn("tervix-v0.1.0/tervix.exe", names)
                    self.assertFalse(any(".env" in name for name in names))
                    build = json.loads(bundle.read("tervix-v0.1.0/BUILD.json"))
                    self.assertEqual(build["commit"], "a" * 40)
                digest = hashlib.sha256(archive.read_bytes()).hexdigest()
                self.assertEqual((output / f"{archive.name}.sha256").read_text().split()[0], digest)
                with self.assertRaises(ValueError):
                    package(exe, output, "0.1.0", "a" * 40)

    def test_rejects_missing_executable(self):
        with patch.object(package_release, "validate_version"):
            with self.assertRaises(ValueError):
                package(Path("not-a-real-executable.exe"), Path("unused"), "0.1.0")


if __name__ == "__main__":
    unittest.main()
