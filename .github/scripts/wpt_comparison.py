#!/usr/bin/env python3
"""Prepare dependency checkouts and render a cross-repository WPT comparison."""

import argparse
import json
import os
from pathlib import Path
import re
import tomllib

from wpt_diff_to_pr import Diff, END_MARKER, render


def use_checkout(manifest, dependency, checkout):
    if dependency not in {"taffy", "parley"}:
        raise ValueError("Only Taffy and Parley checkouts are supported")
    text = manifest.read_text()
    original = tomllib.loads(text)["workspace"]["dependencies"][dependency]
    matches = list(re.finditer(rf"(?ms)^{dependency}\s*=\s*\{{.*?\}}", text))
    if len(matches) != 1 or tomllib.loads(matches[0][0])[dependency] != original:
        raise ValueError(f"Expected one inline workspace dependency for {dependency}")
    crate = checkout / "parley" if dependency == "parley" else checkout
    if not (crate / "Cargo.toml").is_file():
        raise ValueError(f"Missing {dependency} manifest in {crate}")
    source_keys = {"version", "git", "rev", "branch", "tag", "path", "registry", "registry-index"}
    options = {key: value for key, value in original.items() if key not in source_keys}
    entries = [f"path = {json.dumps(str(crate.resolve()))}"]
    entries += [f"{key} = {json.dumps(value)}" for key, value in options.items()]
    replacement = dependency + " = { " + ", ".join(entries) + " }"
    match = matches[0]
    updated = text[:match.start()] + replacement + text[match.end():]
    tomllib.loads(updated)
    manifest.write_text(updated)


def commit(repo, sha):
    return f"[{sha[:12]}](https://github.com/{repo}/commit/{sha})"


def summary(info, entries, areas, run_url):
    section = render(Diff(entries), run_url, areas)
    section = section.replace("compared to `main`", "compared to the PR merge-base")
    provenance = [
        "### Revisions",
        "",
        "| Dependency | Base | Candidate |",
        "| --- | --- | --- |",
        f"| {info['dependency'].title()} | {commit(info['repository'], info['dependency_base'])} | {commit(info['repository'], info['dependency_head'])} |",
        f"| Blitz | {commit('DioxusLabs/blitz', info['blitz_base'])} | {commit('DioxusLabs/blitz', info['blitz_head'])} |",
        "",
        f"WPT: {commit('web-platform-tests/wpt', info['wpt_revision'])} (`css` + `svg`).",
        "Base: PR merge-base. Both runs use the same WPT revision.",
    ]
    if info["blitz_base"] != info["blitz_head"]:
        provenance += ["", "**Includes Blitz compatibility changes**, not only dependency changes."]
    return section.replace(END_MARKER, "\n".join(provenance) + "\n" + END_MARKER)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    patch = commands.add_parser("use-checkout")
    patch.add_argument("manifest", type=Path)
    patch.add_argument("dependency", choices=["taffy", "parley"])
    patch.add_argument("checkout", type=Path)
    report = commands.add_parser("summary")
    report.add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "use-checkout":
        use_checkout(args.manifest, args.dependency, args.checkout)
        return
    directory = args.directory
    info = json.loads(os.environ["RUN_INFO"])
    entries = json.loads((directory / "wptdiff.json").read_text())
    areas = json.loads((directory / "wptareas.json").read_text())
    section = summary(info, entries, areas, os.environ["RUN_URL"])
    (directory / "summary.md").write_text(section + "\n")
    (directory / "run-info.json").write_text(json.dumps(info, indent=2) + "\n")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
        output.write(section + "\n")


if __name__ == "__main__":
    main()
