#!/usr/bin/env python3
"""Tests for wpt_area_changes.py. Run with `python3 -m unittest discover .github/scripts`."""

import unittest

from wpt_area_changes import areas_of, compare

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
    def test_areas_use_only_url_path(self):
        path = "css/css-backgrounds/box-shadow-radius-generated.html"
        for suffix in (
            "",
            "?width=200&height=40&spread=50&radius=100px%20/%2020px",
            "#fragment/with/slashes",
            "?query/with/slashes#fragment/with/slashes",
        ):
            with self.subTest(suffix=suffix):
                self.assertEqual(areas_of(path + suffix), ["css", "css/css-backgrounds"])

    def test_variants_remain_distinct_in_area_totals(self):
        path = "css/css-backgrounds/box-shadow-radius-generated.html"
        first = path + "?radius=100px%20/%2020px"
        second = path + "?radius=20px%20/%204px"
        before = {first: {None: "FAIL"}, second: {None: "PASS"}}
        after = {first: {None: "PASS"}, second: {None: "FAIL"}}
        counts = {"before": 1, "after": 1, "total": 2, "gained": 1, "lost": 1}
        self.assertEqual(
            compare(before, after),
            [{"area": area, **counts} for area in ("css", "css/css-backgrounds")],
        )

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
