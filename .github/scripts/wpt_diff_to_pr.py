#!/usr/bin/env python3
"""Summarise the output of `wpt diff --format json` and publish it into the PR description.

The section is delimited by HTML comment markers so that re-runs of the
workflow replace the previous results instead of appending a new section.
"""

import argparse
import json
import os
import subprocess
import sys

START_MARKER = "<!-- wpt-results-start -->"
END_MARKER = "<!-- wpt-results-end -->"

PASSING_STATUSES = {"PASS", "OK"}
MAX_DIFF_LINES = 400


class Change:
    """A single changed test, in the shape rendered into the PR description."""

    def __init__(self, entry):
        self.test = entry["test"]
        self.kind = entry["kind"]

        if self.kind == "added":
            self.before, self.after = None, entry["status"]
            self.status = "ADD"
            self.counts = entry["counts"]
            self.delta = self.counts["pass"]
        elif self.kind == "removed":
            self.before, self.after = entry["status"], None
            self.status = "REM"
            self.counts = entry["counts"]
            self.delta = -self.counts["pass"]
        else:
            self.before, self.after = entry["before"], entry["after"]
            self.status = f"{self.before} => {self.after}"
            self.counts = entry["counts_after"]
            self.delta = self.counts["pass"] - entry["counts_before"]["pass"]

        self.newly_passing = self.kind == "changed" and (
            self.before not in PASSING_STATUSES and self.after in PASSING_STATUSES
        )
        self.newly_failing = self.kind == "changed" and (
            self.before in PASSING_STATUSES and self.after not in PASSING_STATUSES
        )

    @property
    def marker(self):
        if self.newly_passing or self.kind == "added":
            return "+"
        if self.newly_failing or self.kind == "removed":
            return "-"
        return "!"


class Diff:
    def __init__(self, entries):
        self.changes = sorted((Change(entry) for entry in entries), key=lambda c: c.test)

    @property
    def is_empty(self):
        return not self.changes

    def count(self, predicate):
        return sum(1 for change in self.changes if predicate(change))

    @property
    def subtests_gained(self):
        return sum(change.delta for change in self.changes if change.delta > 0)

    @property
    def subtests_lost(self):
        return -sum(change.delta for change in self.changes if change.delta < 0)

    def status_delta(self, status):
        """The change in the number of tests with the given status."""
        return self.count(lambda c: c.after == status) - self.count(
            lambda c: c.before == status
        )


def format_lines(diff):
    """Render the changes as diff-syntax lines, aligned into columns."""

    def counts_of(change):
        return "[{}/{}]".format(change.counts["pass"], change.counts["total"])

    status_width = max(len(change.status) for change in diff.changes)
    counts_width = max(len(counts_of(change)) for change in diff.changes)
    delta_width = max(len(f"{change.delta:+}") for change in diff.changes)

    return [
        "{} {:<{}}  {:>{}}  {:>{}}  {}".format(
            change.marker,
            change.status,
            status_width,
            counts_of(change),
            counts_width,
            f"{change.delta:+}",
            delta_width,
            change.test,
        )
        for change in diff.changes
    ]


def render(diff, run_url):
    if diff.is_empty:
        headline = "No changes in test results compared to `main`."
    else:
        gained, lost = diff.subtests_gained, diff.subtests_lost
        headline = (
            f"Subtests: **{gained}** newly passing, **{lost}** newly failing "
            f"(net {gained - lost:+})."
        )
        parts = []
        for count, label in [
            (diff.count(lambda c: c.kind == "added"), "added"),
            (diff.count(lambda c: c.kind == "removed"), "removed"),
        ]:
            if count:
                parts.append(f"**{count}** {label}")
        if parts:
            headline += " Tests: " + ", ".join(parts) + "."
        for status, label in [("CRASH", "Crashes"), ("TIMEOUT", "Timeouts")]:
            delta = diff.status_delta(status)
            if delta:
                headline += f" {label}: **{delta:+}**."

    out = [START_MARKER, "## WPT results", "", headline, ""]

    if not diff.is_empty:
        lines = format_lines(diff)
        truncated = len(lines) > MAX_DIFF_LINES
        shown = lines[:MAX_DIFF_LINES]
        out.append("<details>")
        out.append(f"<summary>Full diff ({len(lines)} changed tests)</summary>")
        out.append("")
        out.append("```diff")
        out.extend(shown)
        if truncated:
            out.append(f"# ... and {len(lines) - len(shown)} more (see the workflow logs)")
        out.append("```")
        out.append("")
        out.append("</details>")
        out.append("")

    if run_url:
        out.append(f"<sub>Generated by the [WPT workflow]({run_url}).</sub>")
    out.append(END_MARKER)

    return "\n".join(out)


def splice(body, section):
    body = body or ""
    start = body.find(START_MARKER)
    end = body.find(END_MARKER)
    if start != -1 and end != -1 and end > start:
        return body[:start] + section + body[end + len(END_MARKER):]
    if body.strip():
        return body.rstrip() + "\n\n" + section + "\n"
    return section + "\n"


def gh_api(*args, method=None, fields=None):
    cmd = ["gh", "api"]
    if method:
        cmd += ["-X", method]
    cmd += list(args)
    for key, value in (fields or {}).items():
        cmd += ["-f", f"{key}={value}"]
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("diff_file")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--pr", default=os.environ.get("PR_NUMBER"))
    parser.add_argument("--run-url", default=os.environ.get("RUN_URL"))
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    with open(args.diff_file, encoding="utf-8") as file:
        diff = Diff(json.load(file))

    section = render(diff, args.run_url)

    step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if step_summary:
        with open(step_summary, "a", encoding="utf-8") as file:
            file.write(section + "\n")

    if args.dry_run or not args.pr:
        print(section)
        return 0

    body = json.loads(gh_api(f"repos/{args.repo}/pulls/{args.pr}")).get("body") or ""
    gh_api(
        f"repos/{args.repo}/pulls/{args.pr}",
        method="PATCH",
        fields={"body": splice(body, section)},
    )
    print(f"Updated the description of PR #{args.pr}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
