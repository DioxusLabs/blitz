import json
from pathlib import Path
import tempfile
import tomllib
import unittest

from wpt_comparison import summary, use_checkout


class CheckoutTest(unittest.TestCase):
    def patch(self, entry, dependency):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            crate = root / "dependency" / "parley" if dependency == "parley" else root / "dependency"
            crate.mkdir(parents=True)
            (crate / "Cargo.toml").write_text('[package]\nname = "' + dependency + '"\nversion = "99.0.0"\n')
            manifest = root / "Cargo.toml"
            manifest.write_text("[workspace.dependencies]\n" + entry + "\n")
            use_checkout(manifest, dependency, root / "dependency")
            result = tomllib.loads(manifest.read_text())["workspace"]["dependencies"][dependency]
            self.assertEqual(result["path"], str(crate))
            return result

    def test_git_dependency_preserves_features(self):
        result = self.patch('taffy = { git = "https://github.com/DioxusLabs/taffy", rev = "abc", default-features = false, features = [\n"std", "grid",\n] }', "taffy")
        self.assertEqual(result["features"], ["std", "grid"])
        self.assertFalse(result["default-features"])
        self.assertNotIn("git", result)
        self.assertNotIn("rev", result)

    def test_registry_dependency_does_not_retain_version_constraint(self):
        result = self.patch('parley = { version = "0.12", default-features = false, features = ["std"] }', "parley")
        self.assertNotIn("version", result)
        self.assertEqual(result["features"], ["std"])

    def test_git_parley_dependency(self):
        result = self.patch('parley = { git = "https://github.com/linebender/parley", branch = "main", features = ["std"] }', "parley")
        self.assertNotIn("git", result)
        self.assertNotIn("branch", result)

    def test_unsupported_package(self):
        with self.assertRaises(ValueError):
            use_checkout(Path("missing"), "unknown", Path("missing"))


class SummaryTest(unittest.TestCase):
    def test_records_revisions_and_baseline(self):
        info = {"repository": "DioxusLabs/parley", "dependency": "parley",
                "dependency_base": "a" * 40, "dependency_head": "b" * 40,
                "blitz_base": "c" * 40, "blitz_head": "c" * 40, "wpt_revision": "d" * 40}
        section = summary(info, [], [], "https://example.com/run")
        self.assertIn("No changes in test results compared to the PR merge-base.", section)
        for key in ("dependency_base", "dependency_head", "blitz_base", "wpt_revision"):
            self.assertIn(info[key], section)
        self.assertNotIn("compatibility changes", section)
        info["blitz_head"] = "e" * 40
        self.assertIn("Includes Blitz compatibility changes", summary(info, [], [], None))
        self.assertEqual(json.loads(json.dumps(info)), info)


if __name__ == "__main__":
    unittest.main()
