import json
import subprocess
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import run
import domain_cases
import decision_cases
import expression_cases
import json_fault_cases
import store_cases
import executable_store_cases
import validation_cases


class ParityHarnessTests(unittest.TestCase):
    def test_executable_store_vectors_cover_lifecycle_and_admission_boundaries(self):
        operations = executable_store_cases.operations()
        self.assertEqual(len(operations), len({operation["id"] for operation in operations}))
        by_id = {operation["id"]: operation for operation in operations}
        self.assertEqual("resolve", by_id["resolve-activate"]["action"])
        self.assertEqual(0, by_id["resolve-activate"]["resolution"])
        self.assertEqual(1, by_id["reject"]["resolution"])
        self.assertEqual(1001, by_id["pending-size-1001"]["pageSize"])
        self.assertTrue(by_id["cancel-admit"]["cancelled"])
        self.assertTrue(all(isinstance(op["document"], str)
                            for op in operations if op["action"] in ("put", "admit", "accept")))

    def test_executable_store_replies_fail_closed_on_missing_or_reordered_operations(self):
        ops = [{"id": "a"}, {"id": "b"}]
        replies = [{"id": "a", "outcome": "ok"}, {"id": "b", "outcome": "error"}]
        self.assertEqual(2, len(executable_store_cases.operation_pairs(ops, replies, replies)))
        with self.assertRaisesRegex(ValueError, "count mismatch"):
            executable_store_cases.operation_pairs(ops, replies, replies[:1])
        with self.assertRaisesRegex(ValueError, "reordered/missing"):
            executable_store_cases.operation_pairs(ops, replies, list(reversed(replies)))

    def test_executable_store_reference_cannot_pass_with_only_rejections(self):
        replies = [{"id": operation["id"], "outcome": "error"}
                   for operation in executable_store_cases.operations()]
        with self.assertRaisesRegex(RuntimeError, "immutable executable persistence"):
            executable_store_cases.validate_reference(replies)

    def test_store_vectors_exercise_real_versions_cursors_and_native_faults(self):
        operations = store_cases.operations()
        self.assertEqual(len(operations), len({operation["id"] for operation in operations}))
        by_id = {operation["id"]: operation for operation in operations}
        self.assertEqual("health", by_id["health-before"]["action"])
        self.assertEqual("initialize", by_id["initialize"]["action"])
        self.assertTrue(all(isinstance(operation["document"], str)
                            for operation in operations if operation["action"] == "put"))
        self.assertEqual(200, by_id["page-size-200"]["pageSize"])
        self.assertEqual(201, by_id["page-size-201"]["pageSize"])
        self.assertEqual("page1", by_id["page2"]["cursorFrom"])
        self.assertIn("minimum", by_id["corrupt-infinite-schema"]["query"])
        self.assertTrue(by_id["cancel-put"]["cancelled"])

    def test_store_snapshot_preserves_raw_blob_and_timestamp_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.db"
            connection = sqlite3.connect(path)
            try:
                connection.executescript("""
                    CREATE TABLE decision_contract_versions (
                        contract_name TEXT, contract_digest TEXT, accepted_at TEXT, contract_json BLOB);
                    CREATE TABLE decision_contract_resources (
                        contract_name TEXT, tenant TEXT, application TEXT, environment TEXT);
                    CREATE TABLE decision_contract_current (contract_name TEXT, contract_digest TEXT);
                    CREATE TABLE flaggo_schema_versions (component TEXT, version INTEGER);
                """)
                blob = b'{"n":1.00,"z":-0e0}'
                time = "2026-03-01T10:00:00.1234567+00:00"
                connection.execute("INSERT INTO decision_contract_versions VALUES(?,?,?,?)",
                                   ("name", "digest", time, blob))
                connection.commit()
            finally:
                connection.close()
            first = store_cases.snapshot(path)
            self.assertEqual(("name", "digest", time, blob), first["versions"][0])
            self.assertEqual(first, store_cases.snapshot(path))

    def test_decision_vectors_cover_runtime_and_evaluator_with_raw_documents(self):
        requests = decision_cases.cases()
        self.assertEqual(len(requests), len({request["id"] for request in requests}))
        self.assertEqual({"compile", "evaluate", "runtime"},
                         {request["decisionAction"] for request in requests})
        self.assertTrue(all(isinstance(request["contract"], str) for request in requests))
        self.assertTrue(all(isinstance(request["input"], str) for request in requests))
        by_id = {request["id"]: request for request in requests}
        self.assertEqual(256, len(json.loads(by_id["decision-rule-limit-256"]["executable"])["rules"]))
        self.assertEqual(257, len(json.loads(by_id["decision-rule-limit-257"]["executable"])["rules"]))
        self.assertIn("activeFault", by_id["decision-input-before-active"])

    def test_decision_restart_requires_artifacts_and_discards_admission_source(self):
        request = decision_cases.request("first-match")
        artifact = {"executable": {}, "executableDigest": "digest",
                    "checkedExecutable": {"rules": []}}
        responses = {request["id"]: {"status": "value",
                     "value": {"outcome": "decision"}, "artifact": artifact}}
        restarted = decision_cases.restart_cases([request], responses)
        self.assertEqual(artifact, restarted[0]["artifact"])
        with self.assertRaises(json.JSONDecodeError):
            json.loads(restarted[0]["executable"])
        restarted[0]["artifact"]["executable"]["changed"] = True
        self.assertNotIn("changed", artifact["executable"])
        del responses[request["id"]]["artifact"]
        with self.assertRaisesRegex(ValueError, "missing its persisted executable"):
            decision_cases.restart_cases([request], responses)

    def test_decision_fault_classification_and_provenance_are_not_ignored(self):
        request = {"id": "a"}
        expected = {"a": {"status": "value", "value": {
            "outcome": "error", "telemetry": {"outcome": "failure", "category": "integrity"},
            "exceptionType": "Flaggo.Decision.DecisionResultValidationException",
        }}}
        actual = json.loads(json.dumps(expected))
        actual["a"]["value"]["telemetry"]["category"] = "internal"
        self.assertEqual(0, run.compare([request], expected, actual)["matched"])
        expected["a"]["value"] = {"outcome": "decision",
                                  "decision": {"evaluation": {"source": "rule", "rule": "first"}}}
        actual["a"]["value"] = {"outcome": "decision",
                                "decision": {"evaluation": {"source": "default"}}}
        self.assertEqual(0, run.compare([request], expected, actual)["matched"])

    def test_json_fault_vectors_preserve_raw_lexemes_and_admission_context(self):
        requests = json_fault_cases.cases(run.ROOT)
        self.assertEqual(len(requests), len({request["id"] for request in requests}))
        self.assertTrue(all(isinstance(request["document"], str) for request in requests))
        self.assertTrue(any(request.get("validationValue") == "1e400" for request in requests))
        self.assertTrue(any(request["domainAction"] == "rawRoundtrip" for request in requests))
        self.assertTrue(any(request.get("withExpressions") for request in requests))
        self.assertTrue(all(isinstance(request["validationContract"], str)
                            for request in requests if request["domainModel"] == "runtimeInput"))

    def test_native_fault_gate_checks_exception_categories_not_just_rejection(self):
        requests = [{"id": "a", "domainModel": "contract", "document": "{}"}]
        expected = {"a": {"status": "runtime-fault", "exceptionType": "System.NotSupportedException"}}
        actual = {"a": {"status": "runtime-fault", "exceptionType": "System.ArgumentException"}}
        self.assertEqual(0, run.compare(requests, expected, actual, fault_types=True)["matched"])
        actual["a"]["exceptionType"] = expected["a"]["exceptionType"]
        self.assertEqual(1, run.compare(requests, expected, actual, fault_types=True)["matched"])
        actual["a"]["status"] = "json-error"
        self.assertEqual(0, run.compare(requests, expected, actual, fault_types=True)["matched"])

    def test_production_adapter_cases_opt_in_without_changing_legacy_cases(self):
        requests = expression_cases.compiler_cases(run.ROOT)
        self.assertEqual(len(requests), len({request["id"] for request in requests}))
        self.assertTrue(all(request["withExpressions"] is True for request in requests))
        self.assertEqual({"validate", "digest", "canonicalize"},
                         {request["domainAction"] for request in requests})
        self.assertTrue(all(isinstance(request["document"], str) for request in requests))
        self.assertFalse(any(request.get("withExpressions")
                             for request in validation_cases.cases(run.ROOT)))
        formatted = next(request for request in requests
                         if request["id"] == "compiler-formatted-digest")
        self.assertIn("sum( evidence.recovery_failure )<= 0", formatted["document"])

    def test_syntax_and_cancellation_cases_are_distinct_and_explicit(self):
        syntax = expression_cases.syntax_cases()
        self.assertEqual(len(syntax), len({request["id"] for request in syntax}))
        self.assertTrue(all(isinstance(request["syntaxExpression"], str) for request in syntax))
        cancellation = expression_cases.cancellation_cases()
        self.assertTrue(all(request["cancelled"] is True for request in cancellation))
        self.assertTrue(all(request["compileOnly"] is False for request in cancellation))
        self.assertTrue(all(request["emitChecked"] is True for request in cancellation))

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
            with patch("sys.argv", ["run.py", "--reference-root", "oracle", "--report", str(report)]):
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

    def test_syntax_errors_require_exact_native_diagnostics(self):
        requests = [{"id": "a", "syntaxExpression": "not valid {"}]
        expected = {"a": {"status": "compile-error", "message": "ERROR: 1:5: unexpected token"}}
        actual = {"a": {"status": "compile-error", "message": "different diagnostic"}}
        self.assertEqual(0, run.compare(
            requests, expected, actual, error_messages=True)["matched"])
        actual["a"]["message"] = expected["a"]["message"]
        self.assertEqual(1, run.compare(
            requests, expected, actual, error_messages=True)["matched"])

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
