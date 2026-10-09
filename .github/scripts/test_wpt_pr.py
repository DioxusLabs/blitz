import base64
import copy
import unittest

from wpt_pr import blitz_revision, current_result, publish, resolve, result_body, should_run
from wpt_diff_to_pr import END_MARKER, START_MARKER

HEAD = "a" * 40
BASE = "b" * 40
MERGE_BASE = "c" * 40
BLITZ = "d" * 40
OVERRIDE = "e" * 40
WPT = "f" * 40
TOOLS = "1" * 40


def pull_request(repository="DioxusLabs/taffy", body=None):
    return {"number": 42, "state": "open", "body": body,
            "base": {"sha": BASE, "repo": {"full_name": repository}},
            "head": {"sha": HEAD}}


def api(path):
    if "/compare/" in path:
        return {"merge_base_commit": {"sha": MERGE_BASE}}
    if path.endswith("/commits/main"):
        return {"sha": BLITZ}
    if path.endswith("/commits/" + OVERRIDE):
        return {"sha": OVERRIDE}
    if "/contents/wpt/WPT_COMMIT?ref=" + BLITZ in path:
        return {"content": base64.b64encode((WPT + "\n").encode()).decode()}
    raise AssertionError(path)


class RevisionTest(unittest.TestCase):
    def test_optional_field(self):
        self.assertEqual(blitz_revision(None), "")
        self.assertEqual(blitz_revision("Other text\nblitz-revision: " + OVERRIDE.upper()), OVERRIDE)

    def test_rejects_short_branch_and_duplicate_fields(self):
        for body in ("blitz-revision: main", "blitz-revision: deadbeef", "blitz-revision:",
                     "blitz-revision: " + OVERRIDE + "\nblitz-revision: " + OVERRIDE):
            with self.subTest(body=body), self.assertRaises(ValueError):
                blitz_revision(body)

    def test_description_edits_only_run_for_field_changes(self):
        event = {"action": "edited", "pull_request": pull_request(), "changes": {"title": {"from": "old"}}}
        self.assertFalse(should_run(event))
        event["changes"] = {"body": {"from": "Old description"}}
        event["pull_request"]["body"] = "New description and WPT results"
        self.assertFalse(should_run(event))
        event["pull_request"]["body"] += "\nblitz-revision: " + OVERRIDE
        self.assertTrue(should_run(event))
        event["changes"]["body"]["from"] = "blitz-revision: " + OVERRIDE
        self.assertFalse(should_run(event))
        event["pull_request"]["body"] = "Override removed"
        self.assertTrue(should_run(event))

    def test_correcting_invalid_field_reruns(self):
        event = {"action": "edited", "pull_request": pull_request(body="blitz-revision: " + OVERRIDE),
                 "changes": {"body": {"from": "blitz-revision: main"}}}
        self.assertTrue(should_run(event))

    def test_push_and_open_always_run(self):
        for action in ("opened", "reopened", "synchronize"):
            self.assertTrue(should_run({"action": action}))

    def test_resolves_both_packages_and_immutable_revisions(self):
        for repository in ("DioxusLabs/taffy", "DioxusLabs/parley", "linebender/parley"):
            info = resolve({"pull_request": pull_request(repository)}, TOOLS, api)
            self.assertEqual(info["dependency_base"], MERGE_BASE)
            self.assertEqual(info["dependency_head"], HEAD)
            self.assertEqual(info["blitz_base"], BLITZ)
            self.assertEqual(info["blitz_head"], BLITZ)
            self.assertEqual(info["wpt_revision"], WPT)
            self.assertEqual(info["tools_revision"], TOOLS)

    def test_override_only_changes_candidate_blitz(self):
        info = resolve({"pull_request": pull_request(body="blitz-revision: " + OVERRIDE)}, TOOLS, api)
        self.assertEqual(info["blitz_base"], BLITZ)
        self.assertEqual(info["blitz_head"], OVERRIDE)
        self.assertEqual(info["blitz_override"], OVERRIDE)


class PublishTest(unittest.TestCase):
    def setUp(self):
        self.pr = pull_request(body="Author's description")
        self.info = resolve({"pull_request": self.pr}, TOOLS, api)
        self.run = {"event": "pull_request", "conclusion": "success", "head_sha": HEAD}
        self.section = START_MARKER + "\nWPT results\n" + END_MARKER

    def test_current_result(self):
        self.assertTrue(current_result(self.info, self.run, self.pr, "DioxusLabs/taffy"))

    def test_stale_and_mismatched_results_are_ignored(self):
        variants = [
            ("run", "head_sha", "0" * 40), ("run", "event", "push"),
            ("run", "conclusion", "failure"), ("pr", "state", "closed"),
            ("pr", "number", 43), ("info", "repository", "DioxusLabs/parley"),
            ("pr", "body", "blitz-revision: " + OVERRIDE),
            ("pr", "body", "blitz-revision: invalid"),
        ]
        for target, key, value in variants:
            with self.subTest(target=target, key=key):
                objects = copy.deepcopy({"info": self.info, "run": self.run, "pr": self.pr})
                objects[target][key] = value
                self.assertFalse(current_result(objects["info"], objects["run"], objects["pr"], "DioxusLabs/taffy"))
        self.pr["head"]["sha"] = "0" * 40
        self.assertFalse(current_result(self.info, self.run, self.pr, "DioxusLabs/taffy"))

    def test_override_removal_invalidates_result(self):
        self.pr["body"] = "blitz-revision: " + OVERRIDE
        info = resolve({"pull_request": self.pr}, TOOLS, api)
        self.pr["body"] = "Override removed"
        self.assertFalse(current_result(info, self.run, self.pr, "DioxusLabs/taffy"))

    def test_splices_only_results_and_is_idempotent(self):
        body = result_body(self.pr["body"], self.section)
        self.assertTrue(body.startswith("Author's description"))
        self.assertEqual(result_body(body, self.section), body)
        self.assertEqual(body.count(START_MARKER), 1)

    def test_rejects_malformed_or_oversized_results(self):
        for section in ("unmarked", self.section + self.section, "before" + self.section):
            with self.subTest(section=section), self.assertRaises(ValueError):
                result_body("", section)
        with self.assertRaises(ValueError):
            result_body("x" * 65000, self.section)

    def test_publishes_only_to_verified_pr(self):
        calls = []

        def fake_api(path, body=None):
            calls.append((path, body))
            return self.pr

        self.assertTrue(publish(self.info, self.section, self.run, "DioxusLabs/taffy", fake_api))
        self.assertEqual(calls[1][0], "repos/DioxusLabs/taffy/pulls/42")
        self.assertIn("Author's description", calls[1][1]["body"])
        calls.clear()
        self.pr["head"]["sha"] = "0" * 40
        self.assertFalse(publish(self.info, self.section, self.run, "DioxusLabs/taffy", fake_api))
        self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
