"""Fast checks for the optional Linux memory profiler; no model/backend network."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("profile_memory", ROOT / "scripts/profile-memory.py")
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


class MemoryProfilerTests(unittest.TestCase):
    def test_proc_fields_require_kib_units(self):
        self.assertEqual(profile.kb_fields("VmRSS:\t123 kB\nVmHWM: 456 kB\nThreads: 4\n"),
                         {"VmRSS": 123, "VmHWM": 456})

    def test_pressure_records_are_exactly_sized_and_lf_framed(self):
        wire = profile.pressure_frame()
        self.assertEqual(len(wire), 128 * 1024)
        self.assertEqual(wire.count(b"\n"), 1)
        self.assertEqual(json.loads(wire)["type"], "tool_execution_update")
        with self.assertRaises(ValueError):
            profile.pressure_frame(1)

    def test_metrics_capture_receive_status_without_payloads(self):
        self.assertEqual(profile.METRICS.findall(
            "hibiscus RPC metrics: peak_queued_bytes=4096 peak_records=2 failed=true\n"),
            [("4096", "2", "true")])

    @unittest.skipUnless(sys.platform == "linux", "requires /proc")
    def test_process_counters_are_per_pid_and_have_a_birth_identifier(self):
        result = profile.process_memory(os.getpid())
        self.assertGreater(result["rss_kib"], 0)
        self.assertGreater(int(result["birth"]), 0)
        self.assertGreater(result["hwm_kib"], 0)
        self.assertIsNone(profile.process_memory(-1))

    @unittest.skipUnless(sys.platform == "linux", "profiler requires Linux")
    def test_failed_run_cannot_leave_a_stale_success_report(self):
        with tempfile.TemporaryDirectory(prefix="hibiscus-memory-failure-") as temp:
            report = Path(temp) / "report.json"
            report.write_text('{"complete":true}')
            result = subprocess.run([sys.executable, str(ROOT / "scripts/profile-memory.py"),
                                     "--binary", "/bin/true", "--scenario", "idle", "--output", str(report)],
                                    capture_output=True, timeout=5)
            self.assertNotEqual(result.returncode, 0)
            data = json.loads(report.read_text())
            self.assertFalse(data["complete"])
            self.assertEqual(data["results"], [])

    @unittest.skipUnless(sys.platform == "linux", "mock backend writes a /proc birth identifier")
    def test_mock_backend_accepts_unicode_separators_without_splitting_records(self):
        with tempfile.TemporaryDirectory(prefix="hibiscus-memory-test-") as temp:
            root = Path(temp)
            (root / "config.json").write_text(json.dumps({"scenario": "idle"}))
            commands = [{"id": "s", "type": "get_state"},
                        {"id": "p", "type": "prompt", "message": "one\u2028two\u2029three"}]
            wire = b"".join(json.dumps(c, ensure_ascii=False).encode() + b"\n" for c in commands)
            result = subprocess.run([sys.executable, str(ROOT / "scripts/profile-memory.py"), "--backend"],
                                    input=wire, capture_output=True, timeout=5,
                                    env={"MEMORY_PROFILE_ROOT": str(root)})
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            records = [json.loads(line) for line in result.stdout.split(b"\n") if line]
            self.assertEqual([r["id"] for r in records if r["type"] == "response"], ["s", "p"])
            self.assertEqual(records[0]["data"]["sessionName"], "MEMORY_STATE_1")
            self.assertEqual(records[-1]["type"], "agent_settled")


if __name__ == "__main__":
    unittest.main()
