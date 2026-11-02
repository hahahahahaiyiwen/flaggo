import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import http_cases
import run


def results():
    return [
        {"format": "flaggo.http-parity/v1", "service": service,
         "producer": producer, "comparisons": count}
        for service, count in [("contract", 39), ("decision", 29)]
        for producer in ["dotnet", "rust"]
    ]


def output(records):
    return "".join(http_cases.PREFIX + json.dumps(record) + "\n" for record in records)


class HttpAcceptanceTests(unittest.TestCase):
    def test_requires_both_services_and_producers_and_measured_counts(self):
        report = http_cases.parse_results("running 1 test\n" + output(results()))
        self.assertEqual("passed", report["status"])
        self.assertEqual(136, report["cases"])
        self.assertEqual(136, report["matched"])
        self.assertEqual([], report["mismatches"])
        self.assertEqual({"dotnet": 39, "rust": 39}, report["services"]["contract"])
        expanded = results()
        expanded[0]["comparisons"] += 1
        self.assertEqual(137, http_cases.parse_results(output(expanded))["cases"])

    def test_zero_skipped_or_missing_test_results_cannot_pass(self):
        for text in ["", "running 0 tests\ntest result: ok. 0 passed; 1 ignored\n",
                     output(results()[:-1]), output(results()[:2])]:
            with self.subTest(text=text):
                with self.assertRaisesRegex(ValueError, "Missing HTTP parity results"):
                    http_cases.parse_results(text)

    def test_duplicate_malformed_and_wrong_protocol_results_fail(self):
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            http_cases.parse_results(output(results() + results()[:1]))
        for record in [None, [], {}, {"format": "invented"}, {**results()[0], "extra": 1}]:
            with self.subTest(record=record):
                with self.assertRaises(ValueError):
                    http_cases.parse_results(output([record]))
        with self.assertRaises(json.JSONDecodeError):
            http_cases.parse_results(http_cases.PREFIX + "{")

    def test_invalid_identity_types_and_subminimum_counts_fail(self):
        for key, values in [
            ("format", ["future", None]),
            ("service", ["unknown", ["contract"], None]),
            ("producer", ["unknown", ["rust"], None]),
            ("comparisons", [True, False, 0, -1, 38, "39", 39.0, None]),
        ]:
            for value in values:
                with self.subTest(key=key, value=value):
                    changed = results()
                    changed[0] = {**changed[0], key: value}
                    with self.assertRaises(ValueError):
                        http_cases.parse_results(output(changed))

    def test_default_does_not_build_launch_or_reuse_stale_success(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "old.http.log"
            path.write_text(output(results()), encoding="utf-8")
            with patch("http_cases.subprocess.run") as command:
                report = http_cases.verify(Path(directory), path,
                                           enabled=False, build_reference=True)
            command.assert_not_called()
            self.assertEqual("not-requested", report["status"])
            self.assertNotIn("matched", report)
            self.assertEqual(output(results()), path.read_text(encoding="utf-8"))

    def test_builds_both_release_references_and_selects_both_ignored_http_gates(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.http.log"
            responses = [subprocess.CompletedProcess([], 0, "built\n", ""),
                         subprocess.CompletedProcess([], 0, "built\n", ""),
                         subprocess.CompletedProcess([], 0, output(results()), "cargo diagnostics")]
            with patch("http_cases.subprocess.run", side_effect=responses) as command:
                report = http_cases.verify(Path(directory), path,
                                           enabled=True, build_reference=True,
                                           reference_root=Path(directory) / "oracle")
            calls = [call.args[0] for call in command.call_args_list]
            self.assertEqual(["dotnet", "dotnet", "cargo"], [call[0] for call in calls])
            self.assertIn("Flaggo.ContractService.csproj", calls[0][2])
            self.assertIn("Flaggo.DecisionService.csproj", calls[1][2])
            self.assertIn(str(Path(directory) / "oracle"), calls[0][2])
            self.assertEqual(Path(directory) / "oracle", command.call_args_list[0].kwargs["cwd"])
            self.assertEqual(Path(directory), command.call_args_list[2].kwargs["cwd"])
            self.assertEqual(str(Path(directory) / "oracle"),
                             command.call_args_list[2].kwargs["env"]["FLAGGO_DOTNET_REFERENCE_ROOT"])
            for build in calls[:2]:
                self.assertEqual("Release", build[build.index("--configuration") + 1])
            for token in ["--locked", "flaggo-contract-service", "flaggo-decision-service",
                          "--test", "http", "actual_dotnet_", "--ignored", "--nocapture"]:
                self.assertIn(token, calls[2])
            self.assertEqual(136, report["matched"])
            self.assertEqual(str(path), report["log"])
            self.assertIn("cargo diagnostics", path.read_text(encoding="utf-8"))

    def test_no_build_skips_only_reference_builds_and_still_executes_real_tests(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch("http_cases.subprocess.run", return_value=subprocess.CompletedProcess(
                    [], 0, output(results()), "")) as command:
                http_cases.verify(Path(directory), Path(directory) / "http.log",
                                  enabled=True, build_reference=False,
                                  reference_root=Path(directory) / "oracle")
            command.assert_called_once()
            self.assertEqual("cargo", command.call_args.args[0][0])

    def test_failed_reference_build_stops_before_tests_and_preserves_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "http.log"
            with patch("http_cases.subprocess.run", return_value=subprocess.CompletedProcess(
                    [], 1, "", "reference restore failed")) as command:
                with self.assertRaisesRegex(RuntimeError, "exited 1"):
                    http_cases.verify(Path(directory), path, enabled=True, build_reference=True,
                                      reference_root=Path(directory) / "oracle")
            command.assert_called_once()
            self.assertIn("reference restore failed", path.read_text(encoding="utf-8"))

    def test_nonzero_test_exit_wins_even_with_complete_success_markers(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "http.log"
            with patch("http_cases.subprocess.run", return_value=subprocess.CompletedProcess(
                    [], 101, output(results()), "assertion failed")):
                with self.assertRaisesRegex(RuntimeError, "exited 101"):
                    http_cases.verify(Path(directory), path, enabled=True, build_reference=False,
                                      reference_root=Path(directory) / "oracle")
            self.assertIn("assertion failed", path.read_text(encoding="utf-8"))

    def test_cli_http_requires_owned_profile_without_executing_probes(self):
        with patch("sys.argv", ["run.py", "--http", "--reference-root", "oracle",
                                "--report", "unused.json"]):
            with patch("run.execute") as execute, patch("sys.stderr"):
                with self.assertRaises(SystemExit) as error:
                    run.main()
            self.assertEqual(2, error.exception.code)
            execute.assert_not_called()

    def test_cli_default_keeps_http_opt_in(self):
        with patch("sys.argv", ["run.py", "--profile", "--reference-root", "oracle",
                                "--report", "unused.json"]):
            with patch("run.execute", return_value=0) as execute:
                self.assertEqual(0, run.main())
            self.assertFalse(execute.call_args.args[0].http)

    def test_cli_failure_writes_non_success_report_with_http_log_reference(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "failure.json"
            with patch("sys.argv", ["run.py", "--profile", "--http", "--reference-root", "oracle",
                                    "--report", str(path)]):
                with patch("run.execute", side_effect=ValueError("Missing HTTP parity results")):
                    with patch("sys.stderr"):
                        self.assertEqual(2, run.main())
            report = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual("probe-failure", report["outcome"])
            self.assertEqual("not-completed", report["httpServices"]["status"])
            self.assertEqual(str(path.with_suffix(".http.log")), report["httpServices"]["log"])

    def test_live_http_cannot_fall_back_to_current_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch("http_cases.subprocess.run") as command:
                with self.assertRaisesRegex(ValueError, "explicit historical reference"):
                    http_cases.verify(Path(directory), Path(directory) / "http.log",
                                      enabled=True, build_reference=False)
            command.assert_not_called()

    def test_cli_requires_explicit_reference_before_any_probe(self):
        with patch("sys.argv", ["run.py", "--profile", "--report", "unused.json"]):
            with patch("run.execute") as execute, patch("sys.stderr"):
                with self.assertRaises(SystemExit) as error:
                    run.main()
            self.assertEqual(2, error.exception.code)
            execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()
