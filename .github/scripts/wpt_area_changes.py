#!/usr/bin/env python3
"""Compute per-area subtest changes between two WPT reports.

An area is every directory containing a test (`css`, `css/css-grid`, ...).
The unit is a subtest for testharness tests and the whole test for tests
without subtests (e.g. reftests). The denominator is the union of units in
both runs, excluding units that are missing or SKIP in both. Only areas with
at least one unit that changed are written.
"""

import argparse
import json
import sys
from collections import defaultdict

PASS = "PASS"
SKIP = "SKIP"


def load_units(path):
    """Map each test to {unit name: status}; `None` names the whole test."""
    with open(path, encoding="utf-8") as file:
        results = json.load(file)["results"]

    tests = {}
    for result in results:
        subtests = result.get("subtests") or []
        if subtests:
            units = {subtest["name"]: subtest["status"] for subtest in subtests}
        else:
            units = {None: result["status"]}
        tests[result["test"].strip("/")] = units
    return tests


def areas_of(test):
    parts = test.split("/")[:-1]
    return ["/".join(parts[: i + 1]) for i in range(len(parts))]


def compare(before, after):
    totals = defaultdict(lambda: {"before": 0, "after": 0, "total": 0, "gained": 0, "lost": 0})

    for test in before.keys() | after.keys():
        units_before = before.get(test, {})
        units_after = after.get(test, {})
        # A test that lost (or gained) all its subtests, e.g. by timing out, is
        # compared by its subtests rather than by the whole-test status.
        if any(name is not None for name in (*units_before, *units_after)):
            units_before = {k: v for k, v in units_before.items() if k is not None}
            units_after = {k: v for k, v in units_after.items() if k is not None}

        counts = {"before": 0, "after": 0, "total": 0, "gained": 0, "lost": 0}
        for name in units_before.keys() | units_after.keys():
            status_before = units_before.get(name, SKIP)
            status_after = units_after.get(name, SKIP)
            if status_before == SKIP and status_after == SKIP:
                continue
            passed_before = status_before == PASS
            passed_after = status_after == PASS
            counts["total"] += 1
            counts["before"] += passed_before
            counts["after"] += passed_after
            counts["gained"] += passed_after and not passed_before
            counts["lost"] += passed_before and not passed_after

        for area in areas_of(test):
            for key, value in counts.items():
                totals[area][key] += value

    return [
        {"area": area, **counts}
        for area, counts in sorted(totals.items())
        if counts["gained"] or counts["lost"]
    ]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("before_report")
    parser.add_argument("after_report")
    args = parser.parse_args()

    changes = compare(load_units(args.before_report), load_units(args.after_report))
    json.dump(changes, sys.stdout, indent=2)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
