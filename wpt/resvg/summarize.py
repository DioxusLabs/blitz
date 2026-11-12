#!/usr/bin/env python3
"""Per-directory pass counts for the generated resvg tests in a WPT run.

usage: summarize.py [wpt/output/wpt_expectations.txt]
"""

import sys
from collections import defaultdict

path = sys.argv[1] if len(sys.argv) > 1 else "wpt/output/wpt_expectations.txt"
counts = defaultdict(lambda: [0, 0])
for line in open(path):
    name, status = line.split()[:2]
    parts = name.split("/")
    if parts[0] != "resvg" or len(parts) < 4:
        continue
    for key in ((parts[1], parts[2]), (parts[1], "Total")):
        counts[key][0] += status == "PASS"
        counts[key][1] += 1

modes = sorted({mode for mode, _ in counts})
dirs = sorted({d for _, d in counts if d != "Total"}) + ["Total"]
print("| Directory | " + " | ".join(modes) + " |")
print("|---|" + "---:|" * len(modes))
for d in dirs:
    cells = ["%d / %d" % tuple(counts[(m, d)]) if (m, d) in counts else "" for m in modes]
    print("| %s | %s |" % (d, " | ".join(cells)))
