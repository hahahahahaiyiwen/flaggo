"""Strict decoder for flaggo.evidence.query-result/v1 saved artifacts."""

import json
import re
import struct
from decimal import Decimal
from pathlib import Path

RESULT_FORMAT = "flaggo.evidence.query-result/v1"
VALUE_FORMAT = "flaggo.evidence.value/v1"


def _invalid_constant(value):
    raise ValueError(f"Invalid JSON numeric constant: {value}")


def decode_cell(cell):
    if not isinstance(cell, dict):
        raise ValueError("Evidence cell must be a tagged object")
    kind = cell.get("kind")
    expected = {"kind"} if kind == "null" else {"kind", "bits" if kind == "float64" else "value"}
    if set(cell) != expected:
        raise ValueError("Invalid evidence cell fields")
    if kind == "null":
        return None
    value = cell.get("value")
    if kind == "int64" and isinstance(value, str):
        if not re.fullmatch(r"0|-?[1-9][0-9]*", value):
            raise ValueError("Invalid canonical int64")
        integer = int(value)
        if not -(2**63) <= integer < 2**63:
            raise ValueError("Evidence int64 is out of range")
        return integer
    if kind == "float64":
        bits = cell["bits"]
        if not isinstance(bits, str) or not re.fullmatch(r"[0-9a-f]{16}", bits):
            raise ValueError("Invalid binary64 bits")
        return struct.unpack(">d", bytes.fromhex(bits))[0]
    if kind == "text" and isinstance(value, str):
        return value
    if kind == "boolean" and isinstance(value, bool):
        return value
    if kind == "json" and isinstance(value, str):
        return json.loads(value, parse_float=Decimal, parse_constant=_invalid_constant)
    raise ValueError(f"Unsupported or malformed evidence cell: {kind}")


def load_rows(path):
    """Decode saved rows, retaining large integers and exact binary64 values."""
    with Path(path).open(encoding="utf-8") as stream:
        result = json.load(stream, parse_constant=_invalid_constant)
    if result.get("resultFormat") != RESULT_FORMAT or result.get("valueFormat") != VALUE_FORMAT:
        raise ValueError("Unsupported evidence result/value format; legacy files are not reinterpreted")
    rows = result.get("rows")
    if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
        raise ValueError("Evidence result must contain object rows")
    return [{name: decode_cell(cell) for name, cell in row.items()} for row in rows]
