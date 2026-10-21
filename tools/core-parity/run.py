#!/usr/bin/env python3
"""Compare the existing Flaggo CEL compiler with an isolated upstream Rust probe."""

from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import json
import math
import subprocess
import sys
from pathlib import Path

import domain_cases
import validation_cases


ROOT = Path(__file__).resolve().parents[2]
DIRECTORY = Path(__file__).resolve().parent
STATUSES = {"compiled", "compile-error", "value", "evaluation-error", "runtime-fault", "json-error"}
SCHEMAS = {
    "level": {"type": "integer"},
    "pressure": {"type": "number"},
    "label": {"type": "string"},
    "items": {"type": "array", "items": {"type": "integer"}},
    "object": {"type": "object"},
}


def boundary_cases() -> list[dict]:
    def tree(count: int) -> str:
        if count == 1:
            return "true"
        left = count // 2
        return f"({tree(left)} && {tree(count - left)})"

    def depth(count: int) -> str:
        result = "true"
        for _ in range(count - 1):
            result = f"true ? ({result}) : false"
        return result

    return [
        {"id": "expression-bytes-at-limit", "expression": " " * 4095 + "1",
         "resultType": "integer", "compileOnly": True},
        {"id": "expression-bytes-over-limit", "expression": " " * 4096 + "1",
         "resultType": "integer", "compileOnly": True},
        {"id": "ast-nodes-below-limit", "expression": tree(128),
         "resultType": "boolean", "compileOnly": True},
        {"id": "ast-nodes-at-limit", "expression": f"!({tree(128)})",
         "resultType": "boolean", "compileOnly": True},
        {"id": "ast-nodes-over-limit", "expression": tree(129),
         "resultType": "boolean", "compileOnly": True},
        {"id": "ast-depth-at-limit", "expression": depth(32),
         "resultType": "boolean", "compileOnly": True},
        {"id": "ast-depth-over-limit", "expression": depth(33),
         "resultType": "boolean", "compileOnly": True},
        {"id": "utf8-bytes-at-limit", "expression": "'" + "\u00e9" * 2047 + "'",
         "resultType": "string", "compileOnly": True},
        {"id": "utf8-bytes-over-limit", "expression": "'" + "\u00e9" * 2048 + "'",
         "resultType": "string", "compileOnly": True},
        {"id": "static-cost-below-limit", "expression": "'a' + '" + "b" * 998 + "'",
         "resultType": "string", "compileOnly": True},
        {"id": "static-cost-at-limit", "expression": "'a' + '" + "b" * 999 + "'",
         "resultType": "string", "compileOnly": True},
        {"id": "static-cost-over-limit", "expression": "'a' + '" + "b" * 1000 + "'",
         "resultType": "string", "compileOnly": True},
    ]


def parser_stress_cases() -> list[dict]:
    cases = [
        {"id": f"parentheses-{depth}", "expression": "(" * depth + "1" + ")" * depth,
         "resultType": "integer", "compileOnly": True}
        for depth in (64, 128, 249, 250, 256, 2047)
    ]
    for depth in (248, 249):
        cases.extend([
            {"id": f"conditional-parentheses-{depth}",
             "expression": "(" * depth + "true ? 1 : 2" + ")" * depth,
             "resultType": "integer", "compileOnly": True},
            {"id": f"call-parentheses-{depth}",
             "expression": "int(" + "(" * depth + "1" + ")" * depth + ")",
             "resultType": "integer", "compileOnly": True},
        ])
    cases.append({"id": "parentheses-inside-string", "expression": "'" + "(" * 300 + "'",
                  "resultType": "string", "compileOnly": True})
    return cases


def cost_cases() -> list[dict]:
    branch = "'a' + '" + "b" * 599 + "'"
    return [
        {"id": "conditional-maximum-cost", "expression": f"true ? {branch} : {branch}",
         "resultType": "string", "compileOnly": True},
        {"id": "empty-concat-cost", "expression": "('' + '') + '" + "x" * 1000 + "'",
         "resultType": "string", "compileOnly": True},
    ]


def output(command: list[str], requests: list[dict]) -> dict[str, dict]:
    result = subprocess.run(
        command,
        input="".join(json.dumps(request, ensure_ascii=False) + "\n" for request in requests),
        text=True,
        encoding="utf-8",
        capture_output=True,
        cwd=ROOT,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"Probe {command[0]} exited {result.returncode}:\n{result.stderr}"
            f"\nLast responses:\n{result.stdout[-4000:]}"
        )
    responses = {}
    for line in result.stdout.splitlines():
        response = json.loads(line)
        if (not isinstance(response, dict) or not isinstance(response.get("id"), str)
                or response.get("status") not in STATUSES):
            raise ValueError("Probe returned an invalid ID or status.")
        identifier = response["id"]
        if response["status"] == "value" and "value" not in response:
            raise ValueError("A successful value response is missing its value.")
        if identifier in responses:
            raise ValueError(f"Duplicate response ID: {identifier}")
        responses[identifier] = response
    if set(responses) != {request["id"] for request in requests}:
        raise ValueError("Response IDs do not exactly match the requested cases.")
    return responses


def same_value(left: object, right: object) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return type(left) is type(right) and left == right
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return math.isfinite(left) and math.isfinite(right) and left == right
    if type(left) is not type(right):
        return False
    if isinstance(left, list):
        return len(left) == len(right) and all(
            same_value(a, b) for a, b in zip(left, right)
        )
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(same_value(left[key], right[key]) for key in left)
    return left == right


def compare(requests: list[dict], reference: dict, candidate: dict, *, canonical: bool = False) -> dict:
    mismatches = []
    for request in requests:
        identifier = request["id"]
        expected, actual = reference[identifier], candidate[identifier]
        if (canonical and expected["status"] in {"value", "compiled"}
                and not isinstance(expected.get("canonical"), str)):
            raise ValueError(f"Reference response {identifier} is missing its canonical expression.")
        if expected["status"] != actual["status"] or (
            expected["status"] == "value" and not same_value(expected["value"], actual["value"])
        ) or (
            canonical and expected["status"] in {"value", "compiled"}
            and expected["canonical"] != actual.get("canonical")
        ):
            mismatches.append({
                "id": identifier,
                "expression": request.get("expression", request.get("canonicalJson")),
                **({"document": request["document"]} if "domainModel" in request else {}),
                "reference": expected,
                "candidate": actual,
            })
    return {
        "scope": "CEL and domain-library feasibility only; not service parity or cutover readiness.",
        "cases": len(requests),
        "matched": len(requests) - len(mismatches),
        "mismatches": mismatches,
        "notYetVerified": [
            "Complete canonical expression and contract/executable digest conformance",
            "Complete schema/profile typing, static-cost conformance and runtime cancellation",
            "Checked protobuf persistence and restart without parsing source",
            "Full strict JSON and contract validation, including out-of-range JSON numbers "
            "and polymorphic discriminator native-fault compatibility",
            "Service/store/HTTP/currentness/observability/full-loop parity",
        ],
    }


def restart_cases(requests: list[dict], reference: dict) -> list[dict]:
    cases = []
    for request in requests:
        response = reference[request["id"]]
        if response["status"] == "compile-error":
            continue
        if not isinstance(response.get("checked"), dict):
            raise ValueError(f"Compiled response {request['id']} is missing its checked expression.")
        cases.append({
            **request, "expression": "invalid source that must never be parsed {",
            "checked": response["checked"],
        })
    return cases


def corruption_cases(requests: list[dict]) -> list[dict]:
    original = next(request for request in requests if request["id"] == "integer-arithmetic")
    cases = []
    mutations = {
        "checksum": {"checkedAstDigest": "sha256:" + "0" * 64},
        "canonical": {"canonicalExpression": "0"},
        "references": {"referencedAttributes": []},
        "duplicate-references": {"referencedAttributes": ["level", "level"]},
        "protobuf": {"checkedAst": base64.b64encode(b"\xff").decode("ascii"),
                     "checkedAstDigest": "sha256:" + hashlib.sha256(b"\xff").hexdigest()},
        "empty": {"checkedAst": "", "checkedAstDigest": "sha256:" + hashlib.sha256(b"").hexdigest()},
    }
    for name, mutation in mutations.items():
        request = copy.deepcopy(original)
        request["id"] = "corrupt-" + name
        request["checked"].update(mutation)
        cases.append(request)
    request = copy.deepcopy(original)
    request.update(id="corrupt-result-type", resultType="string")
    cases.append(request)
    request = copy.deepcopy(original)
    request.update(id="corrupt-undeclared-attribute", schemas={})
    cases.append(request)
    return cases


def checked_byte_cases(requests: list[dict]) -> tuple[list[dict], dict]:
    original = next(request for request in requests if request["id"] == "integer-arithmetic")
    data = base64.b64decode(original["checked"]["checkedAst"], validate=True)

    def varint(value: int) -> bytes:
        result = bytearray()
        while value > 127:
            result.append((value & 127) | 128)
            value >>= 7
        result.append(value)
        return bytes(result)

    cases, expected = [], {}
    for size in (65536, 65537):
        padding = size - len(data) - 5
        # An unknown length-delimited protobuf field keeps the checked CEL AST intact.
        padded = data + varint((100 << 3) | 2) + varint(padding) + bytes(padding)
        if len(padded) != size:
            raise RuntimeError("Checked-protobuf boundary fixture has an incorrect byte length.")
        request = copy.deepcopy(original)
        identifier = f"checked-bytes-{size}"
        request.update(id=identifier, compileOnly=True)
        request["checked"].update(
            checkedAst=base64.b64encode(padded).decode("ascii"),
            checkedAstDigest="sha256:" + hashlib.sha256(padded).hexdigest(),
        )
        cases.append(request)
        expected[identifier] = (
            {"status": "compiled", "canonical": original["checked"]["canonicalExpression"]}
            if size == 65536 else {"status": "compile-error"}
        )
    return cases, expected


def validate_boundaries(responses: dict) -> None:
    for case in boundary_cases():
        expected = "compile-error" if case["id"].endswith("-over-limit") else "compiled"
        if responses[case["id"]]["status"] != expected:
            raise RuntimeError(f"Reference did not satisfy the exact boundary {case['id']}.")


def execute(args: argparse.Namespace) -> int:
    cases = json.loads((DIRECTORY / "expressions.json").read_text(encoding="utf-8"))
    cases.extend(boundary_cases())
    cases.extend(parser_stress_cases())
    cases.extend(cost_cases())
    if len({case["id"] for case in cases}) != len(cases):
        raise ValueError("Fixture IDs must be unique.")
    requests = [
        {"schemas": SCHEMAS, "attributes": {}, "compileOnly": False,
         "emitChecked": args.profile, **case} for case in cases
    ]
    if not args.no_build:
        subprocess.run(
            ["dotnet", "build", str(DIRECTORY / "dotnet" / "Flaggo.CoreParity.csproj"),
             "--configuration", "Release", "--nologo", "--verbosity", "quiet"],
            cwd=ROOT, check=True,
        )
        subprocess.run(
            ["cargo", "build", "--locked", "--manifest-path",
             str(DIRECTORY / "rust" / "Cargo.toml")],
            cwd=ROOT, check=True,
        )
    oracle = ["dotnet", str(DIRECTORY / "dotnet" / "bin" / "Release" / "net10.0" / "Flaggo.CoreParity.dll")]
    reference = output(oracle, requests)
    validate_boundaries(reference)
    executable = "flaggo-cel-parity-probe.exe" if sys.platform == "win32" else "flaggo-cel-parity-probe"
    command = [str(DIRECTORY / "rust" / "target" / "debug" / executable)]
    if args.profile:
        command.append("--profile")
    candidate = output(command, requests)
    report = compare(requests, reference, candidate, canonical=args.profile)
    report["candidate"] = "bounded-profile-prototype" if args.profile else "upstream-library"
    vectors = json.loads((ROOT / "contracts" / "conformance" / "canonicalization-vectors-v1.json")
                         .read_text(encoding="utf-8"))["vectors"]
    canonical_requests = [{"id": vector["name"], "canonicalJson": vector["input"]} for vector in vectors]
    golden = {vector["name"]: {"status": "value", "value": vector["expected"]} for vector in vectors}
    report["dotnetCanonicalJson"] = compare(canonical_requests, golden, output(oracle, canonical_requests))
    report["rustCanonicalJson"] = compare(canonical_requests, golden, output(command, canonical_requests))
    domain_requests = domain_cases.cases(ROOT)
    report["domainModels"] = compare(
        domain_requests, output(oracle, domain_requests), output(command, domain_requests))
    validation_requests = validation_cases.cases(ROOT)
    report["domainValidation"] = compare(
        validation_requests, output(oracle, validation_requests), output(command, validation_requests))
    if args.profile:
        restart = restart_cases(requests, reference)
        reloaded = output(oracle, restart)
        report["dotnetRestart"] = compare(restart, reference, reloaded, canonical=True)
        report["rustRestart"] = compare(restart, reloaded, output(command, restart), canonical=True)
        rust_stored = restart_cases(requests, candidate)
        report["rustCompiledDotnetRestart"] = compare(
            rust_stored, reference, output(oracle, rust_stored), canonical=True)
        report["rustCompiledRustRestart"] = compare(
            rust_stored, reference, output(command, rust_stored), canonical=True)
        corrupt = corruption_cases(restart)
        rejected = output(oracle, corrupt)
        if any(response["status"] != "compile-error" for response in rejected.values()):
            raise RuntimeError("The reference accepted a corrupted checked expression.")
        report["corruptionRejection"] = compare(corrupt, rejected, output(command, corrupt))
        byte_cases, byte_expected = checked_byte_cases(restart)
        report["dotnetCheckedByteBounds"] = compare(
            byte_cases, byte_expected, output(oracle, byte_cases), canonical=True)
        report["rustCheckedByteBounds"] = compare(
            byte_cases, byte_expected, output(command, byte_cases), canonical=True)
        report["notYetVerified"].remove("Checked protobuf persistence and restart without parsing source")
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    checks = {"expressions": report, "dotnetCanonicalJson": report["dotnetCanonicalJson"],
              "rustCanonicalJson": report["rustCanonicalJson"],
              "domainModels": report["domainModels"], "domainValidation": report["domainValidation"]}
    if args.profile:
        checks.update((name, report[name]) for name in
                      ("dotnetRestart", "rustRestart", "rustCompiledDotnetRestart",
                       "rustCompiledRustRestart", "corruptionRejection",
                       "dotnetCheckedByteBounds", "rustCheckedByteBounds"))
    for name, check in checks.items():
        print(f"{name}: {check['matched']}/{check['cases']} cases match")
    print(f"report: {args.report}")
    return 1 if any(check["mismatches"] for check in checks.values()) else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--profile", action="store_true")
    args = parser.parse_args()
    try:
        return execute(args)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Parity probe failed: {error}", file=sys.stderr)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps({
            "outcome": "probe-failure", "error": str(error),
        }, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
