#!/usr/bin/env python3
"""Tests for wpt_diff_to_pr.py. Run with `python3 -m unittest discover .github/scripts`."""

import unittest

from wpt_diff_to_pr import Diff, format_lines, render, splice

ENTRIES = [
    {
        "kind": "changed",
        "test": "/css/big.html",
        "before": "FAIL",
        "after": "OK",
        "counts_before": {"pass": 233, "total": 23423},
        "counts_after": {"pass": 477, "total": 23423},
        "subtests": [],
    },
    {
        "kind": "changed",
        "test": "/css/subtests-only.html",
        "before": "FAIL",
        "after": "FAIL",
        "counts_before": {"pass": 3, "total": 10},
        "counts_after": {"pass": 9, "total": 10},
        "subtests": [],
    },
    {
        "kind": "changed",
        "test": "/css/regressed.html",
        "before": "OK",
        "after": "TIMEOUT",
        "counts_before": {"pass": 10, "total": 10},
        "counts_after": {"pass": 0, "total": 1},
        "subtests": [],
    },
    {
        "kind": "added",
        "test": "/css/added.html",
        "status": "OK",
        "counts": {"pass": 4, "total": 6},
    },
    {
        "kind": "removed",
        "test": "/css/removed.html",
        "status": "FAIL",
        "counts": {"pass": 2, "total": 3},
    },
]


class FormatLinesTest(unittest.TestCase):
    def test_sorted_aligned_and_marked(self):
        self.assertEqual(
            format_lines(Diff(ENTRIES)),
            [
                "+ ADD                  [4/6]    +4  /css/added.html",
                "+ FAIL => OK     [477/23423]  +244  /css/big.html",
                "- OK => TIMEOUT        [0/1]   -10  /css/regressed.html",
                "- REM                  [2/3]    -2  /css/removed.html",
                "+ FAIL => FAIL        [9/10]    +6  /css/subtests-only.html",
            ],
        )


class RenderTest(unittest.TestCase):
    def test_headline_counts_subtests(self):
        section = render(Diff(ENTRIES), run_url=None)
        self.assertIn(
            "Subtests: **254** newly passing, **12** newly failing (net +242). "
            "Tests: **1** added, **1** removed. Timeouts: **+1**.",
            section,
        )
        self.assertNotIn("Crashes", section)
        self.assertIn("<summary>Full diff (5 changed tests)</summary>", section)

    def test_no_changes(self):
        section = render(Diff([]), run_url="https://example.com/run")
        self.assertIn("No changes in test results compared to `main`.", section)
        self.assertNotIn("```diff", section)
        self.assertIn("https://example.com/run", section)


class SpliceTest(unittest.TestCase):
    def test_replaces_existing_section(self):
        section = render(Diff([]), run_url=None)
        body = splice("Original body.", section)
        self.assertEqual(splice(body, section), body)
        self.assertTrue(body.startswith("Original body."))


if __name__ == "__main__":
    unittest.main()
