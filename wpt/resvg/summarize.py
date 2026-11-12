#!/usr/bin/env python3
"""Per-directory pass counts for the generated resvg tests in a WPT run.

usage: summarize.py [wpt/output/wpt_expectations.txt]

"parity where img passes" counts native/usvg parity only for tests whose usvg
rendering (img) matches resvg's PNG, so both backends failing alike is not a pass.
"""

import sys
from collections import defaultdict

path = sys.argv[1] if len(sys.argv) > 1 else "wpt/output/wpt_expectations.txt"
results = {}
for line in open(path):
    name, status = line.split()[:2]
    parts = name.split("/", 2)
    if parts[0] == "resvg" and len(parts) == 3:
        results[(parts[1], parts[2])] = status == "PASS"

counts = defaultdict(lambda: [0, 0])
for (mode, test), passed in results.items():
    modes = [mode]
    if mode == "parity" and results.get(("img", test)):
        modes.append("parity where img passes")
    for m in modes:
        for d in (test.split("/")[0], "Total"):
            counts[(m, d)][0] += passed
            counts[(m, d)][1] += 1

modes = sorted({mode for mode, _ in counts})
dirs = sorted({d for _, d in counts if d != "Total"}) + ["Total"]
print("| Directory | " + " | ".join(modes) + " |")
print("|---|" + "---:|" * len(modes))
for d in dirs:
    cells = ["%d / %d" % tuple(counts[(m, d)]) if (m, d) in counts else "" for m in modes]
    print("| %s | %s |" % (d, " | ".join(cells)))
