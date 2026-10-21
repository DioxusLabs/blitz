#!/usr/bin/env python3
"""Resolve dependency PR revisions and safely publish WPT artifacts."""

import argparse
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.request

from wpt_diff_to_pr import END_MARKER, START_MARKER, splice

BLITZ = "DioxusLabs/blitz"


def github_api(path, body=None):
    headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"}
    if os.environ.get("GITHUB_TOKEN"):
        headers["Authorization"] = "Bearer " + os.environ["GITHUB_TOKEN"]
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    request = urllib.request.Request(
        "https://api.github.com/" + path, headers=headers, data=data,
        method="PATCH" if body is not None else "GET",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def sha(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", value):
        raise ValueError("Expected a full 40-character commit SHA")
    return value.lower()


def revision_fields(body):
    lines = [line.strip() for line in (body or "").splitlines()]
    return [line.removeprefix("blitz-revision:").strip().lower()
            for line in lines if line.startswith("blitz-revision:")]


def blitz_revision(body):
    fields = revision_fields(body)
    if len(fields) > 1:
        raise ValueError("Only one blitz-revision field is allowed")
    return sha(fields[0]) if fields else ""


def should_run(event):
    if event.get("action") != "edited":
        return True
    previous = event.get("changes", {}).get("body")
    if previous is None:
        return False
    return revision_fields(previous.get("from")) != revision_fields(event["pull_request"].get("body"))


def resolve(event, tools_revision, api=github_api):
    pr = event["pull_request"]
    repository = pr["base"]["repo"]["full_name"]
    dependency = repository.split("/")[1].lower()
    if dependency not in {"taffy", "parley"}:
        raise ValueError("This workflow supports Taffy and Parley repositories")
    head = sha(pr["head"]["sha"])
    base = sha(pr["base"]["sha"])
    merge_base = sha(api(f"repos/{repository}/compare/{base}...{head}")["merge_base_commit"]["sha"])
    blitz_base = sha(api(f"repos/{BLITZ}/commits/main")["sha"])
    override = blitz_revision(pr.get("body"))
    if override:
        sha(api(f"repos/{BLITZ}/commits/{override}")["sha"])
    wpt = api(f"repos/{BLITZ}/contents/wpt/WPT_COMMIT?ref={blitz_base}")
    wpt_revision = sha(base64.b64decode(wpt["content"]).decode().strip())
    return {
        "repository": repository,
        "pr_number": pr["number"],
        "dependency": dependency,
        "dependency_base": merge_base,
        "dependency_head": head,
        "blitz_base": blitz_base,
        "blitz_head": override or blitz_base,
        "blitz_override": override,
        "wpt_revision": wpt_revision,
        "tools_revision": sha(tools_revision),
    }


def current_result(info, run, pr, repository):
    if run["event"] != "pull_request" or run["conclusion"] != "success":
        return False
    if info["repository"] != repository or pr["base"]["repo"]["full_name"] != repository:
        return False
    if pr["state"] != "open" or pr["number"] != info["pr_number"]:
        return False
    head = sha(info["dependency_head"])
    if head != sha(run["head_sha"]) or head != sha(pr["head"]["sha"]):
        return False
    try:
        override = blitz_revision(pr.get("body"))
    except ValueError:
        return False
    return override == info["blitz_override"]


def result_body(body, section):
    section = section.strip()
    if not section.startswith(START_MARKER) or not section.endswith(END_MARKER):
        raise ValueError("Expected a delimited WPT results section")
    if section.count(START_MARKER) != 1 or section.count(END_MARKER) != 1:
        raise ValueError("Expected exactly one WPT results section")
    updated = splice(body, section)
    if len(updated) > 65000:
        raise ValueError("PR description plus WPT results exceeds GitHub's size limit")
    return updated


def publish(info, section, run, repository, api=github_api):
    number = info["pr_number"]
    if type(number) is not int or number < 1:
        raise ValueError("Expected a positive PR number")
    path = f"repos/{repository}/pulls/{number}"
    pr = api(path)
    if not current_result(info, run, pr, repository):
        print("Skipping WPT results: the PR revision or Blitz override changed.")
        return False
    body = result_body(pr.get("body"), section)
    if body != (pr.get("body") or ""):
        api(path, {"body": body})
    return True


def output(name, value):
    with open(os.environ["GITHUB_OUTPUT"], "a") as file:
        file.write(f"{name}={value}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare", "find-artifact", "publish"])
    parser.add_argument("directory", type=Path, nargs="?")
    args = parser.parse_args()
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    repository = os.environ["GITHUB_REPOSITORY"]
    if args.command == "prepare":
        if not should_run(event):
            output("run", "false")
            return
        tools = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=Path(__file__).resolve().parents[2], text=True,
        ).strip()
        info = resolve(event, tools)
        if info["repository"] != repository:
            raise ValueError("PR repository does not match the workflow repository")
        output("run", "true")
        output("run-info", json.dumps(info))
    elif args.command == "find-artifact":
        run = event["workflow_run"]
        artifacts = github_api(f"repos/{repository}/actions/runs/{run['id']}/artifacts")["artifacts"]
        found = any(artifact["name"] == "wpt-diff" and not artifact["expired"] for artifact in artifacts)
        output("found", str(found).lower())
    else:
        if args.directory is None:
            parser.error("publish requires an artifact directory")
        info = json.loads((args.directory / "run-info.json").read_text())
        section = (args.directory / "summary.md").read_text()
        publish(info, section, event["workflow_run"], repository)


if __name__ == "__main__":
    main()
