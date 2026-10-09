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
PENDING_START_MARKER = "<!-- wpt-pending-start -->"
PENDING_END_MARKER = "<!-- wpt-pending-end -->"

PASSING_STATUSES = {"PASS", "OK"}
SUBTEST_PASS = "PASS"
MAX_DIFF_LINES = 400


def subtest_passes(subtest):
    """Whether a changed subtest passed (before, after)."""
    if subtest["kind"] == "added":
        return False, subtest["status"] == SUBTEST_PASS
    if subtest["kind"] == "removed":
        return subtest["status"] == SUBTEST_PASS, False
    return subtest["before"] == SUBTEST_PASS, subtest["after"] == SUBTEST_PASS


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

        # The net change hides subtests that moved in opposite directions, so
        # count each direction from the individual subtest changes if present.
        subtests = entry.get("subtests") or []
        if subtests:
            passes = [subtest_passes(subtest) for subtest in subtests]
            self.gained = sum(1 for before, after in passes if after and not before)
            self.lost = sum(1 for before, after in passes if before and not after)
        else:
            self.gained, self.lost = max(self.delta, 0), max(-self.delta, 0)

        self.newly_passing = self.kind == "changed" and (
            self.before not in PASSING_STATUSES and self.after in PASSING_STATUSES
        )
        self.newly_failing = self.kind == "changed" and (
            self.before in PASSING_STATUSES and self.after not in PASSING_STATUSES
        )

    @property
    def is_relevant(self):
        """False if only non-passing subtest statuses changed (e.g. FAIL to TIMEOUT)."""
        return bool(
            self.kind != "changed"
            or self.before != self.after
            or self.delta
            or self.gained
            or self.lost
        )

    @property
    def marker(self):
        if self.newly_passing or self.kind == "added":
            return "+"
        if self.newly_failing or self.kind == "removed":
            return "-"
        if self.before == self.after and not (self.gained and self.lost):
            if self.delta > 0:
                return "+"
            if self.delta < 0:
                return "-"
        return "!"

    @property
    def delta_text(self):
        if self.gained and self.lost:
            return f"+{self.gained}/-{self.lost}"
        return f"{self.delta:+}"


class Diff:
    def __init__(self, entries):
        changes = (Change(entry) for entry in entries)
        self.changes = sorted((c for c in changes if c.is_relevant), key=lambda c: c.test)

    @property
    def is_empty(self):
        return not self.changes

    def count(self, predicate):
        return sum(1 for change in self.changes if predicate(change))

    @property
    def subtests_gained(self):
        return sum(change.gained for change in self.changes)

    @property
    def subtests_lost(self):
        return sum(change.lost for change in self.changes)

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
    delta_width = max(len(change.delta_text) for change in diff.changes)

    return [
        "{} {:<{}}  {:>{}}  {:>{}}  {}".format(
            change.marker,
            change.status,
            status_width,
            counts_of(change),
            counts_width,
            change.delta_text,
            delta_width,
            change.test,
        )
        for change in diff.changes
    ]


def format_area_lines(areas):
    """Render per-area subtest changes as diff-syntax lines, nested areas indented."""

    def percent(passing, total):
        return 100 * passing / total if total else 0.0

    rows = []
    for area in areas:
        net = area["after"] - area["before"]
        before = percent(area["before"], area["total"])
        after = percent(area["after"], area["total"])
        depth = area["area"].count("/")
        rows.append(
            (
                "+" if net > 0 else "-" if net < 0 else "!",
                "  " * depth + area["area"].rsplit("/", 1)[-1],
                f"{net:+}",
                f"+{area['gained']}",
                f"-{area['lost']}",
                f"{before:.2f}%",
                f"{after:.2f}%",
                f"{after - before:+.2f}%",
                str(area["before"]),
                str(area["after"]),
                str(area["total"]),
            )
        )

    widths = [max(len(row[i]) for row in rows) for i in range(len(rows[0]))]
    template = "{} {:<{}} | {:>{}} ({:>{}} / {:>{}}) | {:>{}} -> {:>{}} ({:>{}}) | {:>{}} -> {:>{}} / {:>{}}"
    return [
        template.format(
            row[0], *(value for i in range(1, len(row)) for value in (row[i], widths[i]))
        ).rstrip()
        for row in rows
    ]


def render(diff, run_url, areas=None):
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

    if areas:
        noun = "area" if len(areas) == 1 else "areas"
        out.append("<details>")
        out.append(f"<summary>Subtest changes by area ({len(areas)} {noun})</summary>")
        out.append("")
        out.append("```diff")
        out.extend(format_area_lines(areas))
        out.append("```")
        out.append("")
        out.append("</details>")
        out.append("")

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


def render_pending_notice(run_url, stale_results):
    link = f"[workflow run]({run_url})" if run_url else "workflow run"
    if stale_results:
        text = (
            f"> New WPT results are being computed ({link}). "
            "The results below are from a previous run and may be out of date."
        )
    else:
        text = f"> WPT results are being computed ({link}) and will be posted here when the run completes."
    return "\n".join([PENDING_START_MARKER, "> [!NOTE]", text, PENDING_END_MARKER])


def splice_pending(body, run_url):
    """Insert (or replace) a pending notice without touching existing results."""
    body = body or ""
    start = body.find(PENDING_START_MARKER)
    end = body.find(PENDING_END_MARKER)
    if start != -1 and end != -1 and end > start:
        notice = render_pending_notice(run_url, stale_results=body.find(START_MARKER) != -1)
        return body[:start] + notice + body[end + len(PENDING_END_MARKER):]
    start = body.find(START_MARKER)
    if start != -1:
        notice = render_pending_notice(run_url, stale_results=True)
        insert_at = start + len(START_MARKER)
        heading = "\n## WPT results\n"
        if body.startswith(heading, insert_at):
            insert_at += len(heading)
        return body[:insert_at] + "\n" + notice + "\n" + body[insert_at:].lstrip("\n")
    notice = render_pending_notice(run_url, stale_results=False)
    section = "\n".join([START_MARKER, "## WPT results", "", notice, END_MARKER])
    return splice(body, section)


def splice_failed(body, run_url):
    """Replace a pending notice with a failure notice. No-op without one."""
    body = body or ""
    start = body.find(PENDING_START_MARKER)
    end = body.find(PENDING_END_MARKER)
    if start == -1 or end == -1 or end <= start:
        return body
    link = f"[workflow run]({run_url})" if run_url else "workflow run"
    notice = "\n".join([
        PENDING_START_MARKER,
        "> [!WARNING]",
        f"> The latest WPT run failed ({link}), so the results below may be out of date.",
        PENDING_END_MARKER,
    ])
    return body[:start] + notice + body[end + len(PENDING_END_MARKER):]


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
    parser.add_argument("diff_file", nargs="?")
    parser.add_argument(
        "--pending",
        action="store_true",
        help="Mark the PR's WPT results as pending instead of posting a diff",
    )
    parser.add_argument(
        "--failed",
        action="store_true",
        help="Replace a pending notice with a failure notice",
    )
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--pr", default=os.environ.get("PR_NUMBER"))
    parser.add_argument("--run-url", default=os.environ.get("RUN_URL"))
    parser.add_argument("--areas", help="per-area changes from wpt_area_changes.py")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    if args.pending or args.failed:
        update = splice_pending if args.pending else splice_failed
        if args.dry_run or not args.pr:
            print(update("", args.run_url))
            return 0
        body = json.loads(gh_api(f"repos/{args.repo}/pulls/{args.pr}")).get("body") or ""
        new_body = update(body, args.run_url)
        if new_body == body:
            print(f"No changes needed to the description of PR #{args.pr}")
            return 0
        gh_api(
            f"repos/{args.repo}/pulls/{args.pr}",
            method="PATCH",
            fields={"body": new_body},
        )
        print(f"Updated the description of PR #{args.pr}")
        return 0

    if not args.diff_file:
        parser.error("diff_file is required unless --pending/--failed is given")

    with open(args.diff_file, encoding="utf-8") as file:
        diff = Diff(json.load(file))

    areas = None
    if args.areas and os.path.exists(args.areas):
        with open(args.areas, encoding="utf-8") as file:
            areas = json.load(file)

    section = render(diff, args.run_url, areas)

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
