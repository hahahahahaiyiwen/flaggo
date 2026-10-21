import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import run
import domain_cases
import validation_cases


class ParityHarnessTests(unittest.TestCase):
    def test_validation_cases_keep_context_and_values_as_raw_json_text(self):
        requests = validation_cases.cases(run.ROOT)
        self.assertEqual(len(requests), len({request["id"] for request in requests}))
        self.assertEqual({"contract", "schema", "runtimeInput", "executable"},
                         {request["domainModel"] for request in requests})
        self.assertTrue(all(isinstance(request["document"], str) for request in requests))
        values = [request for request in requests if request["domainAction"] == "validateValue"]
        self.assertTrue(values)
        self.assertTrue(all(isinstance(request["validationValue"], str) for request in values))
        contexts = [request for request in requests if request["domainModel"] in {"runtimeInput", "executable"}]
        self.assertTrue(all(isinstance(request["validationContract"], str) for request in contexts))

    def test_domain_cases_preserve_raw_duplicates_and_polymorphic_order(self):
        requests = domain_cases.cases(run.ROOT)
        self.assertEqual(len(requests), len({request["id"] for request in requests}))
        by_id = {request["id"]: request for request in requests}
        self.assertEqual('{"a":1,"a":2}', by_id["domain-json-duplicate"]["document"])
        self.assertIn('"\\u0061"', by_id["domain-json-escaped-duplicate"]["document"])
        fixture = json.loads(by_id["domain-fixture"]["document"])
        source = fixture["learning"]["evidence"][0]["source"]
        self.assertEqual("kind", next(iter(source)))
        self.assertTrue(all(isinstance(request["document"], str) for request in requests))

    def test_domain_faults_remain_distinct_from_json_errors(self):
        requests = [{"id": "unicode", "domainModel": "json", "document": '"\\ud800"'}]
        expected = {"unicode": {"status": "runtime-fault"}}
        actual = {"unicode": {"status": "json-error"}}
        comparison = run.compare(requests, expected, actual)
        self.assertEqual(0, comparison["matched"])
        self.assertEqual('"\\ud800"', comparison["mismatches"][0]["document"])

    def test_canonical_difference_fails_even_when_values_match(self):
        requests = [{"id": "a", "expression": "1+2"}]
        expected = {"a": {"status": "value", "value": 3, "canonical": "1 + 2"}}
        actual = {"a": {"status": "value", "value": 3, "canonical": "(1 + 2)"}}
        self.assertEqual(1, run.compare(requests, expected, actual)["matched"])
        self.assertEqual(0, run.compare(requests, expected, actual, canonical=True)["matched"])
        del actual["a"]["canonical"]
        self.assertEqual(0, run.compare(requests, expected, actual, canonical=True)["matched"])
        del expected["a"]["canonical"]
        with self.assertRaisesRegex(ValueError, "missing its canonical expression"):
            run.compare(requests, expected, actual, canonical=True)

    def test_boundary_cases_check_exact_utf8_lengths_and_admission(self):
        cases = {case["id"]: case for case in run.boundary_cases()}
        self.assertEqual(4096, len(cases["expression-bytes-at-limit"]["expression"].encode("utf-8")))
        self.assertEqual(4097, len(cases["expression-bytes-over-limit"]["expression"].encode("utf-8")))
        self.assertEqual(4096, len(cases["utf8-bytes-at-limit"]["expression"].encode("utf-8")))
        self.assertEqual(4098, len(cases["utf8-bytes-over-limit"]["expression"].encode("utf-8")))
        responses = {identifier: {"status": "compile-error" if identifier.endswith("-over-limit")
                                 else "compiled"} for identifier in cases}
        run.validate_boundaries(responses)
        responses["static-cost-at-limit"]["status"] = "compile-error"
        with self.assertRaisesRegex(RuntimeError, "static-cost-at-limit"):
            run.validate_boundaries(responses)

    def test_restart_requires_checked_artifacts_and_never_valid_source(self):
        requests = [{"id": "a", "expression": "1 / 0"}, {"id": "b", "expression": "["}]
        reference = {"a": {"status": "evaluation-error", "checked": {"checkedAst": "bytes"}},
                     "b": {"status": "compile-error"}}
        restart = run.restart_cases(requests, reference)
        self.assertEqual(1, len(restart))
        self.assertEqual("bytes", restart[0]["checked"]["checkedAst"])
        self.assertNotEqual(requests[0]["expression"], restart[0]["expression"])
        del reference["a"]["checked"]
        with self.assertRaisesRegex(ValueError, "missing its checked expression"):
            run.restart_cases(requests, reference)

    def test_failed_probe_preserves_a_failure_report(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "failure.json"
            with patch("sys.argv", ["run.py", "--report", str(report)]):
                with patch("run.execute", side_effect=RuntimeError("probe crashed")):
                    with patch("sys.stderr"):
                        self.assertEqual(2, run.main())
            self.assertEqual(
                {"outcome": "probe-failure", "error": "probe crashed"},
                json.loads(report.read_text(encoding="utf-8")),
            )

    def test_json_value_comparison_preserves_boolean_and_container_types(self):
        self.assertTrue(run.same_value({"a": [1, None]}, {"a": [1.0, None]}))
        self.assertFalse(run.same_value(True, 1))
        self.assertFalse(run.same_value({"a": True}, {"a": 1}))
        self.assertFalse(run.same_value([1], {"0": 1}))
        self.assertFalse(run.same_value(float("inf"), float("inf")))

    def test_errors_and_different_values_are_mismatches(self):
        requests = [{"id": "a", "expression": "size('test')"}]
        expected = {"a": {"status": "compile-error", "message": "reference"}}
        actual = {"a": {"status": "value", "value": 4}}
        self.assertEqual(1, len(run.compare(requests, expected, actual)["mismatches"]))
        expected["a"] = {"status": "value", "value": 5}
        self.assertEqual(1, len(run.compare(requests, expected, actual)["mismatches"]))

    def test_error_wording_is_not_a_semantic_success(self):
        requests = [{"id": "a", "expression": "1 / 0"}]
        expected = {"a": {"status": "evaluation-error", "message": "reference"}}
        actual = {"a": {"status": "evaluation-error", "message": "candidate"}}
        self.assertEqual(1, run.compare(requests, expected, actual)["matched"])
        actual["a"]["status"] = "runtime-fault"
        self.assertEqual(0, run.compare(requests, expected, actual)["matched"])

    def test_process_crashes_surface_stderr(self):
        result = subprocess.CompletedProcess(["probe"], 1, "", "unexpected failure")
        with patch("run.subprocess.run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "unexpected failure"):
                run.output(["probe"], [{"id": "a"}])

    def test_response_ids_and_shapes_are_required(self):
        invalid = [
            [],
            [None],
            [{"id": "a"}],
            [{"id": "a", "status": "compiled"}, {"id": "a", "status": "compiled"}],
            [{"id": "wrong", "status": "compiled"}],
            [{"id": "a", "status": "invented"}],
            [{"id": "a", "status": "value"}],
        ]
        for responses in invalid:
            with self.subTest(responses=responses):
                result = subprocess.CompletedProcess(
                    ["probe"], 0, "".join(json.dumps(value) + "\n" for value in responses), ""
                )
                with patch("run.subprocess.run", return_value=result):
                    with self.assertRaises(ValueError):
                        run.output(["probe"], [{"id": "a"}])


if __name__ == "__main__":
    unittest.main()
