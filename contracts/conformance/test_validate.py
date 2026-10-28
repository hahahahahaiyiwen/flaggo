import unittest
from unittest.mock import patch

import rfc8785

import validate


class CanonicalizationVectorTests(unittest.TestCase):
    def check_vector(self, value, expected, **options):
        vector = {"name": "test-vector", "input": value, "expected": expected}
        vector.update(options)
        report = validate.Report()
        with patch.object(validate, "load_json", return_value={"vectors": [vector]}):
            validate.validate_canonicalization_vectors(report)
        return report

    def test_shared_vectors_match_expected_bytes(self):
        report = validate.Report()
        validate.validate_canonicalization_vectors(report)
        self.assertEqual([], report.errors)
        vectors = validate.load_json(validate.CANONICALIZATION_VECTORS)["vectors"]
        self.assertEqual(1 + len(vectors), report.checks)

    def test_binary64_is_explicit_and_recursive_without_converting_other_types(self):
        report = self.check_vector(
            {"9007199254740993": [
                9007199254740993, -9007199254740993,
                True, False, None, "9007199254740993",
                {"value": 18446744073709551615},
            ]},
            '{"9007199254740993":[9007199254740992,-9007199254740992,'
            'true,false,null,"9007199254740993",'
            '{"value":18446744073709552000}]}',
            numberModel="binary64",
        )
        self.assertEqual([], report.errors)
        self.assertEqual(2, report.checks)

    def test_default_vectors_still_reject_unsafe_integers(self):
        for value in (9007199254740993, -9007199254740993):
            with self.subTest(value=value):
                report = self.check_vector(value, str(value))
                self.assertEqual(1, len(report.errors))
                self.assertIn("exceeds safe integer domain", report.errors[0])

    def test_default_vectors_preserve_safe_integer_boundaries(self):
        report = self.check_vector(
            [-9007199254740991, 9007199254740991],
            "[-9007199254740991,9007199254740991]",
        )
        self.assertEqual([], report.errors)

    def test_global_json_and_canonicalization_remain_strict(self):
        value = validate.strict_json_loads("9007199254740993")
        self.assertIs(type(value), int)
        self.assertEqual(9007199254740993, value)
        with self.assertRaises(rfc8785.IntegerDomainError):
            validate.canonical_json_bytes(value)
        with self.assertRaisesRegex(ValueError, "duplicate JSON object key"):
            validate.strict_json_loads('{"a":1,"a":2}')
        with self.assertRaisesRegex(ValueError, "non-finite JSON number"):
            validate.strict_json_loads("NaN")

    def test_unknown_number_models_fail_explicitly(self):
        for model in ("decimal128", None, 42):
            with self.subTest(model=model):
                report = self.check_vector(1, "1", numberModel=model)
                self.assertEqual(1, len(report.errors))
                self.assertIn("unsupported numberModel", report.errors[0])

    def test_binary64_does_not_accept_non_finite_values(self):
        for value in (float("inf"), float("-inf"), float("nan")):
            with self.subTest(value=value):
                report = self.check_vector(value, "null", numberModel="binary64")
                self.assertEqual(1, len(report.errors))
                self.assertIn("failed:", report.errors[0])

    def test_binary64_still_compares_exact_expected_bytes(self):
        report = self.check_vector(
            9007199254740993, "9007199254740993", numberModel="binary64",
        )
        self.assertEqual(1, len(report.errors))
        self.assertIn("mismatch:", report.errors[0])


if __name__ == "__main__":
    unittest.main()
