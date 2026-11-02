"""Reject invalid coverage evidence rather than reporting an invented percentage."""
import copy
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("coverage_policy", ROOT / "scripts/coverage.py")
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


def report():
    return {"type": "llvm.coverage.json.export",
            "cargo_llvm_cov": {"version": policy.TOOL_VERSION, "manifest_path": str(ROOT / "Cargo.toml")},
            "data": [{"files": [{"filename": str(ROOT / "src/main.rs"),
                                  "summary": {m: {"count": 4, "covered": 3}
                                              for m in policy.METRICS}}]}]}


class CoverageEvidence(unittest.TestCase):
    def test_failed_tool_preflight_cannot_create_a_success_report(self):
        (ROOT / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="coverage-policy-", dir=ROOT / "target") as directory:
            root = Path(directory)
            binary = root / "cargo"
            binary.write_text("#!/bin/sh\nprintf 'cargo-llvm-cov wrong-version\\n'\n")
            binary.chmod(0o700)
            output = root / "report"
            result = subprocess.run(
                [os.sys.executable, str(ROOT / "scripts/coverage.py"), "--output", str(output)],
                cwd=ROOT, env={"PATH": str(root), "HOME": str(root)},
                capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Install cargo-llvm-cov", result.stderr)
            self.assertFalse((output / "summary.json").exists())

    def test_counters_are_recomputed_without_trusting_percentages(self):
        source = report()
        source["data"][0]["files"][0]["summary"]["lines"]["percent"] = 100
        result = policy.summary(source)
        self.assertEqual(result["totals"]["lines"]["percent"], 75)
        self.assertEqual(result["files"]["src/main.rs"]["branches"]["covered"], 3)

    def test_invalid_or_incomplete_evidence_is_rejected(self):
        original = report()
        changes = [lambda r: r.update(type="other"),
                   lambda r: r["cargo_llvm_cov"].update(version="unreviewed"),
                   lambda r: r["cargo_llvm_cov"].update(manifest_path="/other/Cargo.toml"),
                   lambda r: r.update(data=[]),
                   lambda r: r["data"][0]["files"].append(copy.deepcopy(r["data"][0]["files"][0])),
                   lambda r: r["data"][0]["files"][0].update(filename=str(ROOT / "tests/example.rs")),
                   lambda r: r["data"][0]["files"][0]["summary"]["lines"].update(covered=0),
                   lambda r: r["data"][0]["files"][0]["summary"]["lines"].update(covered=True),
                   lambda r: r["data"][0]["files"][0]["summary"]["regions"].update(covered=5),
                   lambda r: r["data"][0]["files"][0]["summary"]["branches"].update(count=0, covered=0)]
        for change in changes:
            with self.subTest(change=changes.index(change)):
                candidate = copy.deepcopy(original)
                change(candidate)
                with self.assertRaises(ValueError):
                    policy.summary(candidate)


if __name__ == "__main__":
    unittest.main()
