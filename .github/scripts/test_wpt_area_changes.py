#!/usr/bin/env python3
"""Tests for wpt_area_changes.py. Run with `python3 -m unittest discover .github/scripts`."""

import unittest

from wpt_area_changes import compare

BEFORE = {
    "css/a/x/ref.html": {None: "FAIL"},
    "css/a/harness.html": {"one": "PASS", "two": "FAIL", "three": "PASS"},
    "css/a/timeout.html": {"one": "PASS", "two": "PASS"},
    "css/b/unchanged.html": {None: "PASS"},
    "css/b/skipped.html": {None: "SKIP"},
    "css/b/removed.html": {None: "PASS"},
}

AFTER = {
    "css/a/x/ref.html": {None: "PASS"},
    "css/a/harness.html": {"one": "PASS", "two": "PASS", "three": "FAIL"},
    "css/a/timeout.html": {None: "TIMEOUT"},
    "css/b/unchanged.html": {None: "PASS"},
    "css/b/skipped.html": {None: "SKIP"},
}


class CompareTest(unittest.TestCase):
    def test_counts_changed_areas(self):
        self.assertEqual(
            compare(BEFORE, AFTER),
            [
                {"area": "css", "before": 6, "after": 4, "total": 8, "gained": 2, "lost": 4},
                {"area": "css/a", "before": 4, "after": 3, "total": 6, "gained": 2, "lost": 3},
                {"area": "css/a/x", "before": 0, "after": 1, "total": 1, "gained": 1, "lost": 0},
                {"area": "css/b", "before": 2, "after": 1, "total": 2, "gained": 0, "lost": 1},
            ],
        )

    def test_no_changes(self):
        self.assertEqual(compare(BEFORE, BEFORE), [])


if __name__ == "__main__":
    unittest.main()
