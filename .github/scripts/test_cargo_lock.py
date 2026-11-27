#!/usr/bin/env python3
"""Checks on the workspace Cargo.lock. Run with `python3 -m unittest discover .github/scripts`."""

import re
import unittest
from pathlib import Path

CARGO_LOCK = Path(__file__).resolve().parents[2] / "Cargo.lock"


def parse_lock(text):
    """Return {(name, version): [dependency strings]} for every package in a Cargo.lock."""
    packages = {}
    for block in text.split("[[package]]")[1:]:
        name = re.search(r'^name = "([^"]+)"', block, re.M).group(1)
        version = re.search(r'^version = "([^"]+)"', block, re.M).group(1)
        deps = re.search(r"^dependencies = \[(.*?)^\]", block, re.M | re.S)
        packages[(name, version)] = re.findall(r'"([^"]+)"', deps.group(1)) if deps else []
    return packages


def resolve(packages, dep):
    """Resolve a dependency string ("syn" or "syn 2.0.119") to the (name, version) it refers to."""
    name, _, version = dep.partition(" ")
    matches = [key for key in packages if key[0] == name and version in ("", key[1])]
    assert len(matches) == 1, f"ambiguous dependency {dep!r}: {matches}"
    return matches[0]


def dependency(packages, package, name):
    for dep in packages[package]:
        if dep.partition(" ")[0] == name:
            return resolve(packages, dep)
    return None


def prettyplease_syn_mismatches(packages):
    """Packages (bindgen) whose syn differs from the syn of the prettyplease they pass syn types to."""
    mismatches = []
    for package in packages:
        prettyplease = dependency(packages, package, "prettyplease")
        syn = dependency(packages, package, "syn")
        if prettyplease and syn:
            prettyplease_syn = dependency(packages, prettyplease, "syn")
            if prettyplease_syn != syn:
                mismatches.append((package, syn, prettyplease, prettyplease_syn))
    return mismatches


MISMATCHED_LOCK = """
[[package]]
name = "bindgen"
version = "0.73.2"
dependencies = [
 "prettyplease",
 "syn 3.0.6",
]

[[package]]
name = "prettyplease"
version = "0.2.37"
dependencies = [
 "syn 2.0.119",
]

[[package]]
name = "syn"
version = "2.0.119"

[[package]]
name = "syn"
version = "3.0.6"
"""


class PrettypleaseSynTest(unittest.TestCase):
    def test_detects_mismatch(self):
        packages = parse_lock(MISMATCHED_LOCK)
        self.assertEqual(
            prettyplease_syn_mismatches(packages),
            [(("bindgen", "0.73.2"), ("syn", "3.0.6"), ("prettyplease", "0.2.37"), ("syn", "2.0.119"))],
        )

    def test_workspace_lock_matches(self):
        # bindgen accepts syn 2 or 3 and prettyplease 0.2 (syn 2) or 0.3 (syn 3) independently, and
        # fails to build when Cargo picks different syn majors for the two
        # (https://github.com/rust-lang/rust-bindgen/issues/3448). Only the Android build compiles
        # bindgen, so check the lockfile instead.
        packages = parse_lock(CARGO_LOCK.read_text())
        self.assertEqual(
            prettyplease_syn_mismatches(packages),
            [],
            "bindgen and prettyplease use different syn versions; "
            "run `cargo update -p prettyplease --precise <version using bindgen's syn>`",
        )


if __name__ == "__main__":
    unittest.main()
