import json
import struct
import tempfile
import unittest
from decimal import Decimal
from pathlib import Path

from flaggo_evidence import RESULT_FORMAT, VALUE_FORMAT, decode_cell, load_rows


class EvidenceDecoderTests(unittest.TestCase):
    def test_shared_cells_preserve_precision_and_reject_invalid_values(self):
        root = Path(__file__).resolve().parents[3]
        corpus = json.loads((root / "contracts/fixtures/evidence-value-v1.json").read_text())
        for cell in corpus["valid"]:
            value = decode_cell(cell)
            if cell["kind"] == "float64":
                self.assertEqual(struct.pack(">d", value).hex(), cell["bits"])
            elif cell["kind"] == "int64":
                self.assertEqual(str(value), cell["value"])
        for cell in corpus["invalid"] + corpus["invalidJsonText"]:
            with self.subTest(cell=cell), self.assertRaises((ValueError, TypeError)):
                decode_cell(cell)
        document = decode_cell({"kind": "json", "value": '{"n":9007199254740993,"d":1.2300,"huge":1e400}'})
        self.assertEqual(document["n"], 9007199254740993)
        self.assertEqual(document["d"].as_tuple(), Decimal("1.2300").as_tuple())
        self.assertEqual(document["huge"], Decimal("1e400"))

    def test_saved_artifacts_require_explicit_formats(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "query.json"
            artifact = {
                "resultFormat": RESULT_FORMAT,
                "valueFormat": VALUE_FORMAT,
                "rows": [{"n": {"kind": "int64", "value": "9007199254740993"}}],
            }
            path.write_text(json.dumps(artifact), encoding="utf-8")
            self.assertEqual(load_rows(path), [{"n": 9007199254740993}])
            del artifact["resultFormat"]
            path.write_text(json.dumps(artifact), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Unsupported"):
                load_rows(path)


if __name__ == "__main__":
    unittest.main()
