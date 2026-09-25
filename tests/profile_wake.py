"""Checks for the optional synthetic terminal-wake profiler."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("profile_wake", ROOT / "scripts/profile-wake.py")
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


class WakeProfilerTests(unittest.TestCase):
    def test_mock_responses_keep_ids_and_wait_for_abort_before_settlement(self):
        commands = [{"id": "state", "type": "get_state"},
                    {"id": "prompt", "type": "prompt", "message": "one\u2028two"},
                    {"id": "clear", "type": "clear_queue"},
                    {"id": "abort", "type": "abort"}]
        wire = b"".join(json.dumps(c, ensure_ascii=False).encode() + b"\n" for c in commands)
        result = subprocess.run([sys.executable, str(ROOT / "scripts/profile-wake.py"), "--backend"],
                                input=wire, capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        rows = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual([r["id"] for r in rows if r["type"] == "response"],
                         ["state", "prompt", "clear", "abort"])
        self.assertEqual(rows[-1]["type"], "agent_settled")

    @unittest.skipUnless(sys.platform == "linux", "requires a Linux controlling PTY")
    def test_probe_completes_without_real_pi_or_credentials(self):
        binary = ROOT / "target/release/hibiscus"
        if not binary.exists():
            self.skipTest("build with cargo build --release --locked first")
        with tempfile.TemporaryDirectory(prefix="hibiscus-wake-check-") as temp:
            report = Path(temp) / "result.json"
            result = subprocess.run([sys.executable, str(ROOT / "scripts/profile-wake.py"),
                                     "--binary", str(binary), "--samples", "5",
                                     "--output", str(report)], capture_output=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            data = json.loads(report.read_text())
            self.assertEqual(data["samples"], 5)
            self.assertGreater(data["min_ms"], 0)
            self.assertLessEqual(data["min_ms"], data["median_ms"])
            self.assertLessEqual(data["median_ms"], data["max_ms"])


if __name__ == "__main__":
    unittest.main()
