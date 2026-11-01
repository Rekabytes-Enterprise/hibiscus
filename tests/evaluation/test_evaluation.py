"""Exercise scoring against actual native tools, not assistant success messages."""
from dataclasses import replace
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from tools.evaluation.execution import environment, process, session_evidence
from tools.evaluation.fixture import Fixture
from tools.evaluation.runner import evaluate, inspect_variant, run_case, write_report
from tools.evaluation.spec import InvalidSpec, inventory, load_plan, load_suite

ROOT = Path(__file__).resolve().parents[2]
BINARY = Path(os.environ.get("HIBISCUS_EVAL_BINARY", ROOT / "target/debug/hibiscus")).resolve()


class SpecTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hibiscus-eval-spec-")
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "evals", self.root / "evals")
        self.path = self.root / "evals/offline.json"

    def tearDown(self):
        self.temp.cleanup()

    def change(self, field, value):
        data = json.loads(self.path.read_text())
        data[field] = value
        self.path.write_text(json.dumps(data))

    def test_live_requires_matching_explicit_flag_and_key_name(self):
        with self.assertRaises(InvalidSpec):
            load_plan(self.path, live=True)
        path = self.path.with_name("live.example.json")
        with self.assertRaises(InvalidSpec):
            load_plan(path)
        self.assertEqual(load_plan(path, live=True).mode, "live")
        data = json.loads(path.read_text())
        data["provider"]["apiKeyEnv"] = "LD_PRELOAD"
        path.write_text(json.dumps(data))
        with self.assertRaises(InvalidSpec):
            load_plan(path, live=True)

    def test_rejects_ambiguous_keys_unknown_fields_and_invalid_bounds(self):
        source = self.path.read_text()
        for text in (source.replace('"formatVersion": 1', '"formatVersion": 1, "formatVersion": 1'),
                     source.replace('"repetitions": 2', '"repetitions": true'),
                     source.replace('"maxSteps": 8', '"maxSteps": 0'),
                     source.replace('"mode": "offline"', '"mode": "offline", "extra": 1')):
            self.path.write_text(text)
            with self.assertRaises(InvalidSpec):
                load_plan(self.path)

    def test_manifest_paths_cannot_escape_and_inventory_rejects_symlinks(self):
        self.change("suite", "../other.json")
        with self.assertRaises(InvalidSpec):
            load_plan(self.path)
        tree = self.root / "tree"
        tree.mkdir()
        (tree / "linked").symlink_to(self.path)
        with self.assertRaises(InvalidSpec):
            inventory(tree)
        self.change("suite", "linked/suite.json")
        (self.root / "evals/linked").symlink_to(self.root / "evals", target_is_directory=True)
        with self.assertRaises(InvalidSpec):
            load_plan(self.path)

    def test_splits_and_task_ids_are_explicit_and_disjoint(self):
        suite = self.root / "evals/suite.json"
        raw = json.loads(suite.read_text())
        raw["tasks"] = raw["tasks"][:2]
        suite.write_text(json.dumps(raw))
        with self.assertRaises(InvalidSpec):
            load_suite(suite)

    def test_frozen_input_identity_changes_with_instruction_or_file_bytes(self):
        first = load_plan(self.path)
        (self.root / "evals/variants/candidate.md").write_text("Different instruction\n")
        second = load_plan(self.path)
        self.assertNotEqual(first.identity, second.identity)
        self.assertNotEqual(first.variants[1].instructions, second.variants[1].instructions)
        self.assertEqual(first.variants[0].identity, second.variants[0].identity)

    def test_report_is_private_and_never_overwrites(self):
        path = self.root / "report.json"
        write_report({"formatVersion": 1}, path)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            write_report({}, path)
        self.assertEqual(json.loads(path.read_text()), {"formatVersion": 1})


class FlowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not BINARY.is_file():
            raise RuntimeError("Build Hibiscus or set HIBISCUS_EVAL_BINARY before evaluation tests")
        cls.plan = load_plan(ROOT / "evals/offline.json")
        cls.suite = load_suite(cls.plan.suite)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hibiscus-eval-test-")
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def case(self, task=None, profile="repair", plan=None):
        return run_case(plan or self.plan, task or self.suite.tasks[1],
                        self.plan.variants[0], BINARY, self.root / "run", profile)

    def test_real_edit_tests_and_grading_pass_for_every_split(self):
        for i, task in enumerate(self.suite.tasks):
            with self.subTest(task=task.id):
                result = run_case(self.plan, task, self.plan.variants[0], BINARY, self.root / str(i))
                self.assertTrue(result["solved"], result)
                self.assertEqual(result["objective"]["changedPaths"], ["app.py"])
                self.assertEqual([t["name"] for t in result["tools"]], ["read", "edit", "bash"])
                self.assertEqual(result["tools"][-1]["exitCode"], 0)
                self.assertEqual(result["tokens"], {"input": 80, "output": 20, "cacheRead": 0})
                self.assertIsNone(result["costUsd"])

    def test_success_claim_with_no_mutation_is_structurally_valid_but_unsolved(self):
        result = self.case(profile="claim_only")
        self.assertTrue(result["structure"]["valid"])
        self.assertEqual(result["execution"]["exitCode"], 0)
        self.assertFalse(result["objective"]["checksPassed"])
        self.assertFalse(result["objective"]["requiredFilesChanged"])
        self.assertFalse(result["solved"])

    def test_unknown_usage_is_null_not_zero_and_tools_still_run(self):
        result = self.case(profile="missing_usage")
        self.assertTrue(result["solved"])
        self.assertEqual(result["tokens"], {"input": None, "output": None, "cacheRead": None})

    def test_tool_and_provider_failures_cannot_be_counted_as_solutions(self):
        for index, profile in enumerate(("bad_tool", "provider_error")):
            result = run_case(self.plan, self.suite.tasks[1], self.plan.variants[0],
                              BINARY, self.root / str(index), profile)
            self.assertFalse(result["solved"])
            if profile == "bad_tool":
                self.assertTrue(result["structure"]["valid"])
                self.assertTrue(all(t["isError"] for t in result["tools"]))
            else:
                self.assertEqual(result["execution"]["error"], "nonzero_exit")

    def test_modified_visible_tests_or_notes_fail_even_with_correct_implementation(self):
        task = self.suite.tasks[1]
        batches = list(task.offline)
        batches.insert(2, json.dumps([{"name": "write", "arguments": {"path": "notes.txt", "content": "overwritten"}}]))
        result = self.case(replace(task, offline=tuple(batches)))
        self.assertTrue(result["objective"]["checksPassed"])
        self.assertFalse(result["objective"]["preservedFilesUnchanged"])
        self.assertFalse(result["solved"])

    def test_agent_workspace_has_no_grader_or_other_split(self):
        task = self.suite.tasks[1]
        batches = list(task.offline)
        batches.insert(0, json.dumps([{"name": "bash", "arguments": {"command":
            "test ! -e verify.py && test ! -e ../verify.py && test ! -e totals && test ! -e task.json"}}]))
        result = self.case(replace(task, offline=tuple(batches)))
        self.assertTrue(result["solved"])
        self.assertEqual(result["tools"][0]["exitCode"], 0)

    def test_repeated_comparison_is_paired_and_starters_fail(self):
        report = evaluate(ROOT / "evals/offline.json", BINARY)
        self.assertTrue(report["structure"]["comparable"])
        self.assertEqual(len(report["results"]), 4)
        self.assertFalse(report["initial"][0]["checksPassed"])
        self.assertEqual([r["variantId"] for r in report["results"]],
                         ["baseline", "candidate", "candidate", "baseline"])
        self.assertEqual(report["summary"][1]["versusBaseline"], {"wins": 0, "losses": 0, "ties": 2})
        self.assertTrue(all(r["solved"] and r["split"] == "selection" for r in report["results"]))
        self.assertNotIn("SYNTHETIC", json.dumps(report))

    def test_native_preflight_rejects_invalid_extension_without_running_code(self):
        from tools.evaluation.spec import Image
        marker = self.root / "must-not-run"
        manifest = {"apiVersion": 999, "id": "probe", "description": "Invalid",
                    "command": ["/bin/sh", "-c", f"touch {marker}"], "tools": []}
        variant = replace(self.plan.variants[0], extensions=(("probe", (
            Image("extension.json", json.dumps(manifest).encode(), 0o600),)),))
        result = inspect_variant(self.plan, variant, BINARY, self.root / "probe")
        self.assertFalse(result["valid"])
        self.assertIn("native_extension_validation_failed", result["errors"])
        self.assertFalse(marker.exists())

    def test_valid_changed_tool_catalog_is_incomparable_without_task_scores(self):
        source = self.root / "evals"
        shutil.copytree(ROOT / "evals", source)
        extension = source / "variants/probe"
        extension.mkdir()
        manifest = {"apiVersion": 1, "id": "probe", "description": "Catalog probe",
                    "command": ["/bin/sh", "-c", "exit 0"], "tools": [
                        {"name": "inspect", "description": "Inspect", "parameters": {"type": "object"}}]}
        (extension / "extension.json").write_text(json.dumps(manifest))
        path = source / "offline.json"
        raw = json.loads(path.read_text())
        raw["variants"][1]["extensions"] = ["variants/probe"]
        path.write_text(json.dumps(raw))
        report = evaluate(path, BINARY)
        self.assertTrue(report["structure"]["valid"], report["structure"])
        self.assertFalse(report["structure"]["comparable"])
        self.assertEqual(report["results"], [])
        self.assertIsNone(report["summary"][0]["solveRate"])
        self.assertTrue(report["structure"]["variants"][1]["extensions"][0]["contentDigest"])

    def test_live_execution_branch_uses_only_explicit_key_and_never_copies_it_to_report(self):
        # Test the live branch through loopback, not a real account or network endpoint.
        from unittest.mock import patch
        task = self.suite.tasks[1]
        with Fixture(task.offline) as fixture:
            provider = {**self.plan.provider, "baseUrl": fixture.base_url,
                        "apiKeyEnv": "HIBISCUS_EVAL_API_KEY"}
            plan = replace(self.plan, mode="live", provider=provider)
            with patch.dict(os.environ, {"HIBISCUS_EVAL_API_KEY": "SYNTHETIC-LIVE-BRANCH",
                                         "HIBISCUS_MODEL": "wrong/model"}):
                result = self.case(plan=plan)
        self.assertTrue(result["solved"], result)
        self.assertEqual(len(fixture.requests), 4)
        self.assertIsNone(result["fixtureProfile"])
        self.assertNotIn("SYNTHETIC-LIVE-BRANCH", json.dumps(result))

    def test_bounded_process_caps_output_and_completed_leader_descendants(self):
        root = self.root / "output"
        root.mkdir()
        result = process(["/bin/sh", "-c", "yes output"], root, environment(root), 3, root / "process")
        self.assertEqual(result["error"], "output_limit")
        self.assertLess(result["durationMs"], 3000)
        pidfile = root / "descendant"
        result = process(["/bin/sh", "-c", f"sleep 30 & echo $! > {pidfile}; exit 0"],
                         root, environment(root), 3, root / "leader")
        self.assertIsNone(result["error"])
        status = subprocess.run(["/bin/ps", "-o", "stat=", "-p", pidfile.read_text().strip()],
                                capture_output=True, text=True, timeout=5).stdout.strip()
        self.assertTrue(not status or status.startswith("Z"), status)

    def test_max_steps_stays_in_denominator_and_partial_usage_is_retained(self):
        plan = replace(self.plan, limits={**self.plan.limits, "maxSteps": 2})
        result = self.case(plan=plan)
        self.assertFalse(result["solved"])
        self.assertEqual(result["execution"]["error"], "nonzero_exit")
        self.assertEqual(result["tokens"]["input"], 40)
        self.assertTrue(result["tools"])

    def test_timeout_kills_shell_descendants_and_does_not_hang(self):
        root = self.root / "timeout"
        root.mkdir()
        pidfile = root / "pid"
        result = process(["/bin/sh", "-c", f"sleep 30 & echo $! > {pidfile}; wait"],
                         root, environment(root), 0.2, root / "process")
        self.assertEqual(result["error"], "timeout")
        self.assertLess(result["durationMs"], 3000)
        pid = int(pidfile.read_text())
        # Zombies can briefly await the host's init reaper. They cannot run code.
        status = subprocess.run(["/bin/ps", "-o", "stat=", "-p", str(pid)],
                                capture_output=True, text=True, timeout=5).stdout.strip()
        self.assertTrue(not status or status.startswith("Z"), status)

    def test_missing_session_is_invalid_and_retains_unknown_usage(self):
        evidence = session_evidence(self.root, ["evaluation", "model"], "prompt")
        self.assertFalse(evidence["valid"])
        self.assertIsNone(evidence["tokens"])


if __name__ == "__main__":
    unittest.main()
